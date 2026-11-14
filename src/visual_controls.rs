use crate::{
    Editor,
    design::{ACCENT, Icon, button},
    features::Stop,
};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
fn timing_direction(direction: Vec2, rail: Vec2) -> Vec2 {
    if direction.length_sq() > 1e-8 {
        direction.normalized()
    } else {
        rail.normalized()
    }
}
fn motion_curve_hit(
    curve: &crate::animation::Curve,
    a: Pos2,
    b: Pos2,
    pointer: Pos2,
    view: crate::canvas::View,
) -> Option<f32> {
    let mut best = (8., 0.);
    let mut previous = view.screen(curve.point(a, b, 0.));
    for i in 1..=64 {
        let next = view.screen(curve.point(a, b, i as f32 / 64.));
        let segment = next - previous;
        let u = ((pointer - previous).dot(segment) / segment.length_sq().max(1e-8)).clamp(0., 1.);
        let distance = pointer.distance(previous + segment * u);
        if distance < best.0 {
            best = (distance, (i as f32 - 1. + u) / 64.);
        }
        previous = next;
    }
    (best.0 < 8. && (0.02..0.98).contains(&best.1)).then_some(best.1)
}

impl Editor {
    fn motion_path(&self, ids: &[(usize, usize)]) -> Option<(usize, Pos2, Pos2, [Pos2; 2])> {
        let &(l, m) = ids.first()?;
        let layer = self.doc.layers.get(l)?;
        let index = layer
            .options
            .tweens
            .iter()
            .position(|s| self.frame >= s.start && self.frame <= s.end)?;
        let t = &layer.options.tweens[index];
        let a = crate::vector::control_bounds(layer.frames[t.start].get(m)?).center();
        let b = crate::vector::control_bounds(layer.frames[t.end].get(m)?).center();
        let rail = if a.distance(b) < 0.1 {
            eframe::egui::vec2(160., 0.)
        } else {
            b - a
        };
        let origin = if a.distance(b) < 0.1 {
            a - rail / 2.
        } else {
            a
        };
        Some((
            index,
            a,
            b,
            [
                origin
                    + timing_direction(
                        rail / 3. + crate::vector::pos(t.curve.path[0]).to_vec2(),
                        rail,
                    ) * rail.length()
                        * t.curve.pace[0],
                origin + rail
                    - timing_direction(
                        rail / 3. - crate::vector::pos(t.curve.path[1]).to_vec2(),
                        rail,
                    ) * rail.length()
                        * t.curve.pace[1],
            ],
        ))
    }

    pub fn visual_canvas_input(
        &mut self,
        ui: &egui::Ui,
        area: Rect,
        view: crate::canvas::View,
        ids: &[(usize, usize)],
    ) -> bool {
        if self.playing
            || !matches!(
                self.tool,
                crate::canvas::Tool::Select
                    | crate::canvas::Tool::Transform
                    | crate::canvas::Tool::Bucket
            )
        {
            return false;
        }
        let Some(screen) = ui.input(|i| i.pointer.interact_pos()) else {
            return false;
        };
        let path = self.motion_path(ids);
        let gradient = ids
            .first()
            .and_then(|&(l, m)| self.doc.layers[l].frames[self.frame].get(m))
            .and_then(|m| m.style.gradient.clone());
        if ui.input(|i| i.pointer.primary_pressed())
            && area.contains(screen)
            && ui.ctx().layer_id_at(screen) == Some(ui.layer_id())
        {
            if let Some((_, _, _, points)) = path {
                if let Some(i) = points
                    .iter()
                    .position(|p| view.screen(*p).distance(screen) < 8.)
                {
                    self.checkpoint();
                    self.animation.path_handle = Some(i);
                }
            }
            if self.animation.path_handle.is_none()
                && self.animation.path_bend.is_none()
                && matches!(
                    self.tool,
                    crate::canvas::Tool::Select | crate::canvas::Tool::Transform
                )
            {
                if let Some((index, a, b, _)) = path {
                    let over_art = ids.iter().any(|&(l, m)| {
                        let marks = crate::animation::marks(&self.doc.layers[l], self.frame);
                        marks.get(m).is_some_and(|mark| {
                            crate::vector::contains_fill(mark, view.world(screen))
                                || crate::canvas::stroke_hit(
                                    mark,
                                    view.world(screen),
                                    1. / view.scale,
                                )
                        })
                    });
                    let layer = ids[0].0;
                    let curve = &self.doc.layers[layer].options.tweens[index].curve;
                    if !over_art && a.distance(b) >= 0.1 {
                        if let Some(t) = motion_curve_hit(curve, a, b, screen, view) {
                            let original = curve.path;
                            self.checkpoint();
                            self.animation.path_bend = Some(crate::animation::MotionBend {
                                layer,
                                span: index,
                                start: view.world(screen),
                                original,
                                t,
                            });
                        }
                    }
                }
            }
            if self.animation.path_handle.is_none() && self.animation.path_bend.is_none() {
                if let Some(g) = &gradient {
                    if let Some(i) = [g.start, g.end]
                        .iter()
                        .position(|p| view.screen(crate::vector::pos(*p)).distance(screen) < 8.)
                    {
                        self.checkpoint();
                        self.materialize_motion(ids);
                        self.animation.gradient_drag = Some(i);
                    }
                }
            }
        }
        let handled = self.animation.path_handle.is_some()
            || self.animation.path_bend.is_some()
            || self.animation.gradient_drag.is_some();
        if ui.input(|i| i.pointer.primary_down()) {
            if let (Some(i), Some((index, a, b, _))) = (self.animation.path_handle, path) {
                let stationary = a.distance(b) < 0.1;
                let rail = if stationary {
                    Vec2::new(160., 0.)
                } else {
                    b - a
                };
                let origin = if stationary { a - rail / 2. } else { a };
                let world = view.world(screen);
                let curve = &mut self.doc.layers[ids[0].0].options.tweens[index].curve;
                curve.pace[i] = if i == 0 {
                    world.distance(origin) / rail.length()
                } else {
                    world.distance(origin + rail) / rail.length()
                }
                .clamp(0., 1.);
                curve.edit_timing();
            }
            if let Some(bend) = self.animation.path_bend {
                if let Some(span) = self
                    .doc
                    .layers
                    .get_mut(bend.layer)
                    .and_then(|l| l.options.tweens.get_mut(bend.span))
                {
                    span.curve
                        .bend_path(bend.t, view.world(screen) - bend.start, bend.original);
                }
            }
            if let Some(i) = self.animation.gradient_drag {
                for &(l, m) in ids {
                    if let Some(g) = self.doc.layers[l].frames[self.frame][m]
                        .style
                        .gradient
                        .as_mut()
                    {
                        if i == 0 {
                            g.start = crate::vector::xy(view.world(screen));
                        } else {
                            g.end = crate::vector::xy(view.world(screen));
                        }
                        self.feature.gradient = g.clone();
                    }
                }
            }
        } else if handled {
            self.animation.path_handle = None;
            self.animation.path_bend = None;
            self.animation.gradient_drag = None;
            self.recover();
        }
        handled
    }
    pub fn visual_canvas_overlays(
        &self,
        p: &egui::Painter,
        view: crate::canvas::View,
        ids: &[(usize, usize)],
    ) {
        if self.playing {
            return;
        }
        if let Some((index, a, b, handles)) = self.motion_path(ids) {
            let curve = &self.doc.layers[ids[0].0].options.tweens[index].curve;
            let span = &self.doc.layers[ids[0].0].options.tweens[index];
            let frames = span.end - span.start + 1;
            let stationary = a.distance(b) < 0.1;
            if !stationary {
                let line = (0..=64)
                    .map(|i| view.screen(curve.point(a, b, i as f32 / 64.)))
                    .collect();
                p.add(egui::Shape::line(
                    line,
                    Stroke::new(1.5, crate::themes::accent()),
                ));
                if let Some(pointer) = p.ctx().input(|i| i.pointer.hover_pos()) {
                    if let Some(t) = motion_curve_hit(curve, a, b, pointer, view) {
                        p.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                        p.circle_filled(
                            view.screen(curve.point(a, b, t)),
                            5.,
                            crate::themes::accent(),
                        );
                    }
                }
            }
            let shown_frames = frames.clamp(2, 256);
            for i in 0..shown_frames {
                let t = i as f32 / (shown_frames - 1) as f32;
                let q = if stationary {
                    (a - Vec2::X * 80.).lerp(a + Vec2::X * 80., curve.time(t))
                } else {
                    curve.point(a, b, curve.time(t))
                };
                p.circle_filled(view.screen(q), 2.5, Color32::from_rgb(180, 120, 250));
            }
            let active = (self.frame - span.start) as f32 / (frames - 1).max(1) as f32;
            let current = if stationary {
                (a - Vec2::X * 80.).lerp(a + Vec2::X * 80., curve.time(active))
            } else {
                curve.point(a, b, curve.time(active))
            };
            p.circle_filled(view.screen(current), 4., Color32::WHITE);
            for (i, h) in handles.iter().enumerate() {
                p.line_segment(
                    [view.screen(if i == 0 { a } else { b }), view.screen(*h)],
                    Stroke::new(1., ACCENT),
                );
                p.circle_filled(view.screen(*h), 5., ACCENT);
                if p.ctx().input(|input| {
                    input
                        .pointer
                        .hover_pos()
                        .is_some_and(|pos| pos.distance(view.screen(*h)) < 8.)
                }) {
                    p.ctx()
                        .set_cursor_icon(if self.animation.path_handle.is_some() {
                            egui::CursorIcon::Grabbing
                        } else {
                            egui::CursorIcon::Grab
                        });
                    p.circle_stroke(view.screen(*h), 9., Stroke::new(2., Color32::WHITE));
                    crate::design::draw(
                        &p,
                        Rect::from_center_size(
                            view.screen(*h) + Vec2::new(18., -16.),
                            Vec2::splat(18.),
                        ),
                        Icon::Tween,
                        ACCENT,
                    );
                }
            }
        }
        if let Some(&(l, m)) = ids.first() {
            if let Some(g) = crate::animation::marks(&self.doc.layers[l], self.frame)
                .get(m)
                .and_then(|m| m.style.gradient.as_ref())
            {
                let a = view.screen(crate::vector::pos(g.start));
                let b = view.screen(crate::vector::pos(g.end));
                p.line_segment([a, b], Stroke::new(2., Color32::BLACK));
                p.line_segment([a, b], Stroke::new(1., Color32::WHITE));
                for point in [a, b] {
                    p.circle_filled(point, 5., Color32::WHITE);
                    p.circle_stroke(point, 5., Stroke::new(1., Color32::BLACK));
                }
            }
        }
    }
    pub fn motion_transform_ui(&mut self, ui: &mut egui::Ui) {
        let ids = self.canvas.selected.clone();
        let (l, m) = ids[0];
        let evaluated = crate::animation::marks(&self.doc.layers[l], self.frame);
        let Some(mark) = evaluated.get(m) else {
            return;
        };
        let center = crate::vector::control_bounds(mark).center();
        // Frame-local marks have no stable correspondence by index.
        let track = mark.style.track;
        let matrix = self.doc.layers[l].frames[0]
            .iter()
            .find(|reference| track != 0 && reference.style.track == track)
            .map_or([1., 0., 0., 1.], |a| crate::animation::delta(a, mark));
        let sx = Vec2::new(matrix[0], matrix[1]).length().max(0.001);
        let sy = (matrix[0] * matrix[3] - matrix[1] * matrix[2]) / sx;
        let angle = matrix[1].atan2(matrix[0]).to_degrees();
        let mut values = [center.x, center.y, angle, sx * 100., sy * 100.];
        let mut edited = [false; 5];
        egui::Grid::new("motion-transform")
            .num_columns(2)
            .show(ui, |ui| {
                for (i, label) in ["X", "Y", "Rotation °", "Scale X %", "Scale Y %"]
                    .iter()
                    .enumerate()
                {
                    ui.label(*label);
                    edited[i] = ui
                        .add(
                            egui::DragValue::new(&mut values[i])
                                .range(if i >= 3 { 1. ..=10000. } else { -1e6..=1e6 })
                                .clamp_existing_to_range(false)
                                .speed(0.5),
                        )
                        .changed();
                    ui.end_row();
                }
            });
        if edited.iter().any(|&changed| changed) {
            self.checkpoint();
            self.autokey_motion(&ids);
            let translation = Vec2::new(values[0] - center.x, values[1] - center.y);
            let scale = Vec2::new(
                if edited[3] {
                    values[3] / (sx * 100.)
                } else {
                    1.
                },
                if edited[4] && sy.abs() >= 0.0001 {
                    values[4] / (sy * 100.)
                } else {
                    1.
                },
            );
            let rotation = (values[2] - angle).to_radians();
            for &(l, m) in &ids {
                if self.doc.layers[l].options.locked {
                    continue;
                }
                let original = self.doc.layers[l].frames[self.frame][m].clone();
                self.doc.layers[l].frames[self.frame][m] =
                    crate::vector::transform(&original, |p| {
                        center + crate::canvas::rotate((p - center) * scale, rotation) + translation
                    });
            }
            self.canvas.selected = ids;
            self.recover();
        }
    }
    pub fn sample_apply_at(&mut self, ids: &[(usize, usize)], p: Pos2, tolerance: f32) {
        if let Some(source) = self.sample_at(p, tolerance, None) {
            self.feature.gradient_on = source.style.gradient.is_some();
            if let Some(g) = &source.style.gradient {
                self.feature.gradient = g.clone();
            }
            if !ids.is_empty() {
                self.checkpoint();
                self.materialize_motion(ids);
                for &(l, m) in ids {
                    if self.doc.layers[l].options.locked {
                        continue;
                    }
                    let target = &mut self.doc.layers[l].frames[self.frame][m];
                    if target.fill.is_none() {
                        continue;
                    }
                    target.fill = source.fill;
                    target.style.gradient = source.style.gradient.clone();
                    let b = crate::vector::control_bounds(target);
                    if let Some(g) = target.style.gradient.as_mut() {
                        let a = crate::vector::control_bounds(&source);
                        let map = |p: [f32; 2]| {
                            crate::vector::xy(
                                b.min
                                    + (crate::vector::pos(p) - a.min)
                                        / a.size().max(Vec2::splat(0.001))
                                        * b.size(),
                            )
                        };
                        g.start = map(g.start);
                        g.end = map(g.end);
                    }
                }
                self.canvas.selected = ids.to_vec();
                self.recover();
            }
        }
    }
    pub fn symbol_thumbnail(
        &mut self,
        ui: &mut egui::Ui,
        id: u64,
        frame: usize,
        size: Vec2,
    ) -> egui::Response {
        let (r, response) = ui.allocate_exact_size(size, Sense::click());
        if ui.is_rect_visible(r) {
            ui.painter()
                .rect_filled(r, 4., Color32::from_rgb(235, 235, 238));
            if let Some(s) = self.doc.extras.symbols.iter().find(|s| s.id == id) {
                let mut d = self.doc.clone();
                d.layers = s.layers.clone();
                d.extras.rig = s.rig.clone();
                d.extras.cameras = Default::default();
                d.extras.scenes = Default::default();
                let marks =
                    crate::features::resolve(&d, frame.min(d.layers[0].frames.len() - 1), false);
                let bounds = marks
                    .iter()
                    .map(|(_, _, m)| crate::vector::control_bounds(m))
                    .reduce(|a, b| a.union(b))
                    .unwrap_or(Rect::from_min_size(Pos2::ZERO, Vec2::splat(100.)));
                let view = crate::canvas::View {
                    center: r.center(),
                    scale: ((r.width() - 12.) / bounds.width().max(1.))
                        .min((r.height() - 12.) / bounds.height().max(1.)),
                    angle: 0.,
                    stage_center: bounds.center(),
                };
                let p = ui.painter().with_clip_rect(r);
                for (_, _, m) in marks {
                    crate::paint::draw(
                        &p,
                        &m,
                        view,
                        false,
                        false,
                        Some(&mut self.animation.thumbnail_cache),
                    );
                }
            }
        }
        response
    }
    pub fn frame_picker_ui(&mut self, ctx: &egui::Context) {
        let Some((l, m, f)) = self.animation.picker else {
            return;
        };
        if f != self.frame || l >= self.doc.layers.len() || m >= self.doc.layers[l].frames[f].len()
        {
            self.animation.picker = None;
            return;
        }
        let Some(instance) = crate::animation::marks(&self.doc.layers[l], f)[m]
            .style
            .instance
            .clone()
        else {
            self.animation.picker = None;
            return;
        };
        let count = self
            .doc
            .extras
            .symbols
            .iter()
            .find(|s| s.id == instance.symbol)
            .map_or(1, |s| s.layers[0].frames.len());
        let mut open = true;
        let mut selected = None;
        egui::Window::new(crate::i18n::tr("Selector de fotogramas"))
            .open(&mut open)
            .default_width(500.)
            .default_pos(ctx.content_rect().center() - egui::vec2(250., 230.))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().max_height(440.).show_rows(
                    ui,
                    94.,
                    count.div_ceil(5),
                    |ui, rows| {
                        for row in rows {
                            ui.horizontal(|ui| {
                                for i in row * 5..((row + 1) * 5).min(count) {
                                    ui.vertical(|ui| {
                                        if self
                                            .symbol_thumbnail(
                                                ui,
                                                instance.symbol,
                                                i,
                                                Vec2::new(80., 64.),
                                            )
                                            .clicked()
                                        {
                                            selected = Some(i);
                                        }
                                        ui.selectable_label(
                                            i == instance.first,
                                            format!("{}", i + 1),
                                        )
                                        .on_hover_text(crate::i18n::tr("Select frame preview"));
                                    });
                                }
                            });
                        }
                    },
                );
            });
        if let Some(i) = selected {
            self.checkpoint();
            self.autokey_motion(&[(l, m)]);
            self.doc.layers[l].frames[f][m]
                .style
                .instance
                .as_mut()
                .unwrap()
                .first = i;
            self.canvas.selected = vec![(l, m)];
            self.recover();
        }
        if !open {
            self.animation.picker = None;
        }
    }
    pub fn visual_gradient_ui(&mut self, ui: &mut egui::Ui) -> bool {
        let old = self.feature.gradient.clone();
        ui.horizontal(|ui| {
            for (radial, icon, label) in [
                (false, Icon::Line, "Linear"),
                (true, Icon::Ellipse, "Radial"),
            ] {
                if button(
                    ui,
                    icon,
                    "",
                    label,
                    self.feature.gradient.radial == radial,
                    true,
                )
                .clicked()
                {
                    self.feature.gradient.radial = radial;
                }
            }
        });
        let (r, response) = ui.allocate_exact_size(
            Vec2::new(ui.available_width().min(290.), 44.),
            Sense::click_and_drag(),
        );
        let bar = Rect::from_min_max(r.min, r.max - Vec2::new(0., 15.));
        let g = &self.feature.gradient;
        for i in 0..100 {
            let t = i as f32 / 99.;
            let c = g.color(crate::vector::pos(g.start).lerp(crate::vector::pos(g.end), t));
            ui.painter().rect_filled(
                Rect::from_min_max(
                    Pos2::new(bar.left() + bar.width() * i as f32 / 100., bar.top()),
                    Pos2::new(
                        bar.left() + bar.width() * (i + 1) as f32 / 100.,
                        bar.bottom(),
                    ),
                ),
                0.,
                Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]),
            );
        }
        for (i, s) in self.feature.gradient.stops.iter().enumerate() {
            let p = Pos2::new(bar.left() + bar.width() * s.t, bar.bottom() + 7.);
            ui.painter().circle_filled(
                p,
                5.,
                Color32::from_rgba_unmultiplied(s.color[0], s.color[1], s.color[2], 255),
            );
            ui.painter().circle_stroke(
                p,
                6.,
                Stroke::new(
                    1.,
                    if i == self.animation.stop {
                        ACCENT
                    } else {
                        Color32::GRAY
                    },
                ),
            );
        }
        if (response.clicked() || response.drag_started())
            && let Some(p) = response.interact_pointer_pos()
        {
            self.animation.stop = self
                .feature
                .gradient
                .stops
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    (bar.left() + bar.width() * a.t - p.x)
                        .abs()
                        .total_cmp(&(bar.left() + bar.width() * b.t - p.x).abs())
                })
                .map_or(0, |p| p.0);
        }
        self.animation.stop = self
            .animation
            .stop
            .min(self.feature.gradient.stops.len() - 1);
        if response.dragged()
            && let Some(p) = response.interact_pointer_pos()
        {
            self.feature.gradient.stops[self.animation.stop].t =
                ((p.x - bar.left()) / bar.width()).clamp(0., 1.);
        }
        ui.horizontal(|ui| {
            let s = &mut self.feature.gradient.stops[self.animation.stop];
            let mut c =
                Color32::from_rgba_unmultiplied(s.color[0], s.color[1], s.color[2], s.color[3]);
            if ui.color_edit_button_srgba(&mut c).changed() {
                s.color = c.to_srgba_unmultiplied();
            }
            let mut alpha = s.color[3] as f32 / 255. * 100.;
            if ui
                .add(
                    egui::DragValue::new(&mut alpha)
                        .range(0. ..=100.)
                        .suffix(crate::i18n::tr("% opacity")),
                )
                .changed()
            {
                s.color[3] = (alpha / 100. * 255.).round() as u8;
            }
        });
        ui.add(
            egui::Slider::new(
                &mut self.feature.gradient.stops[self.animation.stop].t,
                0. ..=1.,
            )
            .text(crate::i18n::tr("Position")),
        );
        ui.horizontal(|ui| {
            if button(
                ui,
                Icon::Plus,
                "",
                "Add color",
                false,
                self.feature.gradient.stops.len() < 32,
            )
            .clicked()
            {
                self.feature.gradient.stops.push(Stop {
                    t: 0.5,
                    color: [255, 255, 255, 255],
                });
                self.animation.stop = self.feature.gradient.stops.len() - 1;
            }
            if button(
                ui,
                Icon::Trash,
                "",
                "Delete color",
                false,
                self.feature.gradient.stops.len() > 2,
            )
            .clicked()
            {
                self.feature.gradient.stops.remove(self.animation.stop);
                self.animation.stop = 0;
            }
        });
        let g = &mut self.feature.gradient;
        let delta = crate::vector::pos(g.end) - crate::vector::pos(g.start);
        let mut angle = delta.angle().to_degrees();
        ui.horizontal(|ui| {
            let (r, res) = ui.allocate_exact_size(Vec2::splat(64.), Sense::click_and_drag());
            let center = r.center();
            if (res.clicked() || res.dragged())
                && let Some(p) = res.interact_pointer_pos()
            {
                angle = (p - center).angle().to_degrees();
            }
            ui.painter()
                .circle_stroke(center, 25., Stroke::new(1., Color32::GRAY));
            let direction = Vec2::angled(angle.to_radians()) * 24.;
            ui.painter()
                .arrow(center, direction, Stroke::new(2., ACCENT));
            ui.add(
                egui::DragValue::new(&mut angle)
                    .suffix(crate::i18n::tr("°"))
                    .prefix(crate::i18n::tr("Rotation ")),
            );
        });
        g.end = crate::vector::xy(
            crate::vector::pos(g.start) + Vec2::angled(angle.to_radians()) * delta.length().max(1.),
        );
        let selected = g.stops[self.animation.stop].clone();
        g.stops.sort_by(|a, b| a.t.total_cmp(&b.t));
        self.animation.stop = g.stops.iter().position(|s| *s == selected).unwrap_or(0);
        *g != old
    }
    pub fn motion_properties(&mut self, ui: &mut egui::Ui) {
        if self.canvas.selected.is_empty() {
            return;
        }
        crate::design::category(ui, "TRANSFORMACIÓN Y ANIMACIÓN");
        self.motion_transform_ui(ui);
        let has_motion = crate::animation::span(&self.doc.layers[self.layer], self.frame).is_some();
        ui.horizontal(|ui| {
            if button(
                ui,
                Icon::Tween,
                "Motion tween",
                "Create motion tween",
                false,
                true,
            )
            .clicked()
            {
                self.create_motion();
            }
            if button(
                ui,
                Icon::Trash,
                "",
                "Remove motion tween",
                false,
                has_motion,
            )
            .clicked()
            {
                let ids = self.canvas.selected.clone();
                self.checkpoint();
                let f = self.frame;
                self.doc.layers[self.layer]
                    .options
                    .tweens
                    .retain(|t| f < t.start || f > t.end);
                self.canvas.selected = ids;
                self.recover();
            }
        });
        let l = self.canvas.selected[0].0;
        let Some(index) = self.doc.layers[l]
            .options
            .tweens
            .iter()
            .position(|s| self.frame >= s.start && self.frame <= s.end)
        else {
            return;
        };
        let mut curve = self.doc.layers[l].options.tweens[index].curve.clone();
        if !ui.input(|i| i.pointer.primary_down()) {
            self.animation.timing_transaction = false;
        }
        let span = &self.doc.layers[l].options.tweens[index];
        timing_controls(
            ui,
            &mut curve,
            &mut self.animation.ease_handle,
            span.end - span.start + 1,
        );
        if curve != self.doc.layers[l].options.tweens[index].curve {
            let ids = self.canvas.selected.clone();
            if !self.animation.timing_transaction {
                self.checkpoint();
            }
            self.animation.timing_transaction = self.animation.ease_handle.is_some();
            self.doc.layers[l].options.tweens[index].curve = curve;
            self.canvas.selected = ids;
            self.recover();
        }
    }
}

/// The graph is the actual timing function. Drag its line to add/shape a point;
/// endpoint rails control departure/arrival speed independently.
pub fn timing_controls(
    ui: &mut egui::Ui,
    curve: &mut crate::animation::Curve,
    handle: &mut Option<usize>,
    frames: usize,
) {
    ui.horizontal(|ui| {
        for (icon, label, pace) in [
            (Icon::Line, "Lineal", [1. / 3., 1. / 3.]),
            (Icon::Next, "Acelerar", [0., 0.6]),
            (Icon::Previous, "Desacelerar", [0.6, 0.]),
            (Icon::Onion, "Suavizar ambos extremos", [0., 0.]),
        ] {
            if button(ui, icon, "", label, false, true).clicked() {
                curve.ease = crate::animation::Curve::default().ease;
                curve.pace = pace;
                curve.timing = Some(Vec::new());
                *handle = None;
            }
        }
    });
    let (area, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width().min(290.), 250.),
        Sense::click_and_drag(),
    );
    let graph = Rect::from_min_max(
        area.min + Vec2::new(12., 8.),
        Pos2::new(area.right() - 12., area.top() + 153.),
    );
    let screen = |p: [f32; 2]| {
        Pos2::new(
            graph.left() + p[0] * graph.width(),
            graph.bottom() - p[1] * graph.height(),
        )
    };
    let normal = |p: Pos2| {
        [
            ((p.x - graph.left()) / graph.width()).clamp(0.001, 0.999),
            ((graph.bottom() - p.y) / graph.height()).clamp(0., 1.),
        ]
    };
    let endpoints = [
        Pos2::new(graph.left(), graph.bottom() + 31.),
        Pos2::new(graph.right(), graph.bottom() + 61.),
    ];
    let knobs = [
        endpoints[0] + Vec2::X * graph.width() * curve.pace[0],
        endpoints[1] - Vec2::X * graph.width() * curve.pace[1],
    ];
    let pick_point = |p: Pos2, curve: &crate::animation::Curve| {
        curve.timing.as_ref().and_then(|points| {
            points
                .iter()
                .enumerate()
                .filter(|(_, q)| screen(**q).distance(p) <= 9.)
                .min_by(|(_, a), (_, b)| {
                    screen(**a).distance(p).total_cmp(&screen(**b).distance(p))
                })
                .map(|(i, _)| i)
        })
    };
    if response.double_clicked() {
        if let Some(i) = response
            .interact_pointer_pos()
            .and_then(|p| pick_point(p, curve))
        {
            curve.timing.as_mut().unwrap().remove(i);
        }
    }
    if response.drag_started() {
        *handle = None;
        if let Some(p) = ui.input(|i| i.pointer.press_origin()) {
            if let Some(i) = knobs.iter().position(|q| q.distance(p) <= 10.) {
                curve.edit_timing();
                *handle = Some(i);
            } else if let Some(i) = pick_point(p, curve) {
                *handle = Some(i + 2);
            } else if graph.expand(8.).contains(p) {
                let q = normal(p);
                if screen([q[0], curve.time(q[0])]).distance(p) <= 10. {
                    let y = curve.time(q[0]);
                    curve.edit_timing();
                    let points = curve.timing.as_mut().unwrap();
                    let nearest = points.iter().enumerate().min_by(|(_, a), (_, b)| {
                        (a[0] - q[0]).abs().total_cmp(&(b[0] - q[0]).abs())
                    });
                    let reuse = nearest.and_then(|(i, a)| {
                        ((a[0] - q[0]).abs() < 0.015 || points.len() == 32).then_some(i)
                    });
                    let index = if let Some(i) = reuse {
                        i
                    } else {
                        let i = points.partition_point(|a| a[0] < q[0]);
                        points.insert(i, [q[0], y]);
                        i
                    };
                    *handle = Some(index + 2);
                }
            }
        }
    }
    if response.dragged() {
        if let (Some(i), Some(p)) = (*handle, response.interact_pointer_pos()) {
            if i < 2 {
                curve.pace[i] = if i == 0 {
                    (p.x - graph.left()) / graph.width()
                } else {
                    (graph.right() - p.x) / graph.width()
                }
                .clamp(0., 1.);
            } else if let Some(points) = &mut curve.timing {
                let i = i - 2;
                if i < points.len() {
                    let a = if i == 0 { [0., 0.] } else { points[i - 1] };
                    let b = if i + 1 == points.len() {
                        [1., 1.]
                    } else {
                        points[i + 1]
                    };
                    let q = normal(p);
                    let margin = ((b[0] - a[0]) * 0.1).min(0.0001);
                    let x = if a[0] + margin < b[0] - margin {
                        q[0].clamp(a[0] + margin, b[0] - margin)
                    } else {
                        points[i][0]
                    };
                    points[i] = [x, q[1].clamp(a[1], b[1])];
                }
            }
        }
    }
    if !ui.input(|i| i.pointer.primary_down()) {
        *handle = None;
    }
    let painter = ui.painter();
    painter.rect_filled(graph, 3., crate::themes::panel());
    for i in 0..=4 {
        let x = graph.left() + graph.width() * i as f32 / 4.;
        let y = graph.bottom() - graph.height() * i as f32 / 4.;
        painter.line_segment(
            [Pos2::new(x, graph.top()), Pos2::new(x, graph.bottom())],
            Stroke::new(0.5, crate::themes::line()),
        );
        painter.line_segment(
            [Pos2::new(graph.left(), y), Pos2::new(graph.right(), y)],
            Stroke::new(0.5, crate::themes::line()),
        );
    }
    painter.line_segment(
        [screen([0., 0.]), screen([1., 1.])],
        Stroke::new(1., Color32::GRAY),
    );
    let line = (0..=96)
        .map(|i| {
            let t = i as f32 / 96.;
            screen([t, curve.time(t)])
        })
        .collect::<Vec<_>>();
    painter.add(egui::Shape::line(
        line,
        Stroke::new(2., crate::themes::accent()),
    ));
    if let Some(points) = &curve.timing {
        for &p in points {
            painter.circle_filled(screen(p), 4., crate::themes::accent());
            painter.circle_stroke(
                screen(p),
                5.,
                Stroke::new(1., crate::themes::color(Color32::WHITE)),
            );
        }
    }
    for i in 0..2 {
        let p = if i == 0 {
            endpoints[i] + Vec2::X * graph.width() * curve.pace[i]
        } else {
            endpoints[i] - Vec2::X * graph.width() * curve.pace[i]
        };
        let other = if i == 0 {
            endpoints[i] + Vec2::X * graph.width()
        } else {
            endpoints[i] - Vec2::X * graph.width()
        };
        painter.line_segment(
            [endpoints[i], other],
            Stroke::new(1., crate::themes::line()),
        );
        painter.circle_filled(endpoints[i], 3., Color32::GRAY);
        painter.line_segment([endpoints[i], p], Stroke::new(2., crate::themes::accent()));
        painter.circle_filled(p, 5., crate::themes::accent());
        let hover = ui
            .interact(
                Rect::from_center_size(p, Vec2::splat(20.)),
                ui.scope_id().with(("speed_hint", i)),
                Sense::hover(),
            )
            .on_hover_text(crate::i18n::tr(if i == 0 {
                "Start speed"
            } else {
                "End speed"
            }));
        if hover.hovered() || *handle == Some(i) {
            painter.circle_stroke(p, 9., Stroke::new(2., crate::themes::accent()));
            crate::design::draw(
                &painter,
                Rect::from_center_size(p + Vec2::new(17., -16.), Vec2::splat(16.)),
                if i == 0 { Icon::Next } else { Icon::Previous },
                crate::themes::accent(),
            );
        }

        painter.text(
            endpoints[i] - Vec2::Y * 10.,
            if i == 0 {
                egui::Align2::LEFT_BOTTOM
            } else {
                egui::Align2::RIGHT_BOTTOM
            },
            crate::i18n::tr(if i == 0 { "Start speed" } else { "End speed" }),
            egui::FontId::proportional(10.),
            crate::themes::muted(),
        );
    }
    let y = area.bottom() - 8.;
    let count = frames.clamp(2, 128);
    for i in 0..count {
        let t = i as f32 / (count - 1) as f32;
        painter.circle_filled(
            Pos2::new(graph.left() + graph.width() * curve.time(t), y),
            1.8,
            crate::themes::accent(),
        );
    }
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(if response.dragged() {
            egui::CursorIcon::Grabbing
        } else {
            egui::CursorIcon::Grab
        });
    }
    ui.small(crate::i18n::tr(
        "Drag the curve to shape it. Double-click a point to remove it.",
    ));
    ui.small(crate::i18n::tr(
        "Handles near the endpoints: slower start / stronger slowdown.",
    ));
}
