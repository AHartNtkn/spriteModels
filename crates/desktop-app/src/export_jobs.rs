use std::{
    any::Any,
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
};

#[cfg(test)]
use std::{sync::mpsc::RecvTimeoutError, time::Duration};

use desktop_app::document::Document;
use relief_render::SheetRequest;
use thiserror::Error;

struct ExportJob {
    tag: u64,
    document: Document,
    request: SheetRequest,
    path: PathBuf,
}

type Exporter =
    Arc<dyn Fn(&Document, &SheetRequest, &Path) -> Result<(), String> + Send + Sync + 'static>;

pub(crate) enum ExportEvent {
    Complete {
        tag: u64,
        path: PathBuf,
    },
    Failed {
        tag: u64,
        path: PathBuf,
        error: String,
    },
}

#[derive(Debug, Error)]
pub(crate) enum ExportWorkerError {
    #[error("an export is already running")]
    Busy,
    #[error("export worker has shut down")]
    ShutDown,
}

pub(crate) struct ExportWorker {
    jobs: Option<Sender<ExportJob>>,
    results: Receiver<ExportEvent>,
    busy: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ExportWorker {
    pub(crate) fn new() -> Self {
        Self::with_exporter(Arc::new(|document, request, path| {
            document
                .export_sheet(path, request)
                .map_err(|error| error.to_string())
        }))
    }

    #[cfg(test)]
    fn new_with_exporter(
        exporter: impl Fn(&Document, &SheetRequest, &Path) -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        Self::with_exporter(Arc::new(exporter))
    }

    fn with_exporter(exporter: Exporter) -> Self {
        let (job_sender, job_receiver) = mpsc::channel();
        let (result_sender, result_receiver) = mpsc::channel();
        let busy = Arc::new(AtomicBool::new(false));
        let thread_busy = Arc::clone(&busy);
        let thread = thread::Builder::new()
            .name("depthsprite-export".to_owned())
            .spawn(move || export_loop(job_receiver, result_sender, thread_busy, exporter))
            .ok();
        Self {
            jobs: Some(job_sender),
            results: result_receiver,
            busy,
            thread,
        }
    }

    pub(crate) fn submit(
        &self,
        tag: u64,
        document: Document,
        request: SheetRequest,
        path: PathBuf,
    ) -> Result<(), ExportWorkerError> {
        self.busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| ExportWorkerError::Busy)?;
        let result = self
            .jobs
            .as_ref()
            .ok_or(ExportWorkerError::ShutDown)
            .and_then(|jobs| {
                jobs.send(ExportJob {
                    tag,
                    document,
                    request,
                    path,
                })
                .map_err(|_| ExportWorkerError::ShutDown)
            });
        if result.is_err() {
            self.busy.store(false, Ordering::Release);
        }
        result
    }

    pub(crate) fn try_recv(&self) -> Result<ExportEvent, mpsc::TryRecvError> {
        self.results.try_recv()
    }

    #[cfg(test)]
    fn recv_timeout(&self, timeout: Duration) -> Result<ExportEvent, RecvTimeoutError> {
        self.results.recv_timeout(timeout)
    }

    pub(crate) fn shutdown(&mut self) {
        self.jobs.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for ExportWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn export_loop(
    jobs: Receiver<ExportJob>,
    results: Sender<ExportEvent>,
    busy: Arc<AtomicBool>,
    exporter: Exporter,
) {
    for job in jobs {
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            exporter(&job.document, &job.request, &job.path)
        }));
        let event = match outcome {
            Ok(Ok(())) => ExportEvent::Complete {
                tag: job.tag,
                path: job.path,
            },
            Ok(Err(error)) => ExportEvent::Failed {
                tag: job.tag,
                path: job.path,
                error,
            },
            Err(payload) => ExportEvent::Failed {
                tag: job.tag,
                path: job.path,
                error: format!("export worker panicked: {}", panic_message(payload)),
            },
        };
        busy.store(false, Ordering::Release);
        if results.send(event).is_err() {
            break;
        }
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
        sync::{Arc, Condvar, Mutex, mpsc},
        time::Duration,
    };

    use desktop_app::document::Document;
    use relief_render::{DirectionCount, SheetRequest};
    use tempfile::tempdir;

    use super::{ExportEvent, ExportWorker, ExportWorkerError};

    fn asset(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("assets/examples")
            .join(name)
    }

    fn request() -> SheetRequest {
        SheetRequest::new(DirectionCount::Eight, 1, 0, 1).unwrap()
    }

    #[test]
    fn worker_exports_owned_document_on_background_thread_and_tags_success() {
        let temp = tempdir().unwrap();
        let destination = temp.path().join("sheet.png");
        let document = Document::open(asset("block.depthsprite")).unwrap();
        let expected_hash = document.model_hash();
        let main_thread = std::thread::current().id();
        let (observed_sender, observed_receiver) = mpsc::channel();
        let mut worker = ExportWorker::new_with_exporter(move |document, request, path| {
            observed_sender
                .send((std::thread::current().id(), document.model_hash()))
                .unwrap();
            document
                .export_sheet(path, request)
                .map_err(|error| error.to_string())
        });

        worker
            .submit(7, document, request(), destination.clone())
            .unwrap();
        let event = worker.recv_timeout(Duration::from_secs(30)).unwrap();

        let (worker_thread, model_hash) = observed_receiver.recv().unwrap();
        assert_eq!(model_hash, expected_hash);
        assert_ne!(worker_thread, main_thread);
        assert!(matches!(
            event,
            ExportEvent::Complete { tag: 7, ref path, .. } if path == &destination
        ));
        assert!(destination.metadata().unwrap().len() > 0);
        worker.shutdown();
    }

    #[test]
    fn worker_rejects_concurrent_export_until_active_job_finishes() {
        let gate = Arc::new((Mutex::new((false, false)), Condvar::new()));
        let export_gate = Arc::clone(&gate);
        let mut worker = ExportWorker::new_with_exporter(move |_document, _request, _path| {
            let (lock, changed) = &*export_gate;
            let mut state = lock.lock().unwrap();
            state.0 = true;
            changed.notify_all();
            while !state.1 {
                state = changed.wait(state).unwrap();
            }
            Ok(())
        });
        let document = Document::open(asset("block.depthsprite")).unwrap();
        worker
            .submit(1, document.clone(), request(), PathBuf::from("one.png"))
            .unwrap();
        {
            let (lock, changed) = &*gate;
            let mut state = lock.lock().unwrap();
            while !state.0 {
                state = changed.wait(state).unwrap();
            }
        }

        assert!(matches!(
            worker.submit(2, document, request(), PathBuf::from("two.png")),
            Err(ExportWorkerError::Busy)
        ));
        {
            let (lock, changed) = &*gate;
            let mut state = lock.lock().unwrap();
            state.1 = true;
            changed.notify_all();
        }
        assert!(matches!(
            worker.recv_timeout(Duration::from_secs(2)).unwrap(),
            ExportEvent::Complete { tag: 1, .. }
        ));
        worker.shutdown();
    }

    #[test]
    fn exporter_panic_becomes_tagged_failure_and_shutdown_does_not_panic() {
        let mut worker = ExportWorker::new_with_exporter(|_document, _request, _path| {
            panic!("injected exporter panic")
        });
        worker
            .submit(
                23,
                Document::open(asset("block.depthsprite")).unwrap(),
                request(),
                PathBuf::from("unused.png"),
            )
            .unwrap();

        let event = worker.recv_timeout(Duration::from_secs(2)).unwrap();
        let ExportEvent::Failed { tag, ref error, .. } = event else {
            panic!("expected controlled export failure");
        };
        assert_eq!(tag, 23);
        assert!(error.contains("panicked"));
        worker.shutdown();
    }
}
