//! Read-only service compatibility. Never starts, stops or reloads a service.

use gio::{
    glib::{self, variant::ToVariant},
    prelude::*,
};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const SERVER: &str = "/usr/libexec/gnome-terminal-server";
const UNIT: &str = "gnome-terminal-server.service";
const BUS: &str = "org.gnome.Terminal";
const MANAGER: &str = "org.freedesktop.systemd1";
const UNIT_IFACE: &str = "org.freedesktop.systemd1.Unit";
const SERVICE_IFACE: &str = "org.freedesktop.systemd1.Service";
const PROPERTIES: [(&str, &str); 11] = [
    (UNIT_IFACE, "LoadState"),
    (UNIT_IFACE, "FragmentPath"),
    (UNIT_IFACE, "DropInPaths"),
    (UNIT_IFACE, "NeedDaemonReload"),
    (UNIT_IFACE, "Transient"),
    (SERVICE_IFACE, "Type"),
    (SERVICE_IFACE, "BusName"),
    (SERVICE_IFACE, "ExecStart"),
    (SERVICE_IFACE, "ExecStartPre"),
    (SERVICE_IFACE, "ExecStartPost"),
    (SERVICE_IFACE, "ExecCondition"),
];
type Properties = BTreeMap<&'static str, glib::Variant>;
type Exec = (String, Vec<String>, bool, u64, u64, u64, u64, u32, i32, i32);

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ServiceRefusal {
    Unavailable,
    Unsupported,
    Changed,
}
type Result<T> = std::result::Result<T, ServiceRefusal>;

fn value<T: glib::variant::FromVariant>(properties: &Properties, name: &str) -> Result<T> {
    properties
        .get(name)
        .and_then(|value| value.get())
        .ok_or(ServiceRefusal::Unsupported)
}

fn policy(properties: &Properties) -> Result<String> {
    let fragment: String = value(properties, "FragmentPath")?;
    let starts: Vec<Exec> = value(properties, "ExecStart")?;
    if value::<String>(properties, "LoadState")? != "loaded"
        || ![
            "/usr/lib/systemd/user/gnome-terminal-server.service",
            "/lib/systemd/user/gnome-terminal-server.service",
        ]
        .contains(&fragment.as_str())
        || value::<bool>(properties, "NeedDaemonReload")?
        || value::<bool>(properties, "Transient")?
        || !value::<Vec<String>>(properties, "DropInPaths")?.is_empty()
        || value::<String>(properties, "Type")? != "dbus"
        || value::<String>(properties, "BusName")? != BUS
        || starts.len() != 1
        || starts[0].0 != SERVER
        || starts[0].1 != [SERVER]
        || starts[0].2
        || !value::<Vec<Exec>>(properties, "ExecStartPre")?.is_empty()
        || !value::<Vec<Exec>>(properties, "ExecStartPost")?.is_empty()
    {
        return Err(ServiceRefusal::Unsupported);
    }
    if !value::<Vec<Exec>>(properties, "ExecCondition")?.is_empty() {
        return Err(ServiceRefusal::Unsupported);
    }
    Ok(fragment)
}

struct Budget {
    deadline: Instant,
    cancel: gio::Cancellable,
    done: Option<mpsc::Sender<()>>,
    watchdog: Option<thread::JoinHandle<()>>,
}
impl Budget {
    fn new() -> Result<Self> {
        let duration = Duration::from_secs(1);
        let deadline = Instant::now() + duration;
        let cancel = gio::Cancellable::new();
        let watched = cancel.clone();
        let (done, receiver) = mpsc::channel();
        let watchdog = thread::Builder::new()
            .name("lyn-service-probe".into())
            .spawn(move || {
                if matches!(
                    receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    watched.cancel();
                }
            })
            .map_err(|_| ServiceRefusal::Unavailable)?;
        Ok(Self {
            deadline,
            cancel,
            done: Some(done),
            watchdog: Some(watchdog),
        })
    }
    fn remaining(&self) -> Result<i32> {
        let remaining = self
            .deadline
            .checked_duration_since(Instant::now())
            .ok_or(ServiceRefusal::Unavailable)?;
        Ok(remaining.as_millis().clamp(1, 500) as i32)
    }
}
impl Drop for Budget {
    fn drop(&mut self) {
        self.done.take();
        if let Some(watchdog) = self.watchdog.take() {
            let _ = watchdog.join();
        }
    }
}

fn call(
    connection: &gio::DBusConnection,
    budget: &Budget,
    destination: &str,
    path: &str,
    interface: &str,
    method: &str,
    parameters: glib::Variant,
) -> Result<glib::Variant> {
    let reply = connection
        .call_sync(
            Some(destination),
            path,
            interface,
            method,
            Some(&parameters),
            None,
            gio::DBusCallFlags::NO_AUTO_START,
            budget.remaining()?,
            Some(&budget.cancel),
        )
        .map_err(|_| ServiceRefusal::Unavailable)?;
    budget.remaining()?;
    if reply.size() > 16384 {
        return Err(ServiceRefusal::Unsupported);
    }
    Ok(reply)
}

fn owner(connection: &gio::DBusConnection, budget: &Budget) -> Result<String> {
    let reply = call(
        connection,
        budget,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "GetNameOwner",
        (MANAGER,).to_variant(),
    )?;
    let (name,) = reply
        .get::<(String,)>()
        .ok_or(ServiceRefusal::Unsupported)?;
    if !name.starts_with(':') || name.len() > 255 {
        return Err(ServiceRefusal::Unsupported);
    }
    Ok(name)
}

fn properties(
    connection: &gio::DBusConnection,
    budget: &Budget,
    owner: &str,
    path: &str,
) -> Result<Properties> {
    let mut properties = BTreeMap::new();
    for (interface, name) in PROPERTIES {
        let reply = call(
            connection,
            budget,
            owner,
            path,
            "org.freedesktop.DBus.Properties",
            "Get",
            (interface, name).to_variant(),
        )?;
        let (value,) = reply
            .get::<(glib::Variant,)>()
            .ok_or(ServiceRefusal::Unsupported)?;
        properties.insert(name, value);
    }
    Ok(properties)
}

struct TrustedFile {
    file: File,
    path: &'static str,
    metadata: fs::Metadata,
}
impl TrustedFile {
    fn open(path: &'static str) -> Result<Self> {
        let canonical = fs::canonicalize(path).map_err(|_| ServiceRefusal::Unsupported)?;
        for parent in canonical.ancestors().skip(1) {
            let metadata = fs::metadata(parent).map_err(|_| ServiceRefusal::Unsupported)?;
            if !metadata.is_dir() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
                return Err(ServiceRefusal::Unsupported);
            }
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&canonical)
            .map_err(|_| ServiceRefusal::Unsupported)?;
        let metadata = file.metadata().map_err(|_| ServiceRefusal::Unsupported)?;
        if !metadata.is_file()
            || metadata.uid() != 0
            || metadata.mode() & 0o022 != 0
            || metadata.nlink() != 1
        {
            return Err(ServiceRefusal::Unsupported);
        }
        let result = Self {
            file,
            path,
            metadata,
        };
        result.recheck()?;
        Ok(result)
    }
    fn recheck(&self) -> Result<()> {
        for metadata in [self.file.metadata(), fs::metadata(self.path)] {
            let metadata = metadata.map_err(|_| ServiceRefusal::Changed)?;
            if (
                metadata.dev(),
                metadata.ino(),
                metadata.ctime(),
                metadata.ctime_nsec(),
                metadata.len(),
                metadata.mode(),
                metadata.uid(),
                metadata.nlink(),
            ) != (
                self.metadata.dev(),
                self.metadata.ino(),
                self.metadata.ctime(),
                self.metadata.ctime_nsec(),
                self.metadata.len(),
                self.metadata.mode(),
                self.metadata.uid(),
                self.metadata.nlink(),
            ) {
                return Err(ServiceRefusal::Changed);
            }
        }
        Ok(())
    }
}

fn activation_agrees(text: &str) -> bool {
    let mut fields = BTreeMap::new();
    let mut section = false;
    for line in text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        if line == "[D-BUS Service]" && !section {
            section = true;
            continue;
        }
        let Some((name, value)) = line.split_once('=') else {
            return false;
        };
        if !section || fields.insert(name.trim(), value.trim()).is_some() {
            return false;
        }
    }
    fields == BTreeMap::from([("Name", BUS), ("SystemdService", UNIT), ("Exec", SERVER)])
}

/// Point-in-time compatibility of the unmodified reference service, not an
/// installation lease or proof that a module is loaded. Recheck before writes.
pub(super) fn inspect_unmodified_service() -> Result<()> {
    let address =
        std::env::var("DBUS_SESSION_BUS_ADDRESS").map_err(|_| ServiceRefusal::Unavailable)?;
    inspect_address(&address)
}

fn inspect_address(address: &str) -> Result<()> {
    if address.len() > 4096 || !address.starts_with("unix:path=/") || address.contains(';') {
        return Err(ServiceRefusal::Unavailable);
    }
    let budget = Budget::new()?;
    let connection = gio::DBusConnection::for_address_sync(
        &address,
        gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
            | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
        None,
        Some(&budget.cancel),
    )
    .map_err(|_| ServiceRefusal::Unavailable)?;
    connection.set_exit_on_close(false);
    let manager = owner(&connection, &budget)?;
    let reply = call(
        &connection,
        &budget,
        &manager,
        "/org/freedesktop/systemd1",
        "org.freedesktop.systemd1.Manager",
        "LoadUnit",
        (UNIT,).to_variant(),
    )?;
    let (path,) = reply
        .get::<(glib::variant::ObjectPath,)>()
        .ok_or(ServiceRefusal::Unsupported)?;
    let first = properties(&connection, &budget, &manager, path.as_str())?;
    let fragment = policy(&first)?;
    let fragment = match fragment.as_str() {
        "/usr/lib/systemd/user/gnome-terminal-server.service" => {
            "/usr/lib/systemd/user/gnome-terminal-server.service"
        }
        "/lib/systemd/user/gnome-terminal-server.service" => {
            "/lib/systemd/user/gnome-terminal-server.service"
        }
        _ => return Err(ServiceRefusal::Unsupported),
    };
    let vendor = TrustedFile::open(fragment)?;
    let executable = TrustedFile::open(SERVER)?;
    if executable.metadata.mode() & 0o111 == 0 || executable.metadata.mode() & 0o6000 != 0 {
        return Err(ServiceRefusal::Unsupported);
    }
    let mut activation =
        TrustedFile::open("/usr/share/dbus-1/services/org.gnome.Terminal.service")?;
    let mut text = String::new();
    (&mut activation.file)
        .take(16385)
        .read_to_string(&mut text)
        .map_err(|_| ServiceRefusal::Unsupported)?;
    if text.len() > 16384 || !activation_agrees(&text) {
        return Err(ServiceRefusal::Unsupported);
    }
    if first != properties(&connection, &budget, &manager, path.as_str())?
        || manager != owner(&connection, &budget)?
    {
        return Err(ServiceRefusal::Changed);
    }
    vendor.recheck()?;
    executable.recheck()?;
    activation.recheck()?;
    budget.remaining()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference() -> Properties {
        let mut properties = BTreeMap::new();
        for (name, text) in [
            ("LoadState", "loaded"),
            (
                "FragmentPath",
                "/usr/lib/systemd/user/gnome-terminal-server.service",
            ),
            ("Type", "dbus"),
            ("BusName", BUS),
        ] {
            properties.insert(name, text.to_variant());
        }
        properties.insert("NeedDaemonReload", false.to_variant());
        properties.insert("Transient", false.to_variant());
        properties.insert("DropInPaths", Vec::<String>::new().to_variant());
        properties.insert(
            "ExecStart",
            vec![(
                SERVER.to_owned(),
                vec![SERVER.to_owned()],
                false,
                0u64,
                0u64,
                0u64,
                0u64,
                0u32,
                0i32,
                0i32,
            )]
            .to_variant(),
        );
        for name in ["ExecStartPre", "ExecStartPost", "ExecCondition"] {
            properties.insert(name, Vec::<Exec>::new().to_variant());
        }
        properties
    }

    #[test]
    fn exact_reference_dbus_types_are_required_before_compatibility() {
        assert!(policy(&reference()).is_ok());
        for (_, name) in PROPERTIES {
            let mut properties = reference();
            properties.remove(name);
            assert_eq!(
                policy(&properties),
                Err(ServiceRefusal::Unsupported),
                "missing {name}"
            );
            properties.insert(name, 42u32.to_variant());
            assert_eq!(
                policy(&properties),
                Err(ServiceRefusal::Unsupported),
                "wrong type {name}"
            );
        }
    }

    #[test]
    fn customized_or_stale_service_is_refused() {
        for (name, replacement) in [
            ("LoadState", "masked".to_variant()),
            (
                "FragmentPath",
                "/home/user/gnome-terminal-server.service".to_variant(),
            ),
            ("Type", "simple".to_variant()),
            ("BusName", "org.example.Other".to_variant()),
            ("Transient", true.to_variant()),
            ("NeedDaemonReload", true.to_variant()),
            (
                "DropInPaths",
                vec!["/tmp/custom.conf".to_owned()].to_variant(),
            ),
        ] {
            let mut properties = reference();
            properties.insert(name, replacement);
            assert_eq!(
                policy(&properties),
                Err(ServiceRefusal::Unsupported),
                "{name}"
            );
        }
        let start: Vec<Exec> = value(&reference(), "ExecStart").unwrap();
        let mut path = start.clone();
        path[0].0 = "/tmp/custom".to_owned();
        let mut args = start.clone();
        args[0].1.push("--custom".to_owned());
        let mut ignore = start.clone();
        ignore[0].2 = true;
        let mut duplicate = start.clone();
        duplicate.extend(start.clone());
        for changed in [vec![], path, args, ignore, duplicate] {
            let mut properties = reference();
            properties.insert("ExecStart", changed.to_variant());
            assert_eq!(policy(&properties), Err(ServiceRefusal::Unsupported));
        }
        for name in ["ExecStartPre", "ExecStartPost", "ExecCondition"] {
            let mut properties = reference();
            properties.insert(name, start.to_variant());
            assert_eq!(policy(&properties), Err(ServiceRefusal::Unsupported));
        }
    }

    #[test]
    fn dbus_activation_must_route_to_the_exact_service_and_server() {
        let reference =
            format!("[D-BUS Service]\nName={BUS}\nSystemdService={UNIT}\nExec={SERVER}\n");
        assert!(activation_agrees(&reference));
        for changed in [
            reference.replace(UNIT, "custom.service"),
            reference.replace(SERVER, "/tmp/custom"),
            reference.replace(BUS, "org.example.Other"),
            reference.replace("SystemdService=", "Other="),
            format!("{reference}Exec={SERVER}\n"),
            format!("{reference}[Other]\n"),
        ] {
            assert!(!activation_agrees(&changed));
        }
    }

    #[test]
    fn silent_bus_authentication_is_cancelled_without_starting_any_service() {
        use std::{
            os::unix::net::UnixListener,
            sync::{
                Arc,
                atomic::{AtomicBool, Ordering},
            },
        };
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("bus");
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let done = Arc::new(AtomicBool::new(false));
        let stopped = done.clone();
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            let mut stream = None;
            while !stopped.load(Ordering::Relaxed) && Instant::now() < deadline {
                if stream.is_none() {
                    stream = listener.accept().ok();
                }
                thread::sleep(Duration::from_millis(2));
            }
        });
        let start = Instant::now();
        let result = inspect_address(&format!("unix:path={}", path.display()));
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        assert_eq!(result, Err(ServiceRefusal::Unavailable));
        assert!(start.elapsed() < Duration::from_millis(1600));
    }

    #[test]
    fn reference_service_read_only_opt_in() {
        if std::env::var_os("LYN_GNOME_SERVICE_READ_ONLY_TEST").is_none() {
            return;
        }
        assert_eq!(inspect_unmodified_service(), Ok(()));
    }
}
