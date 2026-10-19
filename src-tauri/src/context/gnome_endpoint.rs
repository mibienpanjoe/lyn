//! Crash-safe address publication, not peer admission or a context source.

use super::gnome_listener::NativeListener;
use std::{
    ffi::CString,
    fs::{self, File, Metadata, OpenOptions},
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt},
        },
    },
    path::{Path, PathBuf},
};

const LOCK: &str = "lyn-gnome-v1.lock";
const CURRENT: &str = "lyn-gnome-v1.current";
const TEMPORARY: &str = "publication";
const SOCKET: &str = "lyn-gnome-v1.sock";
const MAX_INSTANCES: usize = 64;
const MAX_ENTRIES: usize = 4096;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Identity(u64, u64);

impl Identity {
    fn of(metadata: &Metadata) -> Self {
        Self(metadata.dev(), metadata.ino())
    }
}

fn refused() -> io::Error {
    io::Error::from(io::ErrorKind::PermissionDenied)
}

fn private(metadata: &Metadata, directory: bool) -> bool {
    metadata.uid() == unsafe { libc::geteuid() }
        && metadata.mode() & 0o7777 == if directory { 0o700 } else { 0o600 }
        && if directory {
            metadata.is_dir()
        } else {
            metadata.is_file() && metadata.nlink() == 1
        }
}

fn name(value: &str) -> CString {
    CString::new(value).expect("internal endpoint basename")
}

fn open_at(directory: &File, basename: &str, flags: i32) -> io::Result<File> {
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name(basename).as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            0o600,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn metadata_at(directory: &File, basename: &str) -> io::Result<libc::stat> {
    let mut metadata = std::mem::MaybeUninit::uninit();
    if unsafe {
        libc::fstatat(
            directory.as_raw_fd(),
            name(basename).as_ptr(),
            metadata.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { metadata.assume_init() })
}

fn matches_at(directory: &File, basename: &str, identity: Identity) -> bool {
    metadata_at(directory, basename)
        .is_ok_and(|metadata| Identity(metadata.st_dev, metadata.st_ino) == identity)
}

fn remove_owned(directory: &File, basename: &str, identity: Identity, is_directory: bool) {
    if matches_at(directory, basename, identity) {
        // No recursive cleanup or stale-socket deletion, including error paths.
        unsafe {
            libc::unlinkat(
                directory.as_raw_fd(),
                name(basename).as_ptr(),
                if is_directory { libc::AT_REMOVEDIR } else { 0 },
            );
        }
    }
}

fn instance_name(bytes: &[u8]) -> bool {
    bytes.len() == 35
        && bytes.starts_with(b"gn-")
        && bytes[3..]
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

fn record(directory: &File) -> io::Result<Option<Identity>> {
    let mut file = match open_at(directory, CURRENT, libc::O_RDONLY) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        result => result?,
    };
    let metadata = file.metadata()?;
    if !private(&metadata, false) || metadata.len() != 36 {
        return Err(refused());
    }
    let mut bytes = Vec::with_capacity(37);
    (&mut file).take(37).read_to_end(&mut bytes)?;
    let identity = Identity::of(&metadata);
    if bytes.len() != 36
        || bytes[35] != b'\n'
        || !instance_name(&bytes[..35])
        || !matches_at(directory, CURRENT, identity)
    {
        return Err(refused());
    }
    Ok(Some(identity))
}

struct PublisherLock(File);

impl PublisherLock {
    fn acquire(root: &File) -> io::Result<Self> {
        let file = open_at(root, LOCK, libc::O_RDWR | libc::O_CREAT)?;
        if !private(&file.metadata()?, false) {
            return Err(refused());
        }
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(file))
    }
}

impl Drop for PublisherLock {
    fn drop(&mut self) {
        // flock is tied to an open file description. A concurrent spawn may
        // briefly inherit it before CLOEXEC, so closing only our FD is not
        // enough for immediate clean restart. Never unlink the lock file.
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

/// Prepared address ownership. The broker must drop its listener before this
/// lease, and publish only after its admission worker is ready. Not activated
/// by app startup until the discovery consumer is implemented and tested.
pub(crate) struct NativePublication {
    root: File,
    root_path: PathBuf,
    root_identity: Identity,
    lock: PublisherLock,
    instance: File,
    instance_name: String,
    instance_path: PathBuf,
    instance_identity: Identity,
    // Retain the file, not just its numeric inode: an unlinked inode must not
    // recycle into a replacement before delayed ownership cleanup.
    published: Option<File>,
    temporary: Option<File>,
}

impl NativePublication {
    pub(crate) fn prepare(runtime: &Path) -> io::Result<Self> {
        if !runtime.is_absolute() {
            return Err(refused());
        }
        let root_path: PathBuf = runtime.components().collect();
        // Reject before creating anything; sockaddr_un includes the final NUL.
        if root_path.as_os_str().as_bytes().len() + 1 + 35 + 1 + SOCKET.len() >= 108 {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        let root = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&root_path)?;
        let metadata = root.metadata()?;
        if !private(&metadata, true) {
            return Err(refused());
        }
        let root_identity = Identity::of(&metadata);
        let lock = PublisherLock::acquire(&root)?;
        if !matches_at(&root, LOCK, Identity::of(&lock.0.metadata()?)) {
            return Err(refused());
        }
        record(&root)?;
        let mut instances = 0;
        // Inspect through the pinned directory, not a possibly replaced path.
        let mut inspected = 0;
        for entry in fs::read_dir(format!("/proc/self/fd/{}", root.as_raw_fd()))?.take(MAX_ENTRIES)
        {
            inspected += 1;
            let entry = entry?;
            if instance_name(entry.file_name().as_bytes()) {
                instances += 1;
            }
            if instances >= MAX_INSTANCES {
                return Err(refused());
            }
        }
        if inspected == MAX_ENTRIES {
            return Err(refused());
        }
        let instance_name = format!("gn-{}", uuid::Uuid::new_v4().simple());
        if unsafe { libc::mkdirat(root.as_raw_fd(), name(&instance_name).as_ptr(), 0o700) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // O_NOFOLLOW pins the fresh directory; never accept a pre-existing name.
        let instance = open_at(&root, &instance_name, libc::O_RDONLY | libc::O_DIRECTORY)?;
        let metadata = instance.metadata()?;
        if !private(&metadata, true) {
            return Err(refused());
        }
        let instance_identity = Identity::of(&metadata);
        let instance_path = root_path.join(&instance_name);
        Ok(Self {
            root,
            root_path,
            root_identity,
            lock,
            instance,
            instance_name,
            instance_path,
            instance_identity,
            published: None,
            temporary: None,
        })
    }

    pub(crate) fn instance_path(&self) -> &Path {
        &self.instance_path
    }

    fn recheck(&self) -> io::Result<()> {
        let metadata = fs::symlink_metadata(&self.root_path)?;
        if !private(&metadata, true)
            || Identity::of(&metadata) != self.root_identity
            || !private(&self.lock.0.metadata()?, false)
            || !matches_at(&self.root, LOCK, Identity::of(&self.lock.0.metadata()?))
            || !matches_at(&self.root, &self.instance_name, self.instance_identity)
            || !private(&self.instance.metadata()?, true)
        {
            return Err(refused());
        }
        Ok(())
    }

    pub(crate) fn publish(&mut self, listener: &NativeListener) -> io::Result<()> {
        if self.published.is_some() || listener.socket_path() != self.instance_path.join(SOCKET) {
            return Err(refused());
        }
        self.recheck()?;
        let socket = metadata_at(&self.instance, SOCKET)?;
        if socket.st_mode & libc::S_IFMT != libc::S_IFSOCK
            || socket.st_mode & 0o7777 != 0o600
            || socket.st_uid != unsafe { libc::geteuid() }
            || !listener.owns_endpoint(socket.st_dev, socket.st_ino)
        {
            return Err(refused());
        }
        let previous = record(&self.root)?;
        let file = open_at(
            &self.instance,
            TEMPORARY,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        )?;
        self.temporary = Some(file);
        let file = self.temporary.as_mut().expect("owned publication file");
        let identity = Identity::of(&file.metadata()?);
        if !private(&file.metadata()?, false) {
            return Err(refused());
        }
        writeln!(file, "{}", self.instance_name)?;
        self.recheck()?;
        if previous != record(&self.root)? || !matches_at(&self.instance, TEMPORARY, identity) {
            return Err(refused());
        }
        if unsafe {
            libc::renameat(
                self.instance.as_raw_fd(),
                name(TEMPORARY).as_ptr(),
                self.root.as_raw_fd(),
                name(CURRENT).as_ptr(),
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        self.published = self.temporary.take();
        self.recheck()?;
        if !matches_at(&self.root, CURRENT, identity) {
            return Err(refused());
        }
        Ok(())
    }
}

impl Drop for NativePublication {
    fn drop(&mut self) {
        if let Some(file) = &self.published
            && let Ok(metadata) = file.metadata()
        {
            remove_owned(&self.root, CURRENT, Identity::of(&metadata), false);
        }
        if let Some(file) = &self.temporary
            && let Ok(metadata) = file.metadata()
        {
            remove_owned(&self.instance, TEMPORARY, Identity::of(&metadata), false);
        }
        remove_owned(
            &self.root,
            &self.instance_name,
            self.instance_identity,
            true,
        );
        // PublisherLock unlocks after this cleanup; the kernel releases it on
        // process death as well. Never publish PID heuristics.
    }
}

#[cfg(test)]
#[path = "gnome_endpoint_tests.rs"]
mod tests;
