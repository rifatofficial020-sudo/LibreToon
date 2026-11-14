use crate::{
    Document, Editor,
    canvas::{Tool, View},
    design::{self, Icon},
    document::{Layer, Mark},
    features::*,
    i18n::tr,
};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
pub(crate) fn assemble_project(
    doc: &Document,
    feature: &crate::features::FeatureState,
) -> Document {
    let mut child = doc.clone();
    for (parent, _, _, id) in feature.symbol_stack.iter().rev() {
        let mut next = parent.clone();
        next.extras.symbols = child.extras.symbols.clone();
        next.extras.animation_clips = child.extras.animation_clips.clone();
        next.extras.animation_library = child.extras.animation_library.clone();
        next.extras.library = child.extras.library.clone();
        next.extras.palette = child.extras.palette.clone();
        next.extras.main_palettes = child.extras.main_palettes.clone();
        next.extras.media.assets = child.extras.media.assets.clone();
        next.extras.references = child.extras.references.clone();
        if let Some(s) = next.extras.symbols.iter_mut().find(|s| s.id == *id) {
            s.layers = child.layers.clone();
            s.rig = child.extras.rig.clone();
            s.folders = child.extras.folders.clone();
            s.perspective = child.extras.perspective.clone();
        }
        crate::animation_clips::normalize_asset_ranges(&mut next, *id);
        child = next;
    }
    crate::animation::store_scene(&mut child);
    child
}
impl Editor {
    pub fn leave_symbol_context_for_document_change(&mut self) {
        if !self.feature.symbol_stack.is_empty() {
            self.finish_raster_stroke();
            self.finish_pending_pen();
            self.doc = self.project_document();
            self.feature.symbol_stack.clear();
            self.undo.clear();
            self.redo.clear();
        }
    }
    pub fn prepare_style(&mut self, mut marks: Vec<Mark>) -> Vec<Mark> {
        let bounds = marks.iter().fold(Rect::NOTHING, |r, m| {
            r.union(crate::vector::control_bounds(m))
        });
        for m in &mut marks {
            m.style.start_cap = self.feature.start_cap;
            m.style.end_cap = self.feature.end_cap;
            if m.fill.is_some() && self.feature.gradient_on {
                let mut g = self.feature.gradient.clone();
                g.fit(bounds);
                m.style.gradient = Some(g);
            }
            self.link_drawn_colors(m);
        }
        marks
    }
    pub fn sync_exposures(&mut self) {
        let Some(before) = self.undo.last() else {
            return;
        };
        if before.layers.len() != self.doc.layers.len() {
            return;
        }
        for l in 0..self.doc.layers.len() {
            let layer = &self.doc.layers[l];
            let f = self.frame.min(layer.frames.len() - 1);
            if layer.options.keys.len() != layer.frames.len()
                || before.layers[l].frames.len() != layer.frames.len()
                || before.layers[l].frames[f].identity() == layer.frames[f].identity()
            {
                continue;
            }
            let exposure = layer.exposure(f);
            let marks = layer.frames[f].clone();
            let layer = &mut self.doc.layers[l];
            for i in exposure {
                layer.frames[i] = marks.clone();
            }
        }
    }
    pub fn project_document(&self) -> Document {
        crate::advanced::assemble_project(&self.doc, &self.feature)
    }
    pub fn convert_symbol(&mut self) {
        self.convert_symbol_kind(false);
    }

    pub(crate) fn convert_symbol_kind(&mut self, lip: bool) {
        if lip && self.enable_lip_symbol_on_selection() {
            return;
        }
        if self.canvas.selected.is_empty()
            || self.doc.extras.symbols.len() >= 500
            || self.doc.layers[self.layer].options.locked
        {
            return;
        }
        self.finish_pending_pen();
        let partial = !self.canvas.partial.is_empty();
        if partial {
            let mut state = std::mem::take(&mut self.canvas);
            self.checkpoint();
            self.materialize_fragments(&mut state);
            self.canvas = state;
        }
        let mut ids = self.canvas.selected.clone();
        ids.sort_unstable();
        ids.dedup();
        let mut marks: Vec<_> = ids
            .iter()
            .map(|&(l, m)| crate::animation::marks(&self.doc.layers[l], self.frame)[m].clone())
            .collect();
        for mark in &mut marks {
            if let Some(i) = mark.style.instance.as_mut() {
                i.first = i.first.saturating_add(self.frame.saturating_sub(i.born));
                i.born = 0;
            }
        }
        let bounds = marks.iter().fold(Rect::NOTHING, |b, m| {
            b.union(crate::vector::control_bounds(m))
        });
        let selected_tracks = marks
            .iter()
            .map(|m| m.style.track)
            .filter(|id| *id != 0)
            .collect();
        let rig = self
            .doc
            .extras
            .rig
            .snapshot_targets(&selected_tracks, self.frame);
        let mouth_poses = if lip {
            crate::lip::default_poses(1)
        } else {
            Default::default()
        };
        let count = mouth_poses.len().max(1);
        let id = id();
        let number = self.doc.extras.symbols.len() + 1;
        if !partial {
            self.checkpoint();
        }
        let mut frames = vec![Default::default(); count];
        frames[0] = marks.into();
        let layer = Layer {
            name: "Artwork".into(),
            visible: true,
            frames: frames.into(),
            options: LayerOptions {
                keys: vec![true; count].into(),
                ..Default::default()
            },
        };
        self.doc.extras.symbols.push(Symbol {
            rig,
            library_folder: None,
            mouth_poses,
            folders: Default::default(),
            perspective: Default::default(),
            id,
            name: if lip {
                format!("{} {number}", tr("Lip Sync symbol"))
            } else {
                format!("Symbol {number}")
            },
            layers: vec![layer].into(),
        });
        let mut sorted = ids;
        sorted.sort_unstable();
        for (l, m) in sorted.into_iter().rev() {
            self.doc.layers[l].frames[self.frame].remove(m);
        }
        let mut mark = Mark {
            points: vec![crate::vector::xy(bounds.min), crate::vector::xy(bounds.max)].into(),
            rectangle: true,
            closed: true,
            stroke: false,
            fill: Some([0, 0, 0, 0]),
            ..Default::default()
        };
        mark.style.instance = Some(Instance {
            clock: Default::default(),
            symbol: id,
            matrix: [1., 0., 0., 1., 0., 0.],
            mode: if lip {
                Playback::Single
            } else {
                Playback::Loop
            },
            first: 0,
            born: self.frame,
        });
        let m = self.doc.layers[self.layer].frames[self.frame].len();
        self.doc.layers[self.layer].frames[self.frame].push(mark);
        self.canvas.selected = vec![(self.layer, m)];
        self.feature.selected_symbol = Some(id);
        self.recover();
    }
    pub fn enter_symbol(&mut self, id: u64) {
        if self.feature.symbol_stack.len() >= 8
            || self.feature.symbol_stack.iter().any(|s| s.3 == id)
        {
            return;
        }
        self.finish_pending_pen();
        let Some(symbol) = self.doc.extras.symbols.iter().find(|s| s.id == id) else {
            return;
        };
        let layers = symbol.layers.clone();
        let folders = symbol.folders.clone();
        let perspective = symbol.perspective.clone();
        let rig = symbol.rig.clone();
        self.feature
            .symbol_stack
            .push((self.doc.clone(), self.frame, self.layer, id));
        self.close_lip_session();
        self.animation.play_all = false;
        self.animation.picker = None;
        self.doc.layers = layers;
        self.doc.extras.rig = rig;
        self.rigging = Default::default();
        self.feature.selected_folder = None;
        self.feature.selected_camera = None;
        self.doc.extras.folders = folders;
        self.doc.extras.perspective = perspective;
        self.style_selection.clear();
        self.doc.extras.cameras = Default::default();
        self.doc.extras.media.tracks = Default::default();
        self.frame = 0;
        self.layer = 0;
        self.playing = false;
        self.canvas.clear_selection();
        self.undo.clear();
        self.redo.clear();
    }
    pub fn exit_symbol(&mut self) {
        self.finish_pending_pen();
        if let Some((mut parent, frame, layer, id)) = self.feature.symbol_stack.pop() {
            parent.extras.symbols = self.doc.extras.symbols.clone();
            parent.extras.animation_clips = self.doc.extras.animation_clips.clone();
            parent.extras.animation_library = self.doc.extras.animation_library.clone();
            parent.extras.library = self.doc.extras.library.clone();
            parent.extras.palette = self.doc.extras.palette.clone();
            parent.extras.main_palettes = self.doc.extras.main_palettes.clone();
            parent.extras.media.assets = self.doc.extras.media.assets.clone();
            parent.extras.references = self.doc.extras.references.clone();
            if let Some(s) = parent.extras.symbols.iter_mut().find(|s| s.id == id) {
                s.layers = self.doc.layers.clone();
                s.rig = self.doc.extras.rig.clone();
                s.folders = self.doc.extras.folders.clone();
                s.perspective = self.doc.extras.perspective.clone();
            }
            crate::animation_clips::normalize_asset_ranges(&mut parent, id);
            self.doc = parent;
            self.rigging = Default::default();
            self.feature.selected_folder = None;
            self.feature.selected_camera = None;
            self.style_selection.clear();
            self.frame = frame.min(self.doc.layers[0].frames.len() - 1);
            self.layer = layer.min(self.doc.layers.len() - 1);
            self.playing = false;
            self.canvas.clear_selection();
            self.undo.clear();
            self.redo.clear();
            self.recover();
        }
    }
    pub fn library_ui(&mut self, ui: &mut egui::Ui) {
        self.asset_library_ui(ui);
    }
    pub(crate) fn insert_symbol(&mut self, id: u64) {
        self.insert_symbol_at(id, None);
    }

    pub(crate) fn symbol_bounds(&self, id: u64) -> Option<Rect> {
        let symbol = self
            .doc
            .extras
            .symbols
            .iter()
            .find(|symbol| symbol.id == id)?;
        let bounds = symbol
            .layers
            .iter()
            .flat_map(|layer| {
                layer
                    .frames
                    .iter()
                    .find(|frame| !frame.is_empty())
                    .into_iter()
                    .flatten()
            })
            .fold(Rect::NOTHING, |b, m| {
                b.union(crate::vector::control_bounds(m))
            });
        bounds.is_finite().then_some(bounds)
    }

    pub(crate) fn insert_symbol_at(&mut self, id: u64, at: Option<Pos2>) {
        if self.raster_active() || self.adjustment_active() {
            self.status = tr("Select a vector layer to place a symbol or image").into();
            return;
        }
        if !self.symbol_insert_allowed(id)
            || self.doc.layers[self.layer].options.locked
            || !self.doc.layers[self.layer].visible
        {
            return;
        }
        let Some(bounds) = self.symbol_bounds(id) else {
            return;
        };
        self.checkpoint();
        let offset = at.map_or(Vec2::ZERO, |at| at - bounds.center());
        let mut m = Mark {
            points: vec![
                crate::vector::xy(bounds.min + offset),
                crate::vector::xy(bounds.max + offset),
            ]
            .into(),
            rectangle: true,
            closed: true,
            fill: Some([0, 0, 0, 0]),
            stroke: false,
            ..Default::default()
        };
        m.style.instance = Some(Instance {
            clock: Default::default(),
            symbol: id,
            matrix: [1., 0., 0., 1., offset.x, offset.y],
            mode: Playback::Loop,
            first: 0,
            born: self.frame,
        });
        let index = self.doc.layers[self.layer].frames[self.frame].len();
        self.doc.layers[self.layer].frames[self.frame].push(m);
        self.activate_tool(Tool::Select);
        self.canvas.selected = vec![(self.layer, index)];
        self.recover();
    }
    pub(crate) fn symbol_in_use(&self, id: u64) -> bool {
        let references = |layers: &[Layer]| {
            layers
                .iter()
                .flat_map(|l| l.frames.iter().flatten())
                .any(|m| m.style.instance.as_ref().is_some_and(|i| i.symbol == id))
        };
        self.doc.extras.rig.character_symbol == Some(id)
            || self
                .doc
                .extras
                .scenes
                .iter()
                .any(|scene| scene.rig.character_symbol == Some(id))
            || self
                .doc
                .extras
                .symbols
                .iter()
                .any(|symbol| symbol.rig.character_symbol == Some(id))
            || self
                .doc
                .extras
                .animation_clips
                .iter()
                .any(|clip| clip.symbol == id)
            || references(&self.doc.layers)
            || self
                .doc
                .extras
                .scenes
                .iter()
                .enumerate()
                .any(|(i, s)| i != self.doc.extras.active_scene && references(&s.layers))
            || self
                .doc
                .extras
                .symbols
                .iter()
                .any(|s| references(&s.layers))
            || self
                .feature
                .symbol_stack
                .iter()
                .any(|(doc, _, _, editing)| *editing == id || references(&doc.layers))
    }
    pub(crate) fn symbol_insert_allowed(&self, id: u64) -> bool {
        let mut pending = vec![id];
        let mut seen = std::collections::HashSet::new();
        while let Some(id) = pending.pop() {
            if self.feature.symbol_stack.iter().any(|s| s.3 == id) {
                return false;
            }
            if !seen.insert(id) {
                continue;
            }
            if let Some(symbol) = self.doc.extras.symbols.iter().find(|s| s.id == id) {
                pending.extend(
                    symbol
                        .layers
                        .iter()
                        .flat_map(|l| l.frames.iter().flatten())
                        .filter_map(|m| m.style.instance.as_ref().map(|i| i.symbol)),
                );
            }
        }
        true
    }
    pub fn gradient_ui(&mut self, ui: &mut egui::Ui) {
        design::category(ui, "RELLENO Y DEGRADADO");
        let mut changed = ui
            .checkbox(&mut self.feature.gradient_on, tr("Degradado"))
            .changed();
        if self.feature.gradient_on {
            changed |= self.visual_gradient_ui(ui);
        }
        if changed && !self.canvas.selected.is_empty() {
            let ids = self.canvas.selected.clone();
            self.checkpoint();
            for &(l, m) in &ids {
                let mark = &mut self.doc.layers[l].frames[self.frame][m];
                if mark.fill.is_some() {
                    mark.style.gradient = if self.feature.gradient_on {
                        Some(self.feature.gradient.clone())
                    } else {
                        None
                    };
                }
            }
            self.canvas.selected = ids;
            self.recover();
        }
    }
    pub fn advanced_properties(&mut self, ui: &mut egui::Ui) {
        self.motion_properties(ui);
        if let Some(&(l, m)) = self.canvas.selected.first() {
            if let Some(instance) = crate::animation::marks(&self.doc.layers[l], self.frame)[m]
                .style
                .instance
                .clone()
            {
                if self.doc.layers[l].frames[self.frame][m]
                    .style
                    .symbol_pivot
                    .is_some()
                    && design::button(
                        ui,
                        Icon::Undo,
                        "Reset symbol pivot",
                        "",
                        false,
                        !self.doc.layers[l].options.locked,
                    )
                    .clicked()
                {
                    let selection = self.canvas.selected.clone();
                    self.checkpoint();
                    self.doc.layers[l].frames[self.frame][m].style.symbol_pivot = None;
                    self.canvas.selected = selection;
                    self.recover();
                }
                if self.lip_instance_properties(ui, l, m, instance.symbol) {
                    return;
                }
                design::category(ui, "SÍMBOLO");
                let mut settings = instance.clone();
                egui::ComboBox::from_id_salt("symbol-clock")
                    .selected_text(if settings.clock == SymbolClock::Scene {
                        tr("Sincronizado con la escena")
                    } else {
                        tr("Línea de tiempo independiente")
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut settings.clock,
                            SymbolClock::Scene,
                            tr("Sincronizado con la escena"),
                        );
                        ui.selectable_value(
                            &mut settings.clock,
                            SymbolClock::Independent,
                            tr("Línea de tiempo independiente"),
                        );
                    });
                ui.horizontal(|ui| {
                    for (mode, icon, label) in [
                        (Playback::Loop, Icon::Loop, "Loop"),
                        (Playback::Once, Icon::Play, "Play once"),
                        (Playback::Single, Icon::Film, "Single frame"),
                    ] {
                        if design::button(ui, icon, "", label, settings.mode == mode, true)
                            .clicked()
                        {
                            settings.mode = mode;
                        }
                    }
                });
                if self
                    .symbol_thumbnail(
                        ui,
                        instance.symbol,
                        settings.first,
                        eframe::egui::vec2(180., 96.),
                    )
                    .clicked()
                {
                    self.animation.picker = Some((l, m, self.frame));
                }
                ui.label(format!("{} {}", tr("Fotograma"), settings.first + 1));
                if settings != instance {
                    self.checkpoint();
                    self.autokey_motion(&[(l, m)]);
                    self.doc.layers[l].frames[self.frame][m].style.instance = Some(settings);
                    self.canvas.selected = vec![(l, m)];
                    self.recover();
                }
                if design::button(
                    ui,
                    Icon::Node,
                    tr("Editar símbolo"),
                    tr("Editar símbolo"),
                    false,
                    true,
                )
                .clicked()
                {
                    self.enter_symbol(instance.symbol);
                }
                ui.separator();
                return;
            }
        }
        if self.tool == Tool::Perspective {
            self.perspective_properties(ui);
            return;
        }
        if self.tool == Tool::Camera {
            self.camera_properties(ui);
            return;
        }
        let fill = self
            .canvas
            .selected
            .iter()
            .any(|&(l, m)| self.doc.layers[l].frames[self.frame][m].fill.is_some());
        if fill
            || matches!(
                self.tool,
                Tool::Bucket
                    | Tool::Brush
                    | Tool::FillBrush
                    | Tool::Rectangle
                    | Tool::Ellipse
                    | Tool::Polygon
                    | Tool::Pen
            )
        {
            self.gradient_ui(ui);
            ui.separator();
        }
        if matches!(
            self.tool,
            Tool::VectorBrush | Tool::Line | Tool::Pen | Tool::Width
        ) || self
            .canvas
            .selected
            .iter()
            .any(|&(l, m)| self.doc.layers[l].frames[self.frame][m].stroke)
        {
            let old = (self.feature.start_cap, self.feature.end_cap);
            for (start, label) in [(true, "Punta de entrada"), (false, "Punta final")] {
                let value = if start {
                    &mut self.feature.start_cap
                } else {
                    &mut self.feature.end_cap
                };
                ui.label(tr(label));
                ui.horizontal(|ui| {
                    for (cap, icon, label) in [
                        (Cap::Round, Icon::CapRound, "Round"),
                        (Cap::Flat, Icon::CapFlat, "Flat"),
                        (Cap::Square, Icon::CapSquare, "Square"),
                        (Cap::Triangle, Icon::CapTriangle, "Triangle"),
                    ] {
                        if design::button(ui, icon, "", label, *value == cap, true).clicked() {
                            *value = cap;
                        }
                    }
                });
            }
            if old != (self.feature.start_cap, self.feature.end_cap)
                && !self.canvas.selected.is_empty()
            {
                let ids = self.canvas.selected.clone();
                self.checkpoint();
                for &(l, m) in &ids {
                    let m = &mut self.doc.layers[l].frames[self.frame][m];
                    m.style.start_cap = self.feature.start_cap;
                    m.style.end_cap = self.feature.end_cap;
                }
                self.canvas.selected = ids;
                self.recover();
            }
        }
    }
    pub fn camera_view(&self, mut view: View) -> View {
        if self.playing || self.studio.stage_only || self.tool == Tool::Camera {
            if let Some(camera) = self
                .doc
                .extras
                .cameras
                .iter()
                .find(|c| self.frame >= c.start && self.frame < c.start + c.duration)
            {
                let camera = crate::animation::camera_at(camera, self.frame);
                view.stage_center = crate::vector::pos(camera.center);
                view.scale *= camera.zoom;
                view.angle -= camera.angle;
            }
        }
        view
    }
    pub fn add_camera(&mut self) {
        if !self.feature.symbol_stack.is_empty()
            || self.doc.extras.cameras.len() >= 1000
            || self
                .doc
                .extras
                .cameras
                .last()
                .is_some_and(|c| c.start + c.duration >= 10000)
        {
            return;
        }
        self.checkpoint();
        let start = self
            .doc
            .extras
            .cameras
            .last()
            .map_or(0, |c| c.start + c.duration);
        let duration = if start == 0 {
            self.doc.layers[0].frames.len()
        } else {
            self.doc.fps as usize
        };
        let id = id();
        let number = self.doc.extras.cameras.len() + 1;
        self.doc.extras.cameras.push(Camera {
            optics: Default::default(),
            animation: Default::default(),

            id,
            name: format!("Camera {}", number),
            start,
            duration,
            center: [self.doc.width as f32 / 2., self.doc.height as f32 / 2.],
            zoom: 1.,
            angle: 0.,
        });
        let count = (start + duration).min(10000);
        for l in &mut self.doc.layers {
            if crate::animation_clips::resize_track(l, count) {
                continue;
            }
            l.init_keys();
            l.frames.resize(count, Default::default());
            l.options.keys.resize(count, true);
        }
        self.feature.selected_camera = Some(id);
        self.frame = start.min(count - 1);
        self.recover();
    }
    pub fn camera_properties(&mut self, ui: &mut egui::Ui) {
        design::category(ui, "CÁMARA");
        ui.horizontal(|ui| {
            if design::button(
                ui,
                Icon::Plus,
                "",
                tr("Añadir cámara"),
                false,
                self.feature.symbol_stack.is_empty(),
            )
            .clicked()
            {
                self.add_camera();
            }
            if design::button(
                ui,
                Icon::Trash,
                "",
                tr("Eliminar cámara"),
                false,
                self.feature.selected_camera.is_some(),
            )
            .clicked()
            {
                if let Some(id) = self.feature.selected_camera.take() {
                    self.checkpoint();
                    self.doc.extras.cameras.retain(|c| c.id != id);
                    self.normalize_cameras();
                    self.recover();
                }
            }
        });
        let list = self.doc.extras.cameras.clone();
        for c in &list {
            if ui
                .selectable_label(self.feature.selected_camera == Some(c.id), &c.name)
                .clicked()
            {
                self.feature.selected_camera = Some(c.id);
                self.frame = c.start;
            }
        }
        if let Some(index) = self
            .feature
            .selected_camera
            .and_then(|id| self.doc.extras.cameras.iter().position(|c| c.id == id))
        {
            let mut c = self.doc.extras.cameras[index].clone();
            let base = (c.center, c.zoom, c.angle, c.optics.clone());
            let pose = crate::animation::camera_at(&c, self.frame);
            c.center = pose.center;
            c.zoom = pose.zoom;
            c.angle = pose.angle;
            c.optics = pose.optics(&c);
            let evaluated_optics = c.optics.clone();
            ui.horizontal(|ui| {
                for (mode, icon, label) in [
                    (0, Icon::Pan, "Pan camera"),
                    (1, Icon::Rotate, "Rotate camera"),
                    (2, Icon::Zoom, "Zoom camera"),
                ] {
                    if design::button(
                        ui,
                        icon,
                        "",
                        label,
                        self.animation.camera_mode == mode,
                        true,
                    )
                    .clicked()
                    {
                        self.animation.camera_mode = mode;
                    }
                }
            });
            let mut changed = ui.text_edit_singleline(&mut c.name).changed();
            changed |= crate::depth::optics_ui(ui, &mut c.optics);
            ui.label(tr("Posición"));
            ui.horizontal(|ui| {
                for v in &mut c.center {
                    changed |= ui.add(egui::DragValue::new(v).range(-1e6..=1e6)).changed();
                }
            });
            changed |= ui
                .add(egui::Slider::new(&mut c.zoom, 0.05..=10.).text(tr("Zoom")))
                .changed();
            let mut degrees = c.angle.to_degrees();
            if ui
                .add(egui::DragValue::new(&mut degrees).suffix(crate::i18n::tr("°")))
                .changed()
            {
                c.angle = degrees.to_radians();
                changed = true;
            }
            changed |= ui
                .add(
                    egui::DragValue::new(&mut c.duration)
                        .range(1..=10000)
                        .prefix(tr("Duración: ")),
                )
                .changed();
            if !ui.input(|i| i.pointer.primary_down()) {
                self.animation.timing_transaction = false;
            }
            let old_curve = c.animation.curve.clone();
            ui.collapsing(crate::i18n::tr("Camera motion · easing / path"), |ui| {
                crate::visual_controls::timing_controls(
                    ui,
                    &mut c.animation.curve,
                    &mut self.animation.ease_handle,
                    c.duration,
                )
            });
            let timing_changed = old_curve != c.animation.curve;
            changed |= timing_changed;
            if changed {
                if !timing_changed || !self.animation.timing_transaction {
                    self.checkpoint();
                }
                self.animation.timing_transaction =
                    timing_changed && self.animation.ease_handle.is_some();
                let pose_changed = c.center != pose.center
                    || c.zoom != pose.zoom
                    || c.angle != pose.angle
                    || c.optics != evaluated_optics;
                let key = crate::animation::CameraKey {
                    optics: Some(c.optics.clone()),
                    frame: self.frame.saturating_sub(c.start).min(c.duration - 1),
                    center: c.center,
                    zoom: c.zoom,
                    angle: c.angle,
                };
                c.center = base.0;
                c.zoom = base.1;
                c.angle = base.2;
                c.optics = base.3;
                if pose_changed {
                    crate::animation::camera_record(&mut c, key);
                }
                self.doc.extras.cameras[index] = c;
                self.normalize_cameras();
                self.recover();
            }
        }
    }
    pub fn normalize_cameras(&mut self) {
        let mut start = 0;
        let count = self.doc.extras.cameras.len();
        for (index, c) in self.doc.extras.cameras.iter_mut().enumerate() {
            c.start = start;
            c.duration = c.duration.clamp(
                1,
                10000usize.saturating_sub(start + count - index - 1).max(1),
            );
            c.animation.keys.retain(|k| k.frame < c.duration);
            start += c.duration;
        }
        if start > self.doc.layers[0].frames.len() {
            for l in &mut self.doc.layers {
                if crate::animation_clips::resize_track(l, start.min(10000)) {
                    continue;
                }
                l.init_keys();
                l.frames.resize(start.min(10000), Default::default());
                l.options.keys.resize(l.frames.len(), true);
            }
        }
    }
    pub fn fit_camera_duration(&mut self) {
        let count = self.doc.layers[0].frames.len();
        self.doc.extras.cameras.retain(|c| c.start < count);
        for c in &mut self.doc.extras.cameras {
            c.duration = c.duration.min(count - c.start);
            c.animation.keys.retain(|k| k.frame < c.duration);
        }
        if let Some(last) = self.doc.extras.cameras.last_mut() {
            last.duration = count - last.start;
        }
    }
    fn perspective_properties(&mut self, ui: &mut egui::Ui) {
        design::category(ui, "PERSPECTIVA");
        let mut p = self.doc.extras.perspective.clone();
        let mut changed = ui
            .checkbox(&mut p.enabled, tr("Mostrar perspectiva"))
            .changed();
        let mut count = p.points.len();
        if ui
            .add(egui::Slider::new(&mut count, 1..=3).text(tr("Puntos de fuga")))
            .changed()
        {
            p.points
                .resize(count, [self.doc.width as f32 / 2., p.horizon]);
            changed = true;
        }
        if ui
            .add(egui::DragValue::new(&mut p.horizon).prefix(tr("Horizonte: ")))
            .changed()
        {
            for point in &mut p.points {
                point[1] = p.horizon;
            }
            changed = true;
        }
        changed |= ui
            .add(egui::Slider::new(&mut p.rays, 4..=32).text(tr("Líneas de fuga")))
            .changed();
        for (i, point) in p.points.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!("VP {}", i + 1));
                for value in point {
                    changed |= ui.add(egui::DragValue::new(value)).changed();
                }
            });
        }
        if changed {
            self.checkpoint();
            self.doc.extras.perspective = p;
            self.recover();
        }
    }
    pub fn advanced_canvas_input(&mut self, ui: &egui::Ui, area: Rect, view: View) -> bool {
        if !matches!(self.tool, Tool::Perspective | Tool::Camera) {
            return false;
        }
        if ui.input(|i| {
            i.key_down(egui::Key::Space) || i.pointer.button_down(egui::PointerButton::Middle)
        }) {
            return false;
        }
        let ctx = ui.ctx();
        let Some(screen) = ctx.input(|i| i.pointer.interact_pos()) else {
            return true;
        };
        if ctx.input(|i| i.pointer.primary_pressed())
            && area.contains(screen)
            && ctx.layer_id_at(screen) == Some(ui.layer_id())
        {
            if self.tool == Tool::Camera {
                if self.feature.selected_camera.is_some() {
                    self.checkpoint();
                    self.feature.last_pointer = Some(screen);
                }
            } else {
                let p = &self.doc.extras.perspective;
                let hit = p
                    .points
                    .iter()
                    .position(|p| view.screen(crate::vector::pos(*p)).distance(screen) < 14.);
                let world = view.world(screen);
                let horizon = (world.y - p.horizon).abs() * view.scale < 10.;
                if hit.is_some() || horizon {
                    self.checkpoint();
                    self.feature.guide_drag = Some(hit.unwrap_or(usize::MAX));
                }
            }
        }
        if ctx.input(|i| i.pointer.primary_down() || i.pointer.primary_released()) {
            let world = view.world(screen);
            if let Some(previous) = self.feature.last_pointer {
                if let Some(index) = self
                    .feature
                    .selected_camera
                    .and_then(|id| self.doc.extras.cameras.iter().position(|c| c.id == id))
                {
                    let c = &mut self.doc.extras.cameras[index];
                    let mut pose = crate::animation::camera_at(c, self.frame);
                    pose.frame = self.frame.saturating_sub(c.start).min(c.duration - 1);
                    let d = screen - previous;
                    match self.animation.camera_mode {
                        1 => pose.angle += d.x * 0.01,
                        2 => pose.zoom = (pose.zoom * (-d.y * 0.01).exp()).clamp(0.05, 10.),
                        _ => {
                            pose.center = crate::vector::xy(
                                crate::vector::pos(pose.center)
                                    - crate::canvas::rotate(d / view.scale, -view.angle),
                            )
                        }
                    }
                    crate::animation::camera_record(c, pose);
                    self.feature.last_pointer = Some(screen);
                }
            }
            if let Some(index) = self.feature.guide_drag {
                let p = &mut self.doc.extras.perspective;
                p.enabled = true;
                if index == usize::MAX {
                    let delta = world.y - p.horizon;
                    p.horizon = world.y;
                    for point in &mut p.points {
                        point[1] += delta;
                    }
                } else {
                    p.points[index] = crate::vector::xy(world);
                }
            }
            if ctx.input(|i| i.pointer.primary_released())
                && (self.feature.last_pointer.is_some() || self.feature.guide_drag.is_some())
            {
                self.feature.last_pointer = None;
                self.feature.guide_drag = None;
                self.recover();
            }
        } else if self.feature.last_pointer.take().is_some()
            || self.feature.guide_drag.take().is_some()
        {
            self.recover();
        }
        true
    }
    pub fn advanced_overlays(&self, p: &egui::Painter, area: Rect, view: View) {
        let perspective = &self.doc.extras.perspective;
        if perspective.enabled {
            let left = view.world(area.left_bottom());
            let right = view.world(area.right_bottom());
            let color = Color32::from_rgba_unmultiplied(70, 185, 205, 100);
            let span = area.size().length() / view.scale;
            p.line_segment(
                [
                    view.screen(Pos2::new(left.x - span, perspective.horizon)),
                    view.screen(Pos2::new(right.x + span, perspective.horizon)),
                ],
                Stroke::new(1., color),
            );
            for point in &perspective.points {
                let origin = crate::vector::pos(*point);
                for i in 0..=perspective.rays {
                    let t = i as f32 / perspective.rays as f32;
                    let target = left.lerp(right, t);
                    let direction = (target - origin).normalized() * span * 3.;
                    p.line_segment(
                        [
                            view.screen(origin - direction),
                            view.screen(origin + direction),
                        ],
                        Stroke::new(0.5, color),
                    );
                }
                p.circle_stroke(
                    view.screen(origin),
                    6.,
                    Stroke::new(2., Color32::from_rgb(90, 210, 220)),
                );
            }
        }
        if self.tool == Tool::Camera {
            for c in &self.doc.extras.cameras {
                if self.feature.selected_camera != Some(c.id) {
                    continue;
                }
                let name = c.name.clone();
                let c = crate::animation::camera_at(c, self.frame);
                let center = crate::vector::pos(c.center);
                let half = Vec2::new(self.doc.width as f32, self.doc.height as f32) / (2. * c.zoom);
                let points: Vec<_> = [
                    Vec2::new(-half.x, -half.y),
                    Vec2::new(half.x, -half.y),
                    half,
                    Vec2::new(-half.x, half.y),
                    Vec2::new(-half.x, -half.y),
                ]
                .into_iter()
                .map(|v| view.screen(center + crate::canvas::rotate(v, c.angle)))
                .collect();
                p.add(egui::Shape::line(
                    points,
                    Stroke::new(2., Color32::from_rgb(235, 165, 65)),
                ));
                p.text(
                    view.screen(
                        center
                            + crate::canvas::rotate(
                                Vec2::new(-half.x + 16. / view.scale, -half.y + 18. / view.scale),
                                c.angle,
                            ),
                    ),
                    egui::Align2::LEFT_TOP,
                    &name,
                    egui::FontId::proportional(14.),
                    Color32::from_rgb(235, 165, 65),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::Area;
    fn editor() -> Editor {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx);
        let mut e = Editor::new(&cc);
        e.doc = Document::default();
        e.recovery = std::env::temp_dir().join(format!(
            "libretoon-advanced-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ));
        e
    }
    fn rect(x: f32, color: [u8; 4]) -> Mark {
        Mark {
            points: vec![[x, 0.], [x + 100., 100.]].into(),
            rectangle: true,
            closed: true,
            fill: Some(color),
            stroke: false,
            ..Default::default()
        }
    }
    fn fills(e: &Editor) -> Vec<Mark> {
        e.doc.layers[0].frames[e.frame]
            .iter()
            .filter(|m| m.fill.is_some())
            .cloned()
            .collect()
    }
    #[test]
    fn fill_brush_is_clipped_to_existing_fills_and_undo_restores_them() {
        let mut e = editor();
        e.doc.layers[0].frames[0].push(rect(0., [50, 100, 150, 255]));
        e.checkpoint();
        e.fill_brush(vec![rect(50., [200, 50, 30, 255])]);
        let marks = fills(&e);
        let area: f64 = marks
            .iter()
            .map(|m| crate::vector::polygon(m).unsigned_area())
            .sum();
        assert!((area - 10000.).abs() < 0.1);
        assert!(
            marks
                .iter()
                .all(|m| crate::vector::control_bounds(m).right() <= 100.001)
        );
        assert!(marks.iter().any(|m| m.fill == Some([200, 50, 30, 255])));
        e.history(false);
        assert_eq!(e.doc.layers[0].frames[0].len(), 1);
        assert_eq!(e.doc.layers[0].frames[0][0].fill, Some([50, 100, 150, 255]));
        e.doc.layers[0].frames[0].clear();
        e.fill_brush(vec![rect(0., [200, 0, 0, 255])]);
        assert!(e.doc.layers[0].frames[0].is_empty());
    }
    #[test]
    fn toon_budgets_are_per_region_independent_and_survive_fla() {
        let mut e = editor();
        e.doc.layers[0].frames[0].push(rect(0., [180, 120, 80, 255]));
        let brush = vec![rect(50., [0, 0, 0, 255])];
        e.tone_paint(brush.clone(), false);
        let first = serde_json::to_value(&e.doc).unwrap();
        e.tone_paint(brush.clone(), false);
        assert_eq!(first, serde_json::to_value(&e.doc).unwrap());
        e.feature.toon_levels = 2;
        e.tone_paint(brush.clone(), false);
        assert!(fills(&e).iter().any(|m| m.style.shade == 2));
        let second = serde_json::to_value(&e.doc).unwrap();
        e.tone_paint(brush.clone(), false);
        assert_eq!(second, serde_json::to_value(&e.doc).unwrap());
        e.feature.toon_levels = 1;
        e.tone_paint(brush, true);
        assert!(
            fills(&e)
                .iter()
                .any(|m| m.style.light == 1 && m.style.shade == 2)
        );
        let bytes = crate::fla::encode(&e.doc).unwrap();
        let restored = crate::fla::decode(&bytes).unwrap();
        assert_eq!(
            serde_json::to_value(&restored).unwrap(),
            serde_json::to_value(&e.doc).unwrap()
        );
    }
    #[test]
    fn toon_metadata_survives_selection_cuts() {
        let mut e = editor();
        let mut mark = rect(0., [90, 80, 70, 255]);
        mark.style.shade = 1;
        let (outside, inside) = crate::vector::split_rect(
            &mark,
            Rect::from_min_max(Pos2::new(25., 0.), Pos2::new(75., 100.)),
        );
        assert!(outside.iter().chain(&inside).all(|m| m.style.shade == 1));
        e.doc.layers[0].frames[0] = outside.into();
        let before = serde_json::to_value(&e.doc).unwrap();
        e.tone_paint(vec![rect(0., [0, 0, 0, 255])], false);
        assert_eq!(before, serde_json::to_value(&e.doc).unwrap());
    }
    #[test]
    fn exposures_propagate_edits_and_duration_operations_shift_keys() {
        let mut e = editor();
        e.doc.layers[0].frames.resize(3, Default::default());
        let source = vec![rect(0., [30, 50, 70, 255])].into();
        e.doc.layers[0].frames[0] = source;
        e.doc.layers[0].frames[1] = e.doc.layers[0].frames[0].clone();
        e.doc.layers[0].options.keys = vec![true, false, true].into();
        e.frame = 1;
        e.checkpoint();
        e.doc.layers[0].frames[1][0].fill = Some([100, 110, 120, 255]);
        e.recover();
        assert_eq!(
            e.doc.layers[0].frames[0][0].fill,
            Some([100, 110, 120, 255])
        );
        assert!(e.doc.layers[0].frames[2].is_empty());
        e.timeline_edit(2);
        assert_eq!(e.doc.layers[0].frames.len(), 4);
        assert_eq!(
            &e.doc.layers[0].options.keys[..],
            &[true, false, false, true]
        );
        e.timeline_edit(3);
        assert_eq!(e.doc.layers[0].frames.len(), 3);
        e.timeline_edit(1);
        assert!(e.doc.layers[0].frames[1].is_empty());
        assert!(!e.doc.layers[0].frames[0].is_empty());
        assert!(e.doc.validate().is_ok());
    }
    #[test]
    fn clipping_layers_and_opacity_render_without_mutating_source() {
        let mut e = editor();
        e.doc.layers[0].frames[0].push(rect(0., [100, 100, 100, 255]));
        let mut upper = e.doc.layers[0].clone();
        upper.frames = vec![Default::default(); 24].into();
        upper.frames[0].push(rect(50., [230, 60, 20, 255]));
        upper.options.clip = true;
        upper.options.opacity = 0.5;
        e.doc.layers.push(upper);
        let rendered = crate::features::resolve(&e.doc, 0, false);
        let area: f64 = rendered
            .iter()
            .filter(|(l, _, m)| *l == 1 && m.fill.is_some())
            .map(|(_, _, m)| crate::vector::polygon(m).unsigned_area())
            .sum();
        assert!((area - 5000.).abs() < 0.1);
        assert!(
            rendered
                .iter()
                .filter(|(l, _, _)| *l == 1)
                .all(|(_, _, m)| m.fill.unwrap()[3] == 128)
        );
        assert_eq!(e.doc.layers[1].frames[0][0].fill.unwrap()[3], 255);
    }
    #[test]
    fn camera_cuts_are_adjacent_and_export_zoom_includes_stroke_width() {
        let mut e = editor();
        let mut mark = rect(0., [100, 80, 60, 255]);
        mark.stroke = true;
        mark.width = 4.;
        e.doc.layers[0].frames[0].push(mark);
        e.add_camera();
        e.add_camera();
        assert_eq!(e.doc.extras.cameras[0].start, 0);
        assert_eq!(
            e.doc.extras.cameras[1].start,
            e.doc.extras.cameras[0].duration
        );
        e.doc.extras.cameras[0].zoom = 2.;
        let rendered = crate::features::resolve(&e.doc, 0, true);
        assert_eq!(rendered[0].2.width, 8.);
        assert_eq!(rendered[0].2.points[0], [-480., -270.]);
        assert!(e.doc.validate().is_ok());
        let bytes = crate::fla::encode(&e.doc).unwrap();
        let restored = crate::fla::decode(&bytes).unwrap();
        assert_eq!(restored.extras.cameras.len(), 2);
        e.set_duration(1.);
        assert!(e.doc.validate().is_ok());
    }
    #[test]
    fn symbol_editing_and_recovery_snapshot_preserve_the_parent_scene() {
        let mut e = editor();
        e.doc.layers[0].frames[0].push(rect(0., [20, 40, 60, 255]));
        e.canvas.selected = vec![(0, 0)];
        e.convert_symbol();
        let id = e.doc.extras.symbols[0].id;
        assert!(e.doc.layers[0].frames[0][0].style.instance.is_some());
        e.enter_symbol(id);
        e.doc.extras.folders.push(Folder {
            id: 500,
            name: "Inside symbol".into(),
            parent: None,
            collapsed: false,
            transform: Default::default(),
            timeline_slot: None,
            range: None,
            source_shift: 0,
        });
        e.doc.layers[0].options.folder = Some(500);
        e.checkpoint();
        e.doc.layers[0].frames[0][0].fill = Some([220, 80, 30, 255]);
        e.recover();
        let snapshot = e.project_document();
        assert!(snapshot.extras.folders.is_empty());
        assert_eq!(snapshot.extras.symbols[0].folders[0].id, 500);
        assert!(snapshot.layers[0].frames[0][0].style.instance.is_some());
        assert_eq!(
            snapshot.extras.symbols[0].layers[0].frames[0][0].fill,
            Some([220, 80, 30, 255])
        );
        let restored = crate::fla::decode(&crate::fla::encode(&snapshot).unwrap()).unwrap();
        assert_eq!(
            crate::features::resolve(&restored, 0, false)[0].2.fill,
            Some([220, 80, 30, 255])
        );
        e.exit_symbol();
        assert!(e.feature.symbol_stack.is_empty());
        assert_eq!(e.frame, 0);
    }
    #[test]
    fn camera_reordering_ripples_cuts_without_gaps_or_lost_resources() {
        let mut e = editor();
        e.add_camera();
        e.add_camera();
        let first = e.doc.extras.cameras[0].id;
        let second = e.doc.extras.cameras[1].id;
        let duration = e.doc.layers[0].frames.len();
        e.move_camera(1, 0);
        assert_eq!(e.doc.extras.cameras[0].id, second);
        assert_eq!(e.doc.extras.cameras[1].id, first);
        assert_eq!(
            e.doc.extras.cameras[1].start,
            e.doc.extras.cameras[0].duration
        );
        assert_eq!(e.doc.layers[0].frames.len(), duration);
        assert!(e.doc.validate().is_ok());
        e.history(false);
        assert_eq!(e.doc.extras.cameras[0].id, first);
    }
    #[test]
    fn symbol_loop_once_and_single_frame_are_distinct() {
        let mut e = editor();
        e.doc.layers[0].frames[0].push(rect(0., [10, 20, 30, 255]));
        e.canvas.selected = vec![(0, 0)];
        e.convert_symbol();
        let id = e.doc.extras.symbols[0].id;
        e.doc.extras.symbols[0].layers[0]
            .frames
            .resize(2, Default::default());
        e.doc.extras.symbols[0].layers[0].options.keys = vec![true, true].into();
        e.doc.extras.symbols[0].layers[0].frames[1] = vec![rect(0., [200, 100, 50, 255])].into();
        for f in 1..4 {
            e.doc.layers[0].frames[f] = e.doc.layers[0].frames[0].clone();
        }
        assert_eq!(
            crate::features::resolve(&e.doc, 2, false)[0].2.fill,
            Some([10, 20, 30, 255])
        );
        e.doc.layers[0].frames[2][0]
            .style
            .instance
            .as_mut()
            .unwrap()
            .mode = Playback::Once;
        assert_eq!(
            crate::features::resolve(&e.doc, 2, false)[0].2.fill,
            Some([200, 100, 50, 255])
        );
        let i = e.doc.layers[0].frames[2][0]
            .style
            .instance
            .as_mut()
            .unwrap();
        i.mode = Playback::Single;
        i.first = 0;
        assert_eq!(
            crate::features::resolve(&e.doc, 2, false)[0].2.fill,
            Some([10, 20, 30, 255])
        );
        assert_eq!(e.doc.extras.symbols[0].id, id);
    }
    #[test]
    fn gradients_keep_stops_alpha_and_coordinates_through_save_and_transform() {
        let mut e = editor();
        let mut mark = rect(0., [255, 255, 255, 255]);
        let g = Gradient {
            radial: false,
            start: [0., 50.],
            end: [100., 50.],
            stops: vec![
                Stop {
                    t: 0.,
                    color: [255, 0, 0, 255],
                },
                Stop {
                    t: 0.5,
                    color: [0, 255, 0, 128],
                },
                Stop {
                    t: 1.,
                    color: [0, 0, 255, 0],
                },
            ]
            .into(),
        };
        assert_eq!(g.color(Pos2::new(50., 50.)), [0, 255, 0, 128]);
        mark.style.gradient = Some(g);
        let moved = crate::vector::transform(&mark, |p| p + Vec2::new(25., 40.));
        assert_eq!(moved.style.gradient.as_ref().unwrap().start, [25., 90.]);
        e.doc.layers[0].frames[0].push(moved);
        assert!(e.doc.svg(0).contains("linearGradient"));
        let restored = crate::fla::decode(&crate::fla::encode(&e.doc).unwrap()).unwrap();
        assert_eq!(
            restored.layers[0].frames[0][0].style.gradient,
            e.doc.layers[0].frames[0][0].style.gradient
        );
    }
    #[test]
    fn invalid_folder_and_symbol_cycles_are_rejected() {
        let mut e = editor();
        e.doc.extras.folders.push(Folder {
            id: 1,
            name: "A".into(),
            parent: Some(1),
            collapsed: false,
            transform: Default::default(),
            timeline_slot: None,
            range: None,
            source_shift: 0,
        });
        assert!(e.doc.validate().is_err());
        e.doc.extras.folders.clear();
        let mut mark = rect(0., [0, 0, 0, 255]);
        mark.style.instance = Some(Instance {
            clock: Default::default(),
            symbol: 1,
            matrix: [1., 0., 0., 1., 0., 0.],
            mode: Playback::Loop,
            first: 0,
            born: 0,
        });
        let mut l = e.doc.layers[0].clone();
        l.frames[0].push(mark);
        e.doc.extras.symbols.push(Symbol {
            rig: Default::default(),
            library_folder: None,
            mouth_poses: Default::default(),
            folders: Default::default(),
            perspective: Default::default(),
            id: 1,
            name: "recursive".into(),
            layers: vec![l].into(),
        });
        assert!(e.doc.validate().is_err());
    }
    #[test]
    fn library_protects_parent_instances_and_rejects_indirect_recursion() {
        let mut e = editor();
        e.doc.layers[0].frames[0].push(rect(0., [100, 80, 60, 255]));
        e.canvas.selected = vec![(0, 0)];
        e.convert_symbol();
        let a = e.doc.extras.symbols[0].id;
        e.doc.layers[0].frames[0].push(rect(150., [40, 80, 120, 255]));
        e.canvas.selected = vec![(0, 1)];
        e.convert_symbol();
        let b = e.doc.extras.symbols[1].id;
        e.enter_symbol(a);
        assert!(e.symbol_in_use(a));
        assert!(e.symbol_in_use(b));
        assert!(e.symbol_insert_allowed(b));
        e.doc.extras.symbols[1].layers[0].frames[0][0]
            .style
            .instance = Some(Instance {
            clock: Default::default(),
            symbol: a,
            matrix: [1., 0., 0., 1., 0., 0.],
            mode: Playback::Loop,
            first: 0,
            born: 0,
        });
        assert!(!e.symbol_insert_allowed(b));
        let before = e.doc.layers[0].frames[0].len();
        e.insert_symbol(b);
        assert_eq!(e.doc.layers[0].frames[0].len(), before);
    }
    #[test]
    fn non_round_caps_have_distinct_geometry_and_survive_native_save() {
        use geo::BoundingRect;
        let mut e = editor();
        let mut m = Mark {
            points: vec![[20., 50.], [100., 50.]].into(),
            width: 10.,
            ..Default::default()
        };
        m.style.start_cap = Cap::Flat;
        m.style.end_cap = Cap::Flat;
        let flat = crate::width::outline(&m).bounding_rect().unwrap();
        assert!((flat.min().x - 20.).abs() < 0.01);
        m.style.end_cap = Cap::Triangle;
        let triangle = crate::width::outline(&m).bounding_rect().unwrap();
        assert!(triangle.max().x > 107.);
        e.doc.layers[0].frames[0].push(m);
        let restored = crate::fla::decode(&crate::fla::encode(&e.doc).unwrap()).unwrap();
        assert!(restored.layers[0].frames[0][0].style.end_cap == Cap::Triangle);
    }
    #[test]
    #[ignore]
    fn benchmark_scene_features() {
        let mut e = editor();
        e.doc.layers[0].frames[0] = (0..200)
            .map(|i| rect((i % 20) as f32 * 10., [100, 120, 140, 255]))
            .collect();
        let start = std::time::Instant::now();
        for _ in 0..200 {
            std::hint::black_box(crate::features::resolve(&e.doc, 0, false));
        }
        eprintln!(
            "Resolve 200 plain vector shapes: {:?} per frame (200 iterations)",
            start.elapsed() / 200
        );
        let brush = vec![rect(50., [0, 0, 0, 255])];
        let base = e.doc.clone();
        let start = std::time::Instant::now();
        for _ in 0..20 {
            e.doc = base.clone();
            e.tone_paint(brush.clone(), false);
        }
        eprintln!(
            "Toon shading intersecting 200 rectangular fills: {:?} per stroke (20 iterations)",
            start.elapsed() / 20
        );
    }
}
