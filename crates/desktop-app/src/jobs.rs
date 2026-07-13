use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
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
    Failed { generation: u64, error: RenderError },
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
    jobs: Option<Sender<RenderJob>>,
    results: Receiver<RenderEvent>,
    thread: Option<JoinHandle<()>>,
    shutdown: Arc<AtomicBool>,
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
        let (job_sender, job_receiver) = mpsc::channel::<RenderJob>();
        let (result_sender, result_receiver) = mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let thread_shutdown = Arc::clone(&shutdown);
        let thread = thread::Builder::new()
            .name("depthsprite-render".to_owned())
            .spawn(move || render_loop(job_receiver, result_sender, thread_shutdown, renderer))
            .expect("render worker thread must start");
        Self {
            jobs: Some(job_sender),
            results: result_receiver,
            thread: Some(thread),
            shutdown,
        }
    }

    pub(crate) fn submit(
        &self,
        generation: u64,
        model: DepthSpriteModel,
        request: RenderRequest,
    ) -> Result<(), RenderWorkerError> {
        self.jobs
            .as_ref()
            .ok_or(RenderWorkerError::ShutDown)?
            .send(RenderJob {
                generation,
                model,
                request,
            })
            .map_err(|_| RenderWorkerError::ShutDown)
    }

    pub(crate) fn try_recv(&self) -> Result<RenderEvent, mpsc::TryRecvError> {
        self.results.try_recv()
    }

    #[cfg(test)]
    pub(crate) fn recv_timeout(&self, timeout: Duration) -> Result<RenderEvent, RecvTimeoutError> {
        self.results.recv_timeout(timeout)
    }

    pub(crate) fn shutdown(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        self.jobs.take();
        if let Some(thread) = self.thread.take() {
            thread.join().expect("render worker panicked");
        }
    }

    #[cfg(test)]
    fn shutdown_signal_for_test(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.shutdown)
    }
}

impl Drop for RenderWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn render_loop(
    jobs: Receiver<RenderJob>,
    results: Sender<RenderEvent>,
    shutdown: Arc<AtomicBool>,
    renderer: Renderer,
) {
    for job in jobs {
        if shutdown.load(Ordering::Acquire) {
            break;
        }
        let event = match renderer(&job.model, &job.request) {
            Ok(frame) => RenderEvent::Complete(RenderResult {
                generation: job.generation,
                frame,
            }),
            Err(error) => RenderEvent::Failed {
                generation: job.generation,
                error,
            },
        };
        if results.send(event).is_err() {
            break;
        }
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
        let shutdown_requested = worker.shutdown_signal_for_test();
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
        while !shutdown_requested.load(Ordering::SeqCst) {
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
}
