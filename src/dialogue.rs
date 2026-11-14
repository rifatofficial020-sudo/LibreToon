//! Scene markers, named frame ranges and timeline playback controls.
use crate::{
    Document, Editor,
    design::{Icon, button},
    i18n::tr,
};
use eframe::egui::{self, Color32, Pos2, Rect};
use serde::{Deserialize, Serialize};

fn default_color() -> [u8; 3] {
    [91, 158, 255]
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Marker {
    pub frame: usize,
    pub name: String,
    #[serde(default = "default_color")]
    pub color: [u8; 3],
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Tag {
    pub id: u64,
    pub start: usize,
    pub end: usize,
    pub name: String,
    pub color: [u8; 3],
}

#[derive(Clone, Copy)]
enum TagHandle {
    Start,
    End,
    Body,
}

pub(crate) struct TagDrag {
    id: u64,
    handle: TagHandle,
    origin: f32,
    cell: f32,
    original: (usize, usize),
    range: (usize, usize),
}

impl TagHandle {
    fn range(self, (start, end): (usize, usize), delta: isize, last: usize) -> (usize, usize) {
        match self {
            Self::Start => (start.saturating_add_signed(delta).min(end), end),
            Self::End => (start, end.saturating_add_signed(delta).clamp(start, last)),
            Self::Body => {
                let span = end - start;
                let new_start = start.saturating_add_signed(delta).min(last - span);
                (new_start, new_start + span)
            }
        }
    }
}

pub(crate) enum Draft {
    Marker(Marker),
    Tag(Tag),
}

pub fn validate(doc: &Document) -> Result<(), String> {
    let name_valid = |name: &str| !name.is_empty() && name.len() <= 160;
    for (markers, tags) in std::iter::once((&doc.extras.markers, &doc.extras.tags))
        .chain(doc.extras.scenes.iter().map(|s| (&s.markers, &s.tags)))
    {
        if markers.len() > 10000
            || markers
                .iter()
                .any(|m| m.frame >= 10000 || !name_valid(&m.name))
            || tags.len() > 1000
            || tags
                .iter()
                .any(|t| t.id == 0 || t.start > t.end || t.end >= 10000 || !name_valid(&t.name))
        {
            return Err("Invalid timeline markers or animation tags".into());
        }
    }
    Ok(())
}

impl Editor {
    pub(crate) fn timeline_marker_menu(
        &mut self,
        ui: &mut egui::Ui,
        response: &egui::Response,
        origin: f32,
        cell: f32,
        count: usize,
    ) {
        if !self.feature.symbol_stack.is_empty() {
            return;
        }
        let context = ui.ctx().clone();
        let target = response.id.with("annotation-frame");
        if response.secondary_clicked() {
            let frame = response
                .interact_pointer_pos()
                .map_or(self.frame, |p| {
                    ((p.x - origin) / cell).floor().max(0.) as usize
                })
                .min(count - 1);
            context.data_mut(|data| data.insert_temp(target, frame));
        }
        response.context_menu(|ui| {
            let frame = context
                .data(|data| data.get_temp::<usize>(target))
                .unwrap_or(self.frame)
                .min(count - 1);
            let marker = self
                .doc
                .extras
                .markers
                .iter()
                .find(|m| m.frame == frame)
                .cloned();
            let tag = self
                .doc
                .extras
                .tags
                .iter()
                .find(|t| t.start <= frame && frame <= t.end)
                .cloned();
            if button(
                ui,
                Icon::Pin,
                if marker.is_some() {
                    "Edit marker…"
                } else {
                    "New marker…"
                },
                "",
                false,
                true,
            )
            .clicked()
            {
                self.animation.annotation = Some(Draft::Marker(marker.unwrap_or(Marker {
                    frame,
                    name: String::new(),
                    color: default_color(),
                })));
                ui.close();
            }
            if button(
                ui,
                Icon::Tag,
                "New animation tag…",
                "",
                false,
                self.doc.extras.tags.len() < 1000,
            )
            .clicked()
            {
                let (start, end) = self
                    .feature
                    .frame_selection
                    .filter(|&(_, a, b)| (a.min(b)..=a.max(b)).contains(&frame))
                    .map_or((frame, frame), |(_, a, b)| (a.min(b), a.max(b)));
                self.animation.annotation = Some(Draft::Tag(Tag {
                    id: crate::features::id(),
                    start: start.min(count - 1),
                    end: end.min(count - 1),
                    name: String::new(),
                    color: default_color(),
                }));
                ui.close();
            }
            if let Some(tag) = tag
                && button(ui, Icon::Tag, "Edit animation tag…", "", false, true).clicked()
            {
                self.animation.annotation = Some(Draft::Tag(tag));
                ui.close();
            }
            let markers = self.doc.extras.markers.clone();
            let tags = self.doc.extras.tags.clone();
            if !markers.is_empty() || !tags.is_empty() {
                ui.menu_button(tr("Go to marker or tag"), |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(240.)
                        .show(ui, |ui| {
                            for (frame, name, color, tag) in markers
                                .iter()
                                .map(|m| (m.frame, &m.name, m.color, None))
                                .chain(tags.iter().map(|t| (t.start, &t.name, t.color, Some(t.id))))
                                .filter(|(f, ..)| *f < count)
                            {
                                let label = egui::RichText::new(format!("{} · {name}", frame + 1))
                                    .color(Color32::from_rgb(color[0], color[1], color[2]));
                                if ui.button(label).clicked() {
                                    self.finish_pending_pen();
                                    self.frame = frame;
                                    self.animation.selected_tag = tag;
                                    self.playing = false;
                                    self.scrub_media_audio(ui.ctx());
                                    ui.close();
                                }
                            }
                        });
                });
            }
        });
    }

    pub(crate) fn annotation_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.animation.annotation.take() else {
            return;
        };
        let count = self.doc.layers[0].frames.len();
        let mut save = false;
        let mut delete = false;
        let mut cancel = false;
        let response = egui::Modal::new(egui::Id::unique("timeline-annotation")).show(ctx, |ui| {
            ui.set_min_width(300.);
            let (name, color, exists) = match &mut draft {
                Draft::Marker(marker) => {
                    ui.heading(format!("{} · {}", tr("Timeline marker"), marker.frame + 1));
                    (
                        &mut marker.name,
                        &mut marker.color,
                        self.doc
                            .extras
                            .markers
                            .iter()
                            .any(|m| m.frame == marker.frame),
                    )
                }
                Draft::Tag(tag) => {
                    ui.heading(tr("Animation tag"));
                    let mut start = (tag.start + 1).min(count);
                    let mut end = (tag.end + 1).min(count);
                    ui.horizontal(|ui| {
                        ui.label(tr("First frame"));
                        ui.add(egui::DragValue::new(&mut start).range(1..=count));
                        end = end.max(start);
                        ui.label(tr("Last frame"));
                        ui.add(egui::DragValue::new(&mut end).range(start..=count));
                    });
                    tag.start = start - 1;
                    tag.end = end - 1;
                    (
                        &mut tag.name,
                        &mut tag.color,
                        self.doc.extras.tags.iter().any(|t| t.id == tag.id),
                    )
                }
            };
            ui.text_edit_singleline(name);
            ui.horizontal(|ui| {
                ui.label(tr("Color"));
                ui.color_edit_button_srgb(color);
            });
            ui.separator();
            ui.horizontal(|ui| {
                save = button(
                    ui,
                    Icon::Save,
                    "Guardar",
                    "",
                    false,
                    !name.trim().is_empty(),
                )
                .clicked();
                delete = button(ui, Icon::Trash, "Eliminar", "", false, exists).clicked();
                cancel = button(ui, Icon::Close, "Cancelar", "", false, true).clicked();
            });
        });
        if save || delete {
            self.checkpoint();
            match draft {
                Draft::Marker(mut m) => {
                    self.doc.extras.markers.retain(|old| old.frame != m.frame);
                    if save {
                        m.name = clean_name(&m.name);
                        if !m.name.is_empty() {
                            self.doc.extras.markers.push(m);
                        }
                    }
                    self.doc.extras.markers.sort_by_key(|m| m.frame);
                }
                Draft::Tag(mut t) => {
                    self.doc.extras.tags.retain(|old| old.id != t.id);
                    if save {
                        t.name = clean_name(&t.name);
                        if !t.name.is_empty() {
                            self.animation.selected_tag = Some(t.id);
                            self.doc.extras.tags.push(t);
                        }
                    }
                    self.doc.extras.tags.sort_by_key(|t| (t.start, t.end));
                }
            }
            self.recover();
        } else if !cancel && !response.should_close() {
            self.animation.annotation = Some(draft);
        }
    }

    pub(crate) fn loop_range(&self) -> Option<(usize, usize)> {
        if let Some(range) = self.lip.preview_range {
            return Some(range);
        }
        if !self.animation.loop_tag
            || self.animation.play_all
            || !self.feature.symbol_stack.is_empty()
        {
            return None;
        }
        self.active_tag_range()
    }

    pub(crate) fn active_tag_range(&self) -> Option<(usize, usize)> {
        if !self.feature.symbol_stack.is_empty() {
            return None;
        }
        let count = self.doc.layers[0].frames.len();
        let tags = &self.doc.extras.tags;
        let tag = tags
            .iter()
            .find(|t| t.id == self.animation.selected_tag.unwrap_or(0) && t.start < count)
            .or_else(|| {
                tags.iter()
                    .find(|t| t.start <= self.frame && self.frame <= t.end)
            })
            .or_else(|| tags.iter().find(|t| t.start < count))?;
        Some((tag.start, tag.end.min(count - 1)))
    }

    pub(crate) fn toggle_timeline_play(&mut self) {
        self.finish_pending_pen();
        self.animation.play_all = false;
        self.playing = !self.playing;
        if self.playing {
            if let Some((start, end)) = self.loop_range()
                && !(start..=end).contains(&self.frame)
            {
                self.frame = start;
            }
            self.animation.clock = self.frame;
        }
        self.restart_media_audio();
        self.tick = std::time::Instant::now();
    }

    pub(crate) fn space_playback(&mut self, ctx: &egui::Context) {
        if self.hub.home
            || ctx.egui_wants_keyboard_input()
            || self.studio.preferences_open
            || self.studio.export.is_some()
            || self.document_settings.is_some()
            || self.animation.annotation.is_some()
            || self.view_options.draft.is_some()
            || egui::Popup::is_any_open(ctx)
        {
            self.animation.space_press = None;
            return;
        }
        let (pressed,released,mouse,time) = ctx.input(|i| (
            i.events.iter().any(|e| matches!(e,egui::Event::Key{key:egui::Key::Space,pressed:true,repeat:false,modifiers,..} if modifiers.is_none())),
            i.key_released(egui::Key::Space),
            i.pointer.any_down() || i.pointer.any_pressed(),
            i.time,
        ));
        if pressed {
            self.animation.space_press = Some((time, mouse));
        }
        if let Some((start, used)) = &mut self.animation.space_press {
            *used |= mouse;
            if released {
                let toggle = !*used && time - *start <= 0.25;
                self.animation.space_press = None;
                if toggle {
                    self.toggle_timeline_play();
                }
            }
        }
    }

    pub(crate) fn draw_animation_tags(
        &mut self,
        ui: &mut egui::Ui,
        origin: f32,
        y: f32,
        cell: f32,
        clip: Rect,
        count: usize,
    ) {
        if !self.feature.symbol_stack.is_empty() {
            return;
        }
        let ctx = ui.ctx().clone();
        let pointer = ctx.input(|i| i.pointer.interact_pos());
        let cancel = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if cancel {
            self.animation.tag_drag = None;
        }
        if let Some(drag) = &mut self.animation.tag_drag
            && let Some(pos) = pointer
        {
            let delta = ((pos.x - drag.origin) / drag.cell).round() as isize;
            drag.range = drag.handle.range(drag.original, delta, count - 1);
        }
        let tags = self.doc.extras.tags.clone();
        for tag in &tags {
            if tag.start >= count {
                continue;
            }
            let (start, end) = self
                .animation
                .tag_drag
                .as_ref()
                .filter(|drag| drag.id == tag.id)
                .map_or((tag.start, tag.end.min(count - 1)), |drag| drag.range);
            let left = origin + start as f32 * cell;
            let right = origin + (end + 1) as f32 * cell;
            let rect =
                Rect::from_min_max(Pos2::new(left, y), Pos2::new(right, y + 20.)).intersect(clip);
            if !rect.is_positive() {
                continue;
            }
            let [r, g, b] = tag.color;
            let color = Color32::from_rgb(r, g, b);
            let selected = self.animation.selected_tag == Some(tag.id);
            let painter = ui.painter().with_clip_rect(rect);
            painter.rect_filled(
                rect.shrink(1.),
                3.,
                color.gamma_multiply(if selected { 0.72 } else { 0.44 }),
            );
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                &tag.name,
                egui::FontId::proportional(10.),
                Color32::WHITE,
            );
            let response = ui
                .interact(
                    rect,
                    ui.scope_id().with(("animation-tag", tag.id)),
                    egui::Sense::click_and_drag(),
                )
                .on_hover_text(format!("{} · {}–{}", tag.name, start + 1, end + 1));
            let handle_at = |x: f32| {
                let edge = (cell * 0.24).min(7.);
                if x - left <= edge {
                    TagHandle::Start
                } else if right - x <= edge {
                    TagHandle::End
                } else {
                    TagHandle::Body
                }
            };
            if response.hovered()
                && let Some(pos) = pointer
            {
                ctx.set_cursor_icon(match handle_at(pos.x) {
                    TagHandle::Body => egui::CursorIcon::Grab,
                    _ => egui::CursorIcon::ResizeHorizontal,
                });
            }
            if !cancel
                && response.drag_started_by(egui::PointerButton::Primary)
                && let Some(press) = ctx.input(|i| i.pointer.press_origin())
            {
                self.animation.selected_tag = Some(tag.id);
                self.playing = false;
                self.restart_media_audio();
                self.animation.tag_drag = Some(TagDrag {
                    id: tag.id,
                    handle: handle_at(press.x),
                    origin: press.x,
                    cell,
                    original: (start, end),
                    range: (start, end),
                });
            }
            if response.clicked() {
                self.finish_pending_pen();
                self.animation.selected_tag = Some(tag.id);
                self.frame = tag.start;
                self.playing = false;
                self.scrub_media_audio(ui.ctx());
            }
            if response.secondary_clicked() || response.double_clicked() {
                self.animation.annotation = Some(Draft::Tag(tag.clone()));
            }
        }
        if let Some(drag) = &self.animation.tag_drag {
            ctx.set_cursor_icon(match drag.handle {
                TagHandle::Body => egui::CursorIcon::Grabbing,
                _ => egui::CursorIcon::ResizeHorizontal,
            });
        }
        if !ctx.input(|i| i.pointer.primary_down())
            && let Some(drag) = self.animation.tag_drag.take()
            && drag.range != drag.original
            && let Some(index) = self.doc.extras.tags.iter().position(|t| t.id == drag.id)
        {
            self.checkpoint();
            let tag = &mut self.doc.extras.tags[index];
            (tag.start, tag.end) = drag.range;
            self.doc.extras.tags.sort_by_key(|t| (t.start, t.end));
            self.recover();
        }
    }
}

fn clean_name(name: &str) -> String {
    name.trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(40)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_drag_moves_the_range_and_resizes_without_crossing_an_edge() {
        assert_eq!(TagHandle::Body.range((3, 7), 20, 10), (6, 10));
        assert_eq!(TagHandle::Body.range((3, 7), -8, 10), (0, 4));
        assert_eq!(TagHandle::Start.range((3, 7), 6, 10), (7, 7));
        assert_eq!(TagHandle::End.range((3, 7), 6, 10), (3, 10));
    }

    #[test]
    fn animation_tag_loop_includes_end_and_wraps_large_steps() {
        let ctx = egui::Context::default();
        let mut editor = Editor::new(&eframe::CreationContext::_new_kittest(ctx));
        editor.doc.extras.tags.push(Tag {
            id: 1,
            start: 3,
            end: 7,
            name: "Walk".into(),
            color: [64, 120, 240],
        });
        editor.animation.loop_tag = true;
        editor.toggle_timeline_play();
        assert_eq!(editor.frame, 3);
        editor.advance_animation(4);
        assert_eq!(editor.frame, 7);
        editor.advance_animation(1);
        assert_eq!(editor.frame, 3);
        editor.advance_animation(12);
        assert_eq!(editor.frame, 5);
    }
}
