//! One background writer, with the latest pending snapshot per document.
//! Shutdown flushes every pending document before joining the worker.
use crate::{document::Document, write_recovery_document};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Condvar, Mutex},
    thread::JoinHandle,
    time::Duration,
};

#[derive(Default)]
struct State {
    pending: HashMap<PathBuf, Document>,
    in_flight: HashSet<PathBuf>,
    discarded: HashSet<PathBuf>,
    shutdown: bool,
    error: Option<String>,
}
#[derive(Default)]
pub struct RecoveryWriter {
    shared: Arc<(Mutex<State>, Condvar)>,
    worker: Option<JoinHandle<()>>,
}
impl RecoveryWriter {
    pub fn submit(&mut self, path: PathBuf, document: Document) {
        if self.worker.is_none() {
            let shared = self.shared.clone();
            self.worker = Some(std::thread::spawn(move || {
                let (lock, wake) = &*shared;
                let mut cache = crate::fla_zip::RecoveryCompression::default();
                loop {
                    let mut state = lock.lock().unwrap();
                    while state.pending.is_empty() && !state.shutdown {
                        state = wake.wait(state).unwrap();
                    }
                    if !state.shutdown {
                        // A bounded delay groups edits without postponing recovery
                        // indefinitely during continuous drawing.
                        state = wake
                            .wait_timeout_while(state, Duration::from_millis(250), |state| {
                                !state.shutdown
                            })
                            .unwrap()
                            .0;
                    }
                    let pending = std::mem::take(&mut state.pending);
                    state.in_flight.extend(pending.keys().cloned());
                    let shutdown = state.shutdown;
                    drop(state);
                    for (path, doc) in pending {
                        let result = path
                            .parent()
                            .map_or(Ok(()), std::fs::create_dir_all)
                            .map_err(|e| e.to_string())
                            .and_then(|()| {
                                write_recovery_document(
                                    &path,
                                    &doc,
                                    Some(&mut cache),
                                    |temporary, destination| {
                                        let state = lock.lock().unwrap();
                                        if state.discarded.contains(destination) {
                                            return Ok(());
                                        }
                                        std::fs::rename(temporary, destination)
                                    },
                                )
                            });
                        let mut state = lock.lock().unwrap();
                        state.in_flight.remove(&path);
                        if !state.discarded.remove(&path)
                            && let Err(error) = result
                        {
                            state.error = Some(error);
                        }
                    }
                    if shutdown {
                        break;
                    }
                }
            }));
        }
        self.shared.0.lock().unwrap().pending.insert(path, document);
        self.shared.1.notify_one();
    }
    pub(crate) fn discard(&self, path: &std::path::Path) {
        let mut state = self.shared.0.lock().unwrap();
        state.pending.remove(path);
        if state.in_flight.contains(path) {
            state.discarded.insert(path.to_owned());
        }
        if let Err(error) = std::fs::remove_file(path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            state.error = Some(error.to_string());
        }
    }

    pub fn take_error(&self) -> Option<String> {
        self.shared.0.lock().unwrap().error.take()
    }
}
impl Drop for RecoveryWriter {
    fn drop(&mut self) {
        self.shared.0.lock().unwrap().shutdown = true;
        self.shared.1.notify_one();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closing_flushes_latest_snapshot_and_preserves_previous_state() {
        let path = std::env::temp_dir().join(format!(
            "libretoon-recovery-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&path, b"previous recovery").unwrap();
        let mut doc = Document::default();
        {
            let mut writer = RecoveryWriter::default();
            writer.submit(path.clone(), doc.clone());
            for fps in 1..=60 {
                doc.fps = fps;
                writer.submit(path.clone(), doc.clone());
            }
        }
        let restored: Document = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(restored.fps, 60);
        assert!(restored.validate().is_ok());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn closing_flushes_recovery_for_each_open_project() {
        let dir =
            std::env::temp_dir().join(format!("libretoon-recovery-tabs-{}", crate::features::id()));
        let a = dir.join("a.ltoon");
        let b = dir.join("b.ltoon");
        {
            let mut writer = RecoveryWriter::default();
            let mut doc = Document::default();
            writer.submit(a.clone(), doc.clone());
            doc.fps = 30;
            writer.submit(b.clone(), doc.clone());
            doc.fps = 48;
            writer.submit(a.clone(), doc);
        }
        assert_eq!(crate::documents::read_project(&a).unwrap().fps, 48);
        assert_eq!(crate::documents::read_project(&b).unwrap().fps, 30);
        std::fs::remove_file(a).unwrap();
        std::fs::remove_file(b).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn discard_removes_published_and_queued_recovery_before_shutdown() {
        let path = std::env::temp_dir().join(format!("discarded-{}.ltoon", crate::features::id()));
        std::fs::write(&path, b"old recovery").unwrap();
        let mut writer = RecoveryWriter::default();
        writer.submit(path.clone(), Document::default());
        writer.discard(&path);
        drop(writer);
        assert!(!path.exists());
    }

    #[test]
    fn write_failures_are_reported() {
        let mut writer = RecoveryWriter::default();
        // A child of a file cannot be created as a directory.
        let parent =
            std::env::temp_dir().join(format!("libretoon-file-{}.json", std::process::id()));
        std::fs::write(&parent, b"test").unwrap();
        writer.submit(parent.join("recovery.json"), Document::default());
        writer.shared.0.lock().unwrap().shutdown = true;
        writer.shared.1.notify_one();
        writer.worker.take().unwrap().join().unwrap();
        assert!(writer.take_error().is_some());
        std::fs::remove_file(parent).unwrap();
    }
}
