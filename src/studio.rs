use crate::{
    Editor,
    canvas::{Tool, View},
    document::Mark,
    i18n::tr,
};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

pub const TOOLS: &[Tool] = &[
    Tool::Select,
    Tool::Object,
    Tool::Lasso,
    Tool::PolygonSelect,
    Tool::Wand,
    Tool::Transform,
    Tool::Mirror,
    Tool::Brush,
    Tool::FillBrush,
    Tool::ShadeBrush,
    Tool::LightBrush,
    Tool::Eraser,
    Tool::Bucket,
    Tool::Eyedropper,
    Tool::VectorBrush,
    Tool::Pen,
    Tool::Node,
    Tool::Width,
    Tool::Line,
    Tool::Rectangle,
    Tool::Ellipse,
    Tool::Polygon,
    Tool::Pan,
    Tool::Zoom,
    Tool::RotateView,
    Tool::Ruler,
    Tool::Symmetry,
    Tool::Perspective,
    Tool::Camera,
    Tool::Text,
    Tool::Airbrush,
    Tool::SketchBrush,
    Tool::Mannequin,
    Tool::Bones,
    Tool::Puppet,
    Tool::Trim,
];
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct Binding {
    pub key: String,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}
impl Binding {
    pub(crate) fn new(key: &str) -> Self {
        Self {
            key: key.into(),
            ctrl: false,
            shift: false,
            alt: false,
        }
    }
    pub(crate) fn label(&self) -> String {
        if self.key.is_empty() {
            return String::new();
        }
        format!(
            "{}{}{}{}",
            if self.ctrl {
                if cfg!(target_os = "macos") {
                    "Cmd+"
                } else {
                    "Ctrl+"
                }
            } else {
                ""
            },
            if self.shift { "Shift+" } else { "" },
            if self.alt { "Alt+" } else { "" },
            self.key
        )
    }
}
#[derive(Clone, Copy)]
pub(crate) enum LayerColor {
    Vector,
    Raster,
    Camera,
    Rigging,
    Video,
    Audio,
    Animation,
    Adjustment,
    Mannequin,
}

impl LayerColor {
    const ALL: [(Self, &'static str, crate::design::Icon, [u8; 3]); 9] = [
        (
            Self::Vector,
            "Vector layer",
            crate::design::Icon::Pen,
            [124, 151, 182],
        ),
        (
            Self::Raster,
            "Raster layer",
            crate::design::Icon::Brush,
            [102, 172, 164],
        ),
        (
            Self::Camera,
            "Cámaras",
            crate::design::Icon::Camera,
            [230, 157, 55],
        ),
        (
            Self::Rigging,
            "Rigging",
            crate::design::Icon::Bones,
            [86, 211, 214],
        ),
        (
            Self::Video,
            "Video",
            crate::design::Icon::Film,
            [65, 168, 103],
        ),
        (
            Self::Audio,
            "Audio",
            crate::design::Icon::Audio,
            [199, 78, 89],
        ),
        (
            Self::Animation,
            "Animation clip",
            crate::design::Icon::Symbol,
            [163, 119, 235],
        ),
        (
            Self::Adjustment,
            "Adjustment layer",
            crate::design::Icon::Fx,
            [105, 151, 237],
        ),
        (
            Self::Mannequin,
            "Mannequin",
            crate::design::Icon::Mannequin,
            [180, 128, 224],
        ),
    ];
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub view: crate::view_options::Config,
    #[serde(skip_serializing)]
    pub spanish: bool,
    #[serde(deserialize_with = "crate::i18n::deserialize_language")]
    pub language: Option<crate::i18n::Language>,
    pub theme: crate::themes::Theme,
    pub auto_key_paint: bool,
    pub(crate) preview_cache_mib: usize,
    pub(crate) viewport_quality: crate::depth::ViewportQuality,
    pub saving: crate::project_save::Options,
    pub raster_brush: crate::raster::Brush,
    pub(crate) light_presets: Vec<crate::light::Preset>,
    pub(crate) export_presets: Vec<crate::exporter::Preset>,
    pub(crate) favorite_colors: Vec<[u8; 4]>,
    pub(crate) color_preset: usize,
    layer_colors: [[u8; 3]; 9],
    pub bindings: Vec<Binding>,
    pub actions: Vec<Binding>,
    pub dpi: f32,
    pub ffmpeg: String,
}
impl Preferences {
    pub(crate) fn layer_color(&self, kind: LayerColor) -> Color32 {
        let [r, g, b] = self.layer_colors[kind as usize];
        Color32::from_rgb(r, g, b)
    }

    pub fn locale(&self) -> crate::i18n::Language {
        self.language.unwrap_or(if self.spanish {
            crate::i18n::Language::Spanish
        } else {
            crate::i18n::Language::English
        })
    }
}
impl Default for Preferences {
    fn default() -> Self {
        let defaults = [
            "V", "", "L", "", "", "Q", "M", "B", "", "", "", "E", "K", "I", "Y", "P", "A", "W",
            "N", "R", "O", "", "H", "Z", "", "", "S", "", "", "T", "", "", "", "", "",
        ];
        Self {
            view: Default::default(),
            spanish: false,
            language: None,
            theme: Default::default(),
            auto_key_paint: false,
            preview_cache_mib: 256,
            viewport_quality: Default::default(),
            saving: Default::default(),
            raster_brush: Default::default(),
            light_presets: Vec::new(),
            export_presets: Vec::new(),
            favorite_colors: Vec::new(),
            color_preset: 0,
            layer_colors: LayerColor::ALL.map(|(_, _, _, color)| color),
            bindings: TOOLS
                .iter()
                .enumerate()
                .map(|(i, _)| Binding::new(defaults.get(i).copied().unwrap_or("")))
                .collect(),
            actions: crate::shortcuts::defaults(),
            dpi: 96.,
            ffmpeg: String::new(),
        }
    }
}
pub struct Studio {
    pub prefs: Preferences,
    pub preferences_open: bool,
    pub credits_open: bool,
    pub(crate) palette: crate::palette::Panel,
    preference_category: usize,
    pub stage_only: bool,
    pub(crate) layer_objects_open: bool,
    pub(crate) command_helper: bool,
    pub(crate) color_picker: Option<bool>,
    pub onion_before: usize,
    pub onion_after: usize,
    pub clipboard: Vec<Mark>,
    pub clipboard_symbols: crate::shared::SharedVec<crate::features::Symbol>,
    pub horizontal: bool,
    pub vertical: bool,
    pub rulers: bool,
    unit: usize,
    pub(crate) guides: Vec<(bool, f32)>,
    guide_drag: Option<usize>,
    pub strength: f32,
    pub tone_mode: usize,
    capture: Option<usize>,
    shortcut_filter: String,
    held: Option<Instant>,
    brush_menu: bool,
    pub brush: Tool,
    pub export: Option<crate::exporter::ExportSettings>,
    pub ffmpeg_setup: crate::ffmpeg_setup::Setup,
    pub export_job: Option<std::sync::mpsc::Receiver<Result<PathBuf, String>>>,
    pub(crate) export_batch: crate::exporter::ExportBatch,
    pub(crate) export_batch_total: usize,
    pub(crate) export_batch_index: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    pub export_progress: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    pub export_cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    notice: String,
    last_status: String,
    notice_time: Option<Instant>,
}
fn preferences_path() -> PathBuf {
    crate::ffmpeg_setup::data_dir().join("preferences.json")
}

impl Studio {
    pub fn load() -> Self {
        let mut prefs: Preferences = std::fs::read(preferences_path())
            .or_else(|_| std::fs::read(std::env::temp_dir().join("LibreToon/preferences.json")))
            .ok()
            .and_then(|s| serde_json::from_slice(&s).ok())
            .unwrap_or_default();
        if prefs.language.is_none() {
            prefs.language = Some(prefs.locale());
        }
        if prefs.bindings.len() != TOOLS.len() {
            let mut bindings = Preferences::default().bindings;
            for (i, b) in prefs.bindings.into_iter().enumerate().take(TOOLS.len()) {
                bindings[i] = b;
            }
            prefs.bindings = bindings;
        }
        let defaults = crate::shortcuts::defaults();
        for (index, binding) in defaults.into_iter().enumerate() {
            if index >= prefs.actions.len() {
                prefs.actions.push(binding);
            }
        }
        prefs.actions.truncate(crate::shortcuts::ACTIONS.len());
        // Missing fields use Preferences::default; append future actions without touching tools.
        if !prefs.dpi.is_finite() {
            prefs.dpi = 96.;
        }
        prefs.dpi = prefs.dpi.clamp(1., 2400.);
        prefs.view.normalize();
        Self {
            prefs,
            preferences_open: false,
            credits_open: false,
            palette: Default::default(),
            preference_category: 0,
            stage_only: false,
            layer_objects_open: false,
            command_helper: false,
            color_picker: None,
            onion_before: 1,
            onion_after: 1,
            clipboard: vec![],
            clipboard_symbols: Default::default(),
            horizontal: false,
            vertical: false,
            rulers: false,
            unit: 0,
            guides: vec![],
            guide_drag: None,
            strength: 0.2,
            tone_mode: 0,
            capture: None,
            shortcut_filter: String::new(),
            held: None,
            brush_menu: false,
            brush: Tool::Brush,
            export: None,
            ffmpeg_setup: Default::default(),
            export_job: None,
            export_batch: Default::default(),
            export_batch_total: 1,
            export_batch_index: Default::default(),
            export_progress: Default::default(),
            export_cancel: Default::default(),
            notice: String::new(),
            last_status: String::new(),
            notice_time: None,
        }
    }
    pub(crate) fn save(&self) -> Result<(), String> {
        let p = preferences_path();
        std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
        let bytes = serde_json::to_vec_pretty(&self.prefs).map_err(|e| e.to_string())?;
        std::fs::write(p, bytes).map_err(|e| e.to_string())
    }
    fn units_per_pixel(&self) -> f32 {
        match self.unit {
            1 => 2.54 / self.prefs.dpi,
            2 => 25.4 / self.prefs.dpi,
            3 => 1. / self.prefs.dpi,
            _ => 1.,
        }
    }
}
impl Editor {
    pub fn tool_button(&mut self, ui: &mut egui::Ui, slot: Tool) {
        let raster = self.raster_active();
        if slot == Tool::Brush && raster {
            self.raster_toolbar(ui);
            return;
        }
        let vector_only = matches!(
            slot,
            Tool::Pen
                | Tool::VectorBrush
                | Tool::Node
                | Tool::Width
                | Tool::Trim
                | Tool::Rectangle
                | Tool::Ellipse
                | Tool::Line
                | Tool::Polygon
                | Tool::Bucket
                | Tool::Text
        );
        let tool = if slot == Tool::Brush {
            self.studio.brush
        } else {
            slot
        };
        let response = crate::design::button(
            ui,
            tool.icon(),
            "",
            if raster && vector_only {
                tr("Select a vector layer to use this tool")
            } else {
                tr(tool.name())
            },
            self.tool == tool,
            !raster || !vector_only,
        );
        if slot == Tool::Brush {
            crate::raster::badge(ui, response.rect, "V");
            if response.is_pointer_button_down_on() {
                let start = self.studio.held.get_or_insert_with(Instant::now);
                if start.elapsed() > Duration::from_millis(400) {
                    self.studio.brush_menu = true;
                }
                ui.ctx().request_repaint_after(Duration::from_millis(50));
            } else {
                self.studio.held = None;
            }
            if response.secondary_clicked() {
                self.studio.brush_menu = true;
            }
            let tip = response.rect.right_bottom() - Vec2::splat(4.);
            ui.painter().add(egui::Shape::convex_polygon(
                vec![tip, tip - Vec2::new(5., 0.), tip - Vec2::new(0., 5.)],
                crate::themes::muted(),
                Stroke::NONE,
            ));
            if response.clicked()
                && response.interact_pointer_pos().is_some_and(|p| {
                    p.x > response.rect.right() - 12. && p.y > response.rect.bottom() - 12.
                })
            {
                self.studio.brush_menu = true;
            }
            let mut open = self.studio.brush_menu;
            egui::Popup::from_response(&response)
                .at_position(Pos2::new(ui.clip_rect().right() + 6., response.rect.top()))
                .align(egui::emath::RectAlign::RIGHT_START)
                .width(232.)
                .open_bool(&mut open)
                .show(|ui| {
                    ui.set_width(220.);
                    ui.label(
                        egui::RichText::new(tr("Pinceles"))
                            .small()
                            .color(crate::themes::muted()),
                    );
                    ui.separator();
                    for t in [
                        Tool::Brush,
                        Tool::VectorBrush,
                        Tool::FillBrush,
                        Tool::ShadeBrush,
                        Tool::LightBrush,
                        Tool::SketchBrush,
                    ] {
                        let (r, row) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 34.),
                            Sense::click(),
                        );
                        if self.tool == t || row.hovered() {
                            ui.painter().rect_filled(
                                r,
                                3.,
                                if self.tool == t {
                                    crate::themes::color(Color32::from_rgb(47, 73, 104))
                                } else {
                                    crate::themes::color(Color32::from_gray(58))
                                },
                            );
                        }
                        crate::design::draw(
                            ui.painter(),
                            Rect::from_center_size(
                                Pos2::new(r.left() + 18., r.center().y),
                                Vec2::splat(20.),
                            ),
                            t.icon(),
                            crate::themes::muted(),
                        );
                        ui.painter().text(
                            Pos2::new(r.left() + 38., r.center().y),
                            egui::Align2::LEFT_CENTER,
                            tr(t.name()),
                            egui::FontId::proportional(12.),
                            crate::themes::color(Color32::from_gray(220)),
                        );
                        if row.clicked() {
                            self.studio.brush = t;
                            self.activate_tool(t);
                            ui.close();
                        }
                    }
                });
            self.studio.brush_menu = open;
        }
        if response.clicked() && !self.studio.brush_menu {
            self.activate_tool(tool);
        }
    }
    pub fn window_controls(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        for (icon, tip, command) in [
            (
                crate::design::Icon::Close,
                "Cerrar",
                egui::ViewportCommand::Close,
            ),
            (
                crate::design::Icon::Maximize,
                "Maximizar / restaurar",
                egui::ViewportCommand::Maximized(
                    !ctx.input(|i| i.viewport().maximized.unwrap_or(false)),
                ),
            ),
            (
                crate::design::Icon::Minimize,
                "Minimizar",
                egui::ViewportCommand::Minimized(true),
            ),
        ] {
            let (r, response) = ui.allocate_exact_size(Vec2::new(44., 32.), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(
                    r,
                    2.,
                    if matches!(icon, crate::design::Icon::Close) {
                        crate::themes::color(Color32::from_rgb(185, 55, 65))
                    } else {
                        crate::themes::color(Color32::from_gray(65))
                    },
                );
            }
            crate::design::draw(
                ui.painter(),
                r,
                icon,
                crate::themes::color(Color32::from_gray(210)),
            );
            if response.on_hover_text(tr(tip)).clicked() {
                if matches!(command, egui::ViewportCommand::Close) {
                    self.finish_pending_pen();
                }
                ctx.send_viewport_cmd(command);
            }
        }
    }
    pub fn tool_shortcuts(&mut self, ctx: &egui::Context) {
        if self.studio.preferences_open || self.studio.export.is_some() {
            return;
        }
        let found = TOOLS
            .iter()
            .zip(&self.studio.prefs.bindings)
            .find_map(|(&tool, b)| {
                if tool == Tool::Airbrush {
                    return None;
                }
                b.take_pressed(ctx).then_some(tool)
            });
        if let Some(tool) = found {
            self.activate_tool(tool);
            if !self.raster_active()
                && matches!(
                    tool,
                    Tool::Brush
                        | Tool::Airbrush
                        | Tool::VectorBrush
                        | Tool::SketchBrush
                        | Tool::FillBrush
                        | Tool::ShadeBrush
                        | Tool::LightBrush
                )
            {
                self.studio.brush = tool;
            }
        }
    }
    pub fn clipboard_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input()
            || self.animation.scenes_open
            || self.animation.picker.is_some()
            || self.studio.preferences_open
            || self.studio.export.is_some()
            || self.document_settings.is_some()
            || self.view_options.draft.is_some()
            || self.folder_groups.dialog.is_some()
        {
            return;
        }
        // egui-winit delivers native clipboard events for these shortcuts.
        let (mut copy, mut cut, mut paste) = (false, false, false);
        ctx.input_mut(|input| {
            input.events.retain(|event| {
                match event {
                    egui::Event::Copy => copy = true,
                    egui::Event::Cut => cut = true,
                    egui::Event::Paste(_) | egui::Event::PasteImage(_) => paste = true,
                    _ => return true,
                }
                false
            });
            copy |= input.consume_key(egui::Modifiers::COMMAND, egui::Key::C);
            cut |= input.consume_key(egui::Modifiers::COMMAND, egui::Key::X);
            paste |= input.consume_key(egui::Modifiers::COMMAND, egui::Key::V);
        });
        if copy {
            if self.frame_clipboard.focus {
                self.copy_frames_to_clipboard(ctx);
            } else {
                self.copy_canvas_to_clipboard(ctx, false);
            }
        }
        if cut {
            self.copy_canvas_to_clipboard(ctx, true);
        }
        if paste {
            if self.frame_clipboard.focus && self.frame_clipboard.has_frames() {
                self.paste_timeline_frames();
            } else {
                self.paste_canvas_clipboard();
            }
        }
    }
    pub(crate) fn command_helper_ui(&self, ui: &mut egui::Ui, area: egui::Rect) {
        if !self.studio.command_helper {
            return;
        }
        let tool = self.tool;
        let tr = crate::i18n::tr;
        let command = if cfg!(target_os = "macos") {
            "Cmd"
        } else {
            "Ctrl"
        };
        let modifiers: &[(&str, &str)] = match tool {
            Tool::Bones if self.rigging.paint => &[
                ("Drag", "Paint bone weights"),
                ("{cmd} + drag", "Remove bone weights"),
                ("Shift + drag", "Smooth bone weights"),
            ],
            Tool::Bones => &[
                ("Shift + drag", "Move bone anchor"),
                ("Alt + drag", "Scale bone"),
                ("{cmd} + drag", "Stretch or squash bone"),
                ("{cmd} + Shift + drag", "Move bone pose"),
            ],
            Tool::Puppet => &[("Shift + drag", "Move pin anchor")],
            Tool::Select | Tool::Object => &[
                ("Shift + click", "Add or remove from selection"),
                ("Shift + drag", "Move along one axis"),
                ("Double-click a stroke", "Select connected strokes"),
                ("Double-click a symbol", "Edit symbol"),
                ("{cmd} + C / X / V", "Copy / Cut / Paste"),
            ],
            Tool::Lasso | Tool::PolygonSelect | Tool::Wand => &[("Shift", "Add to selection")],
            Tool::Rectangle | Tool::Ellipse | Tool::Polygon | Tool::Line => &[
                ("Shift + drag", "Constrain proportions or angle"),
                ("Alt + drag", "Ignore snapping"),
            ],
            Tool::Pen => &[
                ("Click / drag", "Create a corner / curve"),
                ("Shift + drag", "Snap new tangent to 45°"),
                ("Alt + click an anchor", "Convert anchor to a corner"),
                ("Alt + drag", "Edit one handle independently"),
                ("Click first anchor", "Close path"),
                ("Enter / Esc", "Finish / cancel path"),
                ("Delete", "Delete selected anchor"),
            ],
            Tool::Node => &[
                ("Alt + click an anchor", "Convert anchor to a corner"),
                ("Alt + drag", "Edit one handle independently"),
                ("Delete", "Delete selected anchor"),
            ],
            Tool::Zoom => &[("Alt + click", "Reverse zoom direction")],
            Tool::Transform => &[
                ("Shift + drag object", "Move along one axis"),
                ("Shift + drag corner", "Keep proportions"),
                ("Shift + drag rotation handle", "Snap rotation to 15°"),
                ("Alt + drag", "Ignore snapping"),
                ("{cmd} + C / X / V", "Copy / Cut / Paste"),
            ],
            Tool::Brush if self.doc.layers[self.layer].options.light_mask => &[
                ("Click or drag", "Paint light mask"),
                ("{cmd} + click or drag", "Remove from light mask"),
            ],
            _ => &[],
        };
        let shortcut = TOOLS
            .iter()
            .position(|&candidate| candidate == tool)
            .and_then(|index| self.studio.prefs.bindings.get(index))
            .map(Binding::label)
            .unwrap_or_default();
        egui::Area::new(egui::Id::unique("tool-command-helper"))
            .order(egui::Order::Foreground)
            .fixed_pos(area.right_bottom() - egui::vec2(8., 8.))
            .pivot(egui::Align2::RIGHT_BOTTOM)
            .interactable(false)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.strong(tool.name());
                        if !shortcut.is_empty() {
                            ui.label(egui::RichText::new(shortcut).monospace().weak());
                        }
                    });
                    for (gesture, action) in modifiers {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(tr(gesture).replace("{cmd}", command))
                                    .monospace(),
                            );
                            ui.small(tr(action));
                        });
                    }
                    ui.small(tr("Middle mouse drag · Pan canvas"));
                });
            });
    }

    pub(crate) fn copy_frames_to_clipboard(&mut self, ctx: &egui::Context) {
        if self.copy_timeline_frames() {
            ctx.copy_text("LibreToon frames".to_owned());
        }
    }

    pub(crate) fn copy_canvas_to_clipboard(&mut self, ctx: &egui::Context, cut: bool) {
        if !self.canvas.selected.is_empty() {
            self.copy_selection(cut);
            if !self.studio.clipboard.is_empty() {
                ctx.copy_text("LibreToon artwork".to_owned());
            }
        }
    }

    pub(crate) fn paste_canvas_clipboard(&mut self) {
        if !self.paste_media_image() {
            self.paste_selection();
        }
    }

    pub fn copy_selection(&mut self, cut: bool) {
        let mut ids = self.canvas.selected.clone();
        ids.sort_unstable();
        ids.dedup();
        if ids.is_empty() {
            return;
        }
        self.frame_clipboard.clear();
        self.studio.clipboard_symbols = self.doc.extras.symbols.clone();
        self.studio.clipboard.clear();
        for &(l, m) in &ids {
            if let Some((_, inside)) = self.canvas.partial.get(&(l, m)) {
                self.studio.clipboard.extend(inside.iter().cloned());
            } else if let Some(mark) = self
                .doc
                .layers
                .get(l)
                .and_then(|layer| layer.frames.get(self.frame))
                .and_then(|marks| marks.get(m))
            {
                self.studio.clipboard.push(mark.clone());
            }
        }
        if cut {
            let mut state = std::mem::take(&mut self.canvas);
            self.checkpoint();
            self.materialize_fragments(&mut state);
            ids = state.selected.clone();
            ids.sort_unstable();
            for (l, m) in ids.into_iter().rev() {
                self.doc.layers[l].frames[self.frame].remove(m);
            }
            state.clear_selection();
            self.canvas = state;
            self.recover();
        }
    }
    fn clipboard_symbol_matches(
        &self,
        id: u64,
        checked: &mut std::collections::HashSet<u64>,
    ) -> bool {
        if !checked.insert(id) {
            return true;
        }
        let Some(source) = self.studio.clipboard_symbols.iter().find(|s| s.id == id) else {
            return false;
        };
        let Some(target) = self.doc.extras.symbols.iter().find(|s| s.id == id) else {
            return false;
        };
        // A shared definition is the common same-document path, with no serialization.
        if source.layers.identity() != target.layers.identity() {
            if serde_json::to_vec(source).ok() != serde_json::to_vec(target).ok() {
                return false;
            }
        }
        crate::features::symbol_graph(std::slice::from_ref(source))
            .get(&id)
            .into_iter()
            .flatten()
            .all(|child| self.clipboard_symbol_matches(*child, checked))
    }
    pub fn paste_selection(&mut self) {
        if self.raster_active() || self.adjustment_active() {
            self.status = tr("Select a vector layer to use this tool").into();
            return;
        }
        if self.studio.clipboard.is_empty()
            || !self.doc.layers[self.layer].visible
            || self.doc.layers[self.layer].options.locked
            || self.doc.layers[self.layer].options.mannequin.is_some()
        {
            return;
        }
        if self
            .studio
            .clipboard
            .iter()
            .filter_map(|m| m.style.instance.as_ref())
            .any(|i| {
                !self.doc.extras.symbols.iter().any(|s| s.id == i.symbol)
                    || !self
                        .clipboard_symbol_matches(i.symbol, &mut std::collections::HashSet::new())
                    || !self.symbol_insert_allowed(i.symbol)
            })
        {
            self.status =
                "No se pueden pegar símbolos ausentes, incompatibles o recursivos en este documento".into();
            return;
        }
        let bound = self
            .studio
            .clipboard
            .iter()
            .filter(|m| {
                self.doc
                    .extras
                    .rig
                    .bindings
                    .iter()
                    .any(|b| b.target == m.style.track)
            })
            .count();
        if self.doc.extras.rig.bindings.len() + bound > 512 {
            self.status = "No se pueden duplicar más instancias articuladas (límite: 512)".into();
            return;
        }
        self.finish_pending_pen();
        self.checkpoint();
        let mut pasted_marks = vec![];
        for mark in &self.studio.clipboard {
            let mut pasted = mark.clone();
            let track = crate::features::id();
            if let Some(mut binding) = self
                .doc
                .extras
                .rig
                .bindings
                .iter()
                .find(|b| b.target == mark.style.track)
                .cloned()
            {
                binding.target = track;
                self.doc.extras.rig.bindings.push(binding);
                for bone in &mut self.doc.extras.rig.bones {
                    if bone.targets.contains(&mark.style.track) {
                        bone.targets.push(track);
                    }
                }
            }
            pasted.style.track = track;
            pasted_marks.push(pasted);
        }
        let frame = &mut self.doc.layers[self.layer].frames[self.frame];
        let start = frame.len();
        frame.extend(pasted_marks);
        self.canvas.selected = (start..frame.len()).map(|m| (self.layer, m)).collect();
        self.activate_tool(Tool::Select);
        self.recover();
    }
    pub fn mirror_selection(&mut self, horizontal: bool) {
        let ids = self.canvas.selected.clone();
        if ids.is_empty() {
            return;
        }
        let mut bounds = Rect::NOTHING;
        for &(l, m) in &ids {
            bounds = bounds.union(crate::vector::control_bounds(
                &self.doc.layers[l].frames[self.frame][m],
            ));
        }
        self.checkpoint();
        let center = bounds.center();
        for &(l, m) in &ids {
            let mark = &self.doc.layers[l].frames[self.frame][m];
            self.doc.layers[l].frames[self.frame][m] = crate::vector::transform(mark, |p| {
                if horizontal {
                    Pos2::new(2. * center.x - p.x, p.y)
                } else {
                    Pos2::new(p.x, 2. * center.y - p.y)
                }
            });
        }
        self.canvas.selected = ids;
        self.recover();
    }
    pub fn symmetry_marks(&self, marks: Vec<Mark>) -> Vec<Mark> {
        let mut result = marks;
        if self.studio.horizontal {
            let reflected: Vec<_> = result
                .iter()
                .map(|m| {
                    crate::vector::transform(m, |p| Pos2::new(p.x, self.doc.height as f32 - p.y))
                })
                .collect();
            result.extend(reflected);
        }
        if self.studio.vertical {
            let reflected: Vec<_> = result
                .iter()
                .map(|m| {
                    crate::vector::transform(m, |p| Pos2::new(self.doc.width as f32 - p.x, p.y))
                })
                .collect();
            result.extend(reflected);
        }
        result
    }
    pub fn fill_brush(&mut self, brush: Vec<Mark>) {
        let fills: Vec<_> = self.doc.layers[self.layer].frames[self.frame]
            .iter()
            .filter(|m| m.fill.is_some())
            .cloned()
            .collect();
        let mask = crate::features::ink(
            &fills
                .iter()
                .map(|m| {
                    let mut m = m.clone();
                    m.stroke = false;
                    m
                })
                .collect::<Vec<_>>(),
        );
        let parts = brush
            .iter()
            .flat_map(|m| crate::features::clip_to(m, &mask))
            .collect();
        let parts = self.prepare_style(parts);
        self.commit_paint(self.layer, self.frame, parts);
    }
    pub fn tone_paint(&mut self, brush: Vec<Mark>, light: bool) {
        use geo::BooleanOps;
        let areas: Vec<_> = brush.iter().map(crate::vector::polygon).collect();
        let region = geo::algorithm::bool_ops::unary_union(&areas);
        let originals = self.doc.layers[self.layer].frames[self.frame].clone();
        let mut result = vec![];
        for mark in &originals {
            if mark.style.instance.is_some() {
                result.push(mark.clone());
                continue;
            }
            let Some(color) = mark.fill else {
                result.push(mark.clone());
                continue;
            };
            let level = if light {
                mark.style.light
            } else {
                mark.style.shade
            };
            if level >= self.feature.toon_levels {
                result.push(mark.clone());
                continue;
            }
            let poly = geo::MultiPolygon(vec![crate::vector::polygon(mark)]);
            if !crate::vector::control_bounds(mark).intersects(
                brush.iter().fold(Rect::NOTHING, |b, m| {
                    b.union(crate::vector::control_bounds(m))
                }),
            ) {
                result.push(mark.clone());
                continue;
            }
            let inside = poly.intersection(&region);
            if inside.0.is_empty() {
                result.push(mark.clone());
                continue;
            }
            result.extend(crate::features::styled(
                poly.difference(&region),
                color,
                &mark.style,
            ));
            let mut style = mark.style.clone();
            if light {
                style.light += 1;
            } else {
                style.shade += 1;
            }
            if let Some(g) = style.gradient.as_mut() {
                for stop in &mut g.stops {
                    stop.color = tone(
                        stop.color,
                        self.studio.strength,
                        light,
                        self.studio.tone_mode,
                    );
                }
            }
            result.extend(crate::features::styled(
                inside,
                tone(color, self.studio.strength, light, self.studio.tone_mode),
                &style,
            ));
            if mark.stroke {
                let mut contour = mark.clone();
                contour.fill = None;
                result.push(contour);
            }
        }
        self.doc.layers[self.layer].frames[self.frame] = result.into();
    }
    pub fn studio_tool_options(&mut self, ui: &mut egui::Ui) {
        match self.tool {
            Tool::Ruler => {
                ui.checkbox(&mut self.studio.rulers, tr("Mostrar reglas"));
                let units = ["px", "cm", "mm", "in"];
                egui::ComboBox::from_id_salt("ruler-unit")
                    .selected_text(units[self.studio.unit])
                    .show_ui(ui, |ui| {
                        for (i, u) in units.iter().enumerate() {
                            ui.selectable_value(&mut self.studio.unit, i, *u);
                        }
                    });
                if ui
                    .add(
                        egui::DragValue::new(&mut self.studio.prefs.dpi)
                            .range(1. ..=2400.)
                            .suffix(crate::i18n::tr(" ppi")),
                    )
                    .changed()
                {
                    if let Err(e) = self.studio.save() {
                        self.status = e;
                    }
                }
                ui.label(tr("Arrastra desde las reglas para crear guías. Arrastra una guía fuera para eliminarla."));
                if ui.button(tr("Borrar guías")).clicked() {
                    self.clear_guides();
                }
            }
            Tool::Symmetry => {
                ui.checkbox(&mut self.studio.horizontal, tr("Horizontal"));
                ui.checkbox(&mut self.studio.vertical, tr("Vertical"));
                ui.label(tr(
                    "Ejes en el centro del escenario. Se aplica a pinceles y formas.",
                ));
            }
            Tool::Mirror => {
                ui.add_enabled_ui(!self.canvas.selected.is_empty(), |ui| {
                    if ui.button(tr("Voltear horizontalmente")).clicked() {
                        self.mirror_selection(true);
                    }
                    if ui.button(tr("Voltear verticalmente")).clicked() {
                        self.mirror_selection(false);
                    }
                });
            }
            Tool::ShadeBrush | Tool::LightBrush => {
                ui.add(
                    egui::DragValue::new(&mut self.feature.toon_levels)
                        .range(1..=16)
                        .prefix(tr("Niveles de Cartoon: ")),
                );
                ui.add(
                    egui::Slider::new(&mut self.studio.strength, 0.01..=1.).text(tr("Intensidad")),
                );
                egui::ComboBox::from_id_salt("tone-mode")
                    .selected_text(tr(
                        ["Mezcla", "Exposición", "Contraste"][self.studio.tone_mode]
                    ))
                    .show_ui(ui, |ui| {
                        for (i, s) in ["Mezcla", "Exposición", "Contraste"].iter().enumerate() {
                            ui.selectable_value(&mut self.studio.tone_mode, i, tr(s));
                        }
                    });
            }
            _ => {}
        }
    }
    pub fn rulers_ui(&mut self, ui: &egui::Ui, _p: &egui::Painter, area: Rect, view: View) -> bool {
        let mut consumed = false;
        let ctx = ui.ctx();
        if self.studio.rulers {
            let top = Rect::from_min_max(area.min, Pos2::new(area.right(), area.top() + 22.));
            let left = Rect::from_min_max(area.min, Pos2::new(area.left() + 22., area.bottom()));
            for (vertical, r) in [(false, top), (true, left)] {
                let response =
                    ui.interact(r, ui.scope_id().with(("ruler", vertical)), Sense::drag());
                if response.drag_started() {
                    let pos = response.interact_pointer_pos().unwrap();
                    let world = view.world(pos);
                    self.studio
                        .guides
                        .push((!vertical, if vertical { world.x } else { world.y }));
                    self.studio.guide_drag = Some(self.studio.guides.len() - 1);
                }
                consumed |= response.is_pointer_button_down_on() || response.hovered();
            }
        }
        if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
            if self.tool == Tool::Ruler
                && self.studio.guide_drag.is_none()
                && ctx.input(|i| i.pointer.primary_pressed())
                && area.contains(pos)
            {
                let world = view.world(pos);
                self.studio.guide_drag = self.studio.guides.iter().position(|&(h, v)| {
                    ((if h { world.y } else { world.x }) - v).abs() * view.scale < 6.
                });
            }
            if let Some(index) = self.studio.guide_drag {
                consumed = true;
                let world = view.world(pos);
                let (h, v) = &mut self.studio.guides[index];
                *v = if *h { world.y } else { world.x };
                if !ctx.input(|i| i.pointer.primary_down()) {
                    if !area.contains(pos) {
                        self.studio.guides.remove(index);
                    }
                    self.studio.guide_drag = None;
                }
            }
        }
        consumed
    }
    pub fn cancel_guide_drag(&mut self) {
        self.studio.guide_drag = None;
    }
    pub fn clear_guides(&mut self) {
        self.studio.guides.clear();
        self.studio.guide_drag = None;
    }
    pub fn paint_rulers(&self, p: &egui::Painter, area: Rect, view: View) {
        if !self.studio.rulers {
            return;
        }
        let factor = self.studio.units_per_pixel();
        for vertical in [false, true] {
            let rect = if vertical {
                Rect::from_min_max(area.min, Pos2::new(area.left() + 22., area.bottom()))
            } else {
                Rect::from_min_max(area.min, Pos2::new(area.right(), area.top() + 22.))
            };
            p.rect_filled(rect, 0., crate::themes::panel());
            let start = if vertical {
                Pos2::new(rect.right(), rect.top())
            } else {
                Pos2::new(rect.left(), rect.bottom())
            };
            let end = rect.right_bottom();
            let world_a = view.world(start);
            let world_b = view.world(end);
            let a = (if vertical { world_a.y } else { world_a.x }) * factor;
            let b = (if vertical { world_b.y } else { world_b.x }) * factor;
            let length = if vertical {
                rect.height()
            } else {
                rect.width()
            };
            let range = (b - a).abs();
            if range < 0.00001 {
                continue;
            }
            let target = range * 70. / length.max(1.);
            let power = 10f32.powf(target.log10().floor());
            let step = if target / power <= 1. {
                power
            } else if target / power <= 2. {
                2. * power
            } else if target / power <= 5. {
                5. * power
            } else {
                10. * power
            };
            let minor = step / 5.;
            let low = (a.min(b) / minor).floor() as i64;
            let high = (a.max(b) / minor).ceil() as i64;
            for i in low..=high.min(low + 2000) {
                let value = i as f32 * minor;
                let t = (value - a) / (b - a);
                if !(0. ..=1.).contains(&t) {
                    continue;
                }
                let point = start + (end - start) * t;
                let major = i % 5 == 0;
                let delta = if vertical {
                    Vec2::new(if major { -8. } else { -4. }, 0.)
                } else {
                    Vec2::new(0., if major { -8. } else { -4. })
                };
                p.line_segment(
                    [point, point + delta],
                    Stroke::new(1., crate::themes::muted()),
                );
                if major {
                    let label = if step >= 1. {
                        format!("{value:.0}")
                    } else if step >= 0.1 {
                        format!("{value:.1}")
                    } else {
                        format!("{value:.2}")
                    };
                    let ruler = p.with_clip_rect(rect);
                    let galley = ruler.layout_no_wrap(
                        label,
                        egui::FontId::monospace(8.),
                        crate::themes::muted(),
                    );
                    if vertical {
                        ruler.add(
                            egui::epaint::TextShape::new(
                                Pos2::new(rect.left() + 2., point.y - 3.),
                                galley,
                                crate::themes::muted(),
                            )
                            .with_angle(-std::f32::consts::FRAC_PI_2),
                        );
                    } else {
                        ruler.galley(
                            Pos2::new(point.x + 3., rect.top() + 2.),
                            galley,
                            crate::themes::muted(),
                        );
                    }
                }
            }
        }
        p.rect_filled(
            Rect::from_min_size(area.min, Vec2::splat(22.)),
            0.,
            crate::themes::panel(),
        );
    }
    pub fn paint_guides(&self, p: &egui::Painter, area: Rect, view: View) {
        let span = area.size().length() / view.scale;
        for &(horizontal, value) in &self.studio.guides {
            let point = if horizontal {
                Pos2::new(view.world(area.center()).x, value)
            } else {
                Pos2::new(value, view.world(area.center()).y)
            };
            let d = if horizontal {
                Vec2::new(span, 0.)
            } else {
                Vec2::new(0., span)
            };
            p.line_segment(
                [view.screen(point - d), view.screen(point + d)],
                Stroke::new(1., crate::themes::color(Color32::from_rgb(60, 205, 230))),
            );
        }
        if self.studio.horizontal || self.studio.vertical {
            let c = Pos2::new(self.doc.width as f32 / 2., self.doc.height as f32 / 2.);
            for (yes, d) in [
                (self.studio.horizontal, Vec2::new(span, 0.)),
                (self.studio.vertical, Vec2::new(0., span)),
            ] {
                if yes {
                    p.line_segment(
                        [view.screen(c - d), view.screen(c + d)],
                        Stroke::new(1., Color32::from_rgba_unmultiplied(220, 120, 240, 140)),
                    );
                }
            }
        }
    }
    pub fn stage_mask(&self, p: &egui::Painter, area: Rect, view: View) {
        if !self.studio.stage_only {
            return;
        }
        let corners = [
            Pos2::ZERO,
            Pos2::new(self.doc.width as f32, 0.),
            Pos2::new(self.doc.width as f32, self.doc.height as f32),
            Pos2::new(0., self.doc.height as f32),
        ];
        // Each exterior half-plane covers all pixels outside a rotated rectangle.
        for i in 0..4 {
            let a = view.screen(corners[i]);
            let b = view.screen(corners[(i + 1) % 4]);
            let edge = b - a;
            let distance = |p: Pos2| edge.x * (p.y - a.y) - edge.y * (p.x - a.x);
            let viewport = [
                area.left_top(),
                area.right_top(),
                area.right_bottom(),
                area.left_bottom(),
            ];
            let mut mask = vec![];
            for j in 0..4 {
                let previous = viewport[(j + 3) % 4];
                let current = viewport[j];
                let before = distance(previous);
                let after = distance(current);
                if (before <= 0.) != (after <= 0.) {
                    mask.push(previous + (current - previous) * (before / (before - after)));
                }
                if after <= 0. {
                    mask.push(current);
                }
            }
            if mask.len() < 3 {
                continue;
            }
            p.add(egui::Shape::convex_polygon(
                mask,
                crate::themes::color(Color32::from_rgb(29, 30, 34)),
                Stroke::NONE,
            ));
        }
    }
    pub fn preferences_ui(&mut self, ctx: &egui::Context) {
        if !self.studio.preferences_open {
            return;
        }
        let mut open = true;
        let mut save = false;
        egui::Window::new(tr("Preferencias del programa…"))
            .id(egui::Id::unique("preferences")).open(&mut open).default_size([730.,620.])
            .anchor(egui::Align2::CENTER_CENTER,Vec2::ZERO).show(ctx,|ui| {
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(Vec2::new(155.,560.),egui::Layout::top_down(egui::Align::Min),|ui|{
                        ui.add_space(8.);
                        for (index,label,icon) in [(0,"Interfaz",crate::design::Icon::Settings),(1,"Atajos de teclado",crate::design::Icon::Node),(2,"Opciones de guardado",crate::design::Icon::Save),(3,"Layer colors",crate::design::Icon::Layers),(4,"Performance",crate::design::Icon::Zoom),(5,"Tablet diagnostics",crate::design::Icon::Pen)] {
                            if crate::design::button(ui,icon,label,label,self.studio.preference_category==index,true).clicked(){self.studio.preference_category=index;self.studio.capture=None;}
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.set_min_width(430.);ui.add_space(8.);
                        if self.studio.preference_category==0 {
                            ui.heading(tr("Interfaz"));ui.add_space(16.);
                            egui::Grid::new("interface-options").spacing([24.,16.]).show(ui,|ui|{
                                ui.label(tr("Tema"));save|=crate::themes::selector(ui,&mut self.studio.prefs.theme);ui.end_row();
                                ui.label(tr("Idioma"));let mut locale=self.studio.prefs.locale();egui::ComboBox::from_id_salt("language").selected_text(locale.name()).show_ui(ui,|ui|{for language in crate::i18n::Language::ALL{if ui.selectable_value(&mut locale,language,language.name()).changed(){self.studio.prefs.language=Some(locale);save=true;}}});ui.end_row();

                            });
                        } else if self.studio.preference_category == 5 {
                            ui.heading(tr("Tablet diagnostics"));
                            let pen = &self.drawing.pen;
                            ui.label(tr("Press the pen on this window to test pressure and the eraser."));
                            ui.label(format!("{} · {}", tr("Input source"), pen.source().unwrap_or(tr("Waiting for pen"))));
                            let force = pen.force().unwrap_or(0.);
                            ui.add(egui::ProgressBar::new(force).text(format!("{} · {:.0}%", tr("Pressure"), force * 100.)));
                            ui.label(if pen.eraser() { tr("Eraser tip") } else { tr("Pen tip") });
                            ctx.request_repaint_after(std::time::Duration::from_millis(30));
                        } else if self.studio.preference_category == 4 {
                            ui.heading(tr("Performance"));
                            ui.add_space(12.);
                            ui.horizontal(|ui| {
                                ui.label(tr("Preview cache memory"));
                                let memory = &mut self.studio.prefs.preview_cache_mib;
                                egui::ComboBox::from_id_salt("preview-cache-memory").selected_text(format!("{memory} MiB")).show_ui(ui, |ui| {
                                    for size in [128, 256, 512, 1024] {
                                        save |= ui.selectable_value(memory, size, format!("{size} MiB")).changed();
                                    }
                                });
                            });
                            let (used, count) = self.depth.cache_usage();
                            ui.label(format!("{:.1} MiB · {} {}", used as f64 / 1048576., count, tr("Cached frames")));
                        } else if self.studio.preference_category == 2 {
                            ui.heading(tr("Opciones de guardado"));
                            ui.add_space(12.);
                            let options = &mut self.studio.prefs.saving;
                            save |= ui.checkbox(&mut options.enabled, tr("Autoguardado")).changed();
                            save |= ui.checkbox(&mut options.incremental, tr("Autoguardado incremental")).changed();
                            ui.horizontal(|ui| {
                                ui.label(tr("Intervalo de autoguardado (minutos)"));
                                save |= ui.add(egui::DragValue::new(&mut options.minutes).range(1..=1440)).changed();
                            });
                            ui.label(tr("El intervalo se aplica al modo de autoguardado seleccionado."));
                            ui.label(tr("Incremental: guarda una nueva versión numerada y conserva las anteriores."));
                        } else if self.studio.preference_category == 3 {
                            ui.heading(tr("Layer colors"));
                            ui.add_space(12.);
                            egui::Grid::new("layer-color-preferences").spacing([24., 10.]).show(ui, |ui| {
                                for (kind, label, icon, _) in LayerColor::ALL {
                                    let color = self.studio.prefs.layer_color(kind);
                                    ui.horizontal(|ui| {
                                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(18.), Sense::hover());
                                        crate::design::draw(ui.painter(), rect, icon, color);
                                        ui.label(tr(label));
                                    });
                                    ui.push_id(kind as usize, |ui| {
                                        save |= ui.color_edit_button_srgb(&mut self.studio.prefs.layer_colors[kind as usize]).changed();
                                    });
                                    ui.end_row();
                                }
                            });
                            if crate::design::button(ui, crate::design::Icon::ResetColors, "Reset layer colors", "", false, true).clicked() {
                                self.studio.prefs.layer_colors = LayerColor::ALL.map(|(_, _, _, color)| color);
                                save = true;
                            }
                        } else {
                            ui.heading(tr("Atajos de teclado"));ui.add_space(8.);
                            ui.label(tr("Pulsa un atajo y después la combinación nueva. Escape cancela; Supr lo elimina."));ui.add_space(12.);
                            ui.add(egui::TextEdit::singleline(&mut self.studio.shortcut_filter).hint_text(tr("Buscar herramienta o acción…")));
                            let filter=self.studio.shortcut_filter.to_lowercase();
                            egui::ScrollArea::vertical().max_height(340.).show(ui,|ui|{
                                egui::Grid::new("shortcut-options").spacing([24.,7.]).striped(true).show(ui,|ui|{
                                    for (index,tool) in TOOLS.iter().enumerate(){
                                        if *tool == Tool::Airbrush || !tool.name().to_lowercase().contains(&filter) { continue; }
                                        ui.horizontal(|ui|{let (r,_)=ui.allocate_exact_size(Vec2::splat(20.),Sense::hover());crate::design::draw(ui.painter(),r,tool.icon(),crate::themes::muted());ui.label(tr(tool.name()));});
                                        let binding=&self.studio.prefs.bindings[index];
                                        let label=if self.studio.capture==Some(index){"…".into()}else{let s=binding.label();if s.is_empty(){"—".into()}else{s}};
                                        if ui.add_sized([110.,24.],egui::Button::new(label)).clicked(){self.studio.capture=Some(index);}ui.end_row();
                                    }
                                    for (offset,action) in crate::shortcuts::ACTIONS.iter().enumerate() {
                                        if !tr(action.name()).to_lowercase().contains(&filter) {continue;}
                                        let index=TOOLS.len()+offset;
                                        ui.horizontal(|ui|{let (r,_)=ui.allocate_exact_size(Vec2::splat(20.),Sense::hover());crate::design::draw(ui.painter(),r,action.icon(),crate::themes::muted());ui.label(tr(action.name()));});
                                        let binding=&self.studio.prefs.actions[offset];
                                        let label=if self.studio.capture==Some(index){"…".into()}else{let s=binding.label();if s.is_empty(){"—".into()}else{s}};
                                        if ui.add_sized([110.,24.],egui::Button::new(label)).clicked(){self.studio.capture=Some(index);}ui.end_row();
                                    }

                                });
                            });
                            ui.add_space(12.);
                            if ui.button(tr("Restablecer atajos")).clicked(){self.studio.prefs.bindings=Preferences::default().bindings;self.studio.prefs.actions=crate::shortcuts::defaults();self.studio.capture=None;save=true;}
                        }
                    });
                });
            });
        if self.studio.preference_category == 1 {
            if let Some(index) = self.studio.capture {
                let event = ctx.input(|i| {
                    i.events.iter().find_map(|e| {
                        if let egui::Event::Key {
                            key,
                            pressed: true,
                            repeat: false,
                            modifiers,
                            ..
                        } = e
                        {
                            Some((*key, *modifiers))
                        } else {
                            None
                        }
                    })
                });
                if let Some((key, m)) = event {
                    if key != egui::Key::Escape {
                        let binding = if key == egui::Key::Delete {
                            Binding::new("")
                        } else {
                            Binding {
                                key: key.name().into(),
                                ctrl: m.ctrl || m.command,
                                shift: m.shift,
                                alt: m.alt,
                            }
                        };
                        if !binding.key.is_empty() && reserved(&binding) {
                            self.status = tr("Atajo reservado para una acción del programa").into();
                        } else {
                            for (j, b) in self
                                .studio
                                .prefs
                                .bindings
                                .iter_mut()
                                .chain(self.studio.prefs.actions.iter_mut())
                                .enumerate()
                            {
                                if j != index && *b == binding {
                                    b.key.clear();
                                }
                            }
                            if index < TOOLS.len() {
                                self.studio.prefs.bindings[index] = binding;
                            } else {
                                self.studio.prefs.actions[index - TOOLS.len()] = binding;
                            }
                            save = true;
                        }
                    }
                    self.studio.capture = None;
                }
            }
        }
        if save {
            if let Err(e) = self.studio.save() {
                self.status = e;
            }
            crate::i18n::set_locale(self.studio.prefs.locale());
            crate::themes::ensure(ctx, self.studio.prefs.theme);
            crate::i18n::ensure_fonts(ctx, self.studio.prefs.locale());
        }
        self.studio.preferences_open = open;
    }
    pub fn notice_ui(&mut self, ctx: &egui::Context) {
        self.import_notice_ui(ctx);
        if self.studio.last_status.is_empty() {
            self.studio.last_status = self.status.clone();
            return;
        }
        if self.status != self.studio.last_status {
            self.studio.last_status = self.status.clone();
            self.studio.notice = self.status.clone();
            self.studio.notice_time = Some(Instant::now());
        }
        if self
            .studio
            .notice_time
            .is_some_and(|t| t.elapsed() < Duration::from_secs(6))
        {
            egui::Area::new(egui::Id::unique("notice"))
                .anchor(egui::Align2::CENTER_BOTTOM, [0., -14.])
                .interactable(false)
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.label(tr(&self.studio.notice));
                    });
                });
            ctx.request_repaint_after(Duration::from_secs(6));
        }
    }
}
fn reserved(b: &Binding) -> bool {
    b.key == "Space"
        || b.key == "Escape"
        || b.key == "Delete"
        || (!b.ctrl && !b.shift && !b.alt && ["F8", "Comma", "Period"].contains(&b.key.as_str()))
        || (b.ctrl && ["A", "C", "X", "V", "S", "D"].contains(&b.key.as_str()))
        || (!b.ctrl && !b.alt && !b.shift && ["X", "D"].contains(&b.key.as_str()))
}
pub fn tone(mut c: [u8; 4], strength: f32, light: bool, mode: usize) -> [u8; 4] {
    for channel in &mut c[..3] {
        let v = *channel as f32 / 255.;
        let s = strength.clamp(0., 1.);
        let out = match mode {
            1 => v * 2f32.powf(if light { s } else { -s }),
            2 => {
                if light {
                    1. - (1. - v).powf(1. + s * 2.)
                } else {
                    v.powf(1. + s * 2.)
                }
            }
            _ => {
                if light {
                    v + (1. - v) * s
                } else {
                    v * (1. - s)
                }
            }
        };
        *channel = (out.clamp(0., 1.) * 255.).round() as u8;
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    fn editor() -> Editor {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx);
        let mut e = Editor::new(&cc);
        e.doc = crate::Document::default();
        e.recovery = std::env::temp_dir().join(format!(
            "libretoon-studio-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ));
        e
    }
    #[test]
    fn symmetry_preserves_nodes_and_width_profile() {
        let mut e = editor();
        e.studio.horizontal = true;
        e.studio.vertical = true;
        let m = Mark {
            points: vec![[10., 20.], [30., 40.]].into(),
            nodes: vec![crate::document::Node::corner([10., 20.])].into(),
            width_profile: vec![crate::width::WidthStop { t: 0., width: 0.2 }].into(),
            ..Default::default()
        };
        let marks = e.symmetry_marks(vec![m]);
        assert_eq!(marks.len(), 4);
        assert_eq!(
            marks[3].nodes[0].position,
            [e.doc.width as f32 - 10., e.doc.height as f32 - 20.]
        );
        assert_eq!(marks[3].width_profile[0].width, 0.2);
    }
    #[test]
    fn clipboard_and_mirror_are_undoable() {
        let mut e = editor();
        e.doc.layers[0].frames[0].push(Mark {
            points: vec![[10., 20.], [40., 60.]].into(),
            ..Default::default()
        });
        e.canvas.selected = vec![(0, 0)];
        e.copy_selection(true);
        assert!(e.doc.layers[0].frames[0].is_empty());
        e.paste_selection();
        assert_eq!(e.doc.layers[0].frames[0].len(), 1);
        e.mirror_selection(true);
        assert_eq!(e.doc.layers[0].frames[0][0].points[0], [40., 20.]);
        e.history(false);
        assert_eq!(e.doc.layers[0].frames[0][0].points[0], [10., 20.]);
    }
    #[test]
    fn tone_preserves_alpha_and_direction() {
        for mode in 0..3 {
            let c = [90, 120, 180, 123];
            let light = tone(c, 0.3, true, mode);
            let dark = tone(c, 0.3, false, mode);
            for i in 0..3 {
                assert!(light[i] > c[i]);
                assert!(dark[i] < c[i]);
            }
            assert_eq!(light[3], 123);
            assert_eq!(dark[3], 123);
        }
    }
    #[test]
    fn shading_clips_to_fills_preserves_contours_and_undo() {
        use geo::Area;
        let mut e = editor();
        let shape = Mark {
            points: vec![[0., 0.], [100., 100.]].into(),
            rectangle: true,
            closed: true,
            fill: Some([180, 100, 60, 200]),
            stroke: true,
            color: [10, 20, 30, 255],
            ..Default::default()
        };
        let nodes = shape.points.clone();
        e.doc.layers[0].frames[0].push(shape);
        let brush = Mark {
            points: vec![[50., -20.], [150., 120.]].into(),
            rectangle: true,
            closed: true,
            fill: Some([0, 0, 0, 255]),
            stroke: false,
            ..Default::default()
        };
        e.checkpoint();
        e.tone_paint(vec![brush], false);
        let marks = &e.doc.layers[0].frames[0];
        let contour = marks.iter().find(|m| m.stroke).unwrap();
        assert_eq!(contour.points.as_slice(), nodes.as_slice());
        assert_eq!(contour.color, [10, 20, 30, 255]);
        let area: f64 = marks
            .iter()
            .filter(|m| m.fill.is_some())
            .map(|m| crate::vector::polygon(m).unsigned_area())
            .sum();
        assert!((area - 10000.).abs() < 0.1);
        assert!(
            marks
                .iter()
                .filter_map(|m| m.fill)
                .any(|c| c[0] < 180 && c[3] == 200)
        );
        assert!(
            marks
                .iter()
                .all(|m| crate::vector::control_bounds(m).right() <= 100.01)
        );
        let restored = crate::fla::decode(&crate::fla::encode(&e.doc).unwrap()).unwrap();
        assert!(restored.validate().is_ok());
        e.history(false);
        assert_eq!(e.doc.layers[0].frames[0].len(), 1);
        assert_eq!(e.doc.layers[0].frames[0][0].fill, Some([180, 100, 60, 200]));
    }
    #[test]
    fn guides_drag_from_rulers_and_can_be_removed_without_drawing() {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut e = Editor::new(&cc);
        e.doc = crate::Document::default();
        e.activate_tool(Tool::Ruler);
        let tick = |e: &mut Editor, events: Vec<egui::Event>| {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000., 700.))),
                    events,
                    ..Default::default()
                },
                |ui| e.canvas_ui(ui),
            );
            out.textures_delta.clear();
        };
        let button = |p: Pos2, pressed| egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        tick(&mut e, vec![]);
        tick(
            &mut e,
            vec![
                egui::Event::PointerMoved(Pos2::new(300., 15.)),
                button(Pos2::new(300., 15.), true),
            ],
        );
        tick(
            &mut e,
            vec![egui::Event::PointerMoved(Pos2::new(300., 200.))],
        );
        tick(&mut e, vec![button(Pos2::new(300., 200.), false)]);
        assert_eq!(e.studio.guides.len(), 1);
        assert!(e.studio.guides[0].0);
        assert!(e.doc.layers[0].frames[0].is_empty());
        tick(&mut e, vec![button(Pos2::new(300., 200.), true)]);
        tick(
            &mut e,
            vec![egui::Event::PointerMoved(Pos2::new(300., -20.))],
        );
        tick(&mut e, vec![button(Pos2::new(300., -20.), false)]);
        assert!(e.studio.guides.is_empty());
        assert!(e.undo.is_empty());
    }
    #[test]
    fn stage_mask_respects_rotation_and_does_not_change_the_document() {
        let mut e = editor();
        e.studio.stage_only = true;
        let ctx = egui::Context::default();
        let area = Rect::from_min_size(Pos2::ZERO, Vec2::splat(800.));
        for angle in [0., 0.4, 1.5] {
            let view = View {
                center: Pos2::new(400., 400.),
                stage_center: Pos2::new(e.doc.width as f32 / 2., e.doc.height as f32 / 2.),
                scale: 0.5,
                angle,
            };
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(area),
                    ..Default::default()
                },
                |ui| e.stage_mask(ui.painter(), area, view),
            );
            out.textures_delta.clear();
            let meshes = ctx.tessellate(out.shapes, out.pixels_per_point);
            let contains = |p: Pos2| {
                meshes.iter().any(|primitive| {
                    let egui::epaint::Primitive::Mesh(mesh) = &primitive.primitive else {
                        return false;
                    };
                    mesh.indices.chunks_exact(3).any(|t| {
                        let a = mesh.vertices[t[0] as usize].pos;
                        let b = mesh.vertices[t[1] as usize].pos;
                        let c = mesh.vertices[t[2] as usize].pos;
                        let cross = |a: Vec2, b: Vec2| a.x * b.y - a.y * b.x;
                        let signs = [
                            cross(b - a, p - a),
                            cross(c - b, p - b),
                            cross(a - c, p - c),
                        ];
                        signs.iter().all(|v| *v >= 0.) || signs.iter().all(|v| *v <= 0.)
                    })
                })
            };
            assert!(!contains(view.center));
            assert!(contains(view.screen(Pos2::new(-100., 100.))));
            assert!(contains(view.screen(Pos2::new(-100., -100.))));
        }
        assert!(e.doc.layers[0].frames[0].is_empty());
        assert!(e.undo.is_empty());
    }
    #[test]
    fn custom_tool_shortcuts_keep_modifiers_and_defaults_are_english() {
        assert!(!Preferences::default().spanish);
        let mut e = editor();
        e.studio.prefs.bindings[25] = Binding {
            key: "G".into(),
            ctrl: false,
            shift: true,
            alt: true,
        };
        let serialized = serde_json::to_vec(&e.studio.prefs).unwrap();
        let restored: Preferences = serde_json::from_slice(&serialized).unwrap();
        assert_eq!(restored.bindings[25].label(), "Shift+Alt+G");
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::G,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers {
                        shift: true,
                        alt: true,
                        ..Default::default()
                    },
                }],
                ..Default::default()
            },
            |_| e.tool_shortcuts(&ctx),
        );
        out.textures_delta.clear();
        assert!(e.tool == Tool::Ruler);
    }
}

#[cfg(test)]
mod chrome_tests {
    use super::*;
    #[test]
    fn custom_caption_buttons_send_native_window_commands() {
        for (index, x) in [370., 320., 270.].into_iter().enumerate() {
            let ctx = egui::Context::default();
            let cc = eframe::CreationContext::_new_kittest(ctx.clone());
            let mut e = Editor::new(&cc);
            let mut commands = vec![];
            for phase in 0..3 {
                let p = Pos2::new(x, 20.);
                let mut out = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400., 100.))),
                        events: if phase == 0 {
                            vec![]
                        } else {
                            vec![
                                egui::Event::PointerMoved(p),
                                egui::Event::PointerButton {
                                    pos: p,
                                    button: egui::PointerButton::Primary,
                                    pressed: phase == 1,
                                    modifiers: Default::default(),
                                },
                            ]
                        },
                        ..Default::default()
                    },
                    |ui| {
                        ui.allocate_ui_with_layout(
                            Vec2::new(ui.available_width(), 32.),
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| e.window_controls(ui),
                        );
                    },
                );
                out.textures_delta.clear();
                for viewport in out.viewport_output.values() {
                    commands.extend(viewport.commands.clone());
                }
            }
            assert!(
                commands.iter().any(|command| match index {
                    0 => matches!(command, egui::ViewportCommand::Close),
                    1 => matches!(command, egui::ViewportCommand::Maximized(true)),
                    _ => matches!(command, egui::ViewportCommand::Minimized(true)),
                }),
                "button {index}: {commands:?}"
            );
        }
    }
}
