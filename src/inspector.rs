use crate::{
    Editor,
    canvas::Tool,
    design::{self, Icon},
    shared::SharedVec,
    width::{self, WidthStop},
};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};

pub struct DocumentSettings {
    pub creating: bool,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}
impl Editor {
    pub fn open_document_settings(&mut self) {
        self.document_settings = Some(DocumentSettings {
            creating: false,
            width: self.doc.width,
            height: self.doc.height,
            fps: self.doc.fps,
        });
    }
    pub fn apply_document_settings(&mut self, width: u32, height: u32, fps: u32) {
        if !(1..=16384).contains(&width)
            || !(1..=16384).contains(&height)
            || !(1..=240).contains(&fps)
        {
            return;
        }
        if (width, height, fps) == (self.doc.width, self.doc.height, self.doc.fps) {
            return;
        }
        self.finish_pending_pen();
        self.checkpoint();
        self.doc.width = width;
        self.doc.height = height;
        self.doc.fps = fps;
        self.canvas.fit();
        self.playing = false;
        self.tick = std::time::Instant::now();
        self.recover();
    }
    pub fn set_fps(&mut self, fps: u32) {
        if fps == self.doc.fps || !(1..=240).contains(&fps) {
            return;
        }
        self.checkpoint();
        self.doc.fps = fps;
        self.tick = std::time::Instant::now();
        self.recover();
    }
    pub fn set_duration(&mut self, seconds: f32) {
        if !seconds.is_finite() {
            return;
        }
        let count = ((seconds * self.doc.fps as f32).round() as usize).clamp(1, 10000);
        if count == self.doc.layers[0].frames.len() {
            return;
        }
        self.finish_pending_pen();
        self.checkpoint();
        for layer in &mut self.doc.layers {
            layer.init_keys();
            let symbol_context = !self.feature.symbol_stack.is_empty();
            let held = if symbol_context {
                layer.frames.last().cloned().unwrap_or_default()
            } else {
                Default::default()
            };
            layer.frames.resize(count, held);
            layer.options.keys.resize(count, !symbol_context);
            layer.options.tweens.retain(|t| t.start < count - 1);
            for t in &mut layer.options.tweens {
                t.end = t.end.min(count - 1);
                layer.options.keys[t.end] = true;
            }
        }
        self.doc.extras.rig.trim_keys(count);
        self.fit_camera_duration();
        self.frame = self.frame.min(count - 1);
        self.playing = false;
        self.recover();
    }
    pub fn document_settings_ui(&mut self, ctx: &egui::Context) {
        let Some(mut settings) = self.document_settings.take() else {
            return;
        };
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new(crate::i18n::tr(if settings.creating {
            "Nuevo proyecto"
        } else {
            "Ajustes de documento"
        }))
        .id(egui::Id::unique("document-settings"))
        .open(&mut open)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .resizable(false)
        .show(ctx, |ui| {
            let presets = [
                ("HD · 1280 × 720", 1280, 720),
                ("Full HD · 1920 × 1080", 1920, 1080),
                ("4K UHD · 3840 × 2160", 3840, 2160),
                ("4K DCI · 4096 × 2160", 4096, 2160),
                ("Vertical · 1080 × 1920", 1080, 1920),
                ("Cuadrado · 1080 × 1080", 1080, 1080),
            ];
            let label = presets
                .iter()
                .find(|(_, w, h)| *w == settings.width && *h == settings.height)
                .map_or("Personalizada", |(label, _, _)| *label);
            egui::ComboBox::from_id_salt("resolution-preset")
                .selected_text(label)
                .width(250.)
                .show_ui(ui, |ui| {
                    for (label, w, h) in presets {
                        if ui
                            .selectable_label(settings.width == w && settings.height == h, label)
                            .clicked()
                        {
                            settings.width = w;
                            settings.height = h;
                        }
                    }
                });
            ui.add_space(10.);
            egui::Grid::new("document-size")
                .spacing([18., 10.])
                .show(ui, |ui| {
                    ui.label(crate::i18n::tr("Ancho"));
                    ui.add(
                        egui::DragValue::new(&mut settings.width)
                            .range(1..=16384)
                            .suffix(crate::i18n::tr(" px"))
                            .update_while_editing(false),
                    );
                    ui.end_row();
                    ui.label(crate::i18n::tr("Alto"));
                    ui.add(
                        egui::DragValue::new(&mut settings.height)
                            .range(1..=16384)
                            .suffix(crate::i18n::tr(" px"))
                            .update_while_editing(false),
                    );
                    ui.end_row();
                    ui.label(crate::i18n::tr("Velocidad"));
                    ui.add(
                        egui::DragValue::new(&mut settings.fps)
                            .range(1..=240)
                            .suffix(crate::i18n::tr(" fps"))
                            .update_while_editing(false),
                    );
                    ui.end_row();
                });
            ui.add_space(12.);
            ui.horizontal(|ui| {
                if ui
                    .button(crate::i18n::tr(if settings.creating {
                        "Crear"
                    } else {
                        "Aplicar"
                    }))
                    .clicked()
                {
                    apply = true;
                }
                if ui.button(crate::i18n::tr("Cancelar")).clicked() {
                    cancel = true;
                }
            });
        });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            cancel = true;
            apply = false;
        }
        if apply {
            if settings.creating {
                let mut doc = crate::fresh_document();
                doc.width = settings.width;
                doc.height = settings.height;
                doc.fps = settings.fps;
                self.add_document(doc, None);
            } else {
                self.apply_document_settings(settings.width, settings.height, settings.fps);
            }
        } else if open && !cancel {
            self.document_settings = Some(settings);
        }
    }
    pub fn swap_colors(&mut self) {
        std::mem::swap(&mut self.color, &mut self.fill_color);
        std::mem::swap(&mut self.stroke_enabled, &mut self.fill_enabled);
    }
    pub fn color_swatches(&mut self, ui: &mut egui::Ui) {
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.studio.color_picker = None;
        }
        let (r, _) = ui.allocate_exact_size(Vec2::new(65., 58.), Sense::hover());
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(Rect::from_min_size(
                r.min + Vec2::new(24., 20.),
                Vec2::splat(30.),
            )),
            |ui| {
                ui.spacing_mut().interact_size = Vec2::splat(32.);
                let response = self.paint_color_picker(ui, false);
                ui.painter()
                    .rect_filled(response.rect.shrink(5.), 0., crate::themes::panel());
            },
        );
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(Rect::from_min_size(r.min, Vec2::splat(30.))),
            |ui| {
                ui.spacing_mut().interact_size = Vec2::splat(32.);
                self.paint_color_picker(ui, true);
            },
        );
        ui.advance_cursor_after_rect(r);
        ui.add_space(4.);
        ui.horizontal(|ui| {
            if design::button(
                ui,
                Icon::SwapColors,
                "",
                "Intercambiar colores · X",
                false,
                true,
            )
            .clicked()
            {
                self.swap_colors();
                self.change_selected_style(true, true, false, false);
            }
            if design::button(ui, Icon::ResetColors, "", "Blanco y negro · D", false, true)
                .clicked()
            {
                self.color = Color32::BLACK;
                self.fill_color = Color32::WHITE;
                self.stroke_enabled = true;
                self.fill_enabled = true;
                self.change_selected_style(true, true, false, false);
            }
        });
    }
    fn paint_color_picker(&mut self, ui: &mut egui::Ui, fill: bool) -> egui::Response {
        let color = if fill { self.fill_color } else { self.color };
        let response = ui
            .add_sized(
                ui.spacing().interact_size,
                egui::Button::new("").fill(color),
            )
            .on_hover_text(crate::i18n::tr(if fill { "Relleno" } else { "Contorno" }));
        if response.clicked() || response.double_clicked() {
            self.studio.color_picker = None;
            self.active_fill = fill;
            self.studio.palette.clear_linked_selection();
        }
        if response.double_clicked() {
            self.studio.palette.open = true;
        }
        response
    }

    pub(crate) fn apply_picker_color(&mut self, color: Color32, fill: bool) {
        if fill {
            self.fill_color = color;
            self.fill_enabled = true;
            self.feature.gradient_on = false;
        } else {
            self.color = color;
            self.stroke_enabled = true;
        }
        self.active_fill = fill;
    }

    pub fn properties_ui(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(10.);
            if self.doc.layers[self.layer].options.light.is_some() {
                self.light_properties(ui);
                return;
            }
            if !self.playing
                && !self.doc.layers[self.layer].options.locked
                && self
                    .feature
                    .frame_selection
                    .is_some_and(|(layer, _, frame)| (layer, frame) == (self.layer, self.frame))
                && self.doc.layers[self.layer].frames[self.frame]
                    .iter()
                    .any(|mark| crate::lip::has_mouth_key(mark, self.frame))
            {
                design::category(ui, "LIPSYNC");
                self.mouth_timeline_menu(ui, self.layer, self.frame, true);
            }
            self.lip_pose_properties(ui);
            self.height_guide_properties(ui);
            if self.doc.layers[self.layer].options.light_mask {
                design::category(ui, "Light mask");
                ui.small(crate::i18n::tr(
                    "Click to paint. Ctrl or Cmd + click to remove.",
                ));
            }
            if self.adjustment_active() {
                self.adjustment_properties(ui);
                return;
            }
            design::category(ui, "HERRAMIENTA");
            if self.raster_active() && Self::raster_tool(self.tool) {
                self.raster_settings(ui);
                return;
            }
            if matches!(self.tool, Tool::Bones | Tool::Puppet) {
                self.rig_properties(ui);
                return;
            }
            if self.tool == Tool::Text {
                self.text_properties(ui);
                return;
            }
            if self.tool == Tool::Mannequin {
                self.mannequin_properties(ui);
                return;
            }
            self.drawing_options(ui);
            self.tool_options_ui(ui);
            self.order_properties_ui(ui);
            self.advanced_properties(ui);
            if self.canvas.selected.first().is_some_and(|&(l, m)| {
                self.doc.layers[l].frames[self.frame][m]
                    .style
                    .instance
                    .is_some()
            }) {
                return;
            }
            if !self.canvas.selected.is_empty()
                && matches!(
                    self.tool,
                    Tool::Select
                        | Tool::Transform
                        | Tool::Object
                        | Tool::Lasso
                        | Tool::PolygonSelect
                        | Tool::Wand
                        | Tool::Width
                        | Tool::Node
                )
            {
                self.selection_properties_ui(ui);
                return;
            }
            ui.horizontal(|ui| {
                design::draw(
                    ui.painter(),
                    Rect::from_min_size(ui.cursor().min, Vec2::splat(20.)),
                    self.tool.icon(),
                    crate::themes::accent(),
                );
                ui.add_space(27.);
                ui.heading(self.tool.name());
            });
            ui.add_space(14.);
            if self.tool == Tool::Width {
                if let Some(&(l, m)) = self.canvas.selected.first() {
                    let mark = &self.doc.layers[l].frames[self.frame][m];
                    if mark.stroke {
                        if self.width_profile != mark.width_profile {
                            self.width_profile = mark.width_profile.clone();
                            self.width_preset = width::Preset::Custom;
                        }
                        self.width = mark.width;
                    }
                }
            }
            if matches!(
                self.tool,
                Tool::Rectangle | Tool::Pen | Tool::Ellipse | Tool::Polygon
            ) {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.fill_enabled, crate::i18n::tr("Relleno"));
                    if ui.color_edit_button_srgba(&mut self.fill_color).clicked() {
                        self.fill_color = self.fill_color.to_opaque();
                    }
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.stroke_enabled, crate::i18n::tr("Contorno"));
                    if ui.color_edit_button_srgba(&mut self.color).clicked() {
                        self.color = self.color.to_opaque();
                    }
                });
                ui.add_space(8.);
            }
            if matches!(
                self.tool,
                Tool::Brush
                    | Tool::Airbrush
                    | Tool::SketchBrush
                    | Tool::FillBrush
                    | Tool::ShadeBrush
                    | Tool::LightBrush
                    | Tool::VectorBrush
                    | Tool::Rectangle
                    | Tool::Pen
                    | Tool::Width
                    | Tool::Line
                    | Tool::Ellipse
                    | Tool::Polygon
                    | Tool::Eraser
            ) {
                design::category(ui, "TRAZO");
                ui.label(crate::i18n::tr("Grosor"));
                let size = if self.tool == Tool::Eraser {
                    &mut self.eraser_width
                } else {
                    &mut self.width
                };
                let response =
                    ui.add(egui::Slider::new(size, 1.0..=80.).suffix(crate::i18n::tr(" px")));
                if response.changed() && self.tool == Tool::Width {
                    let ids = self.canvas.selected.clone();
                    self.checkpoint();
                    for &(l, m) in &ids {
                        self.doc.layers[l].frames[self.frame][m].width = self.width;
                    }
                    self.canvas.selected = ids;
                    self.recover();
                }
            }
            if matches!(
                self.tool,
                Tool::Brush
                    | Tool::Airbrush
                    | Tool::SketchBrush
                    | Tool::FillBrush
                    | Tool::ShadeBrush
                    | Tool::LightBrush
                    | Tool::VectorBrush
            ) {
                ui.add_space(10.);
                design::category(ui, "SUAVIZADO");
                ui.label(crate::i18n::tr("Suavizado"));
                ui.add(
                    egui::Slider::new(&mut self.smoothing, 0.0..=1.)
                        .custom_formatter(|v, _| format!("{:.0}%", v * 100.)),
                );
            }
            if self.tool == Tool::Pen {
                design::category(ui, "Pen mode");
                if design::button(
                    ui,
                    Icon::Node,
                    "Edit nodes only",
                    "Edit existing curves without adding a path",
                    self.feature.pen_edit_only,
                    true,
                )
                .clicked()
                {
                    self.finish_pending_pen();
                    self.feature.pen_edit_only = !self.feature.pen_edit_only;
                }
            }
            if self.tool == Tool::Bucket {
                design::category(ui, "FILL METHOD");
                for (gap, icon) in crate::bucket::Gap::ALL.into_iter().zip([
                    Icon::Line,
                    Icon::CapRound,
                    Icon::Width,
                    Icon::Transform,
                ]) {
                    if design::button(ui, icon, gap.label(), "", self.gap == gap, true).clicked() {
                        self.gap = gap;
                    }
                }
                ui.small(crate::i18n::tr(
                    "Shift + click to recalculate an existing fill",
                ));
                design::category(ui, "MULTI-FRAME FILL");
                let has_selection = self
                    .feature
                    .frame_selection
                    .is_some_and(|(layer, _, _)| layer == self.layer);
                if design::button(
                    ui,
                    Icon::Film,
                    "Fill selected keyframes",
                    "Pick a point to preview fills across the selected range",
                    self.feature.bucket_range_armed,
                    has_selection && self.feature.bucket_range.is_none(),
                )
                .clicked()
                {
                    self.feature.bucket_range_armed = !self.feature.bucket_range_armed;
                }
                if !has_selection {
                    ui.small(crate::i18n::tr(
                        "Shift-click timeline cells to select a range",
                    ));
                }
                if ui
                    .checkbox(
                        &mut self.feature.bucket_next,
                        crate::i18n::tr("Fill next keyframes"),
                    )
                    .changed()
                {
                    self.feature.bucket_range_armed = false;
                }
                if self.feature.bucket_next {
                    ui.horizontal(|ui| {
                        ui.label(crate::i18n::tr("Additional keyframes"));
                        ui.add(
                            egui::DragValue::new(&mut self.feature.bucket_next_count).range(1..=32),
                        );
                    });
                }
            }
            if matches!(
                self.tool,
                Tool::VectorBrush
                    | Tool::Pen
                    | Tool::Width
                    | Tool::Line
                    | Tool::Ellipse
                    | Tool::Polygon
            ) {
                ui.add_space(12.);
                self.width_profile_ui(ui);
            }
            if !self.canvas.selected.is_empty() && self.tool != Tool::Bucket {
                ui.add_space(12.);
                if ui
                    .add_enabled(
                        self.fill_enabled || self.stroke_enabled,
                        egui::Button::new(crate::i18n::tr("Aplicar estilo a la selección")),
                    )
                    .clicked()
                {
                    self.apply_drawing_style();
                }
            }
        });
    }
    pub(crate) fn width_profile_ui(&mut self, ui: &mut egui::Ui) {
        design::category(ui, "PERFIL DEL CONTORNO");
        let previous = self.width_preset;
        let mut changed = false;
        let mut dragging = false;
        egui::ComboBox::from_id_salt("width-preset")
            .selected_text(self.width_preset.label())
            .show_ui(ui, |ui| {
                for preset in width::Preset::ALL {
                    ui.selectable_value(&mut self.width_preset, preset, preset.label());
                }
            });
        if previous != self.width_preset {
            self.width_profile = self.width_preset.stops();
            self.profile_point = None;
            changed = true;
        }
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), 112.),
            Sense::click_and_drag(),
        );
        let graph = rect.shrink2(Vec2::new(8., 10.));
        let painter = ui.painter();
        painter.rect_filled(
            rect,
            3.,
            crate::themes::color(Color32::from_rgb(25, 26, 30)),
        );
        for i in 0..=4 {
            let y = graph.bottom() - graph.height() * i as f32 / 4.;
            painter.line_segment(
                [Pos2::new(graph.left(), y), Pos2::new(graph.right(), y)],
                Stroke::new(0.5, crate::themes::color(Color32::from_gray(60))),
            );
        }
        let screen = |stop: WidthStop| {
            Pos2::new(
                graph.left() + stop.t * graph.width(),
                graph.bottom() - stop.width * graph.height() / 4.,
            )
        };
        let points: Vec<_> = (0..=96)
            .map(|i| {
                let t = i as f32 / 96.;
                screen(WidthStop {
                    t,
                    width: width::sample(&self.width_profile, t),
                })
            })
            .collect();
        painter.add(egui::Shape::line(
            points,
            Stroke::new(2., crate::themes::accent()),
        ));
        for (i, stop) in self.width_profile.iter().enumerate() {
            painter.circle_filled(
                screen(*stop),
                if self.profile_point == Some(i) {
                    4.5
                } else {
                    3.5
                },
                if self.profile_point == Some(i) {
                    crate::themes::color(Color32::WHITE)
                } else {
                    crate::themes::accent()
                },
            );
        }
        if response.drag_started() || response.clicked() {
            let point = ui
                .input(|i| i.pointer.press_origin())
                .or(response.interact_pointer_pos());
            if let Some(point) = point {
                let closest = self
                    .width_profile
                    .iter()
                    .enumerate()
                    .find(|(_, s)| screen(**s).distance(point) < 10.)
                    .map(|(i, _)| i);
                if let Some(index) = closest {
                    self.profile_point = Some(index);
                } else {
                    let t = ((point.x - graph.left()) / graph.width()).clamp(0., 1.);
                    let w = ((graph.bottom() - point.y) / graph.height() * 4.).clamp(0., 4.);
                    self.profile_point = Some(width::edit_stop(&mut self.width_profile, t, w));
                    changed = true;
                    self.width_preset = width::Preset::Custom;
                }
            }
        }
        if response.dragged() {
            dragging = true;
            if let (Some(index), Some(point)) =
                (self.profile_point, response.interact_pointer_pos())
            {
                let n = self.width_profile.len();
                if index < n {
                    let t = ((point.x - graph.left()) / graph.width()).clamp(0., 1.);
                    let value = ((graph.bottom() - point.y) / graph.height() * 4.).clamp(0., 4.);
                    let min = if index > 0 {
                        self.width_profile[index - 1].t + 0.001
                    } else {
                        0.
                    };
                    let max = if index + 1 < n {
                        self.width_profile[index + 1].t - 0.001
                    } else {
                        1.
                    };
                    self.width_profile[index].t = if index == 0 {
                        0.
                    } else if index + 1 == n {
                        1.
                    } else {
                        t.clamp(min, max)
                    };
                    self.width_profile[index].width = value;
                    changed = true;
                    self.width_preset = width::Preset::Custom;
                }
            }
        }
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(crate::i18n::tr("Inicio"))
                    .size(10.)
                    .color(crate::themes::muted()),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(crate::i18n::tr("Final"))
                        .size(10.)
                        .color(crate::themes::muted()),
                );
            });
        });
        if let Some(index) = self.profile_point.filter(|i| *i < self.width_profile.len()) {
            let mut value = self.width_profile[index].width * 100.;
            let r = ui.add(
                egui::DragValue::new(&mut value)
                    .range(0.0..=400.)
                    .suffix(crate::i18n::tr(" %"))
                    .prefix(crate::i18n::tr("Grosor: "))
                    .update_while_editing(false),
            );
            dragging |= r.dragged();
            if r.changed() {
                self.width_profile[index].width = value / 100.;
                changed = true;
                self.width_preset = width::Preset::Custom;
            }
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.width_profile.len() < 128,
                    egui::Button::new(crate::i18n::tr("+ Punto")),
                )
                .clicked()
            {
                if self.width_profile.is_empty() {
                    self.width_profile = width::Preset::Custom.stops();
                }
                let pair = self
                    .width_profile
                    .windows(2)
                    .max_by(|a, b| (a[1].t - a[0].t).total_cmp(&(b[1].t - b[0].t)))
                    .unwrap();
                let t = (pair[0].t + pair[1].t) * 0.5;
                let value = width::sample(&self.width_profile, t);
                self.profile_point = Some(width::edit_stop(&mut self.width_profile, t, value));
                changed = true;
                self.width_preset = width::Preset::Custom;
            }
            let removable = self
                .profile_point
                .is_some_and(|i| i > 0 && i + 1 < self.width_profile.len());
            if ui
                .add_enabled(removable, egui::Button::new(crate::i18n::tr("− Punto")))
                .clicked()
            {
                self.width_profile
                    .remove(self.profile_point.take().unwrap());
                changed = true;
                self.width_preset = width::Preset::Custom;
            }
        });
        if self.tool == Tool::Width || !self.canvas.selected.is_empty() {
            let ids = self.canvas.selected.clone();
            if changed && !ids.is_empty() {
                if !self.profile_transaction {
                    self.checkpoint();
                    self.profile_transaction = true;
                }
                for &(l, m) in &ids {
                    if self.doc.layers[l].frames[self.frame][m].stroke {
                        self.doc.layers[l].frames[self.frame][m].width_profile =
                            self.width_profile.clone();
                    }
                }
                self.canvas.selected = ids;
            }
            if self.profile_transaction && !dragging {
                self.profile_transaction = false;
                self.recover();
            }
        } else if ui
            .add_enabled(
                !self.canvas.selected.is_empty(),
                egui::Button::new(crate::i18n::tr("Aplicar perfil a la selección")),
            )
            .clicked()
        {
            self.apply_width_profile(self.width_profile.clone());
        }
    }
    pub fn apply_width_profile(&mut self, profile: SharedVec<WidthStop>) {
        let ids = self.canvas.selected.clone();
        if ids.is_empty() {
            return;
        }
        self.checkpoint();
        for &(l, m) in &ids {
            if self.doc.layers[l].frames[self.frame][m].stroke {
                self.doc.layers[l].frames[self.frame][m].width_profile = profile.clone();
            }
        }
        self.canvas.selected = ids;
        self.recover();
    }
}

#[cfg(test)]
mod ux_tests {
    use super::*;
    fn run(test: impl FnOnce(&mut Editor)) {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx);
        let mut editor = Editor::new(&cc);
        editor.doc = crate::Document::default();
        let path = std::env::temp_dir().join(format!(
            "libretoon-ux-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ));
        editor.recovery = path.clone();
        test(&mut editor);
        drop(editor);
        let _ = std::fs::remove_file(path);
    }
    #[test]
    fn document_settings_preserve_art_and_undo_dimensions_and_fps() {
        run(|e| {
            e.doc.layers[0].frames[0].push(crate::document::Mark {
                points: vec![[22., 33.]].into(),
                ..Default::default()
            });
            e.apply_document_settings(3840, 2160, 120);
            assert_eq!((e.doc.width, e.doc.height, e.doc.fps), (3840, 2160, 120));
            assert_eq!(e.doc.layers[0].frames[0][0].points[0], [22., 33.]);
            e.history(false);
            assert_eq!((e.doc.width, e.doc.height, e.doc.fps), (960, 540, 12));
            assert!(e.doc.validate().is_ok());
            e.apply_document_settings(0, 2160, 120);
            assert_eq!(e.doc.width, 960);
        });
    }
    #[test]
    fn timeline_duration_resizes_all_layers_and_undo_recovers_trimmed_art() {
        run(|e| {
            e.doc.layers[0].frames[23].push(crate::document::Mark {
                points: vec![[2., 3.]].into(),
                ..Default::default()
            });
            let layer = e.doc.layers[0].clone();
            e.doc.layers.push(layer);
            e.set_duration(4.);
            assert!(e.doc.layers.iter().all(|l| l.frames.len() == 48));
            e.set_duration(1.);
            assert!(e.doc.layers.iter().all(|l| l.frames.len() == 12));
            e.history(false);
            assert_eq!(e.doc.layers[0].frames[23][0].points[0], [2., 3.]);
            assert!(e.doc.validate().is_ok());
        });
    }
    #[test]
    fn color_swap_exchanges_both_colors_and_none_states() {
        run(|e| {
            e.fill_color = Color32::RED;
            e.color = Color32::BLUE;
            e.fill_enabled = true;
            e.stroke_enabled = false;
            e.swap_colors();
            assert_eq!(e.fill_color, Color32::BLUE);
            assert_eq!(e.color, Color32::RED);
            assert!(!e.fill_enabled && e.stroke_enabled);
            e.swap_colors();
            assert_eq!(e.fill_color, Color32::RED);
        });
    }
}
