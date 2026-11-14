//! One reader, one replaceable pending request and one result. Imports never
//! block painting and repeated Open requests cannot spawn unbounded workers.
use crate::{Document, Editor};
use std::{
    path::PathBuf,
    sync::{Arc, Condvar, Mutex},
    thread::JoinHandle,
};
#[derive(Default)]
struct State {
    pending: Option<(u64, PathBuf, Option<RecoverySource>)>,
    result: Option<Loaded>,
    generation: u64,
    busy: bool,
    shutdown: bool,
}
struct RecoverySource {
    saved: Option<PathBuf>,
    stamp: Option<crate::project_save::Stamp>,
    dirty: bool,
}

type Loaded = (
    PathBuf,
    Result<Document, String>,
    Option<crate::project_save::Stamp>,
);
#[derive(Default)]
pub struct Loader {
    shared: Arc<(Mutex<State>, Condvar)>,
    worker: Option<JoinHandle<()>>,
    saved_copy: bool,
    restoring: bool,
}
impl Loader {
    pub fn request_saved_copy(&mut self, path: PathBuf) {
        self.request(path);
        self.saved_copy = true;
    }
    pub fn request(&mut self, path: PathBuf) {
        self.enqueue(path, None);
    }

    pub(crate) fn restore(
        &mut self,
        recovery: PathBuf,
        saved: Option<PathBuf>,
        stamp: Option<crate::project_save::Stamp>,
        dirty: bool,
    ) {
        self.enqueue(
            recovery,
            Some(RecoverySource {
                saved,
                stamp,
                dirty,
            }),
        );
    }

    pub(crate) fn cancel_restore(&mut self) {
        if self.restoring {
            let mut state = self.shared.0.lock().unwrap();
            state.generation = state.generation.wrapping_add(1);
            state.pending = None;
            state.result = None;
            state.busy = false;
            self.restoring = false;
        }
    }

    fn enqueue(&mut self, path: PathBuf, recovery: Option<RecoverySource>) {
        self.saved_copy = false;
        self.restoring = recovery.is_some();
        if self.worker.is_none() {
            let shared = self.shared.clone();
            self.worker = Some(std::thread::spawn(move || {
                let (lock, wake) = &*shared;
                loop {
                    let mut state = lock.lock().unwrap();
                    while state.pending.is_none() && !state.shutdown {
                        state = wake.wait(state).unwrap();
                    }
                    if state.shutdown {
                        break;
                    }
                    let (generation, path, recovery) = state.pending.take().unwrap();
                    drop(state);
                    let read_saved = |path: &std::path::Path| {
                        let stamp = crate::project_save::stamp(path);
                        let doc = crate::documents::read_project(path)?;
                        if stamp != crate::project_save::stamp(path) {
                            return Err(
                                "The file changed while it was being opened. Try opening it again."
                                    .into(),
                            );
                        }
                        Ok((doc, stamp))
                    };
                    let result = std::panic::catch_unwind(|| {
                        if let Some(recovery) = recovery {
                            let saved = || {
                                read_saved(
                                    recovery
                                        .saved
                                        .as_deref()
                                        .ok_or("No se encuentra el proyecto")?,
                                )
                            };
                            let recovered = || {
                                crate::documents::read_project(&path)
                                    .map(|doc| (doc, recovery.stamp))
                            };
                            if recovery.dirty {
                                recovered().or_else(|_| saved())
                            } else {
                                saved().or_else(|_| recovered())
                            }
                        } else {
                            read_saved(&path)
                        }
                    })
                    .unwrap_or_else(|_| Err("El importador no pudo procesar este archivo".into()));
                    let (result, stamp) = match result {
                        Ok((doc, stamp)) => (Ok(doc), stamp),
                        Err(error) => (Err(error), None),
                    };
                    let mut state = lock.lock().unwrap();
                    if generation == state.generation {
                        state.result = Some((path, result, stamp));
                        state.busy = false;
                    }
                }
            }));
        }
        let mut state = self.shared.0.lock().unwrap();
        state.generation = state.generation.wrapping_add(1);
        state.pending = Some((state.generation, path, recovery));
        state.result = None;
        state.busy = true;
        self.shared.1.notify_one();
    }
    pub fn take(&self) -> Option<Loaded> {
        self.shared.0.lock().unwrap().result.take()
    }
    pub fn busy(&self) -> bool {
        self.shared.0.lock().unwrap().busy
    }
}
impl Drop for Loader {
    fn drop(&mut self) {
        self.shared.0.lock().unwrap().shutdown = true;
        self.shared.1.notify_one();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
impl Editor {
    pub fn poll_open(&mut self, ctx: &eframe::egui::Context) {
        if let Some((path, result, stamp)) = self.loader.take() {
            let restoring = std::mem::take(&mut self.loader.restoring);
            if restoring {
                self.finish_restoring_tab(path, result, stamp);
                return;
            }
            match result {
                Ok(doc) => {
                    let imported = doc.extras.imported;
                    let migrating = !crate::native_project::is_native_path(&path);
                    if self.loader.saved_copy {
                        let name = format!(
                            "{} ({})",
                            path.file_name().unwrap_or_default().to_string_lossy(),
                            crate::i18n::tr("Copia guardada")
                        );
                        self.add_document(doc, None);
                        if let Some(index) = self.hub.active {
                            self.hub.tabs[index].record.name = name;
                            self.hub.tabs[index].record.dirty = true;
                        }
                        self.persist_documents();
                    } else {
                        self.add_document(doc, Some(path));
                        if let Some(index) = self.hub.active {
                            self.hub.tabs[index].record.stamp = stamp;
                            self.hub.tabs[index].record.dirty = migrating;
                        }
                        self.persist_documents();
                    }
                    if imported {
                        self.import_report_open = true;
                        self.status =
                            "FLA importado. Consulta el informe de compatibilidad.".into();
                    } else if migrating && !self.loader.saved_copy {
                        self.status =
                            crate::i18n::tr("Legacy project opened. Save it as .ltoon.").into();
                    } else {
                        self.status = "Proyecto abierto".into();
                    }
                }
                Err(e) => {
                    self.status = crate::i18n::message("No se pudo abrir: {error}", &[("error", e)])
                }
            }
        }
        if self.loader.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn background_reader_delivers_latest_request_and_reports_errors() {
        let dir = std::env::temp_dir().join(format!("lt-loader-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let valid = dir.join("valid.json");
        let mut doc = Document::default();
        doc.fps = 59;
        std::fs::write(&valid, serde_json::to_vec(&doc).unwrap()).unwrap();
        let mut loader = Loader::default();
        for _ in 0..30 {
            loader.request(dir.join("missing.json"));
        }
        loader.request(valid.clone());
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let result = loop {
            if let Some(r) = loader.take() {
                break r;
            }
            assert!(std::time::Instant::now() < until);
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert_eq!(result.0, valid);
        assert_eq!(result.1.unwrap().fps, 59);
        assert!(!loader.busy());
        drop(loader);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
