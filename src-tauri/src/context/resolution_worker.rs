//! One running context job and one coalesced latest pending job.

use std::{
    io,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
};

type Job = Box<dyn FnOnce() + Send + 'static>;

#[derive(Default)]
struct State {
    pending: Option<Job>,
    stopped: bool,
}
#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    ready: Condvar,
}

pub(crate) struct ResolutionWorker {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl ResolutionWorker {
    pub(crate) fn start() -> io::Result<Self> {
        let shared = Arc::new(Shared::default());
        let state = shared.clone();
        let thread = thread::Builder::new()
            .name("lyn-context-resolution".into())
            .spawn(move || {
                loop {
                    let job = {
                        let Ok(mut pending) = state.state.lock() else {
                            return;
                        };
                        while pending.pending.is_none() && !pending.stopped {
                            let Ok(next) = state.ready.wait(pending) else {
                                return;
                            };
                            pending = next;
                        }
                        if pending.stopped {
                            return;
                        }
                        pending.pending.take()
                    };
                    // A failed job must not strand subsequent captures. No shared
                    // registry/session/queue guard is held while executing it.
                    if let Some(job) = job {
                        let _ = catch_unwind(AssertUnwindSafe(job));
                    }
                }
            })?;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }

    pub(crate) fn submit(&self, job: Job) -> bool {
        let retired = {
            let Ok(mut state) = self.shared.state.lock() else {
                return false;
            };
            if state.stopped {
                return false;
            }
            state.pending.replace(job)
        };
        self.shared.ready.notify_one();
        // Jobs may own app handles. Never destroy a retired job under the
        // queue lock (app-state destruction can itself stop this worker).
        drop(retired);
        true
    }

    fn shutdown(&self) {
        let retired = {
            let Ok(mut state) = self.shared.state.lock() else {
                return;
            };
            state.stopped = true;
            state.pending.take()
        };
        self.shared.ready.notify_all();
        drop(retired);
    }
}

impl Drop for ResolutionWorker {
    fn drop(&mut self) {
        self.shutdown();
        // Don't block the UI on an in-flight filesystem/Git operation at exit.
        // The single active job completes; the idle worker wakes and exits.
        self.thread.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{Arc, Mutex, mpsc},
        time::Duration,
    };

    #[test]
    fn latest_pending_replaces_old_work_without_blocking_submit() {
        let worker = ResolutionWorker::start().unwrap();
        let (started, wait_started) = mpsc::channel();
        let (release, wait_release) = mpsc::channel();
        let (finished, wait_finished) = mpsc::channel();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let first_seen = seen.clone();
        assert!(worker.submit(Box::new(move || {
            first_seen.lock().unwrap().push(1);
            started.send(()).unwrap();
            wait_release.recv_timeout(Duration::from_secs(2)).unwrap();
        })));
        wait_started.recv_timeout(Duration::from_secs(2)).unwrap();
        let second_seen = seen.clone();
        assert!(worker.submit(Box::new(move || second_seen.lock().unwrap().push(2))));
        let third_seen = seen.clone();
        assert!(worker.submit(Box::new(move || {
            third_seen.lock().unwrap().push(3);
            finished.send(()).unwrap();
        })));
        release.send(()).unwrap();
        wait_finished.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(*seen.lock().unwrap(), vec![1, 3]);
    }

    #[test]
    fn shutdown_refuses_new_work_and_wakes_idle_worker() {
        let mut worker = ResolutionWorker::start().unwrap();
        worker.shutdown();
        assert!(!worker.submit(Box::new(|| panic!("stopped work ran"))));
        worker.thread.take().unwrap().join().unwrap();
    }
}
