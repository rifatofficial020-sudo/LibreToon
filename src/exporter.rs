use crate::{Document, Editor, canvas::View, i18n::tr};
use eframe::egui::{self, Pos2, Rect, Vec2};
use std::{
    io::{Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ExportBatch {
    #[default]
    Range,
    Scenes,
    Tags,
}

impl ExportBatch {
    fn label(self) -> &'static str {
        match self {
            Self::Range => "Frame range",
            Self::Scenes => "All scenes",
            Self::Tags => "All animation tags",
        }
    }

    fn entries(
        self,
        doc: &Document,
        settings: &ExportSettings,
    ) -> Result<Vec<(ExportSettings, PathBuf)>, String> {
        let mut entries = Vec::new();
        if self == Self::Range {
            return Ok(vec![(settings.clone(), settings.target()?)]);
        }
        let mut offset = 0;
        for index in 0..doc.extras.scenes.len().max(1) {
            let scene = crate::animation::scene_doc(doc, index);
            let count = scene.layers[0].frames.len();
            let append = |name: &str,
                          first: usize,
                          last: usize,
                          entries: &mut Vec<_>|
             -> Result<(), String> {
                let slug: String = name
                    .chars()
                    .take(48)
                    .map(|c| {
                        if c.is_alphanumeric() || c == '-' || c == '_' {
                            c
                        } else {
                            '_'
                        }
                    })
                    .collect();
                let mut job = settings.clone();
                job.first = offset + first + 1;
                job.last = offset + last + 1;
                job.name = format!("{}-{:03}-{slug}", settings.name, entries.len() + 1);
                if job.format == 1 {
                    job.svg_sequence = true;
                }
                if job.format == SPRITE_SHEET {
                    job.sheet_size().map_err(|error| tr(error).to_owned())?;
                }
                entries.push((job.clone(), job.target()?));
                Ok(())
            };
            if self == Self::Scenes {
                let name = doc
                    .extras
                    .scenes
                    .get(index)
                    .map_or("Scene", |scene| scene.name.as_str());
                append(name, 0, count - 1, &mut entries)?;
            } else {
                for tag in scene.extras.tags.iter().filter(|tag| tag.start < count) {
                    append(&tag.name, tag.start, tag.end.min(count - 1), &mut entries)?;
                }
            }
            offset += count;
        }
        if entries.is_empty() {
            return Err(tr("No animation tags to export.").into());
        }
        Ok(entries)
    }
}

#[derive(Clone)]
pub struct ExportSettings {
    pub name: String,
    pub folder: String,
    pub format: usize,
    pub first: usize,
    pub last: usize,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub quality: u32,
    pub loops: i32,
    pub colors: u32,
    pub dither: bool,
    pub svg_sequence: bool,
    pub(crate) sheet: SheetSettings,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct SheetSettings {
    columns: u32,
    rows: u32,
    spacing: u32,
}

impl Default for SheetSettings {
    fn default() -> Self {
        Self {
            columns: 4,
            rows: 0,
            spacing: 0,
        }
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Preset {
    name: String,
    format: usize,
    width: u32,
    height: u32,
    fps: u32,
    quality: u32,
    loops: i32,
    colors: u32,
    dither: bool,
    svg_sequence: bool,
    sheet: SheetSettings,
}

impl Preset {
    fn capture(name: String, settings: &ExportSettings) -> Self {
        Self {
            name,
            format: settings.format,
            width: settings.width,
            height: settings.height,
            fps: settings.fps,
            quality: settings.quality,
            loops: settings.loops,
            colors: settings.colors,
            dither: settings.dither,
            svg_sequence: settings.svg_sequence,
            sheet: settings.sheet.clone(),
        }
    }

    fn apply(&self, settings: &mut ExportSettings) {
        settings.format = self.format.min(SPRITE_SHEET);
        settings.width = self.width.clamp(2, 4096);
        settings.height = self.height.clamp(2, 4096);
        settings.fps = self.fps.clamp(1, 120);
        settings.quality = self.quality.clamp(1, 100);
        settings.loops = self.loops.clamp(0, 100);
        settings.colors = self.colors.clamp(2, 256);
        settings.dither = self.dither;
        settings.svg_sequence = self.svg_sequence;
        settings.sheet = self.sheet.clone();
    }
}

const SPRITE_SHEET: usize = 7;

impl ExportSettings {
    fn sheet_size(&self) -> Result<[u32; 2], &'static str> {
        let count = self.last.saturating_sub(self.first) as u64 + 1;
        if !(1..=10000).contains(&self.sheet.columns)
            || self.sheet.rows > 10000
            || self.sheet.spacing > 256
        {
            return Err("Invalid sprite sheet grid.");
        }
        let columns = u64::from(self.sheet.columns);
        let rows = if self.sheet.rows == 0 {
            count.div_ceil(columns)
        } else {
            u64::from(self.sheet.rows)
        };
        if rows * columns < count {
            return Err("The grid needs at least one cell per frame.");
        }
        let spacing = u64::from(self.sheet.spacing);
        let width = columns * u64::from(self.width) + (columns - 1) * spacing;
        let height = rows * u64::from(self.height) + (rows - 1) * spacing;
        if width > 16384 || height > 16384 || width * height > 64 * 1024 * 1024 {
            return Err("Sprite sheet is too large. Reduce the frame size, range or spacing.");
        }
        Ok([width as u32, height as u32])
    }
}

const FORMATS: &[&str] = &["MP4", "SVG", "MKV", "AVI", "MOV", "GIF", "PNG sequence"];
impl ExportSettings {
    pub(crate) fn use_png_sequence(&mut self) {
        self.format = 6;
    }
    fn extension(&self) -> &str {
        match self.format {
            0 => "mp4",
            1 => "svg",
            2 => "mkv",
            3 => "avi",
            4 => "mov",
            6 | SPRITE_SHEET => "png",
            _ => "gif",
        }
    }
    fn target(&self) -> Result<PathBuf, String> {
        if self.name.trim().is_empty()
            || self
                .name
                .chars()
                .any(|c| r#"<>:"/\|?*"#.contains(c) || c.is_control())
            || self.name.ends_with(['.', ' '])
        {
            return Err(tr("Nombre de archivo no válido").into());
        }
        let folder = PathBuf::from(&self.folder);
        if !folder.is_dir() {
            return Err(tr("La carpeta no existe").into());
        }
        let mut path = folder.join(self.name.trim());
        path.set_extension(self.extension());
        Ok(path)
    }
}
fn h264_crf(quality: u32) -> u32 {
    if quality >= 100 {
        0
    } else {
        40 - (quality.clamp(1, 99) - 1) * 30 / 98
    }
}
pub(crate) fn hidden(_command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        _command.creation_flags(0x08000000);
    }
}
pub fn ffmpeg_path(configured: &str) -> PathBuf {
    crate::ffmpeg_setup::available(configured).unwrap_or_else(|| {
        if configured.is_empty() {
            PathBuf::from("ffmpeg")
        } else {
            PathBuf::from(configured)
        }
    })
}
impl Editor {
    pub(crate) fn open_sprite_sheet_export(&mut self) {
        self.open_export(false);
        if let Some(settings) = &mut self.studio.export {
            settings.format = SPRITE_SHEET;
            settings.name = "spritesheet".into();
            // A compact starting size keeps a full animation in a practical texture.
            let count = (settings.last - settings.first + 1) as f32;
            settings.sheet.columns = (count.sqrt().ceil() as u32).clamp(1, 100);
            let scale = (2048.
                / (settings.sheet.columns as f32 * settings.width.max(settings.height) as f32))
                .min(1.);
            settings.width = ((settings.width as f32 * scale).round() as u32).max(2);
            settings.height = ((settings.height as f32 * scale).round() as u32).max(2);
        }
    }

    pub fn open_export(&mut self, gif: bool) {
        self.leave_symbol_context_for_document_change();
        self.finish_pending_pen();
        let folder = self
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| {
                std::env::current_dir()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            });
        self.studio.export_batch = ExportBatch::Range;
        self.studio.export = Some(ExportSettings {
            name: "animation".into(),
            folder,
            format: if gif { 5 } else { 0 },
            first: 1,
            last: crate::animation::movie_count(&self.doc),
            width: ((self.doc.width as f32
                * (4096. / self.doc.width.max(self.doc.height) as f32).min(1.))
            .round() as u32)
                .max(2),
            height: ((self.doc.height as f32
                * (4096. / self.doc.width.max(self.doc.height) as f32).min(1.))
            .round() as u32)
                .max(2),
            fps: if gif {
                self.doc.fps.min(100)
            } else {
                self.doc.fps
            },
            quality: 95,
            loops: 0,
            colors: 256,
            dither: true,
            svg_sequence: false,
            sheet: SheetSettings::default(),
        });
    }
    fn select_external_ffmpeg(&mut self, folder: bool) {
        let selected = if folder {
            rfd::FileDialog::new()
                .pick_folder()
                .and_then(|p| crate::ffmpeg_setup::in_folder(&p))
        } else {
            let dialog = rfd::FileDialog::new();
            let dialog = if cfg!(windows) {
                dialog.add_filter("FFmpeg", &["exe"])
            } else {
                dialog
            };
            dialog.pick_file()
        };
        if let Some(path) = selected {
            self.studio.prefs.ffmpeg = path.to_string_lossy().into_owned();
            self.studio.ffmpeg_setup.prompt = false;
            self.studio.ffmpeg_setup.error = None;
            if let Err(e) = self.studio.save() {
                self.status = e;
            }
        }
    }
    fn ffmpeg_setup_ui(&mut self, ctx: &egui::Context) -> bool {
        use std::sync::mpsc::TryRecvError;
        let result = self
            .studio
            .ffmpeg_setup
            .job
            .as_ref()
            .map(|rx| rx.try_recv());
        match result {
            Some(Ok(Ok(path))) => {
                self.studio.ffmpeg_setup.job = None;
                self.studio.prefs.ffmpeg = path.to_string_lossy().into_owned();
                self.studio.ffmpeg_setup.complete = true;
                self.studio
                    .ffmpeg_setup
                    .progress
                    .store(1000, Ordering::Relaxed);
                if let Err(e) = self.studio.save() {
                    self.status = e;
                }
            }
            Some(Ok(Err(e))) => {
                self.studio.ffmpeg_setup.job = None;
                self.studio.ffmpeg_setup.error = Some(e);
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.studio.ffmpeg_setup.job = None;
                self.studio.ffmpeg_setup.error = Some("FFmpeg setup was interrupted".into());
            }
            _ => {}
        }
        if !self.studio.ffmpeg_setup.prompt {
            return false;
        }
        let mut action = 0;
        egui::Modal::new(egui::Id::unique("ffmpeg-setup")).show(ctx,|ui|{
            ui.set_max_width(440.);
            ui.heading(tr("FFmpeg setup"));
            if self.studio.ffmpeg_setup.complete {
                ui.add(egui::ProgressBar::new(1.).show_percentage());
                ui.label(tr("FFmpeg downloaded and configured in LibreToon."));
                if ui.button(tr("Continue")).clicked() {action=5;}
            } else if self.studio.ffmpeg_setup.job.is_some() {
                let phase=self.studio.ffmpeg_setup.phase.load(Ordering::Relaxed);
                ui.label(tr(match phase {
                    0=>"Downloading FFmpeg…",
                    1=>"Verifying download…",
                    2=>"Creating the FFmpeg folder…",
                    _=>"Configuring FFmpeg in LibreToon…"
                }));
                let value=self.studio.ffmpeg_setup.progress.load(Ordering::Relaxed) as f32/1000.;
                ui.add(egui::ProgressBar::new(value).show_percentage());
                if ui.button(tr("Cancelar")).clicked() {self.studio.ffmpeg_setup.cancel.store(true,Ordering::Relaxed);}
            } else {
                ui.label(tr("This exporter needs FFmpeg to work. Download and configure it automatically?"));
                ui.small(tr("FFmpeg is downloaded separately from Imageio/PyPI and saved in your user data folder."));
                ui.hyperlink_to(tr("Download provider"),"https://github.com/imageio/imageio-ffmpeg");
                if let Some(error)=&self.studio.ffmpeg_setup.error {
                    ui.colored_label(egui::Color32::LIGHT_RED,tr("FFmpeg setup could not finish. Retry or select it manually."));
                    ui.collapsing(tr("Details"),|ui|{ui.label(crate::i18n::tr(error));});
                }
                if ui.button(tr("Download and configure automatically")).clicked(){action=1;}
                ui.horizontal(|ui|{
                    if ui.button(tr("Select executable…")).clicked(){action=2;}
                    if ui.button(tr("Select folder…")).clicked(){action=3;}
                });
                if ui.button(tr("Not now")).clicked(){action=5;}
            }
        });
        match action {
            1 => self.studio.ffmpeg_setup.start(),
            2 => self.select_external_ffmpeg(false),
            3 => self.select_external_ffmpeg(true),
            5 => {
                self.studio.ffmpeg_setup.prompt = false;
                self.studio.ffmpeg_setup.complete = false;
            }
            _ => {}
        }
        if self.studio.ffmpeg_setup.job.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        true
    }
    pub fn export_ui(&mut self, ctx: &egui::Context) {
        if self.ffmpeg_setup_ui(ctx) {
            return;
        }
        if let Some(receiver) = &self.studio.export_job {
            match receiver.try_recv() {
                Ok(result) => {
                    self.status = match result {
                        Ok(p) => format!("{} {}", tr("Exportado:"), p.display()),
                        Err(e) => e,
                    };
                    self.studio.export_job = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.status = tr("La exportación se interrumpió").into();
                    self.studio.export_job = None;
                }
                _ => {}
            }
        }
        if self.studio.export_job.is_some() {
            egui::Window::new(tr("Exportando…"))
                .id(egui::Id::unique("export-progress"))
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    let value = self.studio.export_progress.load(Ordering::Relaxed) as f32 / 1000.;
                    let done = self.studio.export_batch_index.load(Ordering::Relaxed);
                    let total = self.studio.export_batch_total;
                    if total > 1 {
                        ui.label(format!(
                            "{} {}/{}",
                            tr("Export queue"),
                            (done + 1).min(total),
                            total
                        ));
                    }
                    ui.add(
                        egui::ProgressBar::new(
                            ((done as f32 + value) / total as f32).clamp(0., 1.),
                        )
                        .show_percentage(),
                    );
                    if ui.button(tr("Cancelar")).clicked() {
                        self.studio.export_cancel.store(true, Ordering::Relaxed);
                    }
                });
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        let Some(mut s) = self.studio.export.take() else {
            return;
        };
        let mut open = true;
        let mut start = false;
        egui::Window::new(tr(match s.format {
            SPRITE_SHEET => "Export to Sprite Sheet…",
            5 => "Exportar GIF…",
            _ => "Exportar vídeo…",
        }))
        .id(egui::Id::unique("export-dialog"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .show(ctx, |ui| {
            egui::Grid::new("export-fields")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.label(tr("Nombre"));
                    ui.text_edit_singleline(&mut s.name);
                    ui.end_row();
                    ui.label(tr("Carpeta"));
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut s.folder);
                        if ui.button(crate::i18n::tr("…")).clicked() {
                            if let Some(p) = rfd::FileDialog::new()
                                .set_directory(&s.folder)
                                .pick_folder()
                            {
                                s.folder = p.to_string_lossy().into_owned();
                            }
                        }
                    });
                    ui.end_row();
                    ui.label(tr("Formato"));
                    if s.format == 5 {
                        ui.label(crate::i18n::tr("GIF"));
                    } else if s.format == SPRITE_SHEET {
                        ui.label(tr("Sprite sheet (PNG)"));
                    } else {
                        egui::ComboBox::from_id_salt("export-format")
                            .selected_text(tr(FORMATS[s.format]))
                            .show_ui(ui, |ui| {
                                for i in [0, 1, 2, 3, 4, 6] {
                                    ui.selectable_value(&mut s.format, i, tr(FORMATS[i]));
                                }
                            });
                    }
                    ui.end_row();
                    if s.format == 1 {
                        ui.label(tr("SVG"));
                        ui.checkbox(&mut s.svg_sequence, tr("Secuencia de fotogramas"));
                        ui.end_row();
                    } else {
                        ui.label(tr("Resolución"));
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut s.width).range(2..=4096));
                            ui.label(crate::i18n::tr("×"));
                            ui.add(egui::DragValue::new(&mut s.height).range(2..=4096));
                            if ui
                                .small_button("↔")
                                .on_hover_text(tr("Conservar proporción"))
                                .clicked()
                            {
                                s.height = ((s.width as f32 * self.doc.height as f32
                                    / self.doc.width as f32)
                                    .round() as u32)
                                    .clamp(2, 4096);
                            }
                        });
                        ui.end_row();
                        if !matches!(s.format, 6 | SPRITE_SHEET) {
                        ui.label(crate::i18n::tr("FPS"));
                        ui.add(
                            egui::DragValue::new(&mut s.fps)
                                .range(1..=if s.format == 5 { 100 } else { 240 }),
                        );
                        ui.end_row();
                        }
                    }
                    if s.format != 1 || s.svg_sequence {
                        ui.label(tr("Fotogramas"));
                        ui.horizontal(|ui| {
                            let count = crate::animation::movie_count(&self.doc);
                            ui.add(egui::DragValue::new(&mut s.first).range(1..=count));
                            ui.label(crate::i18n::tr("–"));
                            ui.add(egui::DragValue::new(&mut s.last).range(s.first..=count));
                        });
                        ui.end_row();
                    }
                    if s.format == SPRITE_SHEET {
                        ui.label(tr("Columns"));
                        ui.add(egui::DragValue::new(&mut s.sheet.columns).range(1..=10000));
                        ui.end_row();
                        ui.label(tr("Rows (0 = automatic)"));
                        ui.add(egui::DragValue::new(&mut s.sheet.rows).range(0..=10000));
                        ui.end_row();
                        ui.label(tr("Cell spacing (px)"));
                        ui.add(egui::DragValue::new(&mut s.sheet.spacing).range(0..=256));
                        ui.end_row();
                    }
                    if matches!(s.format,0|2|3|4) {
                        ui.label(tr("Export quality (1–100)"));
                        ui.horizontal(|ui|{
                            ui.add(egui::Slider::new(&mut s.quality,1..=100));
                            if ui.small_button(tr("Maximum")).clicked(){s.quality=100;}
                        });
                        ui.end_row();
                    }
                    match s.format {
                        0 | 2 => {
                            ui.label(tr("Códec"));
                            ui.label(tr(if s.quality>=100 {"H.264 RGB · lossless"} else {"H.264 / yuv420p"}));
                            ui.end_row();
                        }
                        3 => {
                            ui.label(tr("Códec"));
                            ui.label(crate::i18n::tr("MPEG-4 / AVI"));
                            ui.end_row();
                        }
                        4 => {
                            ui.label(tr("Códec"));
                            ui.label(tr(if s.quality>=100 {"Apple ProRes 4444 XQ"} else {"Apple ProRes 422 HQ"}));
                            ui.end_row();
                        }
                        5 => {
                            ui.label(tr("Colores"));
                            ui.add(egui::Slider::new(&mut s.colors, 4..=256));
                            ui.end_row();
                            ui.label(tr("Repeticiones (0 = infinito)"));
                            ui.add(egui::DragValue::new(&mut s.loops).range(0..=100));
                            ui.end_row();
                            ui.label(tr("Tramado"));
                            ui.checkbox(&mut s.dither, crate::i18n::tr("Sierra"));
                            ui.end_row();
                        }
                        _ => {}
                    }
                });
            if s.format == 1 && !s.svg_sequence {
                ui.label(tr("Exporta el fotograma actual como SVG vectorial."));
            }
            if matches!(s.format,0|2) && s.quality>=100 {
                ui.label(tr("100 preserves RGB pixels without loss. Files are larger and some players may require quality 99."));
            }
            if s.format == 5 {
                ui.label(tr(
                    "GIF usa intervalos de centésimas de segundo; los FPS pueden redondearse.",
                ));
            }
            if !matches!(s.format, 5 | SPRITE_SHEET) {
                ui.horizontal(|ui| {
                    ui.label(tr("Export presets"));
                    if ui.small_button(tr("Project size")).clicked() {
                        s.width = self.doc.width.clamp(2, 4096);
                        s.height = self.doc.height.clamp(2, 4096);
                        s.fps = self.doc.fps;
                    }
                    if ui.small_button(tr("Preview MP4")).clicked() {
                        s.format = 0;
                        let scale = (1280. / self.doc.width as f32).min(720. / self.doc.height as f32).min(1.);
                        s.width = ((self.doc.width as f32 * scale).round() as u32).max(2);
                        s.height = ((self.doc.height as f32 * scale).round() as u32).max(2);
                        s.quality = 75;
                    }
                    if ui.small_button(tr("Transparent PNG")).clicked() { s.format = 6; }
                });
            }
            egui::CollapsingHeader::new(tr("Saved export presets")).show(ui, |ui| {
                let mut save = false;
                let mut remove = None;
                ui.horizontal(|ui| {
                    let id = ui.scope_id().with("export-preset-name");
                    let mut name = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_default();
                    ui.add(egui::TextEdit::singleline(&mut name).char_limit(48).hint_text(tr("Preset name")));
                    if crate::design::button(
                        ui, crate::design::Icon::Save, "", "Save export preset", false,
                        !name.trim().is_empty() && self.studio.prefs.export_presets.len() < 24,
                    ).clicked() {
                        self.studio.prefs.export_presets.push(Preset::capture(name.trim().to_owned(), &s));
                        name.clear();
                        save = true;
                    }
                    ui.data_mut(|d| d.insert_temp(id, name));
                });
                egui::ScrollArea::vertical().max_height(100.).show(ui, |ui| {
                    for (index, preset) in self.studio.prefs.export_presets.iter().enumerate() {
                        ui.horizontal(|ui| {
                            if ui.button(&preset.name).clicked() {
                                preset.apply(&mut s);
                            }
                            if crate::design::button(ui, crate::design::Icon::Trash, "", "Delete export preset", false, true).clicked() {
                                remove = Some(index);
                            }
                        });
                    }
                });
                if let Some(index) = remove {
                    self.studio.prefs.export_presets.remove(index);
                    save = true;
                }
                if save && let Err(error) = self.studio.save() {
                    self.status = error;
                }
            });
            if s.format == SPRITE_SHEET {
                match s.sheet_size() {
                    Ok([width, height]) => {
                        ui.label(format!("{}  {width} × {height} px", tr("Sprite sheet (PNG)")));
                    }
                    Err(error) => {
                        ui.colored_label(egui::Color32::LIGHT_RED, tr(error));
                    }
                }
                ui.label(tr("Frames fill the grid left to right, then top to bottom. Empty cells are transparent."));
            } else if s.format == 6 {
                ui.label(tr("One transparent PNG per project frame. Includes visible video; excludes audio."));
            } else if s.format == 1 {
                ui.label(tr("SVG exports vector artwork. Raster paint, adjustment effects, audio and video are omitted."));
            } else {
                ui.label(tr("Rendered video uses a white background."));
            }
            if !matches!(s.format, 1 | 6 | SPRITE_SHEET) {
                ui.group(|ui| {
                    ui.label(tr("FFmpeg executable"));
                    if ui
                        .text_edit_singleline(&mut self.studio.prefs.ffmpeg)
                        .lost_focus()
                    {
                        let _ = self.studio.save();
                    }
                    ui.horizontal(|ui| {
                        if ui.button(tr("Select executable…")).clicked() {
                            self.select_external_ffmpeg(false);
                        }
                        if ui.button(tr("Select folder…")).clicked() {
                            self.select_external_ffmpeg(true);
                        }
                    });
                    if ui
                        .button(tr("Download and configure automatically"))
                        .clicked()
                    {
                        self.studio.ffmpeg_setup.prompt = true;
                        self.studio.ffmpeg_setup.complete = false;
                    }
                });
            }
            ui.separator();
            egui::ComboBox::from_id_salt("export-batch").selected_text(tr(self.studio.export_batch.label())).show_ui(ui, |ui| {
                for mode in [ExportBatch::Range, ExportBatch::Scenes, ExportBatch::Tags] {
                    ui.selectable_value(&mut self.studio.export_batch, mode, tr(mode.label()));
                }
            });
            let range = if self.studio.export_batch == ExportBatch::Range { format!("{}–{}", s.first, s.last) } else { tr(self.studio.export_batch.label()).into() };
            let audio = if matches!(s.format, 0 | 2 | 3 | 4) { "Audio included when present" } else { "No audio in this export" };
            ui.small(format!("{} × {} · {} fps · {} · {}", s.width, s.height, s.fps, range, tr(audio)));
            if ui.add_enabled(self.studio.export_batch != ExportBatch::Range || s.format != SPRITE_SHEET || s.sheet_size().is_ok(), egui::Button::new(tr("Exportar"))).clicked() {
                if !matches!(s.format, 1 | 6 | SPRITE_SHEET)
                    && crate::ffmpeg_setup::available(&self.studio.prefs.ffmpeg).is_none()
                {
                    self.studio.ffmpeg_setup.prompt = true;
                } else {
                    start = true;
                }
            }
        });
        s.first = s.first.clamp(1, crate::animation::movie_count(&self.doc));
        s.last = s
            .last
            .clamp(s.first, crate::animation::movie_count(&self.doc));
        if start {
            if let Err(e) = self.studio.save() {
                self.status = e;
            }
            let doc = self.project_document();
            match self.studio.export_batch.entries(&doc, &s) {
                Ok(jobs) => {
                    let overwrite = jobs.iter().any(|(job, path)| {
                        if job.format == 6 || (job.format == 1 && job.svg_sequence) {
                            (job.first..=job.last)
                                .any(|f| frame_target(path, f, job.extension()).exists())
                        } else {
                            path.exists()
                        }
                    });
                    if overwrite
                        && rfd::MessageDialog::new()
                            .set_title(tr("Reemplazar archivo"))
                            .set_description(tr("Ya existe un archivo de destino. ¿Reemplazarlo?"))
                            .set_buttons(rfd::MessageButtons::YesNo)
                            .show()
                            != rfd::MessageDialogResult::Yes
                    {
                        self.studio.export = Some(s);
                        return;
                    }
                    let single = self.studio.export_batch == ExportBatch::Range;
                    let current = crate::animation::movie_offset(&doc, self.frame);
                    let ffmpeg = ffmpeg_path(&self.studio.prefs.ffmpeg);
                    self.studio.export_batch_total = jobs.len();
                    self.studio.export_batch_index.store(0, Ordering::Relaxed);
                    let index = self.studio.export_batch_index.clone();
                    self.studio.export_progress.store(0, Ordering::Relaxed);
                    self.studio.export_cancel.store(false, Ordering::Relaxed);
                    let progress = self.studio.export_progress.clone();
                    let cancel = self.studio.export_cancel.clone();
                    let (tx, rx) = std::sync::mpsc::channel();
                    self.studio.export_job = Some(rx);
                    std::thread::spawn(move || {
                        let result = (|| {
                            let mut last = PathBuf::new();
                            for (number, (job, path)) in jobs.into_iter().enumerate() {
                                progress.store(0, Ordering::Relaxed);
                                index.store(number, Ordering::Relaxed);
                                if cancel.load(Ordering::Relaxed) {
                                    return Err(tr("Exportación cancelada").into());
                                }
                                run_export(
                                    &doc,
                                    &job,
                                    if single { current } else { job.first - 1 },
                                    &path,
                                    ffmpeg.clone(),
                                    progress.clone(),
                                    cancel.clone(),
                                )?;
                                last = path;
                            }
                            Ok(last)
                        })();
                        let _ = tx.send(result);
                    });
                }
                Err(e) => {
                    self.status = e;
                    self.studio.export = Some(s);
                }
            }
        } else if open {
            self.studio.export = Some(s);
        }
    }
}
fn frame_target(path: &std::path::Path, frame: usize, extension: &str) -> PathBuf {
    path.with_file_name(format!(
        "{}-{frame:05}.{extension}",
        path.file_stem().unwrap_or_default().to_string_lossy()
    ))
}
fn write_png(path: &std::path::Path, rgba: &[u8], width: u32, height: u32) -> Result<(), String> {
    let temporary = path.with_file_name(format!(".libretoon-png-{}.tmp", crate::features::id()));
    let result = image::save_buffer_with_format(
        &temporary,
        rgba,
        width,
        height,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .map_err(|error| error.to_string())
    .and_then(|()| std::fs::rename(&temporary, path).map_err(|error| error.to_string()));
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

pub(crate) fn run_export(
    doc: &Document,
    s: &ExportSettings,
    current: usize,
    path: &std::path::Path,
    ffmpeg: PathBuf,
    progress: Arc<AtomicUsize>,
    cancel: Arc<AtomicBool>,
) -> Result<(), String> {
    if s.format > SPRITE_SHEET
        || s.first == 0
        || s.first > s.last
        || s.last > crate::animation::movie_count(doc)
        || !(1..=240).contains(&s.fps)
        || (s.format != 1 && (!(2..=4096).contains(&s.width) || !(2..=4096).contains(&s.height)))
    {
        return Err("Parámetros de exportación fuera de rango".into());
    }
    if s.format == 1 {
        if s.svg_sequence {
            for f in s.first..=s.last {
                if cancel.load(Ordering::Relaxed) {
                    return Err(tr("Exportación cancelada").into());
                }
                let p = path.with_file_name(format!(
                    "{}-{f:05}.svg",
                    path.file_stem().unwrap().to_string_lossy()
                ));
                std::fs::write(p, crate::animation::movie_svg(doc, f - 1))
                    .map_err(|e| e.to_string())?;
                progress.store(
                    (f - s.first + 1) * 1000 / (s.last - s.first + 1),
                    Ordering::Relaxed,
                );
            }
        } else {
            std::fs::write(path, crate::animation::movie_svg(doc, current))
                .map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    if matches!(s.format, 6 | SPRITE_SHEET) {
        let mut sheet = if s.format == SPRITE_SHEET {
            let [width, height] = s.sheet_size().map_err(|error| tr(error).to_owned())?;
            Some(image::RgbaImage::new(width, height))
        } else {
            None
        };
        let context = egui::Context::default();
        let mut cache = crate::paint::RenderCache::default();
        let mut video = crate::media::VideoExport::default();
        for frame in s.first..=s.last {
            if cancel.load(Ordering::Relaxed) {
                return Err(tr("Exportación cancelada").into());
            }
            let frames = video.frames(doc, frame - 1, &ffmpeg)?;
            let mut rgba = raster_composited(
                doc,
                frame - 1,
                s.width,
                s.height,
                &context,
                &mut cache,
                &frames,
                None,
                true,
                true,
                true,
                true,
                true,
                None,
            );
            for pixel in rgba.chunks_exact_mut(4) {
                let alpha = u32::from(pixel[3]);
                if alpha != 0 {
                    for channel in &mut pixel[..3] {
                        *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
                    }
                }
            }
            if let Some(sheet) = &mut sheet {
                let index = (frame - s.first) as u32;
                let column = index % s.sheet.columns;
                let row = index / s.sheet.columns;
                let x = column * (s.width + s.sheet.spacing);
                let y = row * (s.height + s.sheet.spacing);
                let source = image::RgbaImage::from_raw(s.width, s.height, rgba)
                    .ok_or("Invalid rendered frame size")?;
                image::GenericImage::copy_from(sheet, &source, x, y)
                    .map_err(|error| error.to_string())?;
            } else {
                write_png(&frame_target(path, frame, "png"), &rgba, s.width, s.height)?;
            }
            progress.store(
                (frame - s.first + 1) * if sheet.is_some() { 990 } else { 1000 }
                    / (s.last - s.first + 1),
                Ordering::Relaxed,
            );
        }
        if let Some(sheet) = sheet {
            if cancel.load(Ordering::Relaxed) {
                return Err(tr("Exportación cancelada").into());
            }
            write_png(path, sheet.as_raw(), sheet.width(), sheet.height())?;
            progress.store(1000, Ordering::Relaxed);
        }
        return Ok(());
    }
    let w = s.width;
    let h = s.height;
    let temp = path.with_file_name(format!(
        ".libretoon-export-{}-{}.{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        s.extension()
    ));
    let mut command = Command::new(&ffmpeg);
    hidden(&mut command);
    command.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostdin",
        "-y",
        "-f",
        "rawvideo",
        "-pixel_format",
        "rgb24",
        "-video_size",
        &format!("{w}x{h}"),
        "-framerate",
        &s.fps.to_string(),
        "-i",
        "pipe:0",
    ]);
    let _audio = if s.format == 5 {
        command.arg("-an");
        Vec::new()
    } else {
        crate::media::audio_export(doc, &mut command, s.first - 1, s.last, s.format == 3, true)?
    };
    match s.format {
        0 | 2 => {
            command.args([
                "-vf",
                "pad=ceil(iw/2)*2:ceil(ih/2)*2:color=white",
                "-c:v",
                if s.quality >= 100 {
                    "libx264rgb"
                } else {
                    "libx264"
                },
                "-pix_fmt",
                if s.quality >= 100 { "rgb24" } else { "yuv420p" },
                "-preset",
                "slow",
                "-crf",
                &h264_crf(s.quality).to_string(),
            ]);
            if s.format == 0 {
                command.args(["-movflags", "+faststart"]);
            }
        }
        3 => {
            command.args([
                "-vf",
                "pad=ceil(iw/2)*2:ceil(ih/2)*2:color=white",
                "-c:v",
                "mpeg4",
                "-q:v",
                &(1 + (100 - s.quality.clamp(1, 100)) * 30 / 99).to_string(),
            ]);
        }
        4 => {
            command.args([
                "-vf",
                "pad=ceil(iw/2)*2:ceil(ih/2)*2:color=white",
                "-c:v",
                "prores_ks",
                "-profile:v",
                if s.quality >= 100 { "5" } else { "3" },
                "-pix_fmt",
                if s.quality >= 100 {
                    "yuv444p10le"
                } else {
                    "yuv422p10le"
                },
                "-q:v",
                &(1 + (100 - s.quality.clamp(1, 100)) * 30 / 99).to_string(),
            ]);
        }
        5 => {
            let filter = format!(
                "split[a][b];[a]palettegen=max_colors={}:stats_mode=single[p];[b][p]paletteuse=new=1:dither={}",
                s.colors,
                if s.dither { "sierra2_4a" } else { "none" }
            );
            command.args(["-filter_complex", &filter, "-loop", &s.loops.to_string()]);
        }
        _ => return Err("Unsupported format".into()),
    }
    command
        .arg(&temp)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| {
        format!(
            "{}: {e}",
            tr("Could not start FFmpeg. Configure it in the export dialog.")
        )
    })?;
    let mut stderr = child.stderr.take().unwrap();
    let log = std::thread::spawn(move || read_log(&mut stderr));
    let mut input = child.stdin.take().unwrap();
    let ctx = egui::Context::default();
    let mut cache = crate::paint::RenderCache::default();
    let count = s.last - s.first + 1;
    let frames = ((count as f64 * s.fps as f64 / doc.fps as f64).ceil() as usize).max(1);
    let mut video = crate::media::VideoExport::default();
    let result = (|| {
        for index in 0..frames {
            if cancel.load(Ordering::Relaxed) {
                return Err(tr("Exportación cancelada").into());
            }
            let frame = s.first - 1
                + ((index as u64 * doc.fps as u64 / s.fps as u64) as usize).min(count - 1);
            let video_frames = video.frames(doc, frame, &ffmpeg)?;
            let pixels = raster_with_video(doc, frame, w, h, &ctx, &mut cache, &video_frames);
            input.write_all(&pixels).map_err(|e| e.to_string())?;
            progress.store((index + 1) * 950 / frames, Ordering::Relaxed);
        }
        Ok(())
    })();
    drop(input);
    if result.is_err() {
        let _ = child.kill();
    }
    // Encoding also supports cancellation after all frames were sent.
    let status = loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => break Err(e.to_string()),
        }
    };
    let log = log.join().unwrap_or_default();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&temp);
        return Err(if log.is_empty() {
            e
        } else {
            format!("{e}\n{log}")
        });
    }
    if cancel.load(Ordering::Relaxed) {
        let _ = std::fs::remove_file(&temp);
        return Err(tr("Exportación cancelada").into());
    }
    if !status?.success() {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("FFmpeg: {log}"));
    }
    std::fs::rename(&temp, path).map_err(|e| e.to_string())?;
    progress.store(1000, Ordering::Relaxed);
    Ok(())
}
/// Rasterize the same tessellated geometry as the editor, with no UI or onion skin.
pub(crate) fn raster(
    doc: &Document,
    frame: usize,
    w: u32,
    h: u32,
    ctx: &egui::Context,
    cache: &mut crate::paint::RenderCache,
) -> Vec<u8> {
    raster_with_video(doc, frame, w, h, ctx, cache, &[])
}

pub(crate) fn raster_with_video(
    doc: &Document,
    frame: usize,
    w: u32,
    h: u32,
    ctx: &egui::Context,
    cache: &mut crate::paint::RenderCache,
    video: &[crate::media::ExportVideoFrame],
) -> Vec<u8> {
    raster_composited(
        doc, frame, w, h, ctx, cache, video, None, false, true, true, true, true, None,
    )
}
pub(crate) fn raster_preview(
    doc: &Document,
    frame: usize,
    clock: usize,
    w: u32,
    h: u32,
    ctx: &egui::Context,
    cache: &mut crate::paint::RenderCache,
    video: &[crate::media::ExportVideoFrame],
    camera: bool,
    post: bool,
    blur: bool,
    full_quality: bool,
    work: Option<&crate::depth::PreviewWork>,
) -> Vec<u8> {
    raster_composited(
        doc,
        frame,
        w,
        h,
        ctx,
        cache,
        video,
        Some(clock),
        false,
        post,
        blur,
        camera,
        full_quality,
        work,
    )
}
// Keep a single shaded image per receiver layer. Keys own their shared mark
// geometry and media storage, so reuse never relies on recycled addresses.
#[derive(PartialEq)]
struct LitPlaneKey {
    layer: usize,
    dimensions: [u32; 5],
    region: Option<LitRegion>,
    artwork: Vec<(usize, usize, crate::Mark)>,
    lights: Vec<(crate::light::ShadingKey, f32)>,
    guide: bool,
    mask_roots: std::collections::HashSet<usize>,
    scene_mask: Option<u64>,
}

impl LitPlaneKey {
    fn same_source(&self, other: &Self) -> bool {
        self.layer == other.layer
            && self.dimensions == other.dimensions
            && self.region == other.region
            && self.guide == other.guide
            && self.mask_roots == other.mask_roots
            && self.scene_mask == other.scene_mask
            && self.artwork == other.artwork
    }
}

struct LitInput {
    key: Arc<LitPlaneKey>,
    assets: Option<crate::shared::SharedVec<crate::media::Asset>>,
    subset: RenderSubset,
    pixels: Vec<u8>,
}

struct LitPlane {
    key: Arc<LitPlaneKey>,
    assets: Option<crate::shared::SharedVec<crate::media::Asset>>,
    pixels: std::sync::Arc<Vec<u8>>,
}

#[derive(Default)]
pub(crate) struct LitPlanes {
    planes: std::collections::VecDeque<LitPlane>,
    inputs: std::collections::VecDeque<LitInput>,
    surfaces: crate::light::SurfaceCache,
    #[cfg(test)]
    hits: usize,
}

impl LitPlanes {
    fn input(
        &mut self,
        key: &LitPlaneKey,
        assets: Option<usize>,
        subset: RenderSubset,
    ) -> Option<Vec<u8>> {
        let index = self.inputs.iter().position(|input| {
            input.subset == subset
                && input.key.same_source(key)
                && input.assets.as_ref().map(|assets| assets.identity()) == assets
        })?;
        let input = self.inputs.remove(index).unwrap();
        let pixels = input.pixels.clone();
        self.inputs.push_back(input);
        Some(pixels)
    }

    fn insert_input(&mut self, input: LitInput) {
        const LIMIT: usize = 64 * 1024 * 1024;
        if input.pixels.len() > LIMIT {
            return;
        }
        let mut bytes =
            input.pixels.len() + self.inputs.iter().map(|i| i.pixels.len()).sum::<usize>();
        while bytes > LIMIT || self.inputs.len() >= 32 {
            bytes -= self.inputs.pop_front().unwrap().pixels.len();
        }
        self.inputs.push_back(input);
    }
    fn get(&mut self, key: &LitPlaneKey, assets: Option<usize>) -> Option<Arc<Vec<u8>>> {
        let index = self.planes.iter().position(|plane| {
            plane.key.layer == key.layer
                && plane.assets.as_ref().map(|assets| assets.identity()) == assets
                && plane.key.as_ref() == key
        })?;
        let plane = self.planes.remove(index).unwrap();
        let pixels = plane.pixels.clone();
        self.planes.push_back(plane);
        #[cfg(test)]
        {
            self.hits += 1;
        }
        Some(pixels)
    }

    fn insert(&mut self, plane: LitPlane) {
        const BUDGET: usize = 96 * 1024 * 1024;
        let size = plane.pixels.len();
        if size > BUDGET {
            return;
        }
        self.planes
            .retain(|cached| cached.key.layer != plane.key.layer);
        let mut bytes = size
            + self
                .planes
                .iter()
                .map(|cached| cached.pixels.len())
                .sum::<usize>();
        while bytes > BUDGET || self.planes.len() >= 32 {
            bytes -= self.planes.pop_front().unwrap().pixels.len();
        }
        self.planes.push_back(plane);
    }
}

fn raster_composited(
    doc: &Document,
    frame: usize,
    w: u32,
    h: u32,
    ctx: &egui::Context,
    cache: &mut crate::paint::RenderCache,
    video: &[crate::media::ExportVideoFrame],
    clock: Option<usize>,
    transparent: bool,
    post: bool,
    camera_blur: bool,
    camera: bool,
    full_quality: bool,
    work: Option<&crate::depth::PreviewWork>,
) -> Vec<u8> {
    let canceled = || work.is_some_and(crate::depth::PreviewWork::canceled);
    let report = |percent| {
        if let Some(work) = work {
            work.report(percent);
        }
    };
    if canceled() {
        return Vec::new();
    }
    report(5);
    cache.begin_frame();
    let (scene, local) = crate::animation::locate(doc, frame);
    let scene = crate::animation::scene_doc(doc, scene);
    let adjust = post
        && scene
            .layers
            .iter()
            .any(|layer| crate::post::visible_effects_at(layer, local));
    let scale = (w as f32 / doc.width as f32).min(h as f32 / doc.height as f32);
    let active_camera = crate::depth::camera(&scene, local);
    let camera_optics = active_camera.map(|c| crate::animation::camera_optics(c, local));
    let optics = camera_optics
        .as_ref()
        .filter(|o| camera_blur && o.strength > 0.);
    let camera_track = active_camera.filter(|_| camera);
    let mut resolved =
        crate::features::resolve_preview(&scene, local, false, clock.unwrap_or(local));
    if let Some(optics) = camera_optics.as_ref().filter(|_| camera_track.is_some()) {
        resolved.retain(|(layer, _, _)| {
            crate::depth::visible(&scene.layers[*layer].options.depth, optics)
        });
    }
    let has_guides = resolved
        .iter()
        .any(|(_, _, mark)| mark.style.surface_height.is_some());
    let has_masks = resolved.iter().any(|(_, _, mark)| mark.style.light_mask);
    let lights: Vec<_> = scene
        .layers
        .iter()
        .enumerate()
        .filter_map(|(index, layer)| {
            let light = layer.options.light.as_ref()?;
            (post
                && layer.visible
                && crate::folder_groups::layer_active(&scene, layer, local)
                && (video.is_empty()
                    || !light.receivers.is_empty()
                    || light.shadow.is_some()
                    || has_guides
                    || has_masks && light.uses_mask()))
            .then_some((index, light, layer.options.opacity))
        })
        .collect();
    let world = if let (Some(track), Some(optics)) = (camera_track, camera_optics.as_ref()) {
        let world = (!lights.is_empty()).then(|| resolved.clone());
        crate::features::project_camera_marks(&scene, local, track, optics, &mut resolved);
        world
    } else {
        None
    };
    report(20);
    let phase = |value| {
        if let Some(work) = work {
            work.phase(value);
        }
    };
    let scoped_fx: Vec<_> = scene
        .layers
        .iter()
        .enumerate()
        .filter(|(_, layer)| {
            adjust
                && layer.options.fx_target.is_some()
                && crate::post::visible_effects_at(layer, local)
                && crate::folder_groups::layer_active(&scene, layer, local)
        })
        .collect();
    let auto_outlines: Vec<_> = scene
        .layers
        .iter()
        .enumerate()
        .filter_map(|(index, layer)| {
            if !adjust
                || !layer.visible
                || layer.options.fx_target.is_some()
                || !crate::folder_groups::layer_active(&scene, layer, local)
                || !crate::post::effects_at(layer, local)
                    .is_some_and(|effects| effects.iter().any(crate::post::Effect::enabled_outline))
            {
                return None;
            }
            let target = resolved
                .iter()
                .filter_map(|(i, _, mark)| {
                    (*i < index
                        && !crate::post::is_adjustment(&scene.layers[*i])
                        && mark.style.surface_height.is_none()
                        && !mark.style.light_mask)
                        .then_some(*i)
                })
                .max()?;
            Some((index, target, layer))
        })
        .collect();
    // Contiguous folder members can be composited together, which keeps outlines
    // and inner shadows off seams where two drawings overlap.
    let mut groups = Vec::<(usize, usize, u64)>::new();
    for (adjustment, row) in &scoped_fx {
        let Some(crate::light::Receiver::Folder(id)) = row.options.fx_target else {
            continue;
        };
        let indices: Vec<_> = scene
            .layers
            .iter()
            .take(*adjustment)
            .enumerate()
            .filter(|(_, layer)| crate::light::Receiver::Folder(id).matches(&scene, layer))
            .map(|(index, _)| index)
            .collect();
        let (Some(&start), Some(&end)) = (indices.first(), indices.last()) else {
            continue;
        };
        if indices.len() < 2
            || indices.len() != end - start + 1
            || video
                .iter()
                .any(|clip| (start + 1..=end).contains(&clip.slot))
            || scene.layers[start..=end]
                .iter()
                .any(|layer| layer.options.adjustment.is_some() || layer.options.light.is_some())
            || groups.iter().any(|(a, b, _)| start <= *b && *a <= end)
        {
            continue;
        }
        groups.push((start, end, id));
    }
    if optics.is_some() || !lights.is_empty() || !scoped_fx.is_empty() || !auto_outlines.is_empty()
    {
        let channels = if transparent || adjust { 4 } else { 3 };
        let mut result = vec![if channels == 4 { 0 } else { 255 }; (w * h) as usize * channels];
        let mut clips = video.iter().enumerate().peekable();
        let render = |cache: &mut crate::paint::RenderCache,
                      result: &mut Vec<u8>,
                      subset: (bool, usize),
                      plane: &crate::depth::LayerDepth,
                      group: Option<(u64, usize)>| {
            let radius = optics.as_ref().map_or(0., |optics| {
                optics.radius(plane.distance - optics.position) * scale
            });
            let pad = if radius >= 0.1 {
                (radius * 2. + 3.).ceil() as u32
            } else {
                0
            };
            let (pw, ph) = (w + pad * 2, h + pad * 2);
            let layer = (!subset.0).then(|| &scene.layers[subset.1]);
            let target_lights = lights.iter().filter(|(index, light, _)| {
                layer.is_some_and(|layer| subset.1 < *index && light.receives(&scene, layer))
            });
            let use_world = target_lights.clone().next().is_some();
            phase(u8::from(use_world));
            let artwork = if use_world {
                world.as_deref().unwrap_or(&resolved)
            } else {
                &resolved
            };
            let source_pad = if use_world {
                // Keep the sampling origin stable. Include artwork beyond the stage
                // within a bounded overscan budget, independent of the camera pose.
                let bounds = artwork
                    .iter()
                    .filter(|(index, _, _)| *index == subset.1)
                    .fold(Rect::NOTHING, |bounds, (_, _, mark)| {
                        bounds.union(crate::vector::control_bounds(mark).expand(mark.width * 0.5))
                    });
                let extent = (-bounds.min.x)
                    .max(-bounds.min.y)
                    .max(bounds.max.x - doc.width as f32)
                    .max(bounds.max.y - doc.height as f32)
                    .max(0.);
                ((extent * scale + 32.) / 64.).ceil().clamp(1., 8.) as u32 * 64
            } else {
                pad
            };
            let region = if full_quality && use_world {
                camera_track.and_then(|track| {
                    let output = lit_output_view(&scene, local, Some(track), plane, w, h, pad);
                    LitRegion::new(output, scale, [pw, ph], |source_scale, step| {
                        target_lights
                            .clone()
                            .map(|(_, light, _)| light.region_padding(local, source_scale, step))
                            .fold(0., f32::max)
                    })
                })
            } else {
                None
            };
            let draw_art = |cache: &mut crate::paint::RenderCache, subset: RenderSubset| {
                raster_flat_in_view(
                    doc,
                    w,
                    h,
                    ctx,
                    cache,
                    video,
                    Some(subset),
                    artwork,
                    source_pad,
                    true,
                    region.map(|r| (r.size, r.view())),
                )
            };
            let mut raw = if let Some(layer) = layer {
                let has_local_guide = has_guides
                    && use_world
                    && artwork
                        .iter()
                        .any(|(l, _, mark)| *l == subset.1 && mark.style.surface_height.is_some());
                let use_masks =
                    has_masks && target_lights.clone().any(|(_, light, _)| light.uses_mask());
                let mask_roots: std::collections::HashSet<_> = if use_masks {
                    resolved
                        .iter()
                        .filter(|(index, _, mark)| *index == subset.1 && mark.style.light_mask)
                        .map(|(_, root, _)| *root)
                        .collect()
                } else {
                    Default::default()
                };
                let scene_mask_id = layer.options.light_receiver.filter(|&id| {
                    use_masks
                        && artwork
                            .iter()
                            .any(|(_, _, mark)| mark.style.light_mask_target == Some(id))
                });
                let key = use_world.then(|| {
                    Arc::new(LitPlaneKey {
                        layer: subset.1,
                        dimensions: [doc.width, doc.height, w, h, source_pad],
                        region,
                        artwork: artwork
                            .iter()
                            .filter(|(index, _, mark)| {
                                *index == subset.1
                                    || scene_mask_id
                                        .is_some_and(|id| mark.style.light_mask_target == Some(id))
                            })
                            .cloned()
                            .collect(),
                        lights: target_lights
                            .clone()
                            .map(|(_, light, opacity)| (light.shading_key(local), *opacity))
                            .collect(),
                        guide: has_local_guide,
                        mask_roots: mask_roots.clone(),
                        scene_mask: scene_mask_id,
                    })
                });
                let assets = key
                    .as_ref()
                    .filter(|key| {
                        key.artwork
                            .iter()
                            .any(|(_, _, mark)| mark.style.media_image.is_some())
                    })
                    .map(|_| &doc.extras.media.assets);
                let cached = key.as_ref().and_then(|key| {
                    cache
                        .lit_planes
                        .get(key, assets.map(|assets| assets.identity()))
                });
                if let Some(pixels) = cached {
                    project_lit_plane(
                        &scene,
                        local,
                        camera_track,
                        plane,
                        w,
                        h,
                        source_pad,
                        pad,
                        region,
                        &pixels,
                    )
                } else {
                    let render_art = |cache: &mut crate::paint::RenderCache, subset| {
                        let Some(key) = &key else {
                            return draw_art(cache, subset);
                        };
                        if let Some(pixels) = cache.lit_planes.input(
                            key,
                            assets.map(|assets| assets.identity()),
                            subset,
                        ) {
                            return pixels;
                        }
                        let pixels = draw_art(cache, subset);
                        cache.lit_planes.insert_input(LitInput {
                            key: key.clone(),
                            assets: assets.cloned(),
                            subset,
                            pixels: pixels.clone(),
                        });
                        pixels
                    };
                    let height =
                        has_local_guide.then(|| render_art(cache, RenderSubset::Height(subset.1)));
                    let scene_mask =
                        scene_mask_id.map(|id| render_art(cache, RenderSubset::SceneLightMask(id)));
                    let masked = !mask_roots.is_empty();
                    let mut pieces = Vec::new();
                    if target_lights.clone().next().is_some() {
                        let artworks = || {
                            artwork.iter().filter(|(index, _, mark)| {
                                *index == subset.1
                                    && mark.style.surface_height.is_none()
                                    && !mark.style.light_mask
                            })
                        };
                        for (_, root, mark) in artworks() {
                            let piece = (masked.then_some(*root), Some(mark.style.surface_piece));
                            if pieces.last() != Some(&piece) {
                                if pieces.len() == 16 || pieces.contains(&piece) {
                                    pieces.clear();
                                    break;
                                }
                                pieces.push(piece);
                            }
                        }
                        // Keep masks isolated to their instance even when per-piece rendering is capped.
                        if masked && pieces.is_empty() {
                            for (_, root, _) in artworks() {
                                let piece = (Some(*root), None);
                                if pieces.last() != Some(&piece) {
                                    pieces.push(piece);
                                }
                            }
                        }
                    }
                    let shade = |surfaces: &mut crate::light::SurfaceCache,
                                 image: &mut Vec<u8>,
                                 mask: Option<&[u8]>| {
                        for (_, light, opacity) in target_lights.clone() {
                            if let Some(region) = region {
                                light.shade_region(
                                    image,
                                    region.size,
                                    region.view(),
                                    region.surface_step,
                                    local,
                                    *opacity,
                                    height.as_deref(),
                                    mask,
                                    surfaces,
                                );
                                continue;
                            }
                            light.apply_padded(
                                image,
                                w,
                                h,
                                source_pad,
                                &scene,
                                local,
                                camera && !use_world,
                                *opacity,
                                height.as_deref(),
                                mask,
                                Some(surfaces),
                            );
                        }
                    };
                    let mut raw;
                    if pieces.len() > 1 || masked {
                        let [sw, sh] =
                            region.map_or([w + source_pad * 2, h + source_pad * 2], |r| r.size);
                        raw = vec![0; sw as usize * sh as usize * 4];
                        let mut last_root = None;
                        let mut mask = None;
                        for (root, piece) in pieces {
                            if canceled() {
                                return;
                            }
                            if root != last_root {
                                mask = root.filter(|root| mask_roots.contains(root)).map(|root| {
                                    render_art(cache, RenderSubset::LightMask(subset.1, root))
                                });
                                if let Some(scene_mask) = &scene_mask {
                                    if let Some(mask) = &mut mask {
                                        composite(mask, scene_mask, 4);
                                    }
                                }
                                last_root = root;
                            }
                            let mut image =
                                render_art(cache, RenderSubset::Piece(subset.1, root, piece));
                            shade(
                                &mut cache.lit_planes.surfaces,
                                &mut image,
                                mask.as_deref().or(scene_mask.as_deref()),
                            );
                            composite(&mut raw, &image, 4);
                        }
                    } else {
                        raw = render_art(cache, RenderSubset::Single(false, subset.1));
                        shade(
                            &mut cache.lit_planes.surfaces,
                            &mut raw,
                            scene_mask.as_deref(),
                        );
                    }
                    if let Some(key) = key {
                        let pixels = Arc::new(raw);
                        let projected = project_lit_plane(
                            &scene,
                            local,
                            camera_track,
                            plane,
                            w,
                            h,
                            source_pad,
                            pad,
                            region,
                            &pixels,
                        );
                        cache.lit_planes.insert(LitPlane {
                            key,
                            assets: assets.cloned(),
                            pixels,
                        });
                        projected
                    } else {
                        raw
                    }
                }
            } else {
                draw_art(cache, RenderSubset::Single(true, subset.1))
            };
            if let Some(layer) = layer {
                if let Some(id) = layer.options.light_receiver {
                    for (index, light, opacity) in &lights {
                        let Some(shadow) = light.shadow.as_ref().filter(|s| s.floor == id) else {
                            continue;
                        };
                        if subset.1 >= *index {
                            continue;
                        }
                        let mut mask: Option<Vec<u8>> = None;
                        for caster in subset.1 + 1..*index {
                            let candidate = &scene.layers[caster];
                            if !candidate.visible
                                || !shadow.caster.map_or_else(
                                    || light.receives(&scene, candidate),
                                    |id| candidate.options.light_receiver == Some(id),
                                )
                                || !resolved.iter().any(|(l, _, mark)| {
                                    *l == caster
                                        && mark.style.surface_height.is_none()
                                        && !mark.style.light_mask
                                })
                            {
                                continue;
                            }
                            let image = raster_flat(
                                doc,
                                w,
                                h,
                                ctx,
                                cache,
                                video,
                                Some(RenderSubset::Single(false, caster)),
                                &resolved,
                                pad,
                                true,
                            );
                            if let Some(target) = &mut mask {
                                composite(target, &image, 4);
                            } else {
                                mask = Some(image);
                            }
                        }
                        if let Some(mask) = mask {
                            shadow.apply(&mut raw, mask, pw, ph, scale, *opacity);
                        }
                    }
                }
            }
            if let Some(layer) = layer {
                for (index, adjustment) in &scoped_fx {
                    if subset.1 < *index
                        && !group.is_some_and(|(id, end)| {
                            *index > end
                                && adjustment.options.fx_target
                                    == Some(crate::light::Receiver::Folder(id))
                        })
                        && adjustment
                            .options
                            .fx_target
                            .is_some_and(|target| target.matches(&scene, layer))
                    {
                        apply_adjustment(
                            &mut cache.post_fx,
                            &mut raw,
                            pw,
                            ph,
                            adjustment,
                            scale,
                            &scene,
                            local,
                            camera,
                            false,
                            FxPass::All,
                        );
                    }
                }
            }
            if !subset.0 {
                for (_, target, adjustment) in &auto_outlines {
                    if *target == subset.1 {
                        apply_adjustment(
                            &mut cache.post_fx,
                            &mut raw,
                            pw,
                            ph,
                            adjustment,
                            scale,
                            &scene,
                            local,
                            camera,
                            false,
                            FxPass::Outline,
                        );
                    }
                }
            }
            let blurred = if let Some(optics) = &optics {
                crate::depth::blur(raw, pw, ph, radius, optics)
            } else {
                raw
            };
            if pad == 0 {
                composite(result, &blurred, channels);
            } else {
                for y in 0..h as usize {
                    let start = ((y + pad as usize) * pw as usize + pad as usize) * 4;
                    composite(
                        &mut result[y * w as usize * channels..(y + 1) * w as usize * channels],
                        &blurred[start..start + w as usize * 4],
                        channels,
                    );
                }
            }
        };
        let mut group_pixels = None;
        for layer in 0..=scene.layers.len() {
            report(20 + (67 * layer / scene.layers.len().max(1)) as u8);
            if canceled() {
                return Vec::new();
            }
            while clips.peek().is_some_and(|(_, c)| c.slot <= layer) {
                let (i, clip) = clips.next().unwrap();
                render(cache, &mut result, (true, i), &clip.depth, None);
            }
            if let Some(l) = scene.layers.get(layer).filter(|l| l.visible) {
                if adjust
                    && l.options.fx_target.is_none()
                    && crate::post::visible_effects_at(l, local)
                    && crate::folder_groups::layer_active(&scene, l, local)
                {
                    phase(2);
                    apply_adjustment(
                        &mut cache.post_fx,
                        &mut result,
                        w,
                        h,
                        l,
                        scale,
                        &scene,
                        local,
                        camera,
                        !lights.iter().any(|(index, _, _)| *index == layer),
                        if auto_outlines.iter().any(|(index, _, _)| *index == layer) {
                            FxPass::WithoutOutline
                        } else {
                            FxPass::All
                        },
                    );
                } else if !crate::post::is_adjustment(l)
                    && resolved.iter().any(|(i, ..)| *i == layer)
                {
                    if let Some(&(_, end, id)) = groups
                        .iter()
                        .find(|(start, end, _)| (*start..=*end).contains(&layer))
                    {
                        let group =
                            group_pixels.get_or_insert_with(|| vec![0; (w * h * 4) as usize]);
                        render(
                            cache,
                            group,
                            (false, layer),
                            &l.options.depth,
                            Some((id, end)),
                        );
                    } else {
                        render(cache, &mut result, (false, layer), &l.options.depth, None);
                    }
                }
            }
            if let Some(&(_, end, id)) = groups.iter().find(|(_, end, _)| *end == layer)
                && let Some(mut group) = group_pixels.take()
            {
                for (index, adjustment) in &scoped_fx {
                    if *index > end
                        && adjustment.options.fx_target == Some(crate::light::Receiver::Folder(id))
                    {
                        apply_adjustment(
                            &mut cache.post_fx,
                            &mut group,
                            w,
                            h,
                            adjustment,
                            scale,
                            &scene,
                            local,
                            camera,
                            false,
                            FxPass::All,
                        );
                    }
                }
                composite(&mut result, &group, channels);
            }
        }
        if canceled() {
            return Vec::new();
        }
        report(95);
        return if adjust {
            finish_adjusted(result, transparent)
        } else {
            result
        };
    }
    if adjust {
        let mut result = vec![0; (w * h * 4) as usize];
        let mut start = 0;
        for (index, layer) in scene.layers.iter().enumerate() {
            report(20 + (67 * index / scene.layers.len().max(1)) as u8);
            if canceled() {
                return Vec::new();
            }
            if !crate::post::visible_effects_at(layer, local)
                || !crate::folder_groups::layer_active(&scene, layer, local)
            {
                continue;
            }
            let at = index * 2 + 1;
            if start < at {
                let group = raster_flat(
                    doc,
                    w,
                    h,
                    ctx,
                    cache,
                    video,
                    Some(RenderSubset::Span(start, at)),
                    &resolved,
                    0,
                    true,
                );
                composite(&mut result, &group, 4);
            }
            phase(2);
            apply_adjustment(
                &mut cache.post_fx,
                &mut result,
                w,
                h,
                layer,
                scale,
                &scene,
                local,
                camera,
                true,
                FxPass::All,
            );
            start = at + 1;
        }
        let group = raster_flat(
            doc,
            w,
            h,
            ctx,
            cache,
            video,
            Some(RenderSubset::Span(start, usize::MAX)),
            &resolved,
            0,
            true,
        );
        composite(&mut result, &group, 4);
        report(95);
        return finish_adjusted(result, transparent);
    }
    raster_flat(
        doc,
        w,
        h,
        ctx,
        cache,
        video,
        None,
        &resolved,
        0,
        transparent,
    )
}
#[derive(Clone, Copy, PartialEq)]
struct LitRegion {
    origin: Vec2,
    size: [u32; 2],
    scale: f32,
    surface_step: usize,
}

impl LitRegion {
    fn new(
        output: View,
        stage_scale: f32,
        size: [u32; 2],
        padding: impl FnOnce(f32, usize) -> f32,
    ) -> Option<Self> {
        let zoom = output.scale / stage_scale;
        if zoom <= 1. {
            return None;
        }
        // Use a world-aligned grid with at least one source sample per output pixel.
        // Quantizing the density lets adjacent camera poses share the same grid.
        let scale = (stage_scale * zoom.ceil()).max(output.scale);
        let surface_step = size[0].max(size[1]).div_ceil(1024) as usize;
        let tile = (surface_step * 16) as f32;
        let corners = [
            Pos2::ZERO,
            Pos2::new(size[0] as f32, 0.),
            Pos2::new(0., size[1] as f32),
            Pos2::new(size[0] as f32, size[1] as f32),
        ];
        let bounds = Rect::from_points(&corners.map(|p| output.world(p) * scale))
            .expand(padding(scale, surface_step).min(512.) + 2.);
        let min = (bounds.min.to_vec2() / tile).floor() * tile;
        let max = (bounds.max.to_vec2() / tile).ceil() * tile;
        let extent = max - min;
        Some(Self {
            origin: min,
            size: [extent.x as u32, extent.y as u32],
            scale,
            surface_step,
        })
    }

    fn view(self) -> View {
        View {
            center: Pos2::ZERO - self.origin,
            stage_center: Pos2::ZERO,
            angle: 0.,
            scale: self.scale,
        }
    }
}

fn lit_output_view(
    doc: &Document,
    frame: usize,
    camera: Option<&crate::features::Camera>,
    plane: &crate::depth::LayerDepth,
    w: u32,
    h: u32,
    pad: u32,
) -> View {
    let pose = camera.map(|camera| crate::animation::camera_at(camera, frame));
    let scale = (w as f32 / doc.width as f32).min(h as f32 / doc.height as f32);
    let view = View {
        center: Pos2::new(w as f32 * 0.5 + pad as f32, h as f32 * 0.5 + pad as f32),
        stage_center: Pos2::new(doc.width as f32 * 0.5, doc.height as f32 * 0.5),
        angle: pose.as_ref().map_or(0., |pose| -pose.angle),
        scale: scale * pose.as_ref().map_or(1., |pose| pose.zoom),
    };
    if camera.is_some() {
        crate::depth::view_for_plane(doc, frame, plane, view)
    } else {
        view
    }
}

fn project_lit_plane(
    doc: &Document,
    frame: usize,
    camera: Option<&crate::features::Camera>,
    plane: &crate::depth::LayerDepth,
    w: u32,
    h: u32,
    source_pad: u32,
    output_pad: u32,
    region: Option<LitRegion>,
    source: &[u8],
) -> Vec<u8> {
    let output_w = w + output_pad * 2;
    let output_h = h + output_pad * 2;
    let ([source_w, source_h], source_view) = if let Some(region) = region {
        (region.size, region.view())
    } else {
        stage_raster_view(doc, w, h, source_pad)
    };
    if camera.is_none() && region.is_none() && source_pad >= output_pad {
        let inset = (source_pad - output_pad) as usize;
        return source
            .chunks_exact(source_w as usize * 4)
            .skip(inset)
            .take(output_h as usize)
            .flat_map(|row| {
                row[inset * 4..(inset + output_w as usize) * 4]
                    .iter()
                    .copied()
            })
            .collect();
    }
    let output = lit_output_view(doc, frame, camera, plane, w, h, output_pad);
    let start = source_view.screen(output.world(Pos2::new(0.5, 0.5))) - Vec2::splat(0.5);
    let dx = crate::canvas::rotate(Vec2::X * (source_view.scale / output.scale), -output.angle);
    let dy = crate::canvas::rotate(Vec2::Y * (source_view.scale / output.scale), -output.angle);
    let mut pixels = vec![0; output_w as usize * output_h as usize * 4];
    let sample = |x: i32, y: i32| -> [u8; 4] {
        if x < 0 || y < 0 || x >= source_w as i32 || y >= source_h as i32 {
            return [0; 4];
        }
        let at = (y as usize * source_w as usize + x as usize) * 4;
        source[at..at + 4].try_into().unwrap()
    };
    for (y, row) in pixels.chunks_exact_mut(output_w as usize * 4).enumerate() {
        let start = start + dy * y as f32;
        for (x, pixel) in row.chunks_exact_mut(4).enumerate() {
            let at = start + dx * x as f32;
            let (ix, iy) = (at.x.floor() as i32, at.y.floor() as i32);
            let (fx, fy) = (at.x - ix as f32, at.y - iy as f32);
            let a = sample(ix, iy);
            let b = sample(ix + 1, iy);
            let c = sample(ix, iy + 1);
            let d = sample(ix + 1, iy + 1);
            for k in 0..4 {
                let top = f32::from(a[k]) * (1. - fx) + f32::from(b[k]) * fx;
                let bottom = f32::from(c[k]) * (1. - fx) + f32::from(d[k]) * fx;
                pixel[k] = (top * (1. - fy) + bottom * fy).round() as u8;
            }
        }
    }
    pixels
}

fn finish_adjusted(rgba: Vec<u8>, transparent: bool) -> Vec<u8> {
    if transparent {
        return rgba;
    }
    let mut rgb = vec![255; rgba.len() / 4 * 3];
    composite(&mut rgb, &rgba, 3);
    rgb
}
#[derive(Clone, Copy)]
enum FxPass {
    All,
    Outline,
    WithoutOutline,
}

fn apply_adjustment(
    fx_cache: &mut crate::post::EffectCache,
    result: &mut Vec<u8>,
    w: u32,
    h: u32,
    layer: &crate::document::Layer,
    scale: f32,
    doc: &Document,
    frame: usize,
    camera: bool,
    composite_light: bool,
    pass: FxPass,
) {
    let opacity = layer.options.opacity.clamp(0., 1.);
    if opacity == 0. {
        return;
    }
    if let Some(mut effects) = crate::post::effects_at(layer, frame) {
        let original = (opacity < 1.).then(|| result.clone());
        match pass {
            FxPass::All => {}
            FxPass::Outline => effects.to_mut().retain(crate::post::Effect::is_outline),
            FxPass::WithoutOutline => effects.to_mut().retain(|effect| !effect.is_outline()),
        }
        crate::post::animate_grain(effects.to_mut(), frame);
        fx_cache.apply(result, w, h, &effects, scale);
        if let Some(original) = original {
            for (p, old) in result.iter_mut().zip(original) {
                *p = (f32::from(old) + (f32::from(*p) - f32::from(old)) * opacity).round() as u8;
            }
        }
    }
    if composite_light && let Some(light) = &layer.options.light {
        light.apply_padded(
            result, w, h, 0, doc, frame, camera, opacity, None, None, None,
        );
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RenderSubset {
    Single(bool, usize),
    Height(usize),
    LightMask(usize, usize),
    SceneLightMask(u64),
    Piece(usize, Option<usize>, Option<u64>),
    Span(usize, usize),
}
impl RenderSubset {
    fn height_only(self) -> bool {
        matches!(self, Self::Height(_))
    }
    fn includes(self, video: bool, index: usize, slot: usize) -> bool {
        match self {
            Self::Height(layer) | Self::LightMask(layer, _) | Self::Piece(layer, _, _) => {
                !video && index == layer
            }
            Self::SceneLightMask(_) => !video,
            Self::Single(kind, id) => kind == video && id == index,
            Self::Span(a, b) => {
                let position = 2 * slot + usize::from(!video);
                (a..b).contains(&position)
            }
        }
    }
}
fn composite(rgb: &mut [u8], rgba: &[u8], channels: usize) {
    for (out, c) in rgb.chunks_exact_mut(channels).zip(rgba.chunks_exact(4)) {
        let alpha = c[3] as f32 / 255.;
        for k in 0..channels {
            out[k] = (c[k] as f32 + out[k] as f32 * (1. - alpha))
                .min(255.)
                .round() as u8;
        }
    }
}
fn raster_flat(
    doc: &Document,
    w: u32,
    h: u32,
    ctx: &egui::Context,
    cache: &mut crate::paint::RenderCache,
    video: &[crate::media::ExportVideoFrame],
    subset: Option<RenderSubset>,
    resolved: &[(usize, usize, crate::Mark)],
    padding: u32,
    transparent: bool,
) -> Vec<u8> {
    raster_flat_in_view(
        doc,
        w,
        h,
        ctx,
        cache,
        video,
        subset,
        resolved,
        padding,
        transparent,
        None,
    )
}

fn stage_raster_view(doc: &Document, w: u32, h: u32, padding: u32) -> ([u32; 2], View) {
    let scale = (w as f32 / doc.width as f32).min(h as f32 / doc.height as f32);
    let size = [w + padding * 2, h + padding * 2];
    let view = View {
        center: Pos2::new(size[0] as f32 * 0.5, size[1] as f32 * 0.5),
        scale,
        angle: 0.,
        stage_center: Pos2::new(doc.width as f32 * 0.5, doc.height as f32 * 0.5),
    };
    (size, view)
}
fn raster_flat_in_view(
    doc: &Document,
    w: u32,
    h: u32,
    ctx: &egui::Context,
    cache: &mut crate::paint::RenderCache,
    video: &[crate::media::ExportVideoFrame],
    subset: Option<RenderSubset>,
    resolved: &[(usize, usize, crate::Mark)],
    padding: u32,
    transparent: bool,
    source: Option<([u32; 2], View)>,
) -> Vec<u8> {
    let height_only = subset.is_some_and(RenderSubset::height_only);
    let mask_root = match subset {
        Some(RenderSubset::LightMask(_, root)) => Some(root),
        _ => None,
    };
    let scene_mask = match subset {
        Some(RenderSubset::SceneLightMask(id)) => Some(id),
        _ => None,
    };
    let ([w, h], view) = source.unwrap_or_else(|| stage_raster_view(doc, w, h, padding));
    let size = Vec2::new(w as f32, h as f32);
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            ..Default::default()
        },
        |ui| {
            let painter = ui
                .painter()
                .with_clip_rect(Rect::from_min_size(Pos2::ZERO, size));
            let mut clips = video.iter().enumerate().peekable();
            let paint_video = |index, clip: &crate::media::ExportVideoFrame| {
                if height_only
                    || mask_root.is_some()
                    || scene_mask.is_some()
                    || subset.is_some_and(|s| !s.includes(true, index, clip.slot))
                {
                    return;
                }
                if let Some(mesh) = crate::media::image_mesh(
                    &clip.mark,
                    view,
                    egui::TextureId::User(u64::MAX - index as u64),
                ) {
                    painter.add(mesh);
                }
            };
            for (layer, source, m) in resolved {
                let layer = *layer;
                while clips.peek().is_some_and(|(_, clip)| clip.slot <= layer) {
                    let (index, clip) = clips.next().unwrap();
                    paint_video(index, clip);
                }
                if subset.is_some_and(|s| !s.includes(false, layer, layer)) {
                    continue;
                }
                if let Some(RenderSubset::Piece(_, root, piece)) = subset
                    && (root.is_some_and(|root| *source != root)
                        || piece.is_some_and(|piece| m.style.surface_piece != piece))
                {
                    continue;
                }
                if height_only {
                    if let Some(height) = m.style.surface_height {
                        let mut guide = m.clone();
                        let value = (height.abs() * 255.).round() as u8;
                        let encode = |alpha| {
                            if height >= 0. {
                                [value, 0, 0, alpha]
                            } else {
                                [0, 0, value, alpha]
                            }
                        };
                        guide.color = encode(guide.color[3]);
                        guide.fill = guide.fill.map(|fill| encode(fill[3]));
                        guide.style.gradient = None;
                        crate::paint::draw_composited(&painter, &guide, view, cache);
                    }
                    continue;
                }
                if m.style.surface_height.is_some()
                    || if let Some(root) = mask_root {
                        !m.style.light_mask || *source != root
                    } else if let Some(id) = scene_mask {
                        m.style.light_mask_target != Some(id)
                    } else {
                        m.style.light_mask
                    }
                {
                    continue;
                }
                if m.style.light_mask && m.points.is_empty() && m.nodes.is_empty() {
                    continue;
                }
                if let Some(id) = m.style.media_image
                    && let Some(mesh) =
                        crate::media::image_mesh(&m, view, egui::TextureId::User(id))
                {
                    painter.add(mesh);
                } else {
                    crate::paint::draw_composited(&painter, &m, view, cache);
                }
            }
            for (index, clip) in clips {
                paint_video(index, clip);
            }
        },
    );
    output.textures_delta.clear();
    let meshes = ctx.tessellate(output.shapes, output.pixels_per_point);
    let mut pixels = if transparent {
        vec![0u8; (w * h * 4) as usize]
    } else {
        vec![255u8; (w * h * 3) as usize]
    };
    for primitive in meshes {
        if let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive {
            let clip = primitive
                .clip_rect
                .intersect(Rect::from_min_size(Pos2::ZERO, size));
            let texture = match mesh.texture_id {
                egui::TextureId::User(id) => video
                    .get((u64::MAX - id) as usize)
                    .map(|clip| {
                        let pixels = &clip.pixels;
                        (
                            [pixels.width() as usize, pixels.height() as usize],
                            pixels.as_raw().as_slice(),
                        )
                    })
                    .or_else(|| doc.extras.media.pixels(id)),
                _ => None,
            };
            for t in mesh.indices.chunks_exact(3) {
                let vertices = [
                    mesh.vertices[t[0] as usize],
                    mesh.vertices[t[1] as usize],
                    mesh.vertices[t[2] as usize],
                ];
                triangle(&mut pixels, w, h, vertices, clip, texture);
            }
        }
    }
    pixels
}
fn triangle(
    pixels: &mut [u8],
    w: u32,
    h: u32,
    v: [egui::epaint::Vertex; 3],
    clip: Rect,
    texture: Option<([usize; 2], &[u8])>,
) {
    let channels = if pixels.len() == (w * h * 4) as usize {
        4
    } else {
        3
    };
    let [a, b, c] = v.map(|v| v.pos);
    // Shared edges use opposite coefficients in adjacent triangles, so a pixel
    // cannot slip between them when camera motion places it exactly on the edge.
    let edges = [(b, c), (c, a), (a, b)].map(|(from, to)| {
        let (x, y) = (f64::from(from.x), f64::from(from.y));
        let (u, v) = (f64::from(to.x), f64::from(to.y));
        [y - v, u - x, x * v - y * u]
    });
    let det = edges[0][0] * f64::from(a.x) + edges[0][1] * f64::from(a.y) + edges[0][2];
    if det.abs() < 1e-6 {
        return;
    }
    let direction = det.signum();
    let inverse = det.abs().recip();
    let top_left = edges.map(|[x, y, _]| x * direction > 0. || (x == 0. && y * direction > 0.));
    let min = Pos2::new(a.x.min(b.x).min(c.x), a.y.min(b.y).min(c.y)).max(clip.min);
    let max = Pos2::new(a.x.max(b.x).max(c.x), a.y.max(b.y).max(c.y)).min(clip.max);
    let colors = v.map(|v| v.color.to_array());
    for y in (min.y.floor().max(0.) as u32)..(max.y.ceil().max(0.) as u32).min(h) {
        for x in (min.x.floor().max(0.) as u32)..(max.x.ceil().max(0.) as u32).min(w) {
            let sample_x = f64::from(x) + 0.5;
            let sample_y = f64::from(y) + 0.5;
            let coverage = edges.map(|[a, b, c]| (a * sample_x + b * sample_y + c) * direction);
            if coverage
                .iter()
                .zip(top_left)
                .any(|(&edge, include)| edge < 0. || (edge == 0. && !include))
            {
                continue;
            }
            let weights = coverage.map(|edge| (edge * inverse) as f32);
            let offset = (y * w + x) as usize * channels;
            if let Some(([width, height], data)) = texture {
                let u = (0..3).map(|i| weights[i] * v[i].uv.x).sum::<f32>();
                let v = (0..3).map(|i| weights[i] * v[i].uv.y).sum::<f32>();
                let x = (u * width as f32 - 0.5).clamp(0., (width - 1) as f32);
                let y = (v * height as f32 - 0.5).clamp(0., (height - 1) as f32);
                let ix = x as usize;
                let iy = y as usize;
                let next_x = (ix + 1).min(width - 1);
                let next_y = (iy + 1).min(height - 1);
                let (fx, fy) = (x.fract(), y.fract());
                let samples = [
                    iy * width + ix,
                    iy * width + next_x,
                    next_y * width + ix,
                    next_y * width + next_x,
                ];
                let factors = [
                    (1. - fx) * (1. - fy),
                    fx * (1. - fy),
                    (1. - fx) * fy,
                    fx * fy,
                ];
                let tint = (0..3)
                    .map(|i| weights[i] * colors[i][3] as f32)
                    .sum::<f32>()
                    / 255.;
                let mut alpha = 0.;
                let mut rgb = [0.; 3];
                for (sample, factor) in samples.into_iter().zip(factors) {
                    if factor == 0. {
                        continue;
                    }
                    let start = sample * 4;
                    let a = f32::from(data[start + 3]) / 255. * tint * factor;
                    alpha += a;
                    for c in 0..3 {
                        rgb[c] += f32::from(data[start + c]) * a;
                    }
                }
                for channel in 0..3 {
                    pixels[offset + channel] = (rgb[channel]
                        + f32::from(pixels[offset + channel]) * (1. - alpha))
                        .round() as u8;
                }
                if channels == 4 {
                    pixels[offset + 3] =
                        (alpha * 255. + f32::from(pixels[offset + 3]) * (1. - alpha)).round() as u8;
                }
                continue;
            }
            let alpha = (0..3)
                .map(|i| weights[i] * colors[i][3] as f32)
                .sum::<f32>()
                / 255.;
            if channels == 4 {
                pixels[offset + 3] = (alpha * 255. + pixels[offset + 3] as f32 * (1. - alpha))
                    .min(255.)
                    .round() as u8;
            }
            for channel in 0..3 {
                let source = (0..3)
                    .map(|i| weights[i] * colors[i][channel] as f32)
                    .sum::<f32>();
                pixels[offset + channel] = (source + pixels[offset + channel] as f32 * (1. - alpha))
                    .clamp(0., 255.)
                    .round() as u8;
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn camera_pan_moves_the_lit_artwork_without_relighting_its_silhouette() {
        let mut doc = Document::default();
        doc.width = 96;
        doc.height = 64;
        let mut mark = crate::Mark {
            points: vec![[17.2, 11.7], [71.6, 14.3], [62.9, 49.4], [17.2, 11.7]].into(),
            fill: Some([93, 64, 50, 255]),
            stroke: false,
            closed: true,
            ..Default::default()
        };
        mark.style.surface_piece = 1;
        doc.layers[0].frames[0].push(mark);
        doc.layers[0].frames[1] = doc.layers[0].frames[0].clone();
        let mut layer = doc.layers[0].clone();
        layer.options.light = Some(crate::light::Light::new(96, 64, crate::light::Kind::Rim));
        for frame in &mut layer.frames {
            frame.clear();
        }
        doc.layers.push(layer);
        let key = |frame, x| crate::animation::CameraKey {
            frame,
            center: [x, 32.],
            zoom: 1.,
            angle: 0.,
            optics: None,
        };
        doc.extras.cameras.push(crate::features::Camera {
            id: 10,
            name: "Camera".into(),
            start: 0,
            duration: 2,
            center: [48., 32.],
            zoom: 1.,
            angle: 0.,
            optics: Default::default(),
            animation: crate::animation::CameraAnimation {
                keys: vec![key(0, 48.), key(1, 55.)].into(),
                ..Default::default()
            },
        });
        let ctx = egui::Context::default();
        let mut cache = crate::paint::RenderCache::default();
        let first = raster(&doc, 0, 96, 64, &ctx, &mut cache);
        let second = raster(&doc, 1, 96, 64, &ctx, &mut cache);
        assert_eq!(cache.lit_planes.hits, 1);
        assert_eq!(
            second,
            raster(&doc, 1, 96, 64, &ctx, &mut Default::default())
        );
        doc.layers[0].frames[1][0].fill = Some([20, 110, 140, 255]);
        let edited = raster(&doc, 1, 96, 64, &ctx, &mut cache);
        assert_ne!(second, edited);
        assert_eq!(cache.lit_planes.hits, 1);
        assert_eq!(
            edited,
            raster(&doc, 1, 96, 64, &ctx, &mut Default::default())
        );
        for y in 0..64 {
            let row = y * 96 * 3;
            assert_eq!(&first[row + 21..row + 96 * 3], &second[row..row + 89 * 3]);
        }
    }

    #[test]
    fn export_rasterizes_lit_vector_edges_at_camera_resolution() {
        let mut doc = Document::default();
        doc.width = 96;
        doc.height = 64;
        doc.layers[0].frames[0].push(crate::Mark {
            points: vec![[48.2, 10.], [75., 54.]].into(),
            rectangle: true,
            closed: true,
            fill: Some([0, 0, 0, 255]),
            stroke: false,
            ..Default::default()
        });
        let mut lighting = doc.layers[0].clone();
        lighting.frames[0].clear();
        lighting.options.light = Some(crate::light::Light::new(
            96,
            64,
            crate::light::Kind::Ambient,
        ));
        lighting.options.opacity = 0.;
        doc.layers.push(lighting);
        doc.extras.cameras.push(crate::features::Camera {
            id: 1,
            name: "Camera".into(),
            start: 0,
            duration: 1,
            center: [48., 32.],
            zoom: 4.,
            angle: 0.,
            optics: Default::default(),
            animation: Default::default(),
        });
        let ctx = egui::Context::default();
        let mut cache = crate::paint::RenderCache::default();
        let fast = raster_preview(
            &doc,
            0,
            0,
            96,
            64,
            &ctx,
            &mut cache,
            &[],
            true,
            true,
            false,
            false,
            None,
        );
        let full = raster(&doc, 0, 96, 64, &ctx, &mut cache);
        let preview = raster_preview(
            &doc,
            0,
            0,
            96,
            64,
            &ctx,
            &mut cache,
            &[],
            true,
            true,
            false,
            true,
            None,
        );
        assert_eq!(preview, full);
        assert_eq!(full.len(), 96 * 64 * 3);
        doc.layers.pop();
        let direct = raster(&doc, 0, 96, 64, &ctx, &mut cache);
        let error = |image: &[u8]| {
            image
                .iter()
                .zip(&direct)
                .map(|(p, q)| u64::from(p.abs_diff(*q)))
                .sum::<u64>()
        };
        assert!(
            error(&full) * 2 < error(&fast),
            "full {}, fast {}",
            error(&full),
            error(&fast)
        );
    }

    #[test]
    fn camera_subpixel_translation_does_not_open_shared_triangle_edges() {
        use egui::epaint::Vertex;
        let color = egui::Color32::from_rgba_unmultiplied(60, 120, 180, 128);
        let clip = Rect::from_min_max(Pos2::ZERO, Pos2::new(64., 64.));
        for offset in [0., 0.03125, 0.125, 0.333, 0.9] {
            let corners = [[1., 1.], [63., 1.], [63., 63.], [1., 63.]].map(|[x, y]| Vertex {
                pos: Pos2::new(x + offset, y + offset),
                uv: Pos2::ZERO,
                color,
            });
            let mut pixels = vec![0; 64 * 64 * 4];
            let [a, b, c, d] = corners;
            triangle(&mut pixels, 64, 64, [a, b, c], clip, None);
            triangle(&mut pixels, 64, 64, [a, d, c], clip, None);
            for y in 3..61 {
                for x in 3..61 {
                    assert_eq!(pixels[(y * 64 + x) * 4 + 3], 128, "{x}, {y}, {offset}");
                }
            }
        }
    }

    #[test]
    #[ignore = "manual lighting preview benchmark with a camera project fixture"]
    fn benchmark_lit_camera_cache() {
        let source = PathBuf::from(std::env::var_os("LIBRETOON_CAMERA_FIXTURE").unwrap());
        let doc = crate::documents::read_project(&source).unwrap();
        let ctx = egui::Context::default();
        if let Some(path) = std::env::var_os("LIBRETOON_REFERENCE_FRAME") {
            let reference = image::open(path).unwrap().into_rgb8();
            assert_eq!(reference.dimensions(), (doc.width, doc.height));
            assert!(
                reference.as_raw()
                    == &raster(
                        &doc,
                        0,
                        doc.width,
                        doc.height,
                        &ctx,
                        &mut Default::default()
                    )
            );
        }
        let count = crate::animation::movie_count(&doc);
        let first = count.saturating_sub(4);
        let mut cache = crate::paint::RenderCache::default();
        let mut cold = Duration::ZERO;
        let mut warm = Duration::ZERO;
        for frame in first..count {
            let start = std::time::Instant::now();
            let reference = raster(
                &doc,
                frame,
                doc.width,
                doc.height,
                &ctx,
                &mut Default::default(),
            );
            cold += start.elapsed();
            let start = std::time::Instant::now();
            let rendered = raster(&doc, frame, doc.width, doc.height, &ctx, &mut cache);
            warm += start.elapsed();
            assert_eq!(rendered, reference);
        }
        eprintln!(
            "{} × {}, {} frames, {} plane hits: cold {:?}, cached {:?}",
            doc.width,
            doc.height,
            count - first,
            cache.lit_planes.hits,
            cold,
            warm
        );
        assert!(cache.lit_planes.hits > 0);
    }

    #[test]
    #[ignore = "manual camera and lighting project fixture"]
    fn render_external_camera_fixture() {
        let source = PathBuf::from(std::env::var_os("LIBRETOON_CAMERA_FIXTURE").unwrap());
        let folder = PathBuf::from(std::env::var_os("LIBRETOON_CAMERA_OUTPUT").unwrap());
        let doc = crate::documents::read_project(&source).unwrap();
        let mut cache = crate::paint::RenderCache::default();
        let ctx = egui::Context::default();
        for frame in 0..crate::animation::movie_count(&doc) {
            let pixels = raster(&doc, frame, doc.width, doc.height, &ctx, &mut cache);
            image::save_buffer(
                folder.join(format!("frame-{frame:03}.png")),
                &pixels,
                doc.width,
                doc.height,
                image::ColorType::Rgb8,
            )
            .unwrap();
        }
    }

    #[test]
    fn sprite_sheet_exports_frames_in_order_with_transparent_spacing() {
        let mut doc = Document::default();
        doc.width = 16;
        doc.height = 16;
        doc.layers[0].frames = (0..4)
            .map(|i| {
                vec![crate::Mark {
                    points: vec![[0., 0.], [16., 16.]].into(),
                    fill: Some([50 * i, 20, 100, 255]),
                    rectangle: true,
                    closed: true,
                    stroke: false,
                    ..Default::default()
                }]
                .into()
            })
            .collect::<Vec<_>>()
            .into();
        let mut settings = ExportSettings {
            name: "spritesheet".into(),
            folder: String::new(),
            format: SPRITE_SHEET,
            first: 2,
            last: 4,
            width: 16,
            height: 16,
            fps: 24,
            quality: 100,
            loops: 0,
            colors: 256,
            dither: false,
            svg_sequence: false,
            sheet: SheetSettings {
                columns: 2,
                rows: 1,
                spacing: 2,
            },
        };
        assert!(settings.sheet_size().is_err());
        settings.sheet.rows = 0;
        let path = std::env::temp_dir().join(format!("lt-sprites-{}.png", crate::features::id()));
        let progress = Arc::new(AtomicUsize::new(0));
        run_export(
            &doc,
            &settings,
            0,
            &path,
            PathBuf::from("unused-ffmpeg"),
            progress.clone(),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        let sheet = image::open(&path).unwrap().into_rgba8();
        std::fs::remove_file(path).unwrap();
        assert_eq!(sheet.dimensions(), (34, 34));
        for (x, y, red) in [(8, 8, 50), (26, 8, 100), (8, 26, 150)] {
            assert_eq!(sheet.get_pixel(x, y).0, [red, 20, 100, 255]);
        }
        assert_eq!(sheet.get_pixel(17, 8).0, [0; 4]);
        assert_eq!(sheet.get_pixel(26, 26).0, [0; 4]);
        assert_eq!(progress.load(Ordering::Relaxed), 1000);
    }

    #[test]
    fn video_slot_controls_compositing_above_and_below_animation() {
        let mut doc = crate::fresh_document();
        doc.width = 8;
        doc.height = 8;
        doc.layers[0].frames[0].push(crate::Mark {
            rectangle: true,
            closed: true,
            stroke: false,
            fill: Some([0, 0, 255, 255]),
            points: vec![[0., 0.], [8., 8.]].into(),
            ..Default::default()
        });
        let pixels = std::sync::Arc::new(image::RgbaImage::from_pixel(
            4,
            4,
            image::Rgba([255, 0, 0, 255]),
        ));
        let mut clip = crate::media::ExportVideoFrame {
            depth: Default::default(),
            slot: 0,
            mark: crate::media::video_mark(&doc, Vec2::splat(4.)),
            pixels,
        };
        let ctx = egui::Context::default();
        let mut cache = crate::paint::RenderCache::default();
        let center = (4 * 8 + 4) * 3;
        let below = raster_with_video(&doc, 0, 8, 8, &ctx, &mut cache, std::slice::from_ref(&clip));
        assert_eq!(&below[center..center + 3], &[0, 0, 255]);
        clip.slot = 1;
        let above = raster_with_video(&doc, 0, 8, 8, &ctx, &mut cache, &[clip]);
        assert_eq!(&above[center..center + 3], &[255, 0, 0]);
    }

    #[test]
    fn movie_raster_respects_scene_order_motion_and_camera_keys() {
        let mut d = crate::fresh_document();
        d.width = 64;
        d.height = 64;
        let mark = crate::document::Mark {
            rectangle: true,
            closed: true,
            stroke: false,
            fill: Some([220, 40, 20, 255]),
            points: vec![[4., 16.], [20., 48.]].into(),
            ..Default::default()
        };
        d.layers[0].frames[0] = vec![mark.clone()].into();
        d.layers[0].frames[10] =
            vec![crate::vector::transform(&mark, |p| p + Vec2::new(40., 0.))].into();
        d.layers[0].options.keys[10] = true;
        d.layers[0].options.tweens.push(crate::animation::Tween {
            start: 0,
            end: 10,
            curve: Default::default(),
        });
        let first = crate::animation::scene_from(&d, 1, "Motion".into());
        let mut second = crate::fresh_document();
        let mut blue = mark.clone();
        blue.fill = Some([20, 40, 220, 255]);
        second.layers[0].frames[0] = vec![blue].into();
        d.extras.scenes = vec![
            first,
            crate::animation::scene_from(&second, 2, "Blue".into()),
        ]
        .into();
        let ctx = egui::Context::default();
        let mut cache = Default::default();
        let p = raster(&d, 5, 64, 64, &ctx, &mut cache);
        let pixel = |p: &[u8], x: usize| p[(32 * 64 + x) * 3..(32 * 64 + x) * 3 + 3].to_vec();
        assert_eq!(pixel(&p, 32), vec![220, 40, 20]);
        assert_eq!(pixel(&p, 10), vec![255, 255, 255]);
        let p = raster(&d, 24, 64, 64, &ctx, &mut cache);
        assert_eq!(pixel(&p, 10), vec![20, 40, 220]);
        d.extras.cameras.push(crate::features::Camera {
            optics: Default::default(),
            id: 1,
            name: "Camera".into(),
            start: 0,
            duration: 24,
            center: [32., 32.],
            zoom: 1.,
            angle: 0.,
            animation: Default::default(),
        });
        crate::animation::camera_record(
            &mut d.extras.cameras[0],
            crate::animation::CameraKey {
                optics: None,
                frame: 10,
                center: [52., 32.],
                zoom: 1.,
                angle: 0.,
            },
        );
        let p = raster(&d, 5, 64, 64, &ctx, &mut cache);
        assert_eq!(pixel(&p, 22), vec![220, 40, 20]);
        assert_eq!(pixel(&p, 37), vec![255, 255, 255]);
    }
    #[test]
    fn radial_gradient_is_rasterized_with_multiple_colors_and_alpha() {
        let mut doc = Document::default();
        doc.width = 64;
        doc.height = 64;
        let mut mark = crate::document::Mark {
            points: vec![[0., 0.], [64., 64.]].into(),
            rectangle: true,
            closed: true,
            fill: Some([255, 255, 255, 255]),
            stroke: false,
            ..Default::default()
        };
        mark.style.gradient = Some(crate::features::Gradient {
            radial: true,
            start: [32., 32.],
            end: [60., 32.],
            stops: vec![
                crate::features::Stop {
                    t: 0.,
                    color: [255, 0, 0, 255],
                },
                crate::features::Stop {
                    t: 0.5,
                    color: [0, 255, 0, 255],
                },
                crate::features::Stop {
                    t: 1.,
                    color: [0, 0, 255, 128],
                },
            ]
            .into(),
        });
        doc.layers[0].frames[0].push(mark);
        let pixels = raster(
            &doc,
            0,
            64,
            64,
            &egui::Context::default(),
            &mut Default::default(),
        );
        let pixel = |x: usize| &pixels[(32 * 64 + x) * 3..(32 * 64 + x) * 3 + 3];
        assert!(pixel(32)[0] > 200);
        assert!(pixel(45)[1] > 180);
        assert!(pixel(59)[2] > 200);
        assert!(pixel(59)[0] > 80);
    }
    #[test]
    fn raster_uses_only_visible_document_geometry() {
        let mut d = Document::default();
        d.width = 64;
        d.height = 64;
        d.layers[0].frames[0].push(crate::document::Mark {
            points: vec![[10., 10.], [54., 10.], [54., 54.], [10., 54.]].into(),
            fill: Some([220, 40, 20, 255]),
            stroke: false,
            closed: true,
            ..Default::default()
        });
        let ctx = egui::Context::default();
        let mut cache = Default::default();
        let pixels = raster(&d, 0, 64, 64, &ctx, &mut cache);
        assert_eq!(
            &pixels[(32 * 64 + 32) * 3..(32 * 64 + 32) * 3 + 3],
            &[220, 40, 20]
        );
        assert_eq!(&pixels[0..3], &[255, 255, 255]);
        d.layers[0].visible = false;
        assert!(
            raster(&d, 0, 64, 64, &ctx, &mut cache)
                .iter()
                .all(|&v| v == 255)
        );
    }
    #[test]
    fn quality_scale_increases_fidelity_and_keeps_lossless_at_100() {
        assert_eq!(h264_crf(100), 0);
        assert_eq!(h264_crf(1), 40);
        assert!(h264_crf(95) < 20);
        for q in 1..100 {
            assert!(h264_crf(q + 1) <= h264_crf(q));
        }
    }
    #[test]
    #[ignore = "manual external FFmpeg test; verifies RGB pixels, not merely a quality label"]
    fn maximum_mp4_and_mkv_preserve_every_rgb_pixel() {
        let mut d = Document::default();
        d.width = 96;
        d.height = 64;
        d.fps = 24;
        for f in 0..2 {
            for x in 4..92 {
                let color = if (x + f) % 2 == 0 {
                    [255, 0, 0, 255]
                } else {
                    [0, 0, 255, 255]
                };
                d.layers[0].frames[f].push(crate::document::Mark {
                    points: vec![[x as f32, 4.], [x as f32 + 1., 60.]].into(),
                    rectangle: true,
                    closed: true,
                    stroke: false,
                    fill: Some(color),
                    ..Default::default()
                });
            }
        }
        let ctx = egui::Context::default();
        let mut cache = crate::paint::RenderCache::default();
        let expected: Vec<_> = (0..2)
            .flat_map(|f| raster(&d, f, 96, 64, &ctx, &mut cache))
            .collect();
        let directory =
            std::env::temp_dir().join(format!("libretoon-lossless-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        for format in [0, 2] {
            let settings = ExportSettings {
                name: "rgb-lossless".into(),
                folder: directory.to_string_lossy().into_owned(),
                format,
                first: 1,
                last: 2,
                width: 96,
                height: 64,
                fps: 24,
                quality: 100,
                loops: 0,
                colors: 256,
                dither: true,
                svg_sequence: false,
                sheet: SheetSettings::default(),
            };
            let path = settings.target().unwrap();
            run_export(
                &d,
                &settings,
                0,
                &path,
                ffmpeg_path(""),
                Default::default(),
                Default::default(),
            )
            .unwrap();
            let mut command = Command::new(ffmpeg_path(""));
            hidden(&mut command);
            let decoded = command
                .args(["-v", "error", "-i"])
                .arg(&path)
                .args([
                    "-fps_mode",
                    "passthrough",
                    "-f",
                    "rawvideo",
                    "-pix_fmt",
                    "rgb24",
                    "pipe:1",
                ])
                .output()
                .unwrap();
            assert!(
                decoded.status.success(),
                "{}",
                String::from_utf8_lossy(&decoded.stderr)
            );
            assert_eq!(
                decoded.stdout, expected,
                "Maximum quality must preserve fine colored edges exactly"
            );
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    #[ignore]
    fn export_all_containers_with_ffmpeg() {
        let mut d = Document::default();
        d.width = 64;
        d.height = 64;
        d.fps = 24;
        for (f, color) in [[230, 30, 20, 255], [20, 230, 30, 255], [30, 20, 230, 255]]
            .into_iter()
            .enumerate()
        {
            d.layers[0].frames[f].push(crate::document::Mark {
                points: vec![[10., 10.], [54., 54.]].into(),
                rectangle: true,
                closed: true,
                fill: Some(color),
                stroke: false,
                ..Default::default()
            });
        }
        let dir =
            std::env::temp_dir().join(format!("libretoon-export-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (format, quality) in (0..6).flat_map(|f| {
            if matches!(f, 0 | 2 | 3 | 4) {
                vec![(f, 95), (f, 100)]
            } else {
                vec![(f, 95)]
            }
        }) {
            let s = ExportSettings {
                name: format!("test-quality-{quality}"),
                folder: dir.to_string_lossy().into_owned(),
                format,
                first: 1,
                last: 3,
                width: 64,
                height: 64,
                fps: 24,
                quality,
                loops: 0,
                colors: 64,
                dither: true,
                svg_sequence: false,
                sheet: SheetSettings::default(),
            };
            let path = s.target().unwrap();
            run_export(
                &d,
                &s,
                0,
                &path,
                ffmpeg_path(""),
                Default::default(),
                Default::default(),
            )
            .unwrap();
            assert!(std::fs::metadata(&path).unwrap().len() > 50);
            if format != 1 {
                let mut cmd = Command::new(ffmpeg_path(""));
                hidden(&mut cmd);
                let decoded = cmd
                    .args(["-hide_banner", "-loglevel", "error", "-i"])
                    .arg(&path)
                    .args([
                        "-fps_mode",
                        "passthrough",
                        "-f",
                        "rawvideo",
                        "-pix_fmt",
                        "rgb24",
                        "pipe:1",
                    ])
                    .output()
                    .unwrap();
                assert!(
                    decoded.status.success(),
                    "{}",
                    String::from_utf8_lossy(&decoded.stderr)
                );
                assert_eq!(decoded.stdout.len(), 64 * 64 * 3 * 3, "format={format}");
                for f in 0..3 {
                    let offset = f * 64 * 64 * 3 + (32 * 64 + 32) * 3;
                    let pixel = &decoded.stdout[offset..offset + 3];
                    assert!(pixel[f] > 180, "format={format} frame={f} pixel={pixel:?}");
                    assert!(pixel[(f + 1) % 3] < 80);
                }
            }
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}

/// Drain stderr fully to avoid pipe deadlocks, retaining only the last 64 KiB.
fn read_log(reader: &mut impl Read) -> String {
    let mut tail = Vec::new();
    let mut block = [0u8; 4096];
    loop {
        match reader.read(&mut block) {
            Ok(0) => break,
            Ok(n) => {
                if tail.len() + n > 65536 {
                    let remove = tail.len() + n - 65536;
                    tail.drain(..remove);
                }
                tail.extend_from_slice(&block[..n]);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(&tail).into_owned()
}
#[cfg(test)]
mod audit_log_tests {
    #[test]
    fn ffmpeg_logs_are_drained_with_bounded_storage() {
        let mut bytes = std::io::Cursor::new(vec![b'x'; 300000]);
        let result = super::read_log(&mut bytes);
        assert_eq!(bytes.position(), 300000);
        assert_eq!(result.len(), 65536);
    }
}

// Headless visual QA shares the export rasterizer; it never changes application rendering.
#[cfg(test)]
pub(crate) fn raster_ui_test(
    ctx: &egui::Context,
    output: &mut egui::FullOutput,
    w: u32,
    h: u32,
) -> Vec<u8> {
    let mut textures = std::collections::HashMap::<egui::TextureId, ([usize; 2], Vec<u8>)>::new();
    for (id, deltas) in &output.textures_delta.set {
        for delta in deltas {
            let egui::ImageData::Color(image) = &delta.image;
            let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
            if let Some([x, y]) = delta.pos {
                if let Some((size, data)) = textures.get_mut(id) {
                    for row in 0..image.size[1] {
                        let start = ((y + row) * size[0] + x) * 4;
                        let source = row * image.size[0] * 4;
                        data[start..start + image.size[0] * 4]
                            .copy_from_slice(&rgba[source..source + image.size[0] * 4]);
                    }
                }
            } else {
                textures.insert(*id, (image.size, rgba));
            }
        }
    }
    let mut pixels = [39, 40, 44].repeat((w * h) as usize);
    let mut tinted = std::collections::HashMap::<(egui::TextureId, [u8; 4]), Vec<u8>>::new();
    for primitive in ctx.tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point) {
        if let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive {
            let texture = textures
                .get(&mesh.texture_id)
                .map(|(size, data)| (*size, data.as_slice()));
            for indices in mesh.indices.chunks_exact(3) {
                let vertices = [
                    mesh.vertices[indices[0] as usize],
                    mesh.vertices[indices[1] as usize],
                    mesh.vertices[indices[2] as usize],
                ];
                if vertices.iter().all(|v| v.uv == egui::epaint::WHITE_UV) {
                    triangle(&mut pixels, w, h, vertices, primitive.clip_rect, None);
                } else if let Some((size, data)) = texture {
                    let color = vertices[0].color.to_srgba_unmultiplied();
                    let rgba = tinted.entry((mesh.texture_id, color)).or_insert_with(|| {
                        data.chunks_exact(4)
                            .flat_map(|p| {
                                [
                                    color[0],
                                    color[1],
                                    color[2],
                                    ((p[3] as u16 * color[3] as u16) / 255) as u8,
                                ]
                            })
                            .collect()
                    });
                    triangle(
                        &mut pixels,
                        w,
                        h,
                        vertices,
                        primitive.clip_rect,
                        Some((size, rgba)),
                    );
                }
            }
        }
    }
    pixels
}
