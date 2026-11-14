//! Exposure boundary editing: move an existing key boundary without inventing keys.
use crate::{Editor, features::FrameDrag, i18n::tr};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
#[derive(Clone, Copy)]
pub struct Drag {
    pub layer: usize,
    pub start: usize,
    pub end: usize,
    pub target: usize,
    pub minimum: usize,
    pub maximum: usize,
}
impl Editor {
    pub(crate) fn exposure_limits(
        &self,
        layer: usize,
        start: usize,
        end: usize,
    ) -> Option<(usize, usize)> {
        let data = self.doc.layers.get(layer)?;
        if data.options.locked || data.options.animation_track || data.options.mannequin.is_some() {
            return None;
        }
        if end < data.frames.len() {
            let next = (end + 1..data.frames.len())
                .find(|&f| data.key(f))
                .unwrap_or(10000);
            return Some((start + 1, next.saturating_sub(1).min(9999)));
        }
        let mut minimum = start + 1;
        for (index, data) in self.doc.layers.iter().enumerate() {
            if index == layer {
                continue;
            }
            if data.options.animation_track {
                minimum = minimum.max(crate::animation_clips::track_end(data));
            } else {
                minimum = minimum.max(
                    (0..data.frames.len())
                        .rev()
                        .find(|&f| data.key(f))
                        .unwrap_or(0)
                        + 1,
                );
            }
            minimum = minimum.max(
                data.options
                    .tweens
                    .iter()
                    .map(|s| s.end + 1)
                    .max()
                    .unwrap_or(1),
            );
        }
        minimum = minimum.max(self.doc.extras.media.timeline_end());
        for bone in &self.doc.extras.rig.bones {
            minimum = minimum.max(bone.keys.last().map_or(1, |key| key.frame + 1));
        }
        for binding in &self.doc.extras.rig.bindings {
            for pin in &binding.pins {
                minimum = minimum.max(pin.keys.last().map_or(1, |key| key.frame + 1));
            }
        }
        for camera in &self.doc.extras.cameras {
            minimum = minimum.max(camera.start + 1);
            minimum = minimum.max(
                camera
                    .animation
                    .keys
                    .last()
                    .map_or(1, |key| camera.start + key.frame + 1),
            );
        }
        Some((minimum, 10000))
    }
    pub(crate) fn resize_exposure(
        &mut self,
        layer: usize,
        start: usize,
        end: usize,
        target: usize,
    ) -> bool {
        let Some((minimum, maximum)) = self.exposure_limits(layer, start, end) else {
            return false;
        };
        if target < minimum
            || target > maximum
            || target == end
            || self.doc.layers[layer].exposure(start) != (start..end)
        {
            return false;
        }
        if end < self.doc.layers[layer].frames.len() {
            let drag = FrameDrag {
                layer,
                start: end,
                end,
                grab: end,
                target: Some(target),
            };
            let playhead = self.frame;
            if !self.move_timeline_frames(drag, target) {
                return false;
            }
            self.frame = playhead.clamp(start, target - 1);
            self.feature.frame_selection = Some((layer, start, target - 1));
            return true;
        }
        self.finish_pending_pen();
        self.checkpoint();
        for layer in &mut self.doc.layers {
            if crate::animation_clips::resize_track(layer, target) {
                continue;
            }
            layer.init_keys();
            let last = layer.frames.last().cloned().unwrap_or_default();
            layer.frames.resize(target, last);
            layer.options.keys.resize(target, false);
        }
        self.layer = layer;
        self.frame = self.frame.min(target - 1);
        self.feature.frame_selection = Some((layer, start, target - 1));
        self.canvas.clear_selection();
        self.playing = false;
        self.fit_camera_duration();
        self.recover();
        true
    }
    pub(crate) fn exposure_handles_ui(
        &mut self,
        ui: &mut egui::Ui,
        layer: usize,
        row: Rect,
        grid_left: f32,
        cell: f32,
        visible: std::ops::Range<usize>,
    ) {
        if self.doc.layers[layer].options.locked
            || self.doc.layers[layer].options.animation_track
            || self.doc.layers[layer].options.mannequin.is_some()
        {
            return;
        }
        let count = self.doc.layers[layer].frames.len();
        for f in visible {
            let end = f + 1;
            if end < count && !self.doc.layers[layer].key(end) {
                continue;
            }
            let x = grid_left + end as f32 * cell;
            let handle = Rect::from_min_max(
                Pos2::new(x - 4., row.top() + 1.),
                Pos2::new(x + 4., row.bottom() - 1.),
            );
            let response=ui.interact(handle,ui.scope_id().with(("exposure-end",layer,end)),Sense::drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal).on_hover_text(tr("Drag this edge to extend or shorten the exposure. The final edge changes scene duration."));
            let color = if response.hovered() || response.dragged() {
                crate::themes::accent()
            } else {
                Color32::from_gray(15)
            };
            ui.painter().line_segment(
                [
                    Pos2::new(x - 1., row.top() + 5.),
                    Pos2::new(x - 1., row.bottom() - 5.),
                ],
                Stroke::new(if response.hovered() { 3. } else { 2. }, color),
            );
            if response.drag_started_by(egui::PointerButton::Primary) {
                let start = self.doc.layers[layer].exposure(f).start;
                if let Some((minimum, maximum)) = self.exposure_limits(layer, start, end) {
                    self.feature.exposure_drag = Some(Drag {
                        layer,
                        start,
                        end,
                        target: end,
                        minimum,
                        maximum,
                    });
                    self.feature.frame_drag = None;
                    self.layer = layer;
                    self.frame = f;
                    self.playing = false;
                }
            }
        }
        if let Some(drag) = self
            .feature
            .exposure_drag
            .as_mut()
            .filter(|d| d.layer == layer)
        {
            if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                drag.target = (((pos.x - grid_left) / cell).round().max(1.) as usize).min(10000);
                let valid = (drag.minimum..=drag.maximum).contains(&drag.target);
                let x = grid_left + drag.target as f32 * cell;
                let from = grid_left + drag.start as f32 * cell;
                let ghost = Rect::from_min_max(
                    Pos2::new(from, row.top() + 1.),
                    Pos2::new(x.max(from + 1.), row.bottom() - 1.),
                );
                let color = if valid {
                    crate::themes::accent()
                } else {
                    Color32::LIGHT_RED
                };
                ui.painter()
                    .rect_filled(ghost, 2., color.gamma_multiply(0.2));
                ui.painter().line_segment(
                    [Pos2::new(x, row.top()), Pos2::new(x, row.bottom())],
                    Stroke::new(3., color),
                );
                ui.painter().text(
                    Pos2::new(x + 7., row.center().y),
                    egui::Align2::LEFT_CENTER,
                    format!(
                        "{} {}",
                        drag.target.saturating_sub(drag.start),
                        tr("frames")
                    ),
                    egui::FontId::proportional(11.),
                    color,
                );
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                if pos.x > ui.clip_rect().right() - 18. {
                    ui.scroll_to_rect(
                        Rect::from_min_size(Pos2::new(x, row.top()), Vec2::new(24., row.height())),
                        None,
                    );
                }
            }
        }
    }
    pub(crate) fn finish_exposure_resize(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.feature.exposure_drag = None;
        }
        if ctx.input(|i| i.pointer.primary_released()) {
            if let Some(drag) = self.feature.exposure_drag.take() {
                if !(drag.minimum..=drag.maximum).contains(&drag.target) {
                    self.status = tr("Cannot shorten past animation in other tracks.").into();
                } else {
                    self.resize_exposure(drag.layer, drag.start, drag.end, drag.target);
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
        e.recovery = std::env::temp_dir().join(format!(
            "resize-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ));
        e
    }
    fn keys(e: &Editor) -> Vec<usize> {
        (0..e.doc.layers[0].frames.len())
            .filter(|&f| e.doc.layers[0].key(f))
            .collect()
    }
    #[test]
    fn boundary_drag_retimes_only_existing_key_and_undo_restores_exposures() {
        let mut e = editor();
        e.doc.layers[0].options.keys[8] = true;
        e.doc.layers[0].options.keys[16] = true;
        let a: crate::shared::SharedVec<crate::Mark> = vec![crate::Mark {
            points: vec![[1., 2.], [3., 4.]].into(),
            ..Default::default()
        }]
        .into();
        let b: crate::shared::SharedVec<crate::Mark> = vec![crate::Mark {
            points: vec![[20., 2.], [30., 4.]].into(),
            ..Default::default()
        }]
        .into();
        for f in 0..16 {
            e.doc.layers[0].frames[f] = if f < 8 { a.clone() } else { b.clone() };
        }
        assert!(e.resize_exposure(0, 0, 8, 12));
        assert_eq!(keys(&e), vec![0, 12, 16]);
        assert_eq!(e.doc.layers[0].frames[11][0].points, a[0].points);
        assert_eq!(e.doc.layers[0].frames[12][0].points, b[0].points);
        assert!(!e.resize_exposure(0, 0, 12, 17));
        e.history(false);
        assert_eq!(keys(&e), vec![0, 8, 16]);
        assert!(e.doc.validate().is_ok());
    }
    #[test]
    fn final_exposure_can_extend_and_shorten_without_blank_keys() {
        let mut e = editor();
        assert!(e.resize_exposure(0, 0, 24, 40));
        assert_eq!(e.doc.layers[0].frames.len(), 40);
        assert_eq!(keys(&e), vec![0]);
        assert!(e.resize_exposure(0, 0, 40, 12));
        assert_eq!(e.doc.layers[0].frames.len(), 12);
        assert_eq!(keys(&e), vec![0]);
        assert!(e.doc.validate().is_ok());
    }
    #[test]
    fn shortening_a_scene_never_discards_other_layer_keys() {
        let mut e = editor();
        let other = e.doc.layers[0].clone();
        e.doc.layers.push(other);
        e.doc.layers[1].options.keys[20] = true;
        let before = serde_json::to_vec(&e.doc).unwrap();
        assert!(!e.resize_exposure(0, 0, 24, 10));
        assert_eq!(before, serde_json::to_vec(&e.doc).unwrap());
        assert!(e.resize_exposure(0, 0, 24, 22));
        assert!(e.doc.layers[1].key(20));
    }
    #[test]
    fn pointer_handle_resizes_and_escape_cancels_without_history() {
        let mut e = editor();
        let ctx = egui::Context::default();
        let tick = |e: &mut Editor, events: Vec<egui::Event>| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000., 250.))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    e.exposure_handles_ui(
                        ui,
                        0,
                        Rect::from_min_max(Pos2::new(100., 60.), Pos2::new(950., 88.)),
                        100.,
                        20.,
                        0..e.doc.layers[0].frames.len(),
                    );
                },
            );
            output.textures_delta.clear();
            e.finish_exposure_resize(&ctx);
        };
        let from = Pos2::new(579., 74.);
        let to = Pos2::new(659., 74.);
        let press = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        tick(&mut e, vec![]);
        tick(&mut e, vec![egui::Event::PointerMoved(from)]);
        tick(&mut e, vec![press(from, true)]);
        tick(&mut e, vec![egui::Event::PointerMoved(to)]);
        tick(&mut e, vec![press(to, false)]);
        assert_eq!(e.doc.layers[0].frames.len(), 28);
        assert_eq!(e.undo.len(), 1);
    }
}

#[cfg(test)]
mod workspace_regression {
    use super::*;
    #[test]
    fn exposure_edge_wins_over_frame_body_drag_in_the_full_timeline() {
        let ctx = egui::Context::default();
        let mut e = Editor::new(&eframe::CreationContext::_new_kittest(ctx.clone()));
        let doc = crate::fresh_document();
        e.add_document(doc, None);
        let tick = |e: &mut Editor, events: Vec<egui::Event>| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1440., 900.))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    e.workspace(ui);
                    egui::CentralPanel::default().show(ui, |ui| e.canvas_ui(ui));
                },
            );
            output.textures_delta.clear();
            output
        };
        let output = tick(&mut e, vec![]);
        let edge = output
            .shapes
            .iter()
            .find_map(|shape| {
                if let egui::Shape::LineSegment { points, stroke } = &shape.shape {
                    if stroke.color == Color32::from_gray(15)
                        && (points[0].x - points[1].x).abs() < 0.1
                        && ((points[1].y - points[0].y).abs() - 18.).abs() < 0.1
                    {
                        return Some(points[0] + (points[1] - points[0]) * 0.5);
                    }
                }
                None
            })
            .expect("exposure end handle");
        let press = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let to = edge + Vec2::new(44., 0.);
        let out = tick(&mut e, vec![egui::Event::PointerMoved(edge)]);
        assert_eq!(
            out.platform_output.cursor_icon,
            egui::CursorIcon::ResizeHorizontal
        );
        tick(&mut e, vec![press(edge, true)]);
        tick(&mut e, vec![egui::Event::PointerMoved(to)]);
        tick(&mut e, vec![press(to, false)]);
        assert_eq!(e.doc.layers[0].frames.len(), 26);
        assert_eq!((0..26).filter(|&f| e.doc.layers[0].key(f)).count(), 1);
        assert!(e.feature.frame_drag.is_none());
    }
}
