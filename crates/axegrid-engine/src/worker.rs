//! The engine on a thread of its own. Talking to a device means waiting for
//! it, so a front end with a window hands its commands to a [`Worker`] and
//! draws from the [`View`] the worker publishes after each one. A device
//! that answers slowly, or not at all, then costs the window nothing.

use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::{registry, CommandError, Engine, State};

/// What a front end draws: the editor's state as it was after the last
/// command, with everything that takes the engine to work out.
#[derive(Debug, Default)]
pub struct View {
    pub state: State,
    pub connected: bool,
    /// The selected block's controls; see [`Engine::editor`].
    pub editor: Value,
    pub tuner: Value,
    /// Choice lists of the selected block that are still to be read.
    pub lists_missing: usize,
    /// What Undo and Redo would act on.
    pub next_undo: Option<String>,
    pub next_redo: Option<String>,
    /// Commands that cannot run right now, with the reason.
    disabled: Vec<(&'static str, &'static str)>,
}

impl View {
    /// Why `id` could not run when this view was taken, or `None` if it could.
    pub fn disabled_reason(&self, id: &str) -> Option<&'static str> {
        self.disabled.iter().find(|(command, _)| *command == id).map(|(_, reason)| *reason)
    }
}

impl Engine {
    /// A copy of everything a front end shows.
    pub fn view(&self) -> View {
        View {
            state: self.state.clone(),
            connected: self.is_connected(),
            editor: self.editor(),
            tuner: self.tuner_json(),
            lists_missing: self.lists_missing(),
            next_undo: self.history.next_undo().map(str::to_owned),
            next_redo: self.history.next_redo().map(str::to_owned),
            disabled: registry().iter().filter_map(|c| Some((c.id, (c.enabled)(self).err()?))).collect(),
        }
    }
}

/// What runs after a job, once the view shows what the job did.
type Report = Box<dyn FnOnce() + Send>;
type Job = Box<dyn FnOnce(&mut Engine) -> Report + Send>;

struct Shared {
    view: Mutex<Arc<View>>,
    /// Jobs handed over and not yet finished.
    pending: AtomicUsize,
    /// When the job that is running now began.
    running_since: Mutex<Option<Instant>>,
}

/// A poisoned lock still holds what was last written, which is all these need.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Owns an [`Engine`] on its own thread and runs jobs on it, one at a
/// time, in the order they were handed over.
pub struct Worker {
    jobs: Sender<Job>,
    shared: Arc<Shared>,
}

impl Worker {
    /// Moves `engine` to a new thread. `wake` is called after every job,
    /// when a fresh view is ready, e.g. to repaint a window.
    pub fn spawn(engine: Engine, wake: impl Fn() + Send + 'static) -> io::Result<Self> {
        let shared = Arc::new(Shared {
            view: Mutex::new(Arc::new(engine.view())),
            pending: AtomicUsize::new(0),
            running_since: Mutex::new(None),
        });
        let (jobs, queue) = mpsc::channel::<Job>();
        let theirs = shared.clone();
        std::thread::Builder::new().name("device-session".into()).spawn(move || {
            let mut engine = engine;
            // Ends when the worker's owner drops it.
            for job in queue {
                *lock(&theirs.running_since) = Some(Instant::now());
                let report = job(&mut engine);
                *lock(&theirs.view) = Arc::new(engine.view());
                *lock(&theirs.running_since) = None;
                theirs.pending.fetch_sub(1, Ordering::SeqCst);
                report();
                wake();
            }
        })?;
        Ok(Self { jobs, shared })
    }

    /// Runs `job` on the engine's thread, then `done` with what it returned.
    /// By the time `done` runs, [`Self::view`] shows what the job changed.
    pub fn call<T: Send + 'static>(
        &self,
        job: impl FnOnce(&mut Engine) -> T + Send + 'static,
        done: impl FnOnce(T) + Send + 'static,
    ) {
        self.shared.pending.fetch_add(1, Ordering::SeqCst);
        let job: Job = Box::new(move |engine| {
            let outcome = job(engine);
            Box::new(move || done(outcome))
        });
        if self.jobs.send(job).is_err() {
            // The thread is gone; nothing will ever run this.
            self.shared.pending.fetch_sub(1, Ordering::SeqCst);
        }
    }

    /// Runs one command; see [`Engine::run`].
    pub fn run(
        &self,
        id: impl Into<String>,
        params: Value,
        done: impl FnOnce(Result<Value, CommandError>) + Send + 'static,
    ) {
        let id = id.into();
        self.call(move |engine| engine.run(&id, &params), done);
    }

    /// The state after the last finished job.
    pub fn view(&self) -> Arc<View> {
        lock(&self.shared.view).clone()
    }

    /// Jobs waiting or running.
    pub fn pending(&self) -> usize {
        self.shared.pending.load(Ordering::SeqCst)
    }

    /// How long the job that is running now has been at it.
    pub fn waiting_for(&self) -> Option<Duration> {
        lock(&self.shared.running_since).map(|since| since.elapsed())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use serde_json::json;

    const PATIENCE: Duration = Duration::from_secs(10);

    #[test]
    fn commands_run_in_order_and_the_view_follows() {
        let (woken, wakes) = mpsc::channel();
        let worker = Worker::spawn(Engine::new(), move || woken.send(()).unwrap()).unwrap();
        assert!(!worker.view().connected);
        assert_eq!(worker.view().disabled_reason("preset.next"), Some("no device connected"));

        let (tell, told) = mpsc::channel();
        let report = |tell: &Sender<Result<Value, CommandError>>| {
            let tell = tell.clone();
            move |outcome| tell.send(outcome).unwrap()
        };
        worker.run("device.connect_simulator", Value::Null, report(&tell));
        worker.run("preset.next", Value::Null, report(&tell));
        worker.run("preset.select", json!({ "number": 99 }), report(&tell));
        assert_eq!(told.recv_timeout(PATIENCE).unwrap().unwrap()["connected"], true);
        assert_eq!(told.recv_timeout(PATIENCE).unwrap().unwrap()["name"], "Crunch");
        assert_eq!(told.recv_timeout(PATIENCE).unwrap().unwrap_err().kind(), "device");
        // The view a report sees already holds what its command did.
        let view = worker.view();
        assert!(view.connected && view.state.preset.name == "Crunch");
        assert_eq!(view.disabled_reason("preset.next"), None);
        assert_eq!(view.disabled_reason("edit.undo"), Some("nothing to undo"));
        for _ in 0..3 {
            wakes.recv_timeout(PATIENCE).unwrap();
        }
        assert_eq!((worker.pending(), worker.waiting_for()), (0, None));
    }

    /// What a device that takes its time looks like from the window's side.
    #[test]
    fn a_slow_job_keeps_nobody_waiting() {
        let worker = Worker::spawn(Engine::new(), || {}).unwrap();
        let (started, has_started) = mpsc::channel();
        let (release, released) = mpsc::channel::<()>();
        let (tell, told) = mpsc::channel();
        worker.call(
            move |_| {
                started.send(()).unwrap();
                released.recv_timeout(PATIENCE).unwrap();
            },
            move |()| tell.send(()).unwrap(),
        );
        worker.run("device.connect_simulator", Value::Null, |_| {});
        has_started.recv_timeout(PATIENCE).unwrap();
        // The job is still at it, and the view and the counters answer at once.
        let asked = Instant::now();
        assert!(!worker.view().connected);
        assert_eq!(worker.pending(), 2);
        assert!(worker.waiting_for().is_some());
        assert!(asked.elapsed() < Duration::from_millis(500));
        release.send(()).unwrap();
        told.recv_timeout(PATIENCE).unwrap();
    }

    #[test]
    fn a_job_may_do_anything_with_the_engine() {
        let worker = Worker::spawn(Engine::new(), || {}).unwrap();
        let (tell, told) = mpsc::channel();
        worker.call(|engine| engine.inspect()["connected"].clone(), move |connected| tell.send(connected).unwrap());
        assert_eq!(told.recv_timeout(PATIENCE).unwrap(), false);
    }
}
