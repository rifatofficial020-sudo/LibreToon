//! View-only overlays and gesture-local snapping. Grid density is viewport-bounded.
use crate::{
    Editor,
    canvas::View,
    design::{Icon, button},
    i18n::tr,
};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
use serde::{Deserialize, Serialize};
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Grid {
    pub visible: bool,
    pub size: [f32; 2],
    pub subdivisions: u32,
    pub origin: [f32; 2],
    pub color: [u8; 4],
    pub dots: bool,
    pub stage_only: bool,
}
impl Default for Grid {
    fn default() -> Self {
        Self {
            visible: false,
            size: [64., 64.],
            subdivisions: 4,
            origin: [0., 0.],
            color: [80, 130, 180, 65],
            dots: false,
            stage_only: true,
        }
    }
}
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Snaps {
    pub guides: bool,
    pub horizontal: bool,
    pub vertical: bool,
    pub objects: bool,
    pub grid: bool,
}
impl Snaps {
    pub(crate) fn any(&self) -> bool {
        self.guides || self.horizontal || self.vertical || self.objects || self.grid
    }
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub grid: Grid,
    pub snaps: Snaps,
    pub tolerance: f32,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            grid: Default::default(),
            snaps: Default::default(),
            tolerance: 8.,
        }
    }
}
impl Config {
    pub fn normalize(&mut self) {
        for value in &mut self.grid.size {
            if !value.is_finite() {
                *value = 64.;
            }
            *value = value.clamp(1., 4096.);
        }
        for value in &mut self.grid.origin {
            if !value.is_finite() {
                *value = 0.;
            }
            *value = value.clamp(-1e6, 1e6);
        }
        self.grid.subdivisions = self.grid.subdivisions.clamp(1, 16);
        if !self.tolerance.is_finite() {
            self.tolerance = 8.;
        }
        self.tolerance = self.tolerance.clamp(2., 24.);
    }
}
#[derive(Default)]
pub struct State {
    pub(crate) video_bounds: Option<Rect>,
    pub draft: Option<Config>,
    targets: [Vec<f32>; 2],
    feedback: [Option<f32>; 2],
}
fn nearest(values: &[f32], point: f32) -> Option<f32> {
    let at = values.partition_point(|v| *v < point);
    values
        .get(at)
        .into_iter()
        .chain(at.checked_sub(1).and_then(|i| values.get(i)))
        .copied()
        .min_by(|a, b| (a - point).abs().total_cmp(&(b - point).abs()))
}
fn row(ui: &mut egui::Ui, icon: Icon, label: &str, on: bool) -> bool {
    button(ui, icon, label, label, on, true).clicked()
}
impl Editor {
    pub(crate) fn view_menu(&mut self, ui: &mut egui::Ui) {
        if row(ui, Icon::Ruler, "Mostrar reglas", self.studio.rulers) {
            self.studio.rulers = !self.studio.rulers;
            if self.studio.rulers {
                self.activate_tool(crate::canvas::Tool::Ruler);
            } else {
                self.cancel_guide_drag();
                if self.tool == crate::canvas::Tool::Ruler {
                    self.activate_tool(crate::canvas::Tool::Select);
                }
            }
        }
        if button(
            ui,
            Icon::Trash,
            "Borrar todas las guías",
            "Borrar todas las guías",
            false,
            !self.studio.guides.is_empty(),
        )
        .clicked()
        {
            self.clear_guides();
        }
        ui.separator();
        let mut changed = false;
        if row(
            ui,
            Icon::Grid,
            "Cuadrícula",
            self.studio.prefs.view.grid.visible,
        ) {
            self.studio.prefs.view.grid.visible = !self.studio.prefs.view.grid.visible;
            changed = true;
        }
        if button(
            ui,
            Icon::Settings,
            "Personalizar cuadrícula…",
            "Personalizar cuadrícula…",
            false,
            true,
        )
        .clicked()
        {
            let mut draft = self.studio.prefs.view.clone();
            draft.grid.visible = true;
            self.view_options.draft = Some(draft);
            ui.close();
        }
        let snap_menu = ui.menu_button(format!("        {}", tr("Ajustar")), |ui| {
            let snaps = &mut self.studio.prefs.view.snaps;
            for (icon, label, value) in [
                (Icon::Ruler, "Ajustar a guías", &mut snaps.guides),
                (
                    Icon::Line,
                    "Ajustar al eje horizontal del escenario",
                    &mut snaps.horizontal,
                ),
                (
                    Icon::Symmetry,
                    "Ajustar al eje vertical del escenario",
                    &mut snaps.vertical,
                ),
                (
                    Icon::Object,
                    "Ajustar al borde de otros elementos",
                    &mut snaps.objects,
                ),
                (Icon::Grid, "Ajustar a cuadrícula", &mut snaps.grid),
            ] {
                if row(ui, icon, label, *value) {
                    *value = !*value;
                    changed = true;
                }
            }
            ui.separator();
            ui.small(tr("Mantén Alt para omitir el ajuste temporalmente."));
        });
        crate::design::draw(
            ui.painter(),
            Rect::from_center_size(
                egui::pos2(
                    snap_menu.response.rect.left() + 12.,
                    snap_menu.response.rect.center().y,
                ),
                Vec2::splat(18.),
            ),
            Icon::Snap,
            crate::themes::palette().text,
        );
        if changed {
            if let Err(error) = self.studio.save() {
                self.status = error;
            }
        }
    }
    pub(crate) fn grid_settings_ui(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.view_options.draft.take() else {
            return;
        };
        let mut apply = false;
        let mut cancel = false;
        let response = egui::Modal::new(egui::Id::unique("grid-settings")).show(ctx, |ui| {
            ui.set_width(400.);
            ui.horizontal(|ui| {
                button(ui, Icon::Grid, "", "Cuadrícula", false, false);
                ui.heading(tr("Personalizar cuadrícula…"));
            });
            ui.separator();
            ui.checkbox(&mut draft.grid.visible, tr("Mostrar cuadrícula"));
            egui::Grid::new("grid-options-fields")
                .num_columns(2)
                .spacing(Vec2::new(16., 10.))
                .show(ui, |ui| {
                    ui.label(tr("Tamaño de celda"));
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut draft.grid.size[0])
                                .range(1. ..=4096.)
                                .prefix("X ")
                                .suffix(" px"),
                        );
                        ui.add(
                            egui::DragValue::new(&mut draft.grid.size[1])
                                .range(1. ..=4096.)
                                .prefix("Y ")
                                .suffix(" px"),
                        );
                    });
                    ui.end_row();
                    ui.label(tr("Subdivisiones"));
                    ui.add(egui::DragValue::new(&mut draft.grid.subdivisions).range(1..=16));
                    ui.end_row();
                    ui.label(tr("Origen"));
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut draft.grid.origin[0])
                                .range(-1e6..=1e6)
                                .prefix("X ")
                                .suffix(" px"),
                        );
                        ui.add(
                            egui::DragValue::new(&mut draft.grid.origin[1])
                                .range(-1e6..=1e6)
                                .prefix("Y ")
                                .suffix(" px"),
                        );
                    });
                    ui.end_row();
                    ui.label(tr("Color"));
                    let mut rgb = [
                        draft.grid.color[0],
                        draft.grid.color[1],
                        draft.grid.color[2],
                    ];
                    if ui.color_edit_button_srgb(&mut rgb).changed() {
                        draft.grid.color[..3].copy_from_slice(&rgb);
                    }
                    ui.end_row();
                    ui.label(tr("Opacidad"));
                    let mut opacity = draft.grid.color[3] as f32 / 2.55;
                    if ui
                        .add(egui::Slider::new(&mut opacity, 0. ..=100.).suffix("% "))
                        .changed()
                    {
                        draft.grid.color[3] = (opacity * 2.55).round() as u8;
                    }
                    ui.end_row();
                    ui.label(tr("Estilo"));
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut draft.grid.dots, false, tr("Líneas"));
                        ui.selectable_value(&mut draft.grid.dots, true, tr("Puntos"));
                    });
                    ui.end_row();
                });
            ui.checkbox(&mut draft.grid.stage_only, tr("Solo en el escenario"));
            ui.checkbox(&mut draft.snaps.grid, tr("Ajustar a cuadrícula"));
            ui.horizontal(|ui| {
                button(ui, Icon::Snap, "", "Ajustar", false, false);
                ui.label(tr("Distancia de ajuste"));
                ui.add(egui::Slider::new(&mut draft.tolerance, 2. ..=24.).suffix(" px"));
            });
            ui.small(tr(
                "El ajuste mantiene la misma distancia visual al cambiar el zoom.",
            ));
            ui.separator();
            ui.horizontal(|ui| {
                if button(
                    ui,
                    Icon::ResetColors,
                    "Restablecer",
                    "Restablecer",
                    false,
                    true,
                )
                .clicked()
                {
                    draft.grid = Default::default();
                    draft.grid.visible = true;
                }
                apply = button(ui, Icon::Grid, "Aplicar", "Aplicar", false, true).clicked();
                cancel = button(ui, Icon::Close, "Cancelar", "Cancelar", false, true).clicked();
            });
        });
        cancel |= response.should_close();
        draft.normalize();
        if apply {
            self.studio.prefs.view = draft;
            if let Err(error) = self.studio.save() {
                self.status = error;
            }
        } else if !cancel {
            self.view_options.draft = Some(draft);
        }
    }
    pub(crate) fn prepare_snapping(&mut self, selected: &[(usize, usize)]) {
        let config = &self.studio.prefs.view;
        let mut targets: [Vec<f32>; 2] = Default::default();
        self.view_options.feedback = [None, None];
        if config.snaps.guides {
            for &(horizontal, value) in &self.studio.guides {
                targets[usize::from(horizontal)].push(value);
            }
        }
        if config.snaps.vertical {
            targets[0].push(self.doc.width as f32 / 2.);
        }
        if config.snaps.horizontal {
            targets[1].push(self.doc.height as f32 / 2.);
        }
        if config.snaps.objects {
            let selected: std::collections::HashSet<_> = selected.iter().copied().collect();
            for (layer, data) in self
                .doc
                .layers
                .iter()
                .enumerate()
                .filter(|(_, l)| l.visible && l.options.mannequin.is_none())
            {
                for (index, mark) in crate::animation::marks(data, self.frame).iter().enumerate() {
                    if selected.contains(&(layer, index)) || mark.style.guide {
                        continue;
                    }
                    let corners: Vec<_> = if let Some(instance) = &mark.style.instance {
                        self.animation_instance_bounds(mark)
                            .map(|b| {
                                [
                                    b.left_top(),
                                    b.right_top(),
                                    b.right_bottom(),
                                    b.left_bottom(),
                                ]
                                .map(|p| crate::rig::point(instance.matrix, p))
                                .to_vec()
                            })
                            .unwrap_or_default()
                    } else {
                        crate::vector::flatten(mark, 0.25)
                    };
                    if corners.is_empty() {
                        continue;
                    }
                    let bounds = Rect::from_points(
                        &corners
                            .iter()
                            .map(|p| self.depth_point(*p, layer, self.layer))
                            .collect::<Vec<_>>(),
                    );
                    if bounds.is_finite() {
                        targets[0].extend([bounds.left(), bounds.center().x, bounds.right()]);
                        targets[1].extend([bounds.top(), bounds.center().y, bounds.bottom()]);
                    }
                }
            }
        }
        if config.snaps.objects {
            for bounds in self.video_snap_rects() {
                targets[0].extend([bounds.left(), bounds.center().x, bounds.right()]);
                targets[1].extend([bounds.top(), bounds.center().y, bounds.bottom()]);
            }
        }
        for axis in &mut targets {
            axis.retain(|x| x.is_finite());
            axis.sort_by(f32::total_cmp);
            axis.dedup();
        }
        self.view_options.targets = targets;
    }
    fn snap_axis(&self, axis: usize, anchors: &[f32], scale: f32) -> Option<(f32, f32)> {
        let config = &self.studio.prefs.view;
        let tolerance = config.tolerance / scale.abs().max(0.001);
        let mut best: Option<(f32, f32)> = None;
        for &anchor in anchors {
            let mut candidates = vec![];
            if let Some(value) = nearest(&self.view_options.targets[axis], anchor) {
                candidates.push(value);
            }
            if config.snaps.grid {
                let step = config.grid.size[axis] / config.grid.subdivisions as f32;
                let origin = config.grid.origin[axis];
                candidates.push(origin + ((anchor - origin) / step).round() * step);
            }
            for value in candidates {
                let delta = value - anchor;
                if delta.abs() <= tolerance && best.is_none_or(|(d, _)| delta.abs() < d.abs()) {
                    best = Some((delta, value));
                }
            }
        }
        best
    }
    pub(crate) fn snap_point(&mut self, point: Pos2, view: View, bypass: bool) -> Pos2 {
        self.view_options.feedback = [None, None];
        if bypass || !self.studio.prefs.view.snaps.any() {
            return point;
        }
        let mut out = point;
        for axis in 0..2 {
            if let Some((delta, value)) = self.snap_axis(axis, &[point[axis]], view.scale) {
                out[axis] += delta;
                self.view_options.feedback[axis] = Some(value);
            }
        }
        out
    }
    pub(crate) fn snap_translation(
        &mut self,
        bounds: Rect,
        delta: Vec2,
        view: View,
        bypass: bool,
    ) -> Vec2 {
        self.snap_translation_axes(bounds, delta, view, bypass, None)
    }

    pub(crate) fn snap_translation_axes(
        &mut self,
        bounds: Rect,
        delta: Vec2,
        view: View,
        bypass: bool,
        lock: Option<usize>,
    ) -> Vec2 {
        self.view_options.feedback = [None, None];
        if bypass || !self.studio.prefs.view.snaps.any() {
            return delta;
        }
        let mut out = delta;
        for axis in 0..2 {
            if lock.is_some_and(|active| active != axis) {
                continue;
            }
            let anchors = [
                bounds.min[axis] + delta[axis],
                bounds.center()[axis] + delta[axis],
                bounds.max[axis] + delta[axis],
            ];
            if let Some((d, value)) = self.snap_axis(axis, &anchors, view.scale) {
                out[axis] += d;
                self.view_options.feedback[axis] = Some(value);
            }
        }
        out
    }
    pub(crate) fn paint_snap_feedback(&self, p: &egui::Painter, area: Rect, view: View) {
        let span = area.size().length() / view.scale.max(0.001);
        let center = view.world(area.center());
        for (axis, value) in self.view_options.feedback.iter().enumerate() {
            if let Some(value) = value {
                let mut point = center;
                point[axis] = *value;
                let mut along = Vec2::ZERO;
                along[1 - axis] = span;
                p.line_segment(
                    [view.screen(point - along), view.screen(point + along)],
                    Stroke::new(1.5, Color32::from_rgba_unmultiplied(235, 70, 175, 190)),
                );
            }
        }
    }
    pub(crate) fn paint_grid(&self, p: &egui::Painter, area: Rect, view: View) {
        let grid = &self
            .view_options
            .draft
            .as_ref()
            .unwrap_or(&self.studio.prefs.view)
            .grid;
        if !grid.visible || grid.color[3] == 0 {
            return;
        }
        let corners = [
            area.left_top(),
            area.right_top(),
            area.right_bottom(),
            area.left_bottom(),
        ]
        .map(|p| view.world(p));
        let mut bounds = Rect::from_points(&corners);
        if grid.stage_only {
            bounds = bounds.intersect(Rect::from_min_max(
                Pos2::ZERO,
                Pos2::new(self.doc.width as f32, self.doc.height as f32),
            ));
        }
        if !bounds.is_positive() {
            return;
        }
        let mut steps = [0.; 2];
        for axis in 0..2 {
            let minor = grid.size[axis] / grid.subdivisions as f32;
            let minimum = if grid.dots { 12. } else { 6. };
            steps[axis] = if minor * view.scale >= minimum {
                minor
            } else {
                grid.size[axis]
                    * (minimum / (grid.size[axis] * view.scale).max(0.001))
                        .ceil()
                        .max(1.)
            };
        }
        if grid.dots {
            let count = (bounds.width() / steps[0] + 1.) * (bounds.height() / steps[1] + 1.);
            let factor = (count / 4096.).sqrt().ceil().max(1.);
            steps[0] *= factor;
            steps[1] *= factor;
        }
        let color = Color32::from_rgba_unmultiplied(
            grid.color[0],
            grid.color[1],
            grid.color[2],
            grid.color[3],
        );
        let ranges: [std::ops::RangeInclusive<i64>; 2] = std::array::from_fn(|axis| {
            let start = ((bounds.min[axis] - grid.origin[axis]) / steps[axis]).ceil() as i64;
            let end = ((bounds.max[axis] - grid.origin[axis]) / steps[axis]).floor() as i64;
            start..=end.min(start + 512)
        });
        if grid.dots {
            for x in ranges[0].clone() {
                for y in ranges[1].clone() {
                    let point = Pos2::new(
                        grid.origin[0] + x as f32 * steps[0],
                        grid.origin[1] + y as f32 * steps[1],
                    );
                    p.circle_filled(view.screen(point), 1., color);
                }
            }
        } else {
            for axis in 0..2 {
                for index in ranges[axis].clone() {
                    let value = grid.origin[axis] + index as f32 * steps[axis];
                    let mut a = bounds.min;
                    let mut b = bounds.max;
                    a[axis] = value;
                    b[axis] = value;
                    let major = ((value - grid.origin[axis]) / grid.size[axis])
                        .fract()
                        .abs()
                        < 0.001;
                    p.line_segment(
                        [view.screen(a), view.screen(b)],
                        Stroke::new(
                            if major { 1.2 } else { 0.6 },
                            if major {
                                color
                            } else {
                                color.gamma_multiply(0.5)
                            },
                        ),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn editor() -> Editor {
        let mut e = Editor::new(&eframe::CreationContext::_new_kittest(
            egui::Context::default(),
        ));
        e.doc = crate::fresh_document();
        e.studio.prefs.view = Config::default();
        e
    }
    fn view(scale: f32) -> View {
        View {
            center: Pos2::ZERO,
            stage_center: Pos2::ZERO,
            scale,
            angle: 0.,
        }
    }
    #[test]
    fn snapping_is_opt_in_zoom_independent_and_alt_can_bypass_it() {
        let mut e = editor();
        e.studio.guides = vec![(false, 100.), (true, 200.)];
        e.prepare_snapping(&[]);
        assert_eq!(
            e.snap_point(Pos2::new(103., 204.), view(1.), false),
            Pos2::new(103., 204.)
        );
        e.studio.prefs.view.snaps.guides = true;
        e.prepare_snapping(&[]);
        assert_eq!(
            e.snap_point(Pos2::new(103., 204.), view(1.), false),
            Pos2::new(100., 200.)
        );
        assert_eq!(
            e.snap_point(Pos2::new(103., 204.), view(4.), false),
            Pos2::new(103., 204.)
        );
        assert_eq!(
            e.snap_point(Pos2::new(103., 204.), view(1.), true),
            Pos2::new(103., 204.)
        );
        e.clear_guides();
        e.prepare_snapping(&[]);
        assert_eq!(
            e.snap_point(Pos2::new(103., 204.), view(1.), false),
            Pos2::new(103., 204.)
        );
    }
    #[test]
    fn object_snapping_excludes_selection_and_aligns_visible_bounds() {
        let mut e = editor();
        e.studio.prefs.view.snaps.objects = true;
        e.doc.layers[0].frames[0] = vec![
            crate::Mark {
                points: vec![[100., 100.], [200., 200.]].into(),
                rectangle: true,
                ..Default::default()
            },
            crate::Mark {
                points: vec![[300., 100.], [400., 200.]].into(),
                rectangle: true,
                ..Default::default()
            },
        ]
        .into();
        e.prepare_snapping(&[(0, 0)]);
        let delta = e.snap_translation(
            Rect::from_min_max(Pos2::new(100., 100.), Pos2::new(200., 200.)),
            Vec2::new(103., 40.),
            view(1.),
            false,
        );
        assert_eq!(delta, Vec2::new(100., 40.));
        e.doc.layers[0].visible = false;
        e.prepare_snapping(&[]);
        assert_eq!(
            e.snap_point(Pos2::new(303., 105.), view(1.), false),
            Pos2::new(303., 105.)
        );
    }
    #[test]
    fn grid_and_stage_axes_snap_separately_and_preferences_roundtrip() {
        let mut e = editor();
        e.studio.prefs.view.snaps.vertical = true;
        e.studio.prefs.view.snaps.horizontal = true;
        e.prepare_snapping(&[]);
        assert_eq!(
            e.snap_point(Pos2::new(963., 537.), view(1.), false),
            Pos2::new(960., 540.)
        );
        e.studio.prefs.view.snaps = Snaps {
            grid: true,
            ..Default::default()
        };
        e.studio.prefs.view.grid.size = [40., 60.];
        e.studio.prefs.view.grid.subdivisions = 2;
        e.prepare_snapping(&[]);
        assert_eq!(
            e.snap_point(Pos2::new(42., 58.), view(1.), false),
            Pos2::new(40., 60.)
        );
        let restored: Config =
            serde_json::from_slice(&serde_json::to_vec(&e.studio.prefs.view).unwrap()).unwrap();
        assert_eq!(restored.grid.size, [40., 60.]);
        assert!(restored.snaps.grid);
    }
    #[test]
    fn dense_grid_rendering_is_bounded_and_does_not_modify_artwork() {
        let mut e = editor();
        e.studio.prefs.view.grid.visible = true;
        e.studio.prefs.view.grid.size = [1., 1.];
        e.studio.prefs.view.grid.subdivisions = 16;
        let before = serde_json::to_vec(&e.doc).unwrap();
        let ctx = egui::Context::default();
        for dots in [false, true] {
            e.studio.prefs.view.grid.dots = dots;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000., 800.))),
                    ..Default::default()
                },
                |ui| {
                    e.paint_grid(ui.painter(), ui.available_rect_before_wrap(), view(0.05));
                },
            );
            assert!(output.shapes.len() < 6000);
            output.textures_delta.clear();
        }
        assert_eq!(before, serde_json::to_vec(&e.doc).unwrap());
    }
}
