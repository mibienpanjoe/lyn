//! One native context in an owned child, private framed pipes, no listening socket.
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

const MAX_FRAME: usize = 512 * 1024;
pub(super) const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
const JOB_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    text: Option<String>,
    failed: bool,
}

fn read_frame(reader: &mut impl Read, limit: usize) -> std::io::Result<Vec<u8>> {
    let mut size = [0; 4];
    reader.read_exact(&mut size)?;
    let size = u32::from_le_bytes(size) as usize;
    if size == 0 || size > limit {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    let mut bytes = vec![0; size];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn write_frame(writer: &mut impl Write, bytes: &[u8]) -> std::io::Result<()> {
    if bytes.is_empty() || bytes.len() > MAX_FRAME {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    writer.write_all(&(bytes.len() as u32).to_le_bytes())?;
    writer.write_all(bytes)?;
    writer.flush()
}

#[derive(Debug)]
pub(super) struct SpeechWorker {
    child: Child,
    input: ChildStdin,
    replies: mpsc::Receiver<Result<Reply, ()>>,
    reader: Option<std::thread::JoinHandle<()>>,
    last_used: Instant,
}

impl SpeechWorker {
    pub(super) fn start(app_data: &Path) -> Result<Self, &'static str> {
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            let mut command =
                Command::new(std::env::current_exe().map_err(|_| "MODEL_NOT_AVAILABLE")?);
            command.arg("--lyn-speech-worker").arg(app_data);
            command.env("PARAKEET_DEVICE", "cpu");
            command.env("OMP_NUM_THREADS", "4");
            Self::spawn(&mut command)
        }
        #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
        {
            let _ = app_data;
            Err("MODEL_NOT_AVAILABLE")
        }
    }

    pub(super) fn spawn(command: &mut Command) -> Result<Self, &'static str> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "MODEL_NOT_AVAILABLE")?;
        let input = child.stdin.take().ok_or("ENRICHMENT_FAILED")?;
        let mut output = child.stdout.take().ok_or("ENRICHMENT_FAILED")?;
        let (sender, replies) = mpsc::sync_channel(1);
        let reader = match std::thread::Builder::new()
            .name("lyn-speech-reply".into())
            .spawn(move || {
                loop {
                    let result = read_frame(&mut output, MAX_FRAME)
                        .map_err(|_| ())
                        .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|_| ()));
                    let failed = result.is_err();
                    if sender.try_send(result).is_err() || failed {
                        break;
                    }
                }
            }) {
            Ok(reader) => reader,
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("ENRICHMENT_FAILED");
            }
        };
        Ok(Self {
            child,
            input,
            replies,
            reader: Some(reader),
            last_used: Instant::now(),
        })
    }

    #[cfg(test)]
    pub(super) fn expire_for_test(&mut self) {
        self.last_used = Instant::now() - IDLE_TIMEOUT;
    }

    pub(super) fn idle_remaining(&self) -> Duration {
        IDLE_TIMEOUT.saturating_sub(self.last_used.elapsed())
    }

    pub(super) fn reusable(&mut self) -> bool {
        self.last_used.elapsed() < IDLE_TIMEOUT && matches!(self.child.try_wait(), Ok(None))
    }

    pub(super) fn transcribe(
        &mut self,
        audio: &Path,
        cancelled: &AtomicBool,
    ) -> Result<Option<String>, &'static str> {
        self.transcribe_with_deadline(audio, cancelled, Instant::now() + JOB_TIMEOUT)
    }

    fn transcribe_with_deadline(
        &mut self,
        audio: &Path,
        cancelled: &AtomicBool,
        deadline: Instant,
    ) -> Result<Option<String>, &'static str> {
        let request = serde_json::to_vec(audio.to_str().ok_or("INVALID_MEDIA")?)
            .map_err(|_| "INVALID_MEDIA")?;
        if request.len() > 4096 {
            return Err("INVALID_MEDIA");
        }
        if cancelled.load(Ordering::Acquire) {
            return Err("ENRICHMENT_CANCELLED");
        }
        write_frame(&mut self.input, &request).map_err(|_| "ENRICHMENT_FAILED")?;
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Err("ENRICHMENT_CANCELLED");
            }
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or("ENRICHMENT_TIMEOUT")?;
            match self
                .replies
                .recv_timeout(remaining.min(Duration::from_millis(25)))
            {
                Ok(Ok(reply)) => {
                    self.last_used = Instant::now();
                    return if reply.failed {
                        Err("ENRICHMENT_FAILED")
                    } else {
                        Ok(reply.text)
                    };
                }
                Ok(Err(())) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("ENRICHMENT_FAILED");
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }
}

impl Drop for SpeechWorker {
    fn drop(&mut self) {
        // std::process::Child retains our unreaped direct child; never signal a remembered foreign PID.
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

/// Runs before Tauri/single-instance initialization. Only Linux x86_64 is admitted.
pub(crate) fn helper_mode() -> Option<std::process::ExitCode> {
    let mut args = std::env::args_os().skip(1);
    let mode = args.next()?;
    if mode != "--lyn-speech-worker" && mode != "--lyn-speech-probe" {
        return None;
    }
    let result: Result<(), ()> = (|| {
        let directory = args.next().ok_or(())?;
        if args.next().is_some() {
            return Err(());
        }
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            native::run(mode == "--lyn-speech-probe", Path::new(&directory))
        }
        #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
        {
            let _ = directory;
            Err(())
        }
    })();
    Some(if result.is_ok() {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    })
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod native {
    use super::*;
    use std::{
        ffi::{CString, c_char, c_int, c_void},
        fs::File,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::ffi::OsStrExt,
        },
    };

    type Load = unsafe extern "C" fn(*const c_char) -> *mut c_void;
    type Free = unsafe extern "C" fn(*mut c_void);
    type Transcribe = unsafe extern "C" fn(*mut c_void, *const c_char, c_int) -> *mut c_char;
    type FreeString = unsafe extern "C" fn(*mut c_char);

    struct Engine {
        library: *mut c_void,
        context: *mut c_void,
        free: Free,
        transcribe: Transcribe,
        free_string: FreeString,
    }
    impl Drop for Engine {
        fn drop(&mut self) {
            // Pinned C API: free context before unloading its functions.
            unsafe {
                (self.free)(self.context);
                libc::dlclose(self.library);
            }
        }
    }
    unsafe fn symbol<T: Copy>(library: *mut c_void, name: &'static [u8]) -> Result<T, ()> {
        // Call sites use only signatures from the pinned v0.6.1 header (ABI 10).
        let address = unsafe { libc::dlsym(library, name.as_ptr().cast()) };
        if address.is_null() || std::mem::size_of::<T>() != std::mem::size_of_val(&address) {
            return Err(());
        }
        Ok(unsafe { std::mem::transmute_copy(&address) })
    }

    impl Engine {
        fn open(directory: &Path, model: Option<&Path>) -> Result<Self, ()> {
            let path = CString::new(directory.join("libparakeet.so").as_os_str().as_bytes())
                .map_err(|_| ())?;
            // Exact digest-verified library, RTLD_LOCAL; no PATH or system-library fallback.
            let library = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
            if library.is_null() {
                return Err(());
            }
            let result = (|| unsafe {
                let abi: unsafe extern "C" fn() -> c_int =
                    symbol(library, b"parakeet_capi_abi_version\0")?;
                if abi() != 10 {
                    return Err(());
                }
                let free = symbol(library, b"parakeet_capi_free\0")?;
                let transcribe = symbol(library, b"parakeet_capi_transcribe_path\0")?;
                let free_string = symbol(library, b"parakeet_capi_free_string\0")?;
                let load: Load = symbol(library, b"parakeet_capi_load\0")?;
                // v0.6.1's flat API ignores threads_each for a single backend.
                // These two exact Linux Itanium symbols are pinned with the artifact;
                // refuse if either is absent or the requested budget is not effective.
                let set_threads: unsafe extern "C" fn(c_int) =
                    symbol(library, b"_ZN2pk15set_num_threadsEi\0")?;
                let effective_threads: unsafe extern "C" fn() -> c_int =
                    symbol(library, b"_ZN2pk17effective_threadsEv\0")?;
                let count = std::thread::available_parallelism()
                    .map(|n| n.get().min(4))
                    .unwrap_or(1) as c_int;
                set_threads(count);
                if effective_threads() != count {
                    return Err(());
                }
                let model = model
                    .map(|path| CString::new(path.as_os_str().as_bytes()))
                    .transpose()
                    .map_err(|_| ())?;
                let mut engine = Engine {
                    library,
                    context: std::ptr::null_mut(),
                    free,
                    transcribe,
                    free_string,
                };
                if let Some(model) = model {
                    engine.context = load(model.as_ptr());
                    if engine.context.is_null() {
                        return Ok((engine, false));
                    }
                }
                Ok((engine, true))
            })();
            match result {
                Ok((engine, true)) => Ok(engine),
                Ok((_, false)) => Err(()), // owning Engine performs cleanup
                Err(()) => {
                    unsafe {
                        libc::dlclose(library);
                    }
                    Err(())
                }
            }
        }
        fn transcribe(&mut self, audio: &Path) -> Result<Option<String>, ()> {
            let audio = CString::new(audio.as_os_str().as_bytes()).map_err(|_| ())?;
            // This context is single-thread owned. Returned allocation belongs to the C API.
            let text = unsafe { (self.transcribe)(self.context, audio.as_ptr(), 0) };
            if text.is_null() {
                return Err(());
            }
            let length = unsafe { libc::strnlen(text, MAX_FRAME) };
            let result = if length >= MAX_FRAME {
                Err(())
            } else {
                let bytes = unsafe { std::slice::from_raw_parts(text.cast::<u8>(), length) };
                std::str::from_utf8(bytes)
                    .map(|text| Some(text.to_owned()))
                    .map_err(|_| ())
            };
            unsafe {
                (self.free_string)(text);
            }
            result
        }
    }

    pub(super) fn run(probe: bool, directory: &Path) -> Result<(), ()> {
        // No daemon survives the application. Fence the parent-death race around prctl.
        let parent = unsafe { libc::getppid() };
        if parent <= 1
            || unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) } != 0
            || unsafe { libc::getppid() } != parent
        {
            return Err(());
        }
        // Keep protocol output on a private duplicate; silence all native stdout/stderr.
        let output = unsafe { libc::fcntl(libc::STDOUT_FILENO, libc::F_DUPFD_CLOEXEC, 3) };
        if output < 0 {
            return Err(());
        }
        let mut output = unsafe { File::from_raw_fd(output) };
        let null = File::options()
            .read(true)
            .write(true)
            .open("/dev/null")
            .map_err(|_| ())?;
        if unsafe { libc::dup2(null.as_raw_fd(), libc::STDOUT_FILENO) } < 0
            || unsafe { libc::dup2(null.as_raw_fd(), libc::STDERR_FILENO) } < 0
        {
            return Err(());
        }
        if probe {
            Engine::open(directory, None)?;
            return Ok(());
        }
        let paths = crate::intelligence::model::SpeechPaths::new(directory);
        if !paths.installed_valid() {
            return Err(());
        }
        let mut engine = Engine::open(
            &paths.active_package().join("engine"),
            Some(&paths.model_path()),
        )?;
        let mut input = std::io::stdin().lock();
        loop {
            // A separate parent reaper also enforces idleness. This guard covers a stalled parent.
            let mut poll = libc::pollfd {
                fd: libc::STDIN_FILENO,
                events: libc::POLLIN,
                revents: 0,
            };
            if unsafe { libc::poll(&mut poll, 1, IDLE_TIMEOUT.as_millis() as c_int) } <= 0 {
                return Ok(());
            }
            let request = read_frame(&mut input, 4096).map_err(|_| ())?;
            let stored: String = serde_json::from_slice(&request).map_err(|_| ())?;
            // Parent supplies a database-resolved path; recheck containment before native access.
            let audio = Path::new(&stored).canonicalize().map_err(|_| ())?;
            let root = directory
                .join("media/audio")
                .canonicalize()
                .map_err(|_| ())?;
            if !audio.starts_with(root) || audio.extension().and_then(|s| s.to_str()) != Some("wav")
            {
                return Err(());
            }
            let result = engine.transcribe(&audio);
            let reply = Reply {
                failed: result.is_err(),
                text: result.unwrap_or(None),
            };
            let bytes = serde_json::to_vec(&reply).map_err(|_| ())?;
            write_frame(&mut output, &bytes).map_err(|_| ())?;
            if reply.failed {
                return Err(());
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn pinned_native_signatures_set_the_budget_and_reuse_one_context() {
            let directory = tempfile::tempdir().unwrap();
            let source = directory.path().join("fixture.c");
            std::fs::write(
                &source,
                r#"
                #include <stdlib.h>
                #include <stdio.h>
                #include <string.h>
                static int budget = 8, calls = 0;
                void set_threads(int n) __asm__("_ZN2pk15set_num_threadsEi");
                int effective_threads(void) __asm__("_ZN2pk17effective_threadsEv");
                void set_threads(int n) { budget = n; }
                int effective_threads(void) { return budget; }
                int parakeet_capi_abi_version(void) { return 10; }
                void *parakeet_capi_load(const char *path) { return malloc(1); }
                void parakeet_capi_free(void *ctx) { free(ctx); }
                void parakeet_capi_free_string(char *text) { free(text); }
                char *parakeet_capi_transcribe_path(void *ctx, const char *path, int decoder) {
                    if (!ctx || budget > 4 || budget < 1) return NULL;
                    char text[64]; snprintf(text, sizeof(text), "fixture call %d", ++calls);
                    return strdup(text);
                }
            "#,
            )
            .unwrap();
            let status = Command::new("cc")
                .args(["-shared", "-fPIC", "-o"])
                .arg(directory.path().join("libparakeet.so"))
                .arg(&source)
                .status()
                .unwrap();
            assert!(status.success());
            let mut engine =
                Engine::open(directory.path(), Some(Path::new("fixture.gguf"))).unwrap();
            assert_eq!(
                engine.transcribe(Path::new("one.wav")).unwrap(),
                Some("fixture call 1".into())
            );
            assert_eq!(
                engine.transcribe(Path::new("two.wav")).unwrap(),
                Some("fixture call 2".into())
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(body: &str) -> SpeechWorker {
        let mut command = Command::new("python3");
        command.args(["-u", "-c", body]);
        SpeechWorker::spawn(&mut command).unwrap()
    }
    #[test]
    fn framed_responses_are_bounded_and_require_complete_data() {
        assert!(read_frame(&mut &u32::MAX.to_le_bytes()[..], MAX_FRAME).is_err());
        assert!(read_frame(&mut &[3, 0, 0, 0, 1][..], MAX_FRAME).is_err());
        let mut bytes = Vec::new();
        write_frame(&mut bytes, b"hello").unwrap();
        assert_eq!(
            read_frame(&mut bytes.as_slice(), MAX_FRAME).unwrap(),
            b"hello"
        );
    }
    #[test]
    fn nearby_jobs_reuse_the_same_process_and_context() {
        let mut worker = fixture(
            "import sys,struct,json\nn=0\nwhile True:\n h=sys.stdin.buffer.read(4)\n if not h: break\n sys.stdin.buffer.read(struct.unpack('<I',h)[0]); n+=1\n b=json.dumps({'text':str(n),'failed':False}).encode();sys.stdout.buffer.write(struct.pack('<I',len(b))+b);sys.stdout.buffer.flush()",
        );
        let cancelled = AtomicBool::new(false);
        let pid = worker.child.id();
        assert_eq!(
            worker
                .transcribe(Path::new("note.wav"), &cancelled)
                .unwrap(),
            Some("1".into())
        );
        assert_eq!(
            worker
                .transcribe(Path::new("note.wav"), &cancelled)
                .unwrap(),
            Some("2".into())
        );
        assert_eq!(worker.child.id(), pid);
    }
    #[test]
    fn timeout_cancellation_and_crash_refuse_and_child_is_reaped() {
        let mut worker = fixture("import time;time.sleep(10)");
        assert_eq!(
            worker.transcribe_with_deadline(
                Path::new("note.wav"),
                &AtomicBool::new(false),
                Instant::now() + Duration::from_millis(50)
            ),
            Err("ENRICHMENT_TIMEOUT")
        );
        let now = Instant::now();
        drop(worker);
        assert!(now.elapsed() < Duration::from_secs(2));
        let mut worker = fixture("import time;time.sleep(10)");
        assert_eq!(
            worker.transcribe(Path::new("note.wav"), &AtomicBool::new(true)),
            Err("ENRICHMENT_CANCELLED")
        );
        drop(worker);
        let mut worker = fixture("raise SystemExit(1)");
        assert_eq!(
            worker.transcribe(Path::new("note.wav"), &AtomicBool::new(false)),
            Err("ENRICHMENT_FAILED")
        );
    }
}
