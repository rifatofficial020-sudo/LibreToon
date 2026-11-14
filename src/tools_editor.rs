use crate::{
    Editor,
    canvas::{Canvas, Tool},
    document::Mark,
};
use eframe::egui::{self, Color32, Pos2};

impl Editor {
    pub fn sync_selected_style(&mut self, ids: &[(usize, usize)]) {
        if self.style_selection == ids && self.style_frame == self.frame {
            return;
        }
        self.style_transaction = false;
        self.style_selection = ids.to_vec();
        self.style_frame = self.frame;
        if let Some(&(l, m)) = ids.first() {
            let evaluated = crate::animation::marks(&self.doc.layers[l], self.frame);
            let mark = &evaluated[m];
            self.feature.gradient_on = mark.style.gradient.is_some();
            if let Some(g) = &mark.style.gradient {
                self.feature.gradient = g.clone();
            } else {
                self.feature
                    .gradient
                    .fit(crate::vector::control_bounds(mark));
            }
            self.feature.start_cap = mark.style.start_cap;
            self.feature.end_cap = mark.style.end_cap;
            self.fill_enabled = mark.fill.is_some();
            self.stroke_enabled = mark.stroke;
            if let Some(c) = mark.fill {
                self.fill_color = Color32::from_rgb(c[0], c[1], c[2]);
                self.active_fill = true;
            }
            if mark.stroke {
                let c = mark.color;
                self.color = Color32::from_rgb(c[0], c[1], c[2]);
                self.width = mark.width;
                self.width_profile = mark.width_profile.clone();
                self.width_preset = crate::width::Preset::Custom;
                if mark.fill.is_none() {
                    self.active_fill = false;
                }
            }
        }
    }
    pub fn change_selected_style(&mut self, fill: bool, stroke: bool, width: bool, profile: bool) {
        if self.canvas.selected.is_empty() {
            return;
        }
        if !self.style_transaction {
            let mut state = std::mem::take(&mut self.canvas);
            self.checkpoint();
            self.materialize_fragments(&mut state);
            self.canvas = state;
            self.style_transaction = true;
        }
        for &(l, m) in &self.canvas.selected {
            let mark = &mut self.doc.layers[l].frames[self.frame][m];
            if fill && mark.fill.is_some() {
                mark.fill = Some(self.fill_color.to_array());
                mark.style.palette_fill = None;
                mark.style.gradient = None;
                self.feature.gradient_on = false;
                mark.style.shade = 0;
                mark.style.light = 0;
            }
            if stroke && mark.stroke {
                mark.color = self.color.to_array();
                mark.style.palette_stroke = None;
            }
            if width && mark.stroke {
                mark.width = self.width;
            }
            if profile && mark.stroke {
                mark.width_profile = self.width_profile.clone();
            }
        }
        self.recover();
    }
    pub fn selection_properties_ui(&mut self, ui: &mut egui::Ui) {
        crate::design::category(ui, "SELECCIÓN");
        ui.strong(format!(
            "{} · {}",
            crate::i18n::tr("Selección"),
            self.canvas.selected.len()
        ));
        ui.add_space(12.);
        let fill = self
            .canvas
            .selected
            .iter()
            .any(|&(l, m)| self.doc.layers[l].frames[self.frame][m].fill.is_some());
        let stroke = self
            .canvas
            .selected
            .iter()
            .any(|&(l, m)| self.doc.layers[l].frames[self.frame][m].stroke);
        if fill {
            crate::design::category(ui, "RELLENO");
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Relleno"));
                if ui.color_edit_button_srgba(&mut self.fill_color).changed() {
                    self.change_selected_style(true, false, false, false);
                }
            });
        }
        if stroke {
            crate::design::category(ui, "CONTORNO");
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Contorno"));
                if ui.color_edit_button_srgba(&mut self.color).changed() {
                    self.change_selected_style(false, true, false, false);
                }
            });
            if ui
                .add(
                    egui::Slider::new(&mut self.width, 0.1..=100.)
                        .text(crate::i18n::tr("Grosor"))
                        .suffix(crate::i18n::tr(" px")),
                )
                .changed()
            {
                self.change_selected_style(false, false, true, false);
            }
            self.width_profile_ui(ui);
        }
    }
    pub fn cut_area_selection(&mut self, state: &mut Canvas, points: &[Pos2], add: bool) {
        if points.len() < 3 {
            return;
        }
        let cutter = crate::editing::area(points);
        self.select_fragments(state, add, |mark| crate::editing::split_area(mark, &cutter));
    }

    pub(crate) fn select_fragments(
        &self,
        state: &mut Canvas,
        add: bool,
        split: impl Fn(&Mark) -> (Vec<Mark>, Vec<Mark>),
    ) {
        if !add {
            state.clear_selection();
        }
        for (l, layer) in
            self.doc.layers.iter().enumerate().filter(|(_, layer)| {
                layer.visible && !layer.options.locked && !layer.options.raster
            })
        {
            for (m, mark) in layer.frames[self.frame].iter().enumerate() {
                let id = (l, m);
                if state.selected.contains(&id) {
                    continue;
                }
                let (outside, inside) = split(mark);
                if !inside.is_empty() {
                    state.selected.push(id);
                    if !outside.is_empty() {
                        state.partial.insert(id, (outside, inside));
                    }
                }
            }
        }
        state.pivot = None;
    }
    pub fn erase_segment(&mut self, a: Pos2, b: Pos2) -> bool {
        let areas = crate::editing::eraser_area(a, b, self.eraser_width * 0.5);
        let mut marks = self.doc.layers[self.layer].frames[self.frame].to_vec();
        let mut changed = false;
        for area in areas {
            let mut next = vec![];
            for mark in marks {
                if mark.style.instance.is_some() {
                    next.push(mark);
                    continue;
                }
                let (outside, inside) = crate::editing::split_area(&mark, &area);
                if inside.is_empty() {
                    next.push(mark);
                } else {
                    changed = true;
                    next.extend(outside);
                }
            }
            marks = next;
        }
        if changed {
            self.doc.layers[self.layer].frames[self.frame] = marks.into();
        }
        changed
    }
    pub(crate) fn sample_at(
        &mut self,
        point: Pos2,
        tolerance: f32,
        picker: Option<bool>,
    ) -> Option<Mark> {
        for (layer_index, layer) in self.doc.layers.iter().enumerate().rev().filter(|(_, l)| {
            l.visible && crate::folder_groups::layer_active(&self.doc, l, self.frame)
        }) {
            let point = self.depth_point(point, self.layer, layer_index);
            let marks = crate::animation::marks(layer, self.frame);
            for mark in marks
                .iter()
                .rev()
                .filter(|mark| crate::features::object_visible(layer, mark, self.frame))
            {
                if mark.style.media_image.is_some() {
                    if let Some([r, g, b, _]) = self.doc.extras.media.image_pixel(mark, point) {
                        let color = Color32::from_rgb(r, g, b);
                        if picker.is_none() {
                            self.color = color;
                            self.stroke_enabled = true;
                        }
                        self.apply_picker_color(color, picker.unwrap_or(true));
                        return None;
                    }
                    continue;
                }
                let stroke = crate::canvas::stroke_hit(mark, point, tolerance);
                if stroke {
                    let c = mark.color;
                    let color = Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
                    if let Some(fill) = picker {
                        self.apply_picker_color(color, fill);
                        return None;
                    }
                    self.color = color;
                    self.fill_color = color;
                    self.stroke_enabled = true;
                    self.active_fill = false;
                    self.width = mark.width;
                    self.width_profile = mark.width_profile.clone();
                    self.width_preset = crate::width::Preset::Custom;
                    return None;
                }
                if crate::vector::contains_fill(mark, point) {
                    let c = mark
                        .style
                        .gradient
                        .as_ref()
                        .map_or_else(|| mark.fill.unwrap(), |g| g.color(point));
                    self.apply_picker_color(
                        Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]),
                        picker.unwrap_or(true),
                    );
                    return picker.is_none().then(|| mark.clone());
                }
            }
        }
        None
    }
    pub fn wand_select(&mut self, state: &mut Canvas, point: Pos2, add: bool) {
        let found = self
            .doc
            .layers
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, l)| l.visible && !l.options.locked)
            .find_map(|(l, layer)| {
                layer.frames[self.frame]
                    .iter()
                    .enumerate()
                    .rev()
                    .find_map(|(m, mark)| {
                        if crate::canvas::stroke_hit(mark, point, 3.) {
                            Some((l, m, false))
                        } else if crate::vector::contains_fill(mark, point) {
                            Some((l, m, true))
                        } else {
                            None
                        }
                    })
            });
        if !add {
            state.clear_selection();
        }
        let Some((l, m, target_fill)) = found else {
            return;
        };
        let reference = &self.doc.layers[l].frames[self.frame][m];
        let color = if target_fill {
            reference.fill.unwrap()
        } else {
            reference.color
        };
        let mut candidates: Vec<_> = self.doc.layers[l].frames[self.frame]
            .iter()
            .enumerate()
            .filter_map(|(index, mark)| {
                let c = if target_fill {
                    mark.fill
                } else {
                    mark.stroke.then_some(mark.color)
                }?;
                let diff = c
                    .iter()
                    .zip(color)
                    .map(|(a, b)| (*a as f32 - b as f32).powi(2))
                    .sum::<f32>()
                    .sqrt()
                    / 2.;
                (diff <= self.wand_tolerance).then_some(index)
            })
            .collect();
        candidates.sort_unstable_by(|a, b| b.cmp(a));
        for m in candidates {
            let id = self.separate_component(state, (l, m), !target_fill);
            if !state.selected.contains(&id) {
                state.selected.push(id);
            }
        }
    }
    pub fn split_fill_face(
        &mut self,
        state: &mut Canvas,
        id: (usize, usize),
        point: Pos2,
    ) -> (usize, usize) {
        use geo::{BooleanOps, Buffer, Contains};
        let mark = &self.doc.layers[id.0].frames[self.frame][id.1];
        let Some(color) = mark.fill else {
            return id;
        };
        let bounds = crate::vector::control_bounds(mark);
        let barriers: Vec<_> = self.doc.layers[id.0].frames[self.frame]
            .iter()
            .skip(id.1 + 1)
            .filter(|m| m.stroke && bounds.intersects(crate::vector::control_bounds(m)))
            .map(|m| {
                let points = crate::vector::flatten(m, 0.025);
                geo::LineString::from(
                    points
                        .iter()
                        .map(|p| (p.x as f64, p.y as f64))
                        .collect::<Vec<_>>(),
                )
                .buffer(0.002)
            })
            .collect();
        if barriers.is_empty() {
            return id;
        }
        let original = crate::vector::polygon(mark);
        let ink = geo::algorithm::bool_ops::unary_union(&barriers);
        let faces = original.difference(&ink);
        if faces.0.len() < 2 {
            return id;
        }
        let Some(face) = faces
            .0
            .into_iter()
            .find(|face| face.contains(&geo::Point::new(point.x as f64, point.y as f64)))
        else {
            return id;
        };
        // Expand across the microscopic construction seam, then subtract from the
        // original fill so no ink area is lost or left as an antialiasing crack.
        let selected = face
            .buffer(0.004)
            .intersection(&geo::MultiPolygon(vec![original.clone()]));
        let rest = geo::MultiPolygon(vec![original]).difference(&selected);
        if rest.0.is_empty() {
            return id;
        }
        let mut parts = crate::features::styled(rest, color, &mark.style);
        let chosen = parts.len();
        parts.extend(crate::features::styled(selected, color, &mark.style));
        let count = parts.len();
        self.checkpoint();
        let marks = &mut self.doc.layers[id.0].frames[self.frame];
        marks.remove(id.1);
        for (i, m) in parts.into_iter().enumerate() {
            marks.insert(id.1 + i, m);
        }
        for (l, m) in &mut state.selected {
            if *l == id.0 && *m > id.1 {
                *m += count - 1;
            }
        }
        self.recover();
        (id.0, id.1 + chosen)
    }
    pub fn activate_tool(&mut self, tool: Tool) {
        let tool = if tool == Tool::Airbrush {
            Tool::Brush
        } else {
            tool
        };
        if self.tool == tool {
            return;
        }
        if !matches!(tool, Tool::Select | Tool::Object | Tool::Transform) {
            self.folder_groups.selected = None;
        }
        self.finish_rig_gesture();
        if self.drawing.reference_gizmo.take().is_some() {
            self.recover();
        }
        self.drawing.reference_drag = None;
        self.drawing.reference_orbit = None;
        if matches!(
            tool,
            Tool::Brush
                | Tool::Airbrush
                | Tool::SketchBrush
                | Tool::FillBrush
                | Tool::ShadeBrush
                | Tool::LightBrush
                | Tool::VectorBrush
                | Tool::Pen
                | Tool::Rectangle
                | Tool::Line
                | Tool::Ellipse
                | Tool::Polygon
                | Tool::Eraser
                | Tool::Bucket
        ) {
            self.canvas.clear_selection();
            self.style_selection.clear();
            self.style_transaction = false;
            if tool == Tool::VectorBrush || tool == Tool::Line {
                self.stroke_enabled = true;
            }
            if tool == Tool::Brush || tool == Tool::Bucket {
                self.fill_enabled = true;
            }
        }
        if self.doc.layers[self.layer].options.mannequin.is_some()
            && matches!(
                tool,
                Tool::Text
                    | Tool::Brush
                    | Tool::Airbrush
                    | Tool::SketchBrush
                    | Tool::VectorBrush
                    | Tool::Pen
                    | Tool::Rectangle
                    | Tool::Line
                    | Tool::Ellipse
                    | Tool::Polygon
                    | Tool::FillBrush
                    | Tool::ShadeBrush
                    | Tool::LightBrush
                    | Tool::Bucket
                    | Tool::Eraser
            )
        {
            if let Some(l) = self
                .doc
                .layers
                .iter()
                .position(|l| l.options.mannequin.is_none() && l.visible && !l.options.locked)
            {
                self.layer = l;
            }
        }
        if tool == Tool::SketchBrush {
            self.color = Color32::from_rgb(125, 128, 139);
            self.fill_color = self.color;
        }
        self.drawing.text.editing = None;
        self.drawing.text.canvas_edit = false;
        self.drawing.text.box_drag = None;
        self.drawing.text.transaction = false;
        if tool == Tool::Ruler {
            self.studio.rulers = true;
        }
        if tool == Tool::Perspective {
            self.doc.extras.perspective.enabled = true;
        }
        self.feature.library = false;
        self.tool = tool;
    }
    pub(crate) fn preview_stroke_segment(
        &self,
        id: (usize, usize),
        point: Pos2,
    ) -> Option<(Vec<Mark>, Vec<Mark>)> {
        let marks = crate::animation::marks(&self.doc.layers[id.0], self.frame);
        let mark = &marks[id.1];
        let others: Vec<Mark> = marks
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != id.1)
            .map(|(_, m)| m.clone())
            .collect();
        let mut parts = crate::editing::crossing_parts(mark, &others);
        if parts.len() < 2 {
            return None;
        }
        let closest = parts
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                crate::canvas::axis_distance(a, point)
                    .total_cmp(&crate::canvas::axis_distance(b, point))
            })
            .map(|(i, _)| i)?;
        let selected = parts.remove(closest);
        Some((parts, vec![selected]))
    }
    pub(crate) fn prepare_paint_key(&mut self, layer: usize, frame: usize) {
        if self.studio.prefs.auto_key_paint && !self.doc.layers[layer].key(frame) {
            let pose = crate::animation::marks(&self.doc.layers[layer], frame);
            let data = &mut self.doc.layers[layer];
            data.init_keys();
            data.frames[frame] = pose;
            data.options.keys[frame] = true;
        }
    }
    pub fn commit_paint(&mut self, layer: usize, frame: usize, marks: Vec<Mark>) {
        if self.doc.layers[layer].options.mannequin.is_some()
            || crate::post::is_adjustment(&self.doc.layers[layer])
        {
            return;
        }
        if marks.is_empty() {
            return;
        }
        self.materialize_motion(&[(layer, 0)]);
        self.prepare_paint_key(layer, frame);
        let composed = crate::editing::compose(&self.doc.layers[layer].frames[frame], marks);
        self.doc.layers[layer].frames[frame] = composed.into();
    }
    pub fn select_connected_object(&self, state: &mut Canvas, seed: (usize, usize), add: bool) {
        self.select_connected_marks(state, seed, add, false);
    }

    pub(crate) fn select_connected_strokes(&self, state: &mut Canvas, seed: (usize, usize)) {
        self.select_connected_marks(state, seed, false, true);
    }

    fn select_connected_marks(
        &self,
        state: &mut Canvas,
        seed: (usize, usize),
        add: bool,
        strokes_only: bool,
    ) {
        use geo::{BooleanOps, Intersects};
        let marks = &self.doc.layers[seed.0].frames[self.frame];
        let ink = |mark: &Mark| {
            let mut area = if !strokes_only && mark.fill.is_some() {
                geo::MultiPolygon(vec![crate::vector::polygon(mark)])
            } else {
                geo::MultiPolygon(vec![])
            };
            if mark.stroke {
                area = area.union(&crate::width::outline(mark));
            }
            area
        };
        let mut visited = vec![false; marks.len()];
        visited[seed.1] = true;
        let mut stack = vec![seed.1];
        let mut prepared = std::collections::HashMap::new();
        while let Some(index) = stack.pop() {
            let bounds =
                crate::vector::control_bounds(&marks[index]).expand(marks[index].width * 2. + 0.1);
            let region = prepared
                .entry(index)
                .or_insert_with(|| ink(&marks[index]))
                .clone();
            for j in 0..marks.len() {
                if visited[j]
                    || (strokes_only && !marks[j].stroke)
                    || !crate::features::object_visible(
                        &self.doc.layers[seed.0],
                        &marks[j],
                        self.frame,
                    )
                    || !bounds.intersects(
                        crate::vector::control_bounds(&marks[j]).expand(marks[j].width * 2. + 0.1),
                    )
                {
                    continue;
                }
                let candidate = prepared.entry(j).or_insert_with(|| ink(&marks[j]));
                if region.intersects(candidate) {
                    visited[j] = true;
                    stack.push(j);
                }
            }
        }
        if !add {
            state.clear_selection();
        }
        for (index, yes) in visited.into_iter().enumerate() {
            if yes && !state.selected.contains(&(seed.0, index)) {
                state.selected.push((seed.0, index));
            }
        }
    }
    pub fn tool_options_ui(&mut self, ui: &mut egui::Ui) {
        self.studio_tool_options(ui);
        match self.tool {
            Tool::Zoom => {
                ui.radio_value(&mut self.zoom_out, false, crate::i18n::tr("Acercar (+)"));
                ui.radio_value(&mut self.zoom_out, true, crate::i18n::tr("Alejar (−)"));
            }
            Tool::Polygon => {
                ui.add(
                    egui::Slider::new(&mut self.polygon_sides, 3..=32)
                        .text(crate::i18n::tr("Lados / puntas")),
                );
                ui.checkbox(&mut self.polygon_star, crate::i18n::tr("Estrella"));
                if self.polygon_star {
                    ui.add(
                        egui::Slider::new(&mut self.star_ratio, 0.05..=0.95)
                            .text(crate::i18n::tr("Radio interior")),
                    );
                }
            }
            Tool::Wand => {
                ui.add(
                    egui::Slider::new(&mut self.wand_tolerance, 0. ..=255.)
                        .text(crate::i18n::tr("Tolerancia")),
                );
                ui.label(crate::i18n::tr(
                    "Selecciona los rellenos o contornos de color parecido en la capa.",
                ));
            }
            _ => {}
        }
    }
}
#[cfg(test)]
mod auto_key_tests {
    use super::*;
    fn fixture(enabled: bool) -> Editor {
        let mut e = Editor::new(&eframe::CreationContext::_new_kittest(
            egui::Context::default(),
        ));
        e.doc = crate::fresh_document();
        e.recovery = std::env::temp_dir().join(format!(
            "auto-key-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ));
        let original: crate::shared::SharedVec<crate::Mark> = vec![crate::Mark {
            points: vec![[10., 10.], [50., 50.]].into(),
            ..Default::default()
        }]
        .into();
        let layer = &mut e.doc.layers[0];
        layer.init_keys();
        for f in 0..12 {
            layer.frames[f] = original.clone();
        }
        layer.options.keys[12] = true;
        e.frame = 5;
        e.studio.prefs.auto_key_paint = enabled;
        e
    }
    #[test]
    fn automatic_paint_key_preserves_previous_exposure_and_next_key_and_undo() {
        let mut e = fixture(true);
        let before = serde_json::to_vec(&e.doc).unwrap();
        let original = e.doc.layers[0].frames[0].clone();
        e.checkpoint();
        e.commit_paint(
            0,
            5,
            vec![crate::Mark {
                points: vec![[100., 10.], [110., 20.]].into(),
                ..Default::default()
            }],
        );
        e.recover();
        assert!(e.doc.layers[0].key(5));
        assert_eq!(e.doc.layers[0].frames.len(), 24);
        for f in 0..5 {
            assert_eq!(e.doc.layers[0].frames[f].identity(), original.identity());
        }
        for f in 5..12 {
            assert_eq!(e.doc.layers[0].frames[f].len(), 2);
        }
        assert!(e.doc.layers[0].frames[12].is_empty());
        e.checkpoint();
        e.commit_paint(
            0,
            5,
            vec![crate::Mark {
                points: vec![[120., 10.], [130., 20.]].into(),
                ..Default::default()
            }],
        );
        e.recover();
        assert_eq!(
            e.doc.layers[0].options.keys.iter().filter(|k| **k).count(),
            3
        );
        e.history(false);
        e.history(false);
        assert_eq!(serde_json::to_vec(&e.doc).unwrap(), before);
    }
    #[test]
    fn disabled_auto_key_keeps_existing_exposure_editing_and_empty_strokes_do_nothing() {
        let mut e = fixture(false);
        e.checkpoint();
        e.commit_paint(
            0,
            5,
            vec![crate::Mark {
                points: vec![[100., 10.], [110., 20.]].into(),
                ..Default::default()
            }],
        );
        e.recover();
        assert!(!e.doc.layers[0].key(5));
        assert_eq!(e.doc.layers[0].frames[0].len(), 2);
        let mut e = fixture(true);
        e.commit_paint(0, 5, vec![]);
        assert!(!e.doc.layers[0].key(5));
        let prefs: crate::studio::Preferences = serde_json::from_str("{}").unwrap();
        assert!(!prefs.auto_key_paint);
    }
}
