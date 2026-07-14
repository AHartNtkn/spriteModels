use std::{
    any::Any,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Condvar, Mutex, MutexGuard, PoisonError,
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
};

#[cfg(test)]
use std::{sync::mpsc::RecvTimeoutError, time::Duration};

use depthsprite_format::DepthSpriteModel;
use relief_render::{FrameBuffer, RenderError, RenderRequest, render_model};
use thiserror::Error;

#[derive(Default)]
pub(crate) struct GenerationCounter {
    current: u64,
}

impl GenerationCounter {
    pub(crate) fn current(&self) -> u64 {
        self.current
    }

    pub(crate) fn advance(&mut self) -> u64 {
        self.current = self
            .current
            .checked_add(1)
            .expect("render generation exhausted u64");
        self.current
    }
}

pub(crate) struct RenderResult {
    pub(crate) generation: u64,
    pub(crate) frame: FrameBuffer,
}

pub(crate) enum RenderEvent {
    Complete(RenderResult),
    Failed { generation: u64, error: String },
}

pub(crate) fn install_if_current(
    current: u64,
    result: RenderResult,
    slot: &mut Option<FrameBuffer>,
) -> bool {
    if result.generation != current {
        return false;
    }
    *slot = Some(result.frame);
    true
}

struct RenderJob {
    generation: u64,
    model: DepthSpriteModel,
    request: RenderRequest,
}

type Renderer = Arc<
    dyn Fn(&DepthSpriteModel, &RenderRequest) -> Result<FrameBuffer, RenderError>
        + Send
        + Sync
        + 'static,
>;

#[derive(Debug, Error)]
pub(crate) enum RenderWorkerError {
    #[error("render worker has shut down")]
    ShutDown,
}

pub(crate) struct RenderWorker {
    queue: Arc<RenderQueue>,
    results: Receiver<RenderEvent>,
    thread: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct QueueState {
    latest: Option<RenderJob>,
    shutdown: bool,
}

#[derive(Default)]
struct RenderQueue {
    state: Mutex<QueueState>,
    changed: Condvar,
}

impl RenderWorker {
    pub(crate) fn new() -> Self {
        Self::with_renderer(Arc::new(|model, request| {
            render_model(model.charts(), request)
        }))
    }

    #[cfg(test)]
    fn new_with_renderer(
        renderer: impl Fn(&DepthSpriteModel, &RenderRequest) -> Result<FrameBuffer, RenderError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self::with_renderer(Arc::new(renderer))
    }

    fn with_renderer(renderer: Renderer) -> Self {
        let (result_sender, result_receiver) = mpsc::channel();
        let queue = Arc::new(RenderQueue::default());
        let thread_queue = Arc::clone(&queue);
        let thread = thread::Builder::new()
            .name("depthsprite-render".to_owned())
            .spawn(move || render_loop(thread_queue, result_sender, renderer))
            .ok();
        if thread.is_none() {
            lock_queue(&queue).shutdown = true;
        }
        Self {
            queue,
            results: result_receiver,
            thread,
        }
    }

    pub(crate) fn submit(
        &self,
        generation: u64,
        model: DepthSpriteModel,
        request: RenderRequest,
    ) -> Result<(), RenderWorkerError> {
        let mut state = lock_queue(&self.queue);
        if state.shutdown {
            return Err(RenderWorkerError::ShutDown);
        }
        state.latest = Some(RenderJob {
            generation,
            model,
            request,
        });
        drop(state);
        self.queue.changed.notify_one();
        Ok(())
    }

    pub(crate) fn try_recv(&self) -> Result<RenderEvent, mpsc::TryRecvError> {
        self.results.try_recv()
    }

    #[cfg(test)]
    pub(crate) fn recv_timeout(&self, timeout: Duration) -> Result<RenderEvent, RecvTimeoutError> {
        self.results.recv_timeout(timeout)
    }

    pub(crate) fn shutdown(&mut self) {
        {
            let mut state = lock_queue(&self.queue);
            state.shutdown = true;
            state.latest = None;
        }
        self.queue.changed.notify_all();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for RenderWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn render_loop(queue: Arc<RenderQueue>, results: Sender<RenderEvent>, renderer: Renderer) {
    loop {
        let Some(job) = take_next_job(&queue) else {
            break;
        };
        let event = match catch_unwind(AssertUnwindSafe(|| renderer(&job.model, &job.request))) {
            Ok(Ok(frame)) => RenderEvent::Complete(RenderResult {
                generation: job.generation,
                frame,
            }),
            Ok(Err(error)) => RenderEvent::Failed {
                generation: job.generation,
                error: error.to_string(),
            },
            Err(payload) => RenderEvent::Failed {
                generation: job.generation,
                error: format!("preview renderer panicked: {}", panic_message(payload)),
            },
        };
        if results.send(event).is_err() {
            break;
        }
    }
}

fn lock_queue(queue: &RenderQueue) -> MutexGuard<'_, QueueState> {
    queue.state.lock().unwrap_or_else(PoisonError::into_inner)
}

fn take_next_job(queue: &RenderQueue) -> Option<RenderJob> {
    let mut state = lock_queue(queue);
    loop {
        if state.shutdown {
            return None;
        }
        if let Some(job) = state.latest.take() {
            return Some(job);
        }
        state = queue
            .changed
            .wait(state)
            .unwrap_or_else(PoisonError::into_inner);
    }
}

fn panic_message(payload: Box<dyn Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic payload".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::{
            Arc, Condvar, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use depthsprite_format::load_path;
    use relief_render::{RenderRequest, TargetView, render_model};

    use super::{GenerationCounter, RenderEvent, RenderResult, RenderWorker, install_if_current};

    fn asset(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("assets/examples")
            .join(name)
    }

    fn transparent_frame() -> relief_render::FrameBuffer {
        render_model(&[], &RenderRequest::new(1, 1, TargetView::front_v1())).unwrap()
    }

    #[test]
    fn generation_ids_are_strictly_monotonic() {
        let mut generations = GenerationCounter::default();

        assert_eq!(generations.current(), 0);
        assert_eq!(generations.advance(), 1);
        assert_eq!(generations.advance(), 2);
        assert_eq!(generations.current(), 2);
    }

    #[test]
    fn stale_results_are_rejected_and_current_results_install() {
        let mut slot = None;

        assert!(!install_if_current(
            2,
            RenderResult {
                generation: 1,
                frame: transparent_frame(),
            },
            &mut slot,
        ));
        assert!(slot.is_none());

        assert!(install_if_current(
            2,
            RenderResult {
                generation: 2,
                frame: transparent_frame(),
            },
            &mut slot,
        ));
        assert!(slot.is_some());
    }

    #[test]
    fn worker_renders_owned_snapshot_and_tags_result() {
        let model = load_path(asset("block.depthsprite")).unwrap();
        let mut worker = RenderWorker::new();
        worker
            .submit(
                41,
                model.clone(),
                RenderRequest::new(16, 16, TargetView::isometric_v1()),
            )
            .unwrap();
        drop(model);

        let event = worker.recv_timeout(Duration::from_secs(10)).unwrap();
        let RenderEvent::Complete(result) = event else {
            panic!("expected completed render");
        };
        assert_eq!(result.generation, 41);
        assert_eq!((result.frame.width(), result.frame.height()), (16, 16));

        worker.shutdown();
    }

    #[test]
    fn shutdown_finishes_active_render_without_draining_stale_queue() {
        let calls = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new((Mutex::new((false, false)), Condvar::new()));
        let render_calls = Arc::clone(&calls);
        let render_gate = Arc::clone(&gate);
        let mut worker = RenderWorker::new_with_renderer(move |_model, _request| {
            let call = render_calls.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                let (lock, changed) = &*render_gate;
                let mut state = lock.lock().unwrap();
                state.0 = true;
                changed.notify_all();
                while !state.1 {
                    state = changed.wait(state).unwrap();
                }
            }
            Ok(transparent_frame())
        });
        let shutdown_queue = Arc::clone(&worker.queue);
        let model = load_path(asset("block.depthsprite")).unwrap();
        for generation in [1, 2] {
            worker
                .submit(
                    generation,
                    model.clone(),
                    RenderRequest::new(1, 1, TargetView::front_v1()),
                )
                .unwrap();
        }
        {
            let (lock, changed) = &*gate;
            let mut state = lock.lock().unwrap();
            while !state.0 {
                state = changed.wait(state).unwrap();
            }
        }

        let shutdown_thread = std::thread::spawn(move || worker.shutdown());
        while !super::lock_queue(&shutdown_queue).shutdown {
            std::thread::yield_now();
        }
        {
            let (lock, changed) = &*gate;
            let mut state = lock.lock().unwrap();
            state.1 = true;
            changed.notify_all();
        }
        shutdown_thread.join().unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn rapid_submits_replace_intermediate_queued_job_with_newest() {
        let gate = Arc::new((Mutex::new((false, false)), Condvar::new()));
        let render_gate = Arc::clone(&gate);
        let mut worker = RenderWorker::new_with_renderer(move |_model, request| {
            let frame = render_model(&[], request).unwrap();
            let (lock, changed) = &*render_gate;
            let mut state = lock.lock().unwrap();
            if !state.0 {
                state.0 = true;
                changed.notify_all();
                while !state.1 {
                    state = changed.wait(state).unwrap();
                }
            }
            Ok(frame)
        });
        let model = load_path(asset("block.depthsprite")).unwrap();
        worker
            .submit(
                1,
                model.clone(),
                RenderRequest::new(1, 1, TargetView::front_v1()),
            )
            .unwrap();
        {
            let (lock, changed) = &*gate;
            let mut state = lock.lock().unwrap();
            while !state.0 {
                state = changed.wait(state).unwrap();
            }
        }
        for (generation, side) in [(2, 2), (3, 3)] {
            worker
                .submit(
                    generation,
                    model.clone(),
                    RenderRequest::new(side, side, TargetView::front_v1()),
                )
                .unwrap();
        }
        {
            let (lock, changed) = &*gate;
            let mut state = lock.lock().unwrap();
            state.1 = true;
            changed.notify_all();
        }

        let first = worker.recv_timeout(Duration::from_secs(2)).unwrap();
        let newest = worker.recv_timeout(Duration::from_secs(2)).unwrap();
        let RenderEvent::Complete(first) = first else {
            panic!("expected first render");
        };
        let RenderEvent::Complete(newest) = newest else {
            panic!("expected newest render");
        };
        assert_eq!((first.generation, first.frame.width()), (1, 1));
        assert_eq!((newest.generation, newest.frame.width()), (3, 3));
        assert!(worker.recv_timeout(Duration::from_millis(50)).is_err());
        worker.shutdown();
    }

    #[test]
    fn renderer_panic_is_a_tagged_failure_and_shutdown_does_not_panic() {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut worker = RenderWorker::new_with_renderer(|_model, _request| {
                panic!("injected renderer panic")
            });
            let model = load_path(asset("block.depthsprite")).unwrap();
            worker
                .submit(19, model, RenderRequest::new(1, 1, TargetView::front_v1()))
                .unwrap();
            let event = worker.recv_timeout(Duration::from_secs(2)).unwrap();
            let RenderEvent::Failed { generation, error } = event else {
                panic!("expected controlled render failure");
            };
            assert_eq!(generation, 19);
            assert!(error.to_string().contains("panicked"));
            worker.shutdown();
        }));

        assert!(outcome.is_ok());
    }
}
