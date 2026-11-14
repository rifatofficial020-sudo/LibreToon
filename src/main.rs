#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod advanced;
mod animation;
mod animation_clips;
#[cfg(test)]
mod animation_tests;
#[cfg(test)]
mod appearance_tests;
mod brush;
mod bucket;
mod canvas;
mod character;
#[cfg(test)]
mod compat_tests;
mod depth;
mod design;
mod dialogue;
mod document;
mod documents;
mod drawing;
mod editing;
mod exporter;
mod exposure_resize;
mod features;
mod ffmpeg_setup;
mod fla;
mod fla_zip;
mod folder_groups;
mod frame_clipboard;
mod frame_workflow;
mod gpu_lens;
mod i18n;
mod inspector;
#[cfg(test)]
mod interaction_ux_tests;
mod library;
mod light;
mod lip;
mod loading;
mod mannequin;
mod media;
mod native_project;
mod ordering;
mod paint;
mod palette;
mod post;
mod project_save;
mod raster;
mod recovery;
mod reftool;
mod rig;
#[cfg(test)]
mod rig_tests;
mod rig_ui;
mod scenes;
mod sequencer;
mod shared;
mod shortcuts;
mod studio;
#[cfg(test)]
mod studio_tests;
#[cfg(test)]
mod studio_v2_tests;
mod tablet;
mod themes;
mod tools_editor;
mod typography;
mod update_install;
mod updater;
mod vector;
mod vector_trim;
mod view_options;
mod visual_controls;
mod width;
mod workspace;
mod xfl;
use document::{Document, Mark};
use eframe::egui::{self, Color32};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

struct Editor {
    updater: updater::Updater,
    references: reftool::State,
    raster_cleanup: usize,
    post: post::State,
    lip: lip::State,
    vector_trim: vector_trim::State,
    media: media::MediaEditor,
    loader: loading::Loader,
    import_report_open: bool,
    rigging: rig_ui::RigEditor,
    assets: library::LibraryState,
    clips: animation_clips::Manager,
    frame_clipboard: frame_clipboard::State,
    view_options: view_options::State,
    folder_groups: folder_groups::State,
    hub: documents::Hub,
    drawing: drawing::Drawing,
    feature: features::FeatureState,
    depth: depth::DepthEditor,
    animation: animation::State,
    studio: studio::Studio,
    doc: Document,
    frame: usize,
    layer: usize,
    color: Color32,
    fill_color: Color32,
    fill_enabled: bool,
    stroke_enabled: bool,
    width: f32,
    eraser_width: f32,
    smoothing: f32,
    polygon_sides: usize,
    polygon_star: bool,
    star_ratio: f32,
    zoom_out: bool,
    wand_tolerance: f32,
    style_selection: Vec<(usize, usize)>,
    style_frame: usize,
    style_transaction: bool,
    width_profile: shared::SharedVec<width::WidthStop>,
    width_preset: width::Preset,
    profile_point: Option<usize>,
    gap: bucket::Gap,
    active_fill: bool,
    document_settings: Option<inspector::DocumentSettings>,
    timeline_duration: f32,
    duration_editing: bool,
    profile_transaction: bool,
    renaming_layer: Option<usize>,
    rename_focus: bool,
    tool: canvas::Tool,
    canvas: canvas::Canvas,
    onion: bool,
    playing: bool,
    tick: Instant,
    draft: Option<Mark>,
    undo: Vec<Document>,
    redo: Vec<Document>,
    path: Option<PathBuf>,
    status: String,
    recovery: PathBuf,
    recovery_writer: recovery::RecoveryWriter,
    project_save: project_save::State,
}
impl Editor {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        design::theme(&cc.egui_ctx);
        #[cfg(not(test))]
        let recovery = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("Trazo")
            .join("recovery.json");
        #[cfg(test)]
        let recovery = std::env::temp_dir().join(format!(
            "libretoon-startup-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ));
        let hub = documents::Hub::default();
        let recovered = if hub.has_sessions() {
            None
        } else {
            std::fs::read(&recovery)
                .ok()
                .and_then(|b| serde_json::from_slice::<Document>(&b).ok())
                .filter(|d| d.validate().is_ok())
        };
        let had_recovered = recovered.is_some();
        let status = if recovered.is_some() {
            "Sesión recuperada"
        } else {
            "Dibuja tu primer fotograma"
        }
        .into();
        let mut editor = Self {
            updater: Default::default(),
            references: Default::default(),
            raster_cleanup: 0,
            post: Default::default(),
            lip: Default::default(),
            vector_trim: Default::default(),
            media: Default::default(),
            loader: Default::default(),
            import_report_open: false,
            rigging: Default::default(),
            assets: Default::default(),
            clips: Default::default(),
            frame_clipboard: Default::default(),
            view_options: Default::default(),
            folder_groups: Default::default(),
            hub,
            drawing: Default::default(),
            feature: Default::default(),
            depth: depth::DepthEditor::new(cc),
            animation: Default::default(),
            studio: studio::Studio::load(),
            doc: recovered.unwrap_or_else(fresh_document),
            frame: 0,
            layer: 0,
            color: Color32::from_rgb(89, 72, 220),
            fill_color: Color32::from_rgb(89, 72, 220),
            fill_enabled: true,
            stroke_enabled: true,
            width: 4.,
            eraser_width: 20.,
            smoothing: 0.7,
            polygon_sides: 5,
            polygon_star: false,
            star_ratio: 0.5,
            zoom_out: false,
            wand_tolerance: 0.,
            style_selection: vec![],
            style_frame: 0,
            style_transaction: false,
            width_profile: Default::default(),
            width_preset: Default::default(),
            profile_point: None,
            gap: Default::default(),
            active_fill: true,
            document_settings: None,
            timeline_duration: 2.,
            duration_editing: false,
            profile_transaction: false,
            renaming_layer: None,
            rename_focus: false,
            tool: canvas::Tool::Brush,
            canvas: canvas::Canvas::default(),
            onion: true,
            playing: false,
            tick: Instant::now(),
            draft: None,
            undo: vec![],
            redo: vec![],
            path: None,
            status,
            recovery,
            recovery_writer: recovery::RecoveryWriter::default(),
            project_save: Default::default(),
        };
        editor.initialize_documents(had_recovered);
        i18n::set_locale(editor.studio.prefs.locale());
        themes::ensure(&cc.egui_ctx, editor.studio.prefs.theme);
        i18n::ensure_fonts(&cc.egui_ctx, editor.studio.prefs.locale());
        editor
    }
    fn checkpoint(&mut self) {
        self.hub.dirty();
        self.canvas.clear_selection();
        self.undo.push(self.doc.clone());
        if self.undo.len() > 60 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }
    fn recover(&mut self) {
        if let Some(index) = self.hub.active
            && self.hub.tabs[index].record.dirty
        {
            let record = &mut self.hub.tabs[index].record;
            record.revision = record.revision.wrapping_add(1);
        }
        self.sync_text_styles();
        self.sync_exposures();
        self.recovery_writer
            .submit(self.recovery.clone(), self.project_document());
    }
    fn open(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("LibreToon", &["ltoon"])
            .add_filter("Proyectos anteriores", &["trazo"])
            .pick_file()
        {
            self.open_document_path(path);
        }
    }
    fn import_fla(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Adobe Animate", &["fla"])
            .pick_file()
        {
            self.open_document_path(path);
        }
    }
    fn history(&mut self, redo: bool) {
        self.finish_raster_stroke();
        self.feature.exposure_drag = None;
        self.folder_groups.clear();
        self.vector_trim.clear();
        self.clips.reset_document();
        self.cancel_adjustment_drag();
        self.rigging.cancel_gesture();
        let entry = if redo {
            self.redo.pop()
        } else {
            self.undo.pop()
        };
        if let Some(d) = entry {
            self.feature.layer_selection = Default::default();
            self.hub.dirty();
            if redo {
                self.undo.push(self.doc.clone())
            } else {
                self.redo.push(self.doc.clone())
            };
            self.canvas.clear_selection();
            self.canvas.cancel_drafts();
            self.draft = None;
            self.style_transaction = false;
            self.style_selection.clear();
            self.profile_transaction = false;
            self.animation.timing_transaction = false;
            self.animation.ease_handle = None;
            self.animation.path_handle = None;
            self.animation.path_bend = None;
            self.drawing.text.editing = None;
            self.drawing.text.canvas_edit = false;
            self.drawing.text.transaction = false;
            self.doc = d;
            self.frame = self.frame.min(self.doc.layers[0].frames.len() - 1);
            self.layer = self.layer.min(self.doc.layers.len() - 1);
            self.recover();
        }
    }
}
fn fresh_document() -> Document {
    let mut doc = Document::default();
    doc.width = 1920;
    doc.height = 1080;
    doc.fps = 24;
    let mut keys = vec![false; doc.layers[0].frames.len()];
    keys[0] = true;
    doc.layers[0].options.keys = keys.into();
    doc
}
#[cfg(test)]
fn write_document(path: &std::path::Path, doc: &Document) -> Result<(), String> {
    write_recovery_document(path, doc, None, |temporary, path| {
        std::fs::rename(temporary, path)
    })
}

fn write_recovery_document(
    path: &std::path::Path,
    doc: &Document,
    cache: Option<&mut fla_zip::RecoveryCompression>,
    publish: impl FnOnce(&std::path::Path, &std::path::Path) -> std::io::Result<()>,
) -> Result<(), String> {
    let temporary = path.with_extension("libretoon.tmp");
    let result = (|| {
        let file = std::fs::File::create(&temporary).map_err(|e| e.to_string())?;
        let mut writer = std::io::BufWriter::new(file);
        if native_project::is_native_path(path) {
            native_project::write_with_recovery_cache(doc, &mut writer, cache)?;
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("fla"))
        {
            std::io::Write::write_all(&mut writer, &fla::encode(doc)?)
                .map_err(|e| e.to_string())?;
        } else {
            serde_json::to_writer(&mut writer, doc).map_err(|e| e.to_string())?;
        }
        std::io::Write::flush(&mut writer).map_err(|e| e.to_string())?;
        drop(writer);
        publish(&temporary, path).map_err(|e| e.to_string())
    })();
    let _ = std::fs::remove_file(&temporary);
    result
}
impl eframe::App for Editor {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_open(&ctx);
        self.drawing.pen.update(&ctx);
        if let Some(error) = self.recovery_writer.take_error() {
            self.status =
                crate::i18n::message("No se pudo recuperar sesión: {error}", &[("error", error)]);
        }
        if !self.hub.home
            && !self.studio.preferences_open
            && self.document_settings.is_none()
            && self.studio.export.is_none()
            && ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S))
        {
            self.save_project(false);
        }
        self.action_shortcuts(&ctx);
        self.poll_lip_analysis();
        self.space_playback(&ctx);
        if self.profile_transaction && !ctx.input(|i| i.pointer.primary_down()) {
            self.profile_transaction = false;
            self.recover();
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.project_save.busy() {
                self.project_save.quit_after_save = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            } else if self.project_save.conflicted() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            } else if self.references.importing() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.status = crate::i18n::tr(
                    "Wait for the reference image to finish loading before saving or closing.",
                )
                .into();
            } else {
                self.finish_pending_pen();
                if self.hub.active.is_some() {
                    self.recover();
                }
                self.persist_documents();
                if !self.hub.allow_quit && self.hub.tabs.iter().any(|t| t.record.dirty) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    self.hub.quitting = true;
                    self.hub.closing = self.hub.tabs.iter().position(|t| t.record.dirty);
                }
            }
        }
        if self.media_audio_ready(&ctx) {
            let elapsed = self.tick.elapsed().as_secs_f64();
            let steps = self
                .media_audio_steps()
                .unwrap_or_else(|| (elapsed * self.doc.fps as f64).floor() as usize);
            if steps > 0 {
                let before = crate::animation::movie_offset(&self.doc, self.frame);
                self.advance_animation(steps);
                if crate::animation::movie_offset(&self.doc, self.frame) < before {
                    self.restart_media_audio();
                }
                if self.media_audio_steps().is_some() {
                    self.tick = Instant::now();
                } else {
                    self.tick += Duration::from_secs_f64(steps as f64 / self.doc.fps as f64);
                }
            }
            let period = Duration::from_secs_f64(1. / self.doc.fps as f64);
            ctx.request_repaint_after(period.saturating_sub(self.tick.elapsed()));
        }
        crate::i18n::set_locale(self.studio.prefs.locale());
        themes::ensure(&ctx, self.studio.prefs.theme);
        i18n::ensure_fonts(&ctx, self.studio.prefs.locale());
        if !self.hub.home {
            self.clipboard_shortcuts(&ctx);
        }
        let was_home = self.hub.home;
        self.workspace(ui);
        self.media_dialog(&ctx);
        self.updater.show(&ctx);
        self.update_installation(&ctx);
        if was_home != self.hub.home {
            ctx.request_repaint();
        }
        if !self.hub.home && !was_home {
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::new()
                        .fill(crate::themes::color(Color32::from_rgb(29, 30, 34)))
                        .inner_margin(0.),
                )
                .show(ui, |ui| {
                    self.canvas_ui(ui);
                });
        }
        self.reference_windows(&ctx);
        self.annotation_dialog(&ctx);
        self.lip_session_ui(&ctx);
        self.project_save_ui(&ctx);
        if self.studio.credits_open {
            egui::Window::new(crate::i18n::tr("Credits"))
                .id(egui::Id::unique("libretoon-credits"))
                .open(&mut self.studio.credits_open)
                .collapsible(false)
                .resizable(false)
                .default_width(320.)
                .default_pos(ctx.content_rect().center() - egui::vec2(160., 120.))
                .show(&ctx, |ui| {
                    ui.horizontal(|ui| {
                        crate::design::button(
                            ui,
                            crate::design::Icon::Info,
                            "",
                            "Credits",
                            false,
                            false,
                        );
                        ui.heading(format!("LibreToon {}", env!("CARGO_PKG_VERSION")));
                    });
                    ui.separator();
                    crate::design::category(ui, "Testers:");
                    for name in ["'", "BarelyDead", "Maheshmv666", "Boctok-1 scary"] {
                        ui.label(name);
                    }
                    ui.add_space(8.);
                });
        }
        if ctx
            .input(|input| input.pointer.primary_released() || input.key_pressed(egui::Key::Escape))
        {
            self.assets.drag = None;
        }
    }
}
fn main() -> eframe::Result {
    if let Some(result) = update_install::helper_entry() {
        return result;
    }
    eframe::run_native(
        "LibreToon",
        eframe::NativeOptions {
            #[cfg(windows)]
            event_loop_builder: Some(Box::new(tablet::windows_event_loop)),
            #[cfg(target_os = "linux")]
            event_loop_builder: Some(Box::new(tablet::linux_event_loop)),
            viewport: egui::ViewportBuilder::default()
                .with_decorations(false)
                .with_inner_size([1360., 860.])
                .with_min_inner_size([1050., 700.])
                .with_icon(design::window_icon()),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(Editor::new(cc)))),
    )
}

#[cfg(test)]
mod history_tests {
    use super::*;
    #[test]
    fn undo_redo_restore_geometry_after_shared_snapshot_edits() {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx);
        let mut editor = Editor::new(&cc);
        editor.doc = Document::default();
        let path =
            std::env::temp_dir().join(format!("libretoon-history-{}.json", std::process::id()));
        editor.recovery = path.clone();
        editor.checkpoint();
        editor.doc.layers[0].frames[0].push(Mark {
            points: vec![[10., 20.], [30., 40.]].into(),
            color: [0, 0, 0, 255],
            width: 4.,
            rectangle: false,
            ..Mark::default()
        });
        editor.checkpoint();
        editor.doc.layers[0].frames[0][0].points[0] = [50., 60.];
        editor.history(false);
        assert_eq!(editor.doc.layers[0].frames[0][0].points[0], [10., 20.]);
        editor.history(false);
        assert!(editor.doc.layers[0].frames[0].is_empty());
        editor.history(true);
        assert_eq!(editor.doc.layers[0].frames[0][0].points[0], [10., 20.]);
        editor.history(true);
        assert_eq!(editor.doc.layers[0].frames[0][0].points[0], [50., 60.]);
        drop(editor);
        let restored: Document = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(restored.layers[0].frames[0][0].points[0], [50., 60.]);
        std::fs::remove_file(path).unwrap();
    }
}
