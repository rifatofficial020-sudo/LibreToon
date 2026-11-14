//! Independent document sessions and a bounded, asynchronous project gallery.
use crate::{
    Document, Editor, animation, canvas::Canvas, features::FeatureState, recovery::RecoveryWriter,
};
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::mpsc};

pub struct Session {
    rigging: crate::rig_ui::RigEditor,
    assets: crate::library::LibraryState,
    doc: Document,
    frame: usize,
    layer: usize,
    canvas: Canvas,
    feature: FeatureState,
    animation: animation::State,
    undo: Vec<Document>,
    redo: Vec<Document>,
    path: Option<PathBuf>,
    recovery: PathBuf,
    writer: RecoveryWriter,
}
impl Session {
    fn new(doc: Document, path: Option<PathBuf>, recovery: PathBuf) -> Self {
        Self {
            rigging: Default::default(),
            assets: Default::default(),
            doc,
            path,
            recovery,
            frame: 0,
            layer: 0,
            canvas: Default::default(),
            feature: Default::default(),
            animation: Default::default(),
            undo: vec![],
            redo: vec![],
            writer: Default::default(),
        }
    }
    fn exchange(&mut self, e: &mut Editor) {
        e.studio.palette = Default::default();
        e.feature.exposure_drag = None;
        e.folder_groups.clear();
        e.vector_trim.clear();
        e.depth.invalidate_preview();
        e.close_lip_session();
        e.references.reset_document();
        e.clips.reset_document();
        e.cancel_adjustment_drag();
        e.frame_clipboard.focus = false;
        std::mem::swap(&mut self.rigging, &mut e.rigging);
        std::mem::swap(&mut self.assets, &mut e.assets);
        std::mem::swap(&mut self.doc, &mut e.doc);
        std::mem::swap(&mut self.frame, &mut e.frame);
        std::mem::swap(&mut self.layer, &mut e.layer);
        std::mem::swap(&mut self.canvas, &mut e.canvas);
        std::mem::swap(&mut self.feature, &mut e.feature);
        std::mem::swap(&mut self.animation, &mut e.animation);
        std::mem::swap(&mut self.undo, &mut e.undo);
        std::mem::swap(&mut self.redo, &mut e.redo);
        std::mem::swap(&mut self.path, &mut e.path);
        std::mem::swap(&mut self.recovery, &mut e.recovery);
        std::mem::swap(&mut self.writer, &mut e.recovery_writer);
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Record {
    pub path: Option<PathBuf>,
    pub recovery: PathBuf,
    pub name: String,
    pub dirty: bool,
    #[serde(default)]
    pub stamp: Option<crate::project_save::Stamp>,
    #[serde(skip)]
    pub revision: u64,
}
pub struct Tab {
    pub record: Record,
    state: Option<Session>,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Catalogue {
    recent: Vec<PathBuf>,
    folders: Vec<PathBuf>,
    sessions: Vec<Record>,
}
struct Preview {
    path: PathBuf,
    frames: Result<(Vec<egui::TextureHandle>, f64), String>,
}
struct PreviewFrames {
    pixels: Vec<Vec<u8>>,
    size: [usize; 2],
    duration: f64,
}
fn preview_frames(path: &std::path::Path) -> Result<PreviewFrames, String> {
    let doc = if path == std::path::Path::new(EXAMPLE_PREVIEW_KEY) {
        motion_tween_example()?
    } else {
        read_project(path)?
    };
    let count = animation::movie_count(&doc).max(1);
    let samples = count.min(8);
    let ctx = egui::Context::default();
    let mut cache = Default::default();
    let scale = (192. / doc.width as f32).min(108. / doc.height as f32);
    let size = [
        (doc.width as f32 * scale).round().max(1.) as usize,
        (doc.height as f32 * scale).round().max(1.) as usize,
    ];
    let mut pixels: Vec<Vec<u8>> = (0..samples)
        .map(|i| {
            crate::exporter::raster(
                &doc,
                i * count / samples,
                size[0] as u32,
                size[1] as u32,
                &ctx,
                &mut cache,
            )
        })
        .collect();
    if pixels.iter().all(|p| p == &pixels[0]) {
        pixels.truncate(1);
    }
    Ok(PreviewFrames {
        pixels,
        size,
        duration: count as f64 / doc.fps as f64,
    })
}
pub struct Hub {
    pub home: bool,
    pub active: Option<usize>,
    pub tabs: Vec<Tab>,
    catalogue: Catalogue,
    previews: Vec<Preview>,
    pending: Option<PathBuf>,
    request: mpsc::SyncSender<PathBuf>,
    response: mpsc::Receiver<(PathBuf, Result<PreviewFrames, String>)>,
    pub closing: Option<usize>,
    pub quitting: bool,
    pub allow_quit: bool,
    search: String,
    preview_clock: std::time::Instant,
    pub metadata_dirty: bool,
    live_cache: crate::paint::RenderCache,
    pub tab_extent: (f32, f32),
    pub tab_offset: f32,
}
fn catalogue_path() -> PathBuf {
    #[cfg(test)]
    {
        return std::env::temp_dir()
            .join(format!(
                "libretoon-hub-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ))
            .join("projects.json");
    }
    #[cfg(not(test))]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("LibreToon/projects.json")
    }
}

pub fn read_project(path: &std::path::Path) -> Result<Document, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(256 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 256 * 1024 * 1024 {
        return Err("El proyecto supera el límite de lectura de 256 MiB".into());
    }
    let json = bytes
        .iter()
        .find(|b| !b.is_ascii_whitespace())
        .is_some_and(|b| *b == b'{');
    let native_path = crate::native_project::is_native_path(path);
    let fla_path = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("fla"));
    let mut native_validated = false;
    let mut d: Document = if fla_path && !json {
        crate::fla::decode(&bytes)?
    } else if bytes.starts_with(b"PK") || native_path {
        match crate::native_project::decode_if_native(&bytes)? {
            Some(doc) => {
                native_validated = true;
                doc
            }
            None if native_path => return Err("Missing LibreToon project manifest".into()),
            None => crate::fla::decode(&bytes)?,
        }
    } else {
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())?
    };
    if !native_validated {
        d.validate()?;
    }
    d.extras.media.remove_empty_tracks();
    for scene in &mut d.extras.scenes {
        scene.media_tracks.retain(|track| !track.is_empty());
    }
    Ok(d)
}
impl Default for Hub {
    fn default() -> Self {
        let mut catalogue: Catalogue = std::fs::read(catalogue_path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        for folder in &catalogue.folders {
            if let Ok(entries) = folder.read_dir() {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.extension().is_some_and(|e| {
                        e.eq_ignore_ascii_case("ltoon")
                            || e.eq_ignore_ascii_case("fla")
                            || e.eq_ignore_ascii_case("trazo")
                    }) && !catalogue.recent.contains(&p)
                    {
                        catalogue.recent.push(p);
                    }
                }
            }
        }
        let (tx, requests) = mpsc::sync_channel::<PathBuf>(1);
        let (results, rx) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            while let Ok(path) = requests.recv() {
                let doc = preview_frames(&path);
                if results.send((path, doc)).is_err() {
                    break;
                }
            }
        });
        Self {
            home: true,
            active: None,
            tabs: vec![],
            catalogue,
            previews: vec![],
            pending: None,
            request: tx,
            response: rx,
            closing: None,
            quitting: false,
            allow_quit: false,
            search: String::new(),
            preview_clock: std::time::Instant::now(),
            metadata_dirty: false,
            live_cache: Default::default(),
            tab_extent: (0., 0.),
            tab_offset: 0.,
        }
    }
}
impl Hub {
    pub fn has_sessions(&self) -> bool {
        !self.catalogue.sessions.is_empty()
    }
    pub fn dirty(&mut self) {
        if let Some(i) = self.active {
            self.tabs[i].record.dirty = true;
            self.tabs[i].record.revision = self.tabs[i].record.revision.wrapping_add(1);
            self.metadata_dirty = true;
        }
    }
    fn persist(&mut self) -> Result<(), String> {
        self.metadata_dirty = false;
        self.catalogue.sessions = self.tabs.iter().map(|t| t.record.clone()).collect();
        let p = catalogue_path();
        std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
        let tmp = p.with_extension("tmp");
        std::fs::write(
            &tmp,
            serde_json::to_vec(&self.catalogue).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(tmp, p).map_err(|e| e.to_string())
    }
    fn remember(&mut self, path: PathBuf) {
        self.catalogue.recent.retain(|p| p != &path);
        self.catalogue.recent.insert(0, path.clone());
        self.previews.retain(|p| p.path != path);
    }
}
impl Editor {
    pub fn initialize_documents(&mut self, legacy_recovery: bool) {
        let records = std::mem::take(&mut self.hub.catalogue.sessions);
        for record in records {
            self.hub.tabs.push(Tab {
                record,
                state: None,
            });
        }
        if self.hub.tabs.is_empty() && legacy_recovery {
            self.hub.tabs.push(Tab {
                record: Record {
                    path: None,
                    recovery: self.recovery.clone(),
                    name: "Sesión recuperada".into(),
                    dirty: true,
                    stamp: None,
                    revision: 0,
                },
                state: None,
            });
            self.hub.active = Some(0);
        }
        self.hub.home = true;
    }
    fn park_document(&mut self) {
        self.finish_raster_stroke();
        self.finish_rig_gesture();
        self.finish_pending_pen();
        self.playing = false;
        self.draft = None;
        self.canvas.cancel_drafts();
        self.drawing.cancel();
        self.style_selection.clear();
        self.clear_guides();
        if let Some(i) = self.hub.active.take() {
            self.recover();
            let mut session = Session::new(crate::fresh_document(), None, PathBuf::new());
            session.exchange(self);
            self.hub.tabs[i].state = Some(session);
        }
    }
    pub fn activate_document(&mut self, i: usize) {
        if i >= self.hub.tabs.len() {
            return;
        }
        if self.hub.active != Some(i) {
            if self.hub.tabs[i].state.is_none() {
                let record = &self.hub.tabs[i].record;
                self.status = "Abriendo proyecto…".into();
                self.loader.restore(
                    record.recovery.clone(),
                    record.path.clone(),
                    record.stamp,
                    record.dirty,
                );
                return;
            }
            self.loader.cancel_restore();
            self.park_document();
            if let Some(mut session) = self.hub.tabs[i].state.take() {
                session.exchange(self);
            }
            self.hub.active = Some(i);
        }
        self.loader.cancel_restore();
        self.hub.home = false;
        self.import_report_open = false;
        self.drawing.scroll_to_tab = true;
        self.style_transaction = false;
        self.profile_transaction = false;
        self.duration_editing = false;
        self.renaming_layer = None;
        self.document_settings = None;
    }
    pub(crate) fn finish_restoring_tab(
        &mut self,
        recovery: PathBuf,
        result: Result<Document, String>,
        stamp: Option<crate::project_save::Stamp>,
    ) {
        let Some(index) = self
            .hub
            .tabs
            .iter()
            .position(|tab| tab.record.recovery == recovery && tab.state.is_none())
        else {
            return;
        };
        match result {
            Ok(doc) => {
                let tab = &mut self.hub.tabs[index];
                tab.state = Some(Session::new(doc, tab.record.path.clone(), recovery));
                tab.record.stamp = stamp;
                self.activate_document(index);
                self.status = "Proyecto abierto".into();
            }
            Err(error) => {
                self.status =
                    crate::i18n::message("No se pudo recuperar el documento: {e}", &[("e", error)]);
            }
        }
    }

    pub fn add_document(&mut self, doc: Document, path: Option<PathBuf>) {
        self.loader.cancel_restore();

        self.park_document();
        let recovery = catalogue_path()
            .parent()
            .unwrap()
            .join(format!("sessions/{}.ltoon", crate::features::id()));
        let name = path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| {
                format!(
                    "{} {}",
                    crate::i18n::tr("Sin título"),
                    self.hub.tabs.len() + 1
                )
            });
        let mut session = Session::new(doc, path.clone(), recovery.clone());
        session.exchange(self);
        self.drawing.scroll_to_tab = true;
        self.hub.active = Some(self.hub.tabs.len());
        self.hub.home = false;
        self.hub.tabs.push(Tab {
            state: None,
            record: Record {
                path: path.clone(),
                recovery,
                name,
                dirty: false,
                stamp: path.as_deref().and_then(crate::project_save::stamp),
                revision: 0,
            },
        });
        if let Some(path) = path {
            self.hub.remember(path);
        }
        self.recover();
        self.persist_documents();
    }
    pub fn open_saved_document_copy(&mut self) {
        if let Some(path) = self.path.clone() {
            self.status = crate::i18n::tr("Abriendo copia guardada…").into();
            self.loader.request_saved_copy(path);
        }
    }

    pub fn open_document_path(&mut self, path: PathBuf) {
        let path = path.canonicalize().unwrap_or(path);
        if let Some(i) = self
            .hub
            .tabs
            .iter()
            .position(|t| t.record.path.as_ref() == Some(&path))
        {
            self.activate_document(i);
            return;
        }
        self.status = "Abriendo proyecto…".into();
        self.loader.request(path);
    }

    pub fn persist_documents(&mut self) {
        if let Err(e) = self.hub.persist() {
            self.status = crate::i18n::message(
                "No se pudo guardar la lista de proyectos: {e}",
                &[("e", e.to_string())],
            );
        }
    }
    pub(crate) fn save_snapshot_for(&self, index: usize) -> Option<Document> {
        if self.hub.active == Some(index) {
            Some(self.project_document())
        } else {
            let session = self.hub.tabs.get(index)?.state.as_ref()?;
            Some(crate::advanced::assemble_project(
                &session.doc,
                &session.feature,
            ))
        }
    }
    pub(crate) fn complete_project_save(
        &mut self,
        recovery: &std::path::Path,
        revision: u64,
        path: PathBuf,
        stamp: crate::project_save::Stamp,
        close: bool,
    ) {
        let Some(index) = self
            .hub
            .tabs
            .iter()
            .position(|tab| tab.record.recovery == recovery)
        else {
            return;
        };
        let tab = &mut self.hub.tabs[index];
        tab.record.path = Some(path.clone());
        tab.record.stamp = Some(stamp);
        tab.record.name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        tab.record.dirty = tab.record.revision != revision;
        let clean = !tab.record.dirty;
        if self.hub.active == Some(index) {
            self.path = Some(path.clone());
            self.doc.extras.imported = false;
            for (parent, _, _, _) in &mut self.feature.symbol_stack {
                parent.extras.imported = false;
            }
            self.recover();
        } else if let Some(session) = &mut tab.state {
            session.path = Some(path.clone());
            session.doc.extras.imported = false;
            for (parent, _, _, _) in &mut session.feature.symbol_stack {
                parent.extras.imported = false;
            }
        }
        self.hub.remember(path);
        if clean && close {
            self.close_document(index);
        }
        self.persist_documents();
    }
    fn close_document(&mut self, i: usize) {
        if i >= self.hub.tabs.len() || self.project_save.busy() {
            return;
        }
        let was_active = self.hub.active == Some(i);
        let was_home = self.hub.home;
        if was_active {
            self.park_document();
        }
        let tab = self.hub.tabs.remove(i);
        let recovery = tab.record.recovery.clone();
        drop(tab);
        self.recovery_writer.discard(&recovery);
        if let Some(active) = &mut self.hub.active {
            if *active > i {
                *active -= 1;
            }
        }
        if was_active && self.hub.active.is_none() && !self.hub.tabs.is_empty() {
            self.activate_document(i.min(self.hub.tabs.len() - 1));
        }
        if was_home || self.hub.tabs.is_empty() {
            self.hub.home = true;
        }
        self.hub.closing = None;
        self.persist_documents();
    }
    pub fn document_tabs(&mut self, ui: &mut egui::Ui) {
        let mut activate = None;
        let mut close = None;
        let mut home = false;
        egui::Panel::top("document-tabs")
            .exact_size(42.)
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().interact_size.y = 30.;
                    ui.spacing_mut().scroll = egui::style::ScrollStyle::floating();
                    home = crate::design::button(
                        ui,
                        crate::design::Icon::Home,
                        "Home",
                        "Galería de proyectos",
                        self.hub.home,
                        true,
                    )
                    .clicked();
                    ui.separator();
                    let scroll = egui::ScrollArea::horizontal()
                        .id_salt("open-document-tabs")
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for (i, tab) in self.hub.tabs.iter().enumerate() {
                                    let active = self.hub.active == Some(i) && !self.hub.home;
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 0.;
                                        let label = format!(
                                            "{}{}",
                                            tab.record.name,
                                            if tab.record.dirty { " •" } else { "" }
                                        );
                                        let r = crate::design::button(
                                            ui,
                                            crate::design::Icon::Film,
                                            &label,
                                            "Cambiar de documento",
                                            active,
                                            true,
                                        );
                                        if r.clicked() {
                                            activate = Some(i);
                                        }
                                        if active && self.drawing.scroll_to_tab {
                                            r.scroll_to_me(Some(egui::Align::Center));
                                        }
                                        if crate::design::button(
                                            ui,
                                            crate::design::Icon::Close,
                                            "",
                                            "Cerrar documento",
                                            false,
                                            true,
                                        )
                                        .clicked()
                                        {
                                            close = Some(i);
                                        }
                                    });
                                }
                                if crate::design::button(
                                    ui,
                                    crate::design::Icon::Plus,
                                    "",
                                    "Nuevo documento",
                                    false,
                                    true,
                                )
                                .clicked()
                                {
                                    activate = Some(usize::MAX);
                                }
                            });
                        });
                    self.hub.tab_extent = (scroll.content_size.x, scroll.inner_rect.width());
                    self.hub.tab_offset = scroll.state.clamped_offset().x;
                });
            });
        self.drawing.scroll_to_tab = false;
        if home {
            self.loader.cancel_restore();
            self.finish_pending_pen();
            if self.hub.active.is_some() {
                self.recover();
            }
            self.playing = false;
            self.hub.home = true;
        }
        if let Some(i) = activate {
            if i == usize::MAX {
                self.new_document();
            } else {
                self.activate_document(i);
            }
            self.drawing.scroll_to_tab = true;
        }
        if let Some(i) = close {
            if self.hub.tabs[i].record.dirty {
                self.hub.closing = Some(i);
            } else {
                self.close_document(i);
            }
        }
        self.document_close_dialog(ui.ctx());
        if self.hub.metadata_dirty {
            self.persist_documents();
        }
    }
    pub fn document_close_dialog(&mut self, ctx: &egui::Context) {
        if self.project_save.busy() || self.project_save.conflicted() {
            return;
        }
        if let Some(i) = self.hub.closing {
            let name = self
                .hub
                .tabs
                .get(i)
                .map(|t| t.record.name.clone())
                .unwrap_or_default();
            egui::Modal::new(egui::Id::unique("close-document-confirm")).show(ctx, |ui| {
                ui.heading(crate::i18n::tr("Guardar los cambios"));
                ui.label(crate::i18n::message(
                    "«{name}» tiene cambios sin guardar.",
                    &[("name", name.to_string())],
                ));
                ui.horizontal(|ui| {
                    if ui.button(crate::i18n::tr("Cancelar")).clicked() {
                        self.hub.closing = None;
                        self.hub.quitting = false;
                    }
                    if ui.button(crate::i18n::tr("Descartar")).clicked() {
                        self.close_document(i);
                    }
                    if ui.button(crate::i18n::tr("Guardar y cerrar")).clicked() {
                        self.activate_document(i);
                        self.start_project_save(false, true);
                    }
                });
            });
        }
        if self.hub.quitting && self.hub.closing.is_none() {
            if let Some(i) = self.hub.tabs.iter().position(|t| t.record.dirty) {
                self.hub.closing = Some(i);
            } else {
                self.persist_documents();
                self.hub.allow_quit = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
    pub fn home_ui(&mut self, ui: &mut egui::Ui) {
        while let Ok((path, doc)) = self.hub.response.try_recv() {
            self.hub.pending = None;
            if self.hub.previews.len() >= 96 {
                self.hub.previews.remove(0);
            }
            let frames = doc.map(|frames| {
                let textures = frames
                    .pixels
                    .iter()
                    .enumerate()
                    .map(|(i, pixels)| {
                        ui.ctx().load_texture(
                            format!("gallery-{}-{i}", path.display()),
                            egui::ColorImage::from_rgb(frames.size, pixels),
                            egui::TextureOptions::LINEAR,
                        )
                    })
                    .collect();
                (textures, frames.duration)
            });
            self.hub.previews.push(Preview { path, frames });
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(crate::themes::color(Color32::from_rgb(24,26,32))).inner_margin(egui::Margin::symmetric(28, 12))).show(ui, |ui| {
            ui.horizontal(|ui| {
                if crate::design::button(ui, crate::design::Icon::Folder, "Carpeta", "Añadir carpeta de proyectos", false, true).clicked() {
                    if let Some(p) = rfd::FileDialog::new().pick_folder() {
                        if !self.hub.catalogue.folders.contains(&p) { self.hub.catalogue.folders.push(p.clone()); }
                        if let Ok(entries) = p.read_dir() { for entry in entries.flatten() { let p = entry.path(); if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ltoon") || e.eq_ignore_ascii_case("fla") || e.eq_ignore_ascii_case("trazo")) { self.hub.remember(p); } } }
                        self.persist_documents();
                    }
                }
                if crate::design::button(ui, crate::design::Icon::Open, "Abrir", "Abrir proyecto", false, true).clicked() { self.open(); }
                if crate::design::button(ui, crate::design::Icon::Plus, "Nuevo proyecto", "Crear animación", false, true).clicked() { self.new_document(); }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let width = ui.available_width().clamp(0., 320.);
                    let blue = if crate::themes::palette().light { Color32::from_rgb(28, 91, 175) } else { Color32::from_rgb(123, 182, 255) };
                    ui.add(egui::TextEdit::singleline(&mut self.hub.search)
                        .hint_text(egui::RichText::new(crate::i18n::tr("Buscar por nombre de archivo…")).color(blue))
                        .desired_width(width));
                });
            });
            let query = self.hub.search.to_lowercase();
            let paths: Vec<_> = self.hub.catalogue.recent.iter().filter(|path| {
                path.file_name().is_some_and(|name| name.to_string_lossy().to_lowercase().contains(&query))
            }).cloned().collect();
            let in_progress: Vec<_> = self.hub.tabs.iter().enumerate()
                .filter_map(|(index, tab)| tab.record.name.to_lowercase().contains(&query).then_some(index))
                .collect();
            let seconds = self.hub.preview_clock.elapsed().as_secs_f64();
            let width = (ui.available_width() / 4. - 10.).clamp(175., 264.);
            let mut open_example = false;
            let mut selected_tab = None;
            let mut open_path = None;
            self.hub.live_cache.begin_frame();
            egui::ScrollArea::vertical().id_salt("home-sections").show(ui, |ui| {
                ui.add_space(12.);
                crate::design::category(ui, "EJEMPLOS");
                gallery_row(ui, "examples", |ui| {
                    let (preview, response) = gallery_card(ui, EXAMPLE_TITLE, width);
                    if preview.intersects(ui.clip_rect()) {
                        self.hub.paint_file_preview(ui, preview, std::path::Path::new(EXAMPLE_PREVIEW_KEY), seconds);
                    }
                    open_example = response.on_hover_text(crate::i18n::tr("Open a copy of the example")).clicked();
                });
                ui.add_space(12.);
                crate::design::category(ui, "EN CURSO");
                if in_progress.is_empty() {
                    ui.label(egui::RichText::new(crate::i18n::tr(if query.is_empty() { "No open projects." } else { "No open projects match the search." })).color(crate::themes::muted()));
                } else {
                    gallery_row(ui, "live-projects", |ui| {
                        for &index in &in_progress {
                            let record = &self.hub.tabs[index].record;
                            let (preview, response) = gallery_card(ui, &record.name, width);
                            if response.clicked() { selected_tab = Some(index); }
                            if !preview.intersects(ui.clip_rect()) { continue; }
                            let doc = if self.hub.active == Some(index) {
                                Some(self.project_document())
                            } else {
                                self.hub.tabs[index].state.as_ref().map(|session| session.doc.clone())
                            };
                            if let Some(doc) = doc {
                                let image = fit_thumbnail(preview, Vec2::new(doc.width as f32, doc.height as f32));
                                ui.painter().rect_filled(image, 3., Color32::WHITE);
                                let count = animation::movie_count(&doc).max(1);
                                let frame = (seconds * doc.fps as f64) as usize % count;
                                let view = crate::canvas::View {
                                    center: image.center(), stage_center: Pos2::new(doc.width as f32 * 0.5, doc.height as f32 * 0.5),
                                    scale: image.width() / doc.width as f32, angle: 0.,
                                };
                                let painter = ui.painter().with_clip_rect(image.intersect(ui.clip_rect()));
                                for (_, _, mark) in animation::resolve_movie(&doc, frame, true) {
                                    if mark.style.media_image.is_none() {
                                        crate::paint::draw(&painter, &mark, view, false, false, Some(&mut self.hub.live_cache));
                                    }
                                }
                            } else {
                                let path = if record.dirty { &record.recovery } else { record.path.as_ref().unwrap_or(&record.recovery) }.clone();
                                self.hub.paint_file_preview(ui, preview, &path, seconds);
                            }
                        }
                    });
                }
                ui.add_space(12.);
                crate::design::category(ui, "ARCHIVOS");
                if paths.is_empty() && in_progress.is_empty() {
                    ui.label(crate::i18n::tr("Tu galería empieza aquí. Crea un proyecto, abre un archivo o añade una carpeta."));
                }
                gallery_row(ui, "recent-projects", |ui| {
                    for path in &paths {
                        let name = path.file_name().unwrap_or_default().to_string_lossy();
                        let (preview, response) = gallery_card(ui, &name, width);
                        if preview.intersects(ui.clip_rect()) { self.hub.paint_file_preview(ui, preview, path, seconds); }
                        if response.on_hover_text(path.display().to_string()).clicked() { open_path = Some(path.clone()); }
                    }
                });
            });
            if open_example {
                match motion_tween_example() {
                    Ok(doc) => {
                        self.add_document(doc, None);
                        if let Some(index) = self.hub.active { self.hub.tabs[index].record.name = EXAMPLE_TITLE.into(); }
                    }
                    Err(error) => self.status = error,
                }
            } else if let Some(index) = selected_tab {
                self.activate_document(index);
            } else if let Some(path) = open_path {
                self.open_document_path(path);
            }

        });
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(83));
    }
}

const EXAMPLE_TITLE: &str = "Motion Tween Example";
const EXAMPLE_PREVIEW_KEY: &str = ":builtin-motion-tween-example";

impl Hub {
    fn paint_file_preview(
        &mut self,
        ui: &egui::Ui,
        preview: Rect,
        path: &std::path::Path,
        seconds: f64,
    ) {
        let item = self.previews.iter().find(|item| item.path == path);
        if let Some(Ok((textures, duration))) = item.map(|item| &item.frames)
            && !textures.is_empty()
        {
            let duration = duration.max(0.01);
            let index = ((seconds % duration) / duration * textures.len() as f64) as usize;
            let texture = &textures[index.min(textures.len() - 1)];
            let image = fit_thumbnail(preview, texture.size_vec2());
            ui.painter().image(
                texture.id(),
                image,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
                Color32::WHITE,
            );
        } else {
            crate::design::draw(
                ui.painter(),
                Rect::from_center_size(preview.center(), Vec2::splat(32.)),
                crate::design::Icon::Film,
                crate::themes::muted(),
            );
            if item.is_none()
                && self.pending.is_none()
                && self.request.try_send(path.to_owned()).is_ok()
            {
                self.pending = Some(path.to_owned());
            }
        }
    }
}

fn fit_thumbnail(rect: Rect, size: Vec2) -> Rect {
    Rect::from_center_size(
        rect.center(),
        size * (rect.width() / size.x).min(rect.height() / size.y),
    )
}

fn gallery_row(ui: &mut egui::Ui, id: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.scope(|ui| {
        ui.style_mut().always_scroll_the_only_direction = true;
        egui::ScrollArea::horizontal()
            .id_salt(id)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.;
                    add(ui);
                });
            });
    });
}

fn gallery_card(ui: &mut egui::Ui, title: &str, width: f32) -> (Rect, egui::Response) {
    let image_height = (width - 12.) * 9. / 16.;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, image_height + 41.), egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if !rect.intersects(ui.clip_rect()) {
        return (rect, response);
    }
    ui.painter().rect_filled(
        rect,
        8.,
        if response.hovered() {
            crate::themes::color(Color32::from_rgb(48, 55, 70))
        } else {
            crate::themes::color(Color32::from_rgb(34, 37, 46))
        },
    );
    let preview = Rect::from_min_size(
        rect.min + Vec2::splat(6.),
        Vec2::new(width - 12., image_height),
    );
    ui.painter()
        .rect_filled(preview, 4., crate::themes::palette().canvas);
    let mut job = egui::text::LayoutJob::simple(
        title.into(),
        egui::FontId::proportional(13.),
        crate::themes::palette().text,
        width - 20.,
    );
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let text = ui.painter().layout_job(job);
    ui.painter()
        .with_clip_rect(rect.intersect(ui.clip_rect()))
        .galley(
            Pos2::new(rect.left() + 10., preview.bottom() + 9.),
            text,
            crate::themes::palette().text,
        );
    (preview, response)
}

fn motion_tween_example() -> Result<Document, String> {
    crate::native_project::decode_if_native(include_bytes!(
        "../assets/examples/MotionTween_Example.ltoon"
    ))?
    .ok_or_else(|| "Missing LibreToon example document".into())
}

#[cfg(test)]
#[test]
fn motion_tween_example_is_compact_and_retains_its_animation() {
    let doc = motion_tween_example().unwrap();
    assert_eq!(doc.layers[0].frames.len(), 80);
    assert_eq!(
        doc.layers
            .iter()
            .map(|layer| layer.options.tweens.len())
            .sum::<usize>(),
        4
    );
    assert!(include_bytes!("../assets/examples/MotionTween_Example.ltoon").len() < 1024 * 1024);
}

#[cfg(test)]
#[test]
fn gallery_example_has_a_valid_preview_and_widescreen_fills_its_card() {
    let frames = preview_frames(std::path::Path::new(EXAMPLE_PREVIEW_KEY)).unwrap();
    assert!(frames.pixels.len() > 1);
    assert!(
        frames
            .pixels
            .iter()
            .all(|frame| frame.len() == frames.size[0] * frames.size[1] * 3)
    );
    let card = Rect::from_min_size(Pos2::ZERO, Vec2::new(240., 135.));
    assert_eq!(fit_thumbnail(card, Vec2::new(192., 108.)), card);
    let portrait = fit_thumbnail(card, Vec2::new(90., 160.));
    assert_eq!(portrait.center(), card.center());
    assert!(portrait.width() < card.width());
}
