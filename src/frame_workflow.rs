use crate::{
    Editor,
    canvas::{View, transformed_in_basis},
    design::{Icon, button},
    i18n::tr,
};
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};

#[derive(Clone, Copy)]
struct Transform {
    x: f32,
    y: f32,
    angle: f32,
    scale: f32,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            x: 0.,
            y: 0.,
            angle: 0.,
            scale: 100.,
        }
    }
}

impl Transform {
    fn mark(self, pivot: Pos2, mark: &crate::Mark) -> crate::Mark {
        transformed_in_basis(
            mark,
            pivot,
            Vec2::splat(self.scale / 100.),
            self.angle.to_radians(),
            Vec2::new(self.x, self.y),
            0.,
        )
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for (label, value, range) in [
                ("X", &mut self.x, -20000. ..=20000.),
                ("Y", &mut self.y, -20000. ..=20000.),
                ("°", &mut self.angle, -180. ..=180.),
                ("%", &mut self.scale, 1. ..=500.),
            ] {
                ui.add(
                    egui::DragValue::new(value)
                        .range(range)
                        .prefix(format!("{label} ")),
                );
            }
            if button(ui, Icon::ResetColors, "", "Reset transform", false, true).clicked() {
                *self = Self::default();
            }
        });
    }
}

struct Reference {
    frame: usize,
    transform: Transform,
}

struct Trace {
    layer: usize,
    references: [Reference; 2],
    active: usize,
    drag: Option<(Pos2, Transform, TraceGesture)>,
}

#[derive(Clone, Copy)]
enum TraceGesture {
    Move,
    Rotate,
    Scale,
}

struct Batch {
    layer: usize,
    first: usize,
    last: usize,
    transform: Transform,
}

#[derive(Default)]
pub(crate) struct State {
    trace: Option<Trace>,
    batch: Option<Batch>,
}

impl State {
    pub(crate) fn tracing(&self) -> bool {
        self.trace.is_some()
    }
}

pub(crate) fn editable(layer: &crate::document::Layer) -> bool {
    !layer.options.locked
        && !layer.options.animation_track
        && layer.options.mannequin.is_none()
        && !crate::post::is_adjustment(layer)
        && !layer.options.light_mask
}

impl Editor {
    pub(crate) fn toggle_shift_trace(&mut self) {
        if self.animation.frame_workflow.trace.take().is_some() {
            return;
        }
        let layer = &self.doc.layers[self.layer];
        let last = layer.frames.len() - 1;
        let before = (0..self.frame)
            .rev()
            .find(|&f| layer.key(f))
            .unwrap_or(self.frame);
        let after = (self.frame + 1..=last)
            .find(|&f| layer.key(f))
            .unwrap_or(self.frame);
        let [a, b] = self
            .feature
            .frame_selection
            .filter(|&(layer, a, b)| layer == self.layer && a != b)
            .map_or([before, after], |(_, a, b)| [a.min(b), a.max(b)]);
        self.playing = false;
        self.restart_media_audio();
        self.animation.frame_workflow.trace = Some(Trace {
            layer: self.layer,
            active: 0,
            drag: None,
            references: [a, b].map(|frame| Reference {
                frame: frame.min(last),
                transform: Transform::default(),
            }),
        });
    }

    pub(crate) fn open_batch_transform(&mut self) {
        let Some((layer, a, b)) = self.feature.frame_selection else {
            return;
        };
        if !editable(&self.doc.layers[layer]) {
            return;
        }
        self.playing = false;
        self.restart_media_audio();
        self.animation.frame_workflow.batch = Some(Batch {
            layer,
            first: a.min(b),
            last: a.max(b),
            transform: Transform::default(),
        });
    }

    pub(crate) fn frame_workflow_ui(&mut self, ctx: &egui::Context) {
        if let Some(trace) = &mut self.animation.frame_workflow.trace {
            let mut open = trace.layer < self.doc.layers.len();
            if open {
                let last = self.doc.layers[trace.layer].frames.len();
                egui::Window::new(tr("Shift & Trace"))
                    .open(&mut open)
                    .default_width(250.)
                    .show(ctx, |ui| {
                        ui.small(tr("Drag with Selection to move the reference. Drag its handles to rotate or scale. Switch to a drawing tool to draw."));
                        for (index, reference) in trace.references.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                if ui
                                    .selectable_label(
                                        trace.active == index,
                                        egui::RichText::new(tr(if index == 0 {
                                            "Reference A"
                                        } else {
                                            "Reference B"
                                        }))
                                        .color(TRACE_COLORS[index]),
                                    )
                                    .clicked()
                                {
                                    trace.active = index;
                                }
                                let mut frame = reference.frame + 1;
                                ui.add(
                                    egui::DragValue::new(&mut frame)
                                        .range(1..=last)
                                        .prefix(format!("{} ", tr("Frame"))),
                                );
                                reference.frame = frame - 1;
                            });
                            reference.transform.ui(ui);
                        }
                    });
            }
            if !open {
                self.animation.frame_workflow.trace = None;
            }
        }
        if let Some(mut batch) = self.animation.frame_workflow.batch.take() {
            let Some(layer) = self
                .doc
                .layers
                .get(batch.layer)
                .filter(|layer| editable(layer))
            else {
                return;
            };
            let mut open = true;
            let mut apply = false;
            egui::Window::new(tr("Transform keyframes"))
                .open(&mut open)
                .default_width(285.)
                .show(ctx, |ui| {
                    ui.label(format!(
                        "{} · {}–{}",
                        layer.name,
                        batch.first + 1,
                        batch.last + 1
                    ));
                    batch.transform.ui(ui);
                    apply = button(ui, Icon::Transform, "Aplicar", "", false, true).clicked();
                });
            if apply {
                self.apply_batch_transform(batch);
            } else if open {
                self.animation.frame_workflow.batch = Some(batch);
            }
        }
    }

    pub(crate) fn set_exposure_step(&mut self, step: usize) {
        let Some((index, a, b)) = self.feature.frame_selection else {
            return;
        };
        let Some(layer) = self.doc.layers.get(index).filter(|layer| editable(layer)) else {
            return;
        };
        let count = layer.frames.len();
        let first = layer.exposure(a.min(b).min(count - 1)).start;
        let end = layer.exposure(a.max(b).min(count - 1)).end;
        let keys: Vec<_> = (first..end).filter(|&frame| layer.key(frame)).collect();
        let span = keys.len() * step;
        if span == 0 || count - (end - first) + span > 10000 {
            return;
        }
        let shift = span as isize - (end - first) as isize;
        let map = |frame: usize| {
            if frame >= end {
                frame.saturating_add_signed(shift)
            } else if frame >= first {
                first + keys.partition_point(|&key| key < frame) * step
            } else {
                frame
            }
        };
        let new_count = count.saturating_add_signed(shift);
        self.finish_pending_pen();
        self.checkpoint();
        let layer = &mut self.doc.layers[index];
        layer.init_keys();
        let cels: Vec<_> = keys
            .iter()
            .flat_map(|&frame| std::iter::repeat_n(layer.frames[frame].clone(), step))
            .collect();
        layer.frames.splice(first..end, cels);
        layer
            .options
            .keys
            .splice(first..end, (0..span).map(|i| i % step == 0));
        for tween in &mut layer.options.tweens {
            tween.start = map(tween.start);
            tween.end = map(tween.end);
        }
        layer.options.drawing_classes = std::mem::take(&mut layer.options.drawing_classes)
            .into_iter()
            .map(|(frame, class)| (map(frame), class))
            .collect();
        let count = self
            .doc
            .layers
            .iter()
            .map(|layer| layer.frames.len())
            .max()
            .unwrap_or(new_count)
            .max(new_count);
        for layer in &mut self.doc.layers {
            if layer.frames.len() < count && !crate::animation_clips::resize_track(layer, count) {
                layer.init_keys();
                let last = layer.frames.last().cloned().unwrap_or_default();
                layer.frames.resize(count, last);
                layer.options.keys.resize(count, false);
            }
        }
        self.layer = index;
        self.frame = first;
        self.playing = false;
        self.feature.frame_selection = Some((index, first, first + span - 1));
        self.fit_camera_duration();
        self.restart_media_audio();
        self.recover();
    }

    fn apply_batch_transform(&mut self, batch: Batch) {
        let layer = &self.doc.layers[batch.layer];
        let changes: Vec<_> = (batch.first..=batch.last.min(layer.frames.len() - 1))
            .filter(|&frame| layer.key(frame) && !layer.frames[frame].is_empty())
            .map(|frame| {
                let marks = &layer.frames[frame];
                let pivot = marks
                    .iter()
                    .fold(Rect::NOTHING, |bounds, mark| {
                        bounds.union(crate::vector::control_bounds(mark))
                    })
                    .center();
                let cel: crate::shared::SharedVec<_> = marks
                    .iter()
                    .map(|mark| batch.transform.mark(pivot, mark))
                    .collect();
                (layer.exposure(frame), cel)
            })
            .collect();
        if changes.is_empty() {
            return;
        }
        self.finish_pending_pen();
        self.checkpoint();
        let layer = &mut self.doc.layers[batch.layer];
        for (range, cel) in changes {
            for frame in range {
                layer.frames[frame] = cel.clone();
            }
        }
        self.recover();
    }

    pub(crate) fn shift_trace_input(&mut self, ui: &egui::Ui, area: Rect, view: View) -> bool {
        if self.playing {
            return false;
        }
        let can_align = matches!(
            self.tool,
            crate::canvas::Tool::Select
                | crate::canvas::Tool::Object
                | crate::canvas::Tool::Transform
        );
        let center = Pos2::new(self.doc.width as f32 * 0.5, self.doc.height as f32 * 0.5);
        let Some(trace) = &mut self.animation.frame_workflow.trace else {
            return false;
        };
        let (pos, start, down) = ui.input(|i| {
            (
                i.pointer.interact_pos(),
                can_align && i.pointer.primary_pressed() && !i.key_down(egui::Key::Space),
                i.pointer.primary_down(),
            )
        });
        if start
            && let Some(pos) = pos
            && area.contains(pos)
            && ui.ctx().layer_id_at(pos) == Some(ui.layer_id())
        {
            let initial = trace.references[trace.active].transform;
            let [rotate, scale] = trace_handles(view, center, initial);
            let gesture = if pos.distance(rotate) < 12. {
                TraceGesture::Rotate
            } else if pos.distance(scale) < 12. {
                TraceGesture::Scale
            } else {
                TraceGesture::Move
            };
            trace.drag = Some((view.world(pos), initial, gesture));
        }
        if let Some((origin, initial, gesture)) = trace.drag {
            if let Some(pos) = pos {
                let world = view.world(pos);
                let transform = &mut trace.references[trace.active].transform;
                let pivot = center + Vec2::new(initial.x, initial.y);
                match gesture {
                    TraceGesture::Move => {
                        let delta = world - origin;
                        transform.x = (initial.x + delta.x).clamp(-20000., 20000.);
                        transform.y = (initial.y + delta.y).clamp(-20000., 20000.);
                    }
                    TraceGesture::Rotate => {
                        let angle =
                            ((world - pivot).angle() - (origin - pivot).angle()).to_degrees();
                        transform.angle = (initial.angle + angle + 180.).rem_euclid(360.) - 180.;
                    }
                    TraceGesture::Scale => {
                        transform.scale = (initial.scale * world.distance(pivot)
                            / origin.distance(pivot).max(0.01))
                        .clamp(1., 500.);
                    }
                }
            }
            if !down {
                trace.drag = None;
            }
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            return true;
        }
        false
    }

    pub(crate) fn paint_frame_workflow(&mut self, painter: &egui::Painter, view: View) {
        if self.playing {
            return;
        }
        let mut overlays = Vec::new();
        if let Some(trace) = &self.animation.frame_workflow.trace {
            let center = Pos2::new(self.doc.width as f32 * 0.5, self.doc.height as f32 * 0.5);
            for (i, reference) in trace.references.iter().enumerate() {
                overlays.extend(
                    crate::features::resolve_export(&self.doc, reference.frame, false)
                        .into_iter()
                        .filter(|(layer, _, _)| *layer == trace.layer)
                        .map(|(_, _, mark)| {
                            (reference.transform.mark(center, &mark), TRACE_COLORS[i])
                        }),
                );
            }
        }
        if let Some(batch) = &self.animation.frame_workflow.batch
            && let Some(layer) = self.doc.layers.get(batch.layer)
        {
            let frame = layer
                .exposure(
                    self.frame
                        .clamp(batch.first, batch.last)
                        .min(layer.frames.len() - 1),
                )
                .start;
            let marks = &layer.frames[frame];
            let pivot = marks
                .iter()
                .fold(Rect::NOTHING, |bounds, mark| {
                    bounds.union(crate::vector::control_bounds(mark))
                })
                .center();
            let matrix = crate::folder_groups::layer_matrix(&self.doc, layer);
            overlays.extend(
                crate::features::resolve_export(&self.doc, frame, false)
                    .into_iter()
                    .filter(|(layer, _, _)| *layer == batch.layer)
                    .map(|(_, _, mark)| {
                        let mark = if let Some(inverse) = crate::rig::inverse(matrix) {
                            let local = crate::folder_groups::mark(&mark, inverse);
                            crate::folder_groups::mark(&batch.transform.mark(pivot, &local), matrix)
                        } else {
                            mark
                        };
                        (mark, Color32::LIGHT_GREEN)
                    }),
            );
        }
        let mut cache = std::mem::take(&mut self.animation.thumbnail_cache);
        cache.begin_frame();
        for (mut mark, color) in overlays {
            if mark.style.media_image.is_some() {
                mark.color[3] = ((mark.color[3] as f32) * 0.45).round() as u8;
                self.paint_media_image(painter, view, &mark);
            } else {
                tint(&mut mark, color);
                crate::canvas::paint_mark(painter, &mark, view, false, false, Some(&mut cache));
            }
        }
        self.animation.thumbnail_cache = cache;
        if let Some(trace) = &self.animation.frame_workflow.trace
            && matches!(
                self.tool,
                crate::canvas::Tool::Select
                    | crate::canvas::Tool::Object
                    | crate::canvas::Tool::Transform
            )
        {
            let center = Pos2::new(self.doc.width as f32 * 0.5, self.doc.height as f32 * 0.5);
            let transform = trace.references[trace.active].transform;
            let pivot = view.screen(center + Vec2::new(transform.x, transform.y));
            let color = TRACE_COLORS[trace.active];
            for (position, icon) in trace_handles(view, center, transform)
                .into_iter()
                .zip([Icon::Rotate, Icon::Transform])
            {
                painter.line_segment(
                    [pivot, position],
                    egui::Stroke::new(1., color.gamma_multiply(0.6)),
                );
                painter.circle_filled(position, 10., crate::themes::panel());
                crate::design::draw(
                    painter,
                    Rect::from_center_size(position, Vec2::splat(16.)),
                    icon,
                    color,
                );
            }
        }
    }
}

fn trace_handles(view: View, center: Pos2, transform: Transform) -> [Pos2; 2] {
    let pivot = view.screen(center + Vec2::new(transform.x, transform.y));
    let angle = transform.angle.to_radians() + view.angle;
    [
        pivot + Vec2::angled(angle - std::f32::consts::FRAC_PI_2) * 48.,
        pivot + Vec2::angled(angle) * 58.,
    ]
}

const TRACE_COLORS: [Color32; 2] = [
    Color32::from_rgb(255, 100, 133),
    Color32::from_rgb(65, 189, 255),
];

fn tint(mark: &mut crate::Mark, color: Color32) {
    let color = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 112).to_array();
    mark.color = color;
    if mark.fill.is_some() {
        mark.fill = Some(color);
    }
    mark.style.gradient = None;
}

#[cfg(test)]
#[test]
fn exposure_cadence_and_batch_transform_keep_held_cels_and_undo() {
    let ctx = egui::Context::default();
    let mut editor = Editor::new(&eframe::CreationContext::_new_kittest(ctx));
    editor.doc = crate::Document::default();
    let layer = &mut editor.doc.layers[0];
    layer.options.keys = vec![true, false, false, true, false, true].into();
    let cel: crate::shared::SharedVec<_> = vec![crate::Mark {
        points: vec![[10., 10.], [30., 30.]].into(),
        ..Default::default()
    }]
    .into();
    layer.frames = vec![cel; 6].into();
    layer.options.drawing_classes.insert(3, 2);
    editor.feature.frame_selection = Some((0, 0, 3));
    editor.set_exposure_step(2);
    let layer = &editor.doc.layers[0];
    assert_eq!(&layer.options.keys[..], &[true, false, true, false, true]);
    assert_eq!(layer.options.drawing_classes.get(&2), Some(&2));
    editor.apply_batch_transform(Batch {
        layer: 0,
        first: 0,
        last: 3,
        transform: Transform {
            x: 10.,
            ..Default::default()
        },
    });
    let layer = &editor.doc.layers[0];
    for frame in 0..4 {
        assert_eq!(layer.frames[frame][0].points[0], [20., 10.]);
    }
    assert_eq!(layer.frames[4][0].points[0], [10., 10.]);
    assert_eq!(editor.undo.len(), 2);
    editor.history(false);
    assert_eq!(editor.doc.layers[0].frames[0][0].points[0], [10., 10.]);
}
