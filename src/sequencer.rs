use crate::{
    Editor,
    design::{Icon, button, draw},
    document::Layer,
    features::{Folder, LayerOptions, id},
    i18n::tr,
};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
fn exposure_cell(
    ui: &egui::Ui,
    rect: egui::Rect,
    id: impl std::hash::Hash + std::fmt::Debug,
    label: &str,
    active: bool,
    row: usize,
) -> egui::Response {
    let response = ui.interact(
        rect,
        ui.scope_id().with(("exposure-cell", id)),
        egui::Sense::click(),
    );
    let colors = crate::themes::palette();
    let background = if active {
        crate::themes::accent().gamma_multiply(0.34)
    } else if response.hovered() {
        crate::themes::accent().gamma_multiply(0.18)
    } else if row % 2 == 0 {
        ui.visuals().faint_bg_color
    } else {
        crate::themes::panel()
    };
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    painter.rect_filled(rect.shrink(0.5), 0., background);
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(12.),
        colors.text,
    );
    response
}

#[derive(Clone, Copy)]
enum Row {
    Layer(usize, usize),
    Media(usize),
    Folder(usize, usize),
    Cameras,
    Rigging,
    Turn(bool),
}
#[derive(Default)]
pub(crate) struct LayerSelection {
    indices: Vec<usize>,
    anchor: Option<usize>,
    context: Option<(usize, usize, usize, Option<u64>)>,
}

impl LayerSelection {
    fn click(
        &mut self,
        rows: &[Row],
        active: usize,
        clicked: usize,
        extend: bool,
        toggle: bool,
    ) -> usize {
        if extend {
            let position = |index| {
                rows.iter()
                    .position(|row| matches!(row, Row::Layer(l, _) if *l == index))
            };
            let anchor = self
                .anchor
                .filter(|&l| position(l).is_some())
                .or_else(|| position(active).map(|_| active))
                .unwrap_or(clicked);
            self.anchor = Some(anchor);
            let a = position(anchor).unwrap();
            let b = position(clicked).unwrap();
            self.indices = rows[a.min(b)..=a.max(b)]
                .iter()
                .filter_map(|row| {
                    if let Row::Layer(l, _) = row {
                        Some(*l)
                    } else {
                        None
                    }
                })
                .collect();
        } else if toggle {
            if self.indices.is_empty() {
                self.indices.push(active);
            }
            if let Some(index) = self.indices.iter().position(|&l| l == clicked) {
                if self.indices.len() == 1 {
                    return active;
                }
                self.indices.remove(index);
                let active = if clicked == active {
                    self.indices[self.indices.len() - 1]
                } else {
                    active
                };
                self.anchor = Some(active);
                return active;
            }
            self.indices.push(clicked);
            self.anchor = Some(clicked);
        } else {
            self.indices.clear();
            self.indices.push(clicked);
            self.anchor = Some(clicked);
        }
        clicked
    }
}

#[derive(Clone, Copy)]
struct TimelineViewport {
    cell: f32,
    offset: f32,
    area: Rect,
    panning: bool,
}

impl Default for TimelineViewport {
    fn default() -> Self {
        Self {
            cell: 22.,
            offset: 0.,
            area: Rect::NOTHING,
            panning: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DropSide {
    Above,
    Inside,
    Below,
}
impl DropSide {
    fn for_row(rect: Rect, y: f32, folder: bool) -> Self {
        let fraction = (y - rect.top()) / rect.height();
        if folder && (0.25..=0.75).contains(&fraction) {
            Self::Inside
        } else if fraction < 0.5 {
            Self::Above
        } else {
            Self::Below
        }
    }
}

fn in_folder(doc: &crate::Document, mut parent: Option<u64>, id: u64) -> bool {
    for _ in 0..doc.extras.folders.len() {
        let Some(current) = parent else { break };
        if current == id {
            return true;
        }
        parent = doc
            .extras
            .folders
            .iter()
            .find(|f| f.id == current)
            .and_then(|f| f.parent);
    }
    false
}

fn folder_range(doc: &crate::Document, index: usize) -> std::ops::Range<usize> {
    let folder = &doc.extras.folders[index];
    let mut indices = doc
        .layers
        .iter()
        .enumerate()
        .filter(|(_, layer)| in_folder(doc, layer.options.folder, folder.id))
        .map(|(index, _)| index);
    if let Some(first) = indices.next() {
        first..indices.last().unwrap_or(first) + 1
    } else {
        let slot = folder
            .timeline_slot
            .unwrap_or(doc.layers.len())
            .min(doc.layers.len());
        slot..slot
    }
}

fn rows(e: &Editor, out: &mut Vec<Row>) {
    let folders = &e.doc.extras.folders;
    let indices: std::collections::HashMap<_, _> = folders
        .iter()
        .enumerate()
        .map(|(index, folder)| (folder.id, index))
        .collect();
    let mut top = vec![None; folders.len()];
    for (layer_index, layer) in e.doc.layers.iter().enumerate().rev() {
        let mut parent = layer.options.folder;
        while let Some(&index) = parent.as_ref().and_then(|id| indices.get(id)) {
            if top[index].is_some() {
                break;
            }
            top[index] = Some(layer_index + 1);
            parent = folders[index].parent;
        }
    }
    for (slot, folder) in top.iter_mut().zip(folders.iter()) {
        if slot.is_none() {
            *slot = Some(
                folder
                    .timeline_slot
                    .unwrap_or(e.doc.layers.len())
                    .min(e.doc.layers.len()),
            );
        }
    }
    append_rows(e, &top, None, 0, out);
}

fn append_rows(
    e: &Editor,
    top: &[Option<usize>],
    parent: Option<u64>,
    depth: usize,
    out: &mut Vec<Row>,
) {
    if depth > 200 {
        return;
    }
    let mut children: Vec<_> = e
        .doc
        .extras
        .folders
        .iter()
        .enumerate()
        .filter(|(_, folder)| folder.parent == parent)
        .map(|(index, _)| (top[index].unwrap_or_default(), Row::Folder(index, depth)))
        .chain(
            e.doc
                .layers
                .iter()
                .enumerate()
                .filter(|(_, layer)| layer.options.folder == parent)
                .map(|(index, _)| (index + 1, Row::Layer(index, depth))),
        )
        .collect();
    children.sort_by_key(|(slot, _)| std::cmp::Reverse(*slot));
    for (_, row) in children {
        out.push(row);
        if let Row::Folder(index, _) = row {
            let folder = &e.doc.extras.folders[index];
            if !folder.collapsed {
                append_rows(e, top, Some(folder.id), depth + 1, out);
            }
        }
    }
}

fn retimed(frame: usize, drag: crate::features::FrameDrag, target: usize) -> usize {
    if frame > 0 && (drag.start..=drag.end).contains(&frame) {
        (frame as isize + target as isize - drag.start as isize) as usize
    } else {
        frame
    }
}
impl Editor {
    pub(crate) fn frame_drop_valid(&self, drag: crate::features::FrameDrag, target: usize) -> bool {
        let Some(layer) = self.doc.layers.get(drag.layer) else {
            return false;
        };
        if drag.start > drag.end
            || !(drag.start..=drag.end).contains(&drag.grab)
            || drag.end >= layer.frames.len()
            || target >= 10000
            || layer.options.locked
            || layer.options.mannequin.is_some()
        {
            return false;
        }
        let mut previous = None;
        let mut movable = false;
        for f in (0..layer.frames.len()).filter(|&f| layer.key(f)) {
            let next = retimed(f, drag, target);
            movable |= f > 0 && (drag.start..=drag.end).contains(&f);
            if next >= 10000 || previous.is_some_and(|p| p >= next) {
                return false;
            }
            previous = Some(next);
        }
        movable
            && layer
                .options
                .tweens
                .iter()
                .all(|s| retimed(s.start, drag, target) < retimed(s.end, drag, target))
    }
    pub(crate) fn move_timeline_frames(
        &mut self,
        drag: crate::features::FrameDrag,
        target: usize,
    ) -> bool {
        if target == drag.start || !self.frame_drop_valid(drag, target) {
            return false;
        }
        self.finish_pending_pen();
        self.checkpoint();
        let layer = &mut self.doc.layers[drag.layer];
        layer.init_keys();
        let first = (drag.start..=drag.end).find(|&f| f > 0 && layer.key(f));
        let last = (drag.start..=drag.end)
            .rev()
            .find(|&f| f > 0 && layer.key(f));
        let keys: Vec<_> = (0..layer.frames.len())
            .filter(|&f| layer.key(f))
            .map(|f| (retimed(f, drag, target), layer.frames[f].clone()))
            .collect();
        let count = layer.frames.len().max(keys.last().unwrap().0 + 1);
        let mut owner = 0;
        layer.frames = (0..count)
            .map(|f| {
                if owner + 1 < keys.len() && keys[owner + 1].0 <= f {
                    owner += 1;
                }
                keys[owner].1.clone()
            })
            .collect::<Vec<_>>()
            .into();
        layer.options.keys = vec![false; count].into();
        for (f, _) in keys {
            layer.options.keys[f] = true;
        }
        layer.options.drawing_classes = std::mem::take(&mut layer.options.drawing_classes)
            .into_iter()
            .map(|(f, c)| (retimed(f, drag, target), c))
            .collect();
        for span in &mut layer.options.tweens {
            span.start = retimed(span.start, drag, target);
            span.end = retimed(span.end, drag, target);
        }
        for other in &mut self.doc.layers {
            if other.frames.len() < count {
                if crate::animation_clips::resize_track(other, count) {
                    continue;
                }
                other.init_keys();
                let held = other.frames.last().cloned().unwrap_or_default();
                other.frames.resize(count, held);
                other.options.keys.resize(count, false);
            }
        }
        self.layer = drag.layer;
        self.frame = (target + drag.grab - drag.start).min(count - 1);
        self.feature.frame_selection = first.zip(last).map(|(a, b)| {
            (
                drag.layer,
                retimed(a, drag, target),
                retimed(b, drag, target),
            )
        });
        self.playing = false;
        self.fit_camera_duration();
        self.recover();
        true
    }
    pub(crate) fn timeline_layer_action(&mut self, layer: usize, action: usize) {
        if layer >= self.doc.layers.len() {
            return;
        }
        self.layer = layer;
        if action == 0 {
            self.renaming_layer = Some(layer);
            self.rename_focus = true;
            return;
        }
        if action == 4 {
            self.feature.frame_selection =
                Some((layer, 0, self.doc.layers[layer].frames.len() - 1));
            return;
        }
        if action == 1
            && ((self.doc.layers.len() == 1
                && !self.doc.layers[layer].options.animation_track
                && self.doc.layers[layer].options.adjustment.is_none())
                || self.doc.layers[layer].options.locked)
        {
            return;
        }
        self.finish_pending_pen();
        self.checkpoint();
        match action {
            1 => {
                let count = self.doc.layers[layer].frames.len();
                self.doc.layers.remove(layer);
                if self.doc.layers.is_empty() {
                    let mut blank = crate::fresh_document().layers[0].clone();
                    blank.frames = vec![Default::default(); count].into();
                    blank.options.keys = vec![false; count].into();
                    blank.options.keys[0] = true;
                    self.doc.layers.push(blank);
                }
                self.layer = layer.min(self.doc.layers.len() - 1);
                self.clips.clear_track_selection();
                self.feature.row_drag = None;
                self.renaming_layer = None;
                self.feature.frame_selection = None;
                self.feature.frame_drag = None;
            }
            2 => self.doc.layers[layer].visible = !self.doc.layers[layer].visible,
            3 => self.doc.layers[layer].options.locked = !self.doc.layers[layer].options.locked,
            _ => return,
        }
        self.recover();
    }

    pub fn timeline_edit(&mut self, action: usize) {
        if matches!(
            self.tool,
            crate::canvas::Tool::Bones | crate::canvas::Tool::Puppet
        ) {
            self.rig_key_action(action);
            return;
        }
        self.timeline_layer_edit(action);
    }
    fn timeline_layer_edit(&mut self, action: usize) {
        if self.doc.layers[self.layer].options.animation_track || self.adjustment_active() {
            return;
        }
        if self.doc.layers[self.layer].options.locked
            || self.doc.layers[self.layer].options.mannequin.is_some()
        {
            return;
        }
        self.finish_pending_pen();
        self.checkpoint();
        let f = self.frame;
        let layer = &mut self.doc.layers[self.layer];
        layer.init_keys();
        let exposure = layer.exposure(f);
        if action == 0 && crate::animation::span(layer, f).is_some() {
            layer.frames[f] = crate::animation::marks(layer, f);
        }
        match action {
            0 => {
                layer.options.keys[f] = true;
            }
            1 => {
                layer.options.keys[f] = true;
                layer.frames[f] = Default::default();
                for i in f + 1..exposure.end {
                    layer.frames[i] = Default::default();
                }
            }
            2 => {
                if layer.frames.len() < 10000 {
                    let content = layer.frames[f].clone();
                    layer.options.drawing_classes =
                        std::mem::take(&mut layer.options.drawing_classes)
                            .into_iter()
                            .map(|(frame, class)| {
                                (frame + usize::from(frame >= exposure.end), class)
                            })
                            .collect();
                    for t in &mut layer.options.tweens {
                        if t.start >= exposure.end {
                            t.start += 1;
                        }
                        if t.end >= exposure.end {
                            t.end += 1;
                        }
                    }
                    layer.frames.insert(exposure.end, content);
                    layer.options.keys.insert(exposure.end, false);
                }
            }
            3 => {
                if exposure.len() > 1 {
                    layer.options.drawing_classes =
                        std::mem::take(&mut layer.options.drawing_classes)
                            .into_iter()
                            .map(|(frame, class)| {
                                (frame - usize::from(frame >= exposure.end), class)
                            })
                            .collect();
                    for t in &mut layer.options.tweens {
                        if t.start >= exposure.end {
                            t.start -= 1;
                        }
                        if t.end >= exposure.end {
                            t.end -= 1;
                        }
                    }
                    layer.frames.remove(exposure.end - 1);
                    layer.options.keys.remove(exposure.end - 1);
                }
            }
            4 => {
                let at = exposure.start;
                layer.options.drawing_classes.remove(&at);
                layer
                    .options
                    .tweens
                    .retain(|s| s.start != at && s.end != at);
                let content = if at == 0 {
                    Default::default()
                } else {
                    layer.frames[at - 1].clone()
                };
                if at > 0 {
                    layer.options.keys[at] = false;
                }
                for i in at..exposure.end {
                    layer.frames[i] = content.clone();
                }
            }

            _ => {}
        }
        let count = self
            .doc
            .layers
            .iter()
            .map(|l| l.frames.len())
            .max()
            .unwrap_or(1)
            .max(1);
        for l in &mut self.doc.layers {
            if l.frames.len() < count {
                if crate::animation_clips::resize_track(l, count) {
                    continue;
                }
                l.init_keys();
                let content = if action == 2 {
                    l.frames.last().cloned().unwrap_or_default()
                } else {
                    Default::default()
                };
                l.frames.resize(count, content);
                l.options.keys.resize(count, action != 2);
            }
        }
        self.frame = self.frame.min(count - 1);
        self.fit_camera_duration();
        self.recover();
    }
    pub fn timeline_cell_action(&mut self, layer: usize, frame: usize, action: usize) {
        if layer >= self.doc.layers.len() || frame >= self.doc.layers[layer].frames.len() {
            return;
        }
        self.finish_pending_pen();
        self.layer = layer;
        self.frame = frame;
        self.playing = false;
        if self.doc.layers[layer].options.locked
            || self.doc.layers[layer].options.mannequin.is_some()
        {
            return;
        }
        match action {
            5 => {
                self.frame = self.doc.layers[layer].exposure(frame).start;
                self.create_motion();
            }
            6 => {
                if crate::animation::span(&self.doc.layers[layer], frame).is_some() {
                    self.checkpoint();
                    self.doc.layers[layer]
                        .options
                        .tweens
                        .retain(|s| !(s.start <= frame && frame < s.end));
                    self.recover();
                }
            }
            _ => self.timeline_layer_edit(action),
        }
    }
    fn add_layer_advanced(&mut self, raster: bool) {
        self.finish_raster_stroke();
        self.finish_pending_pen();
        if self.doc.layers.len() >= 100 {
            return;
        }
        self.checkpoint();
        let count = self.doc.layers[0].frames.len();
        let mut keys = vec![false; count];
        keys[0] = true;
        let number = self.doc.layers.len() + 1;
        let slot = self
            .doc
            .extras
            .folders
            .iter()
            .position(|folder| Some(folder.id) == self.feature.selected_folder)
            .map_or(self.doc.layers.len(), |index| {
                folder_range(&self.doc, index).end
            });
        for track in &mut self.doc.extras.media.tracks {
            if track.slot > slot {
                track.slot += 1;
            }
        }
        for folder in &mut self.doc.extras.folders {
            if let Some(position) = &mut folder.timeline_slot
                && *position > slot
            {
                *position += 1;
            }
        }
        self.doc.layers.insert(
            slot,
            Layer {
                name: if raster {
                    format!("{} {number}", tr("Raster layer"))
                } else {
                    format!("Layer {number}")
                },
                visible: true,
                frames: vec![Default::default(); count].into(),
                options: LayerOptions {
                    raster,
                    folder: self.feature.selected_folder,
                    keys: keys.into(),
                    ..Default::default()
                },
            },
        );
        self.layer = slot;
        self.recover();
    }
    fn delete_folder(&mut self) {
        if let Some(id) = self.feature.selected_folder.take() {
            if let Some(parent) = self
                .doc
                .extras
                .folders
                .iter()
                .find(|f| f.id == id)
                .map(|f| f.parent)
            {
                self.checkpoint();
                crate::folder_groups::preserve_ungroup(&mut self.doc, id);
                for f in &mut self.doc.extras.folders {
                    if f.parent == Some(id) {
                        f.parent = parent;
                    }
                }
                for l in &mut self.doc.layers {
                    if l.options.folder == Some(id) {
                        l.options.folder = parent;
                    }
                }
                self.doc.extras.folders.retain(|f| f.id != id);
                self.recover();
            }
        }
    }
    fn exposure_sheet(&mut self, ctx: &egui::Context) {
        if !self.animation.exposure_sheet {
            return;
        }
        let mut open = true;
        let mut next = None;
        let mut edit = None;
        let mut resize = None;
        let mut cadence = None;
        let mut transform = false;
        let mut label_edit = None;
        let selected = self.feature.frame_selection;
        let layers = &self.doc.layers;
        let frames = layers
            .iter()
            .map(|layer| layer.frames.len())
            .max()
            .unwrap_or(0);
        let colors = crate::themes::palette();
        let frame_width = 58.;
        let column_width = 122.;
        egui::Window::new(tr("Exposure sheet"))
            .id(egui::Id::unique("exposure-sheet"))
            .open(&mut open)
            .default_width(660.)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let available = selected.is_some_and(|(layer, _, _)| {
                        crate::frame_workflow::editable(&layers[layer])
                    });
                    for (count, label) in [(1, "On ones"), (2, "On twos"), (3, "On threes")] {
                        if button(ui, Icon::FrameExtend, label, "", false, available).clicked() {
                            cadence = Some(count);
                        }
                    }
                    transform = button(
                        ui,
                        Icon::Transform,
                        "",
                        "Transform keyframes",
                        false,
                        available,
                    )
                    .clicked();
                });
                ui.spacing_mut().item_spacing.y = 0.;
                let table_height = (ui.available_height() - 36.).max(100.);
                egui::ScrollArea::both().max_height(table_height).show_rows(
                    ui,
                    25.,
                    frames + 1,
                    |ui, rows| {
                        for row in rows {
                            let (rect, _) = ui.allocate_exact_size(
                                egui::vec2(frame_width + column_width * layers.len() as f32, 25.),
                                egui::Sense::hover(),
                            );
                            let header = row == 0;
                            let frame = row.saturating_sub(1);
                            let clip = ui.clip_rect();
                            let first = (((clip.left() - rect.left() - frame_width) / column_width)
                                .floor()
                                .max(0.)) as usize;
                            let last = (((clip.right() - rect.left() - frame_width) / column_width)
                                .ceil()
                                .max(0.)) as usize;
                            let number =
                                egui::Rect::from_min_size(rect.min, egui::vec2(frame_width, 25.));
                            if number.intersects(clip) {
                                let label = if header {
                                    tr("Frame").to_owned()
                                } else {
                                    format!("{:03}", frame + 1)
                                };
                                let response = exposure_cell(
                                    ui,
                                    number,
                                    ("number", row),
                                    &label,
                                    header || frame == self.frame,
                                    row,
                                );
                                if !header && response.clicked() {
                                    next = Some((self.layer, frame));
                                }
                            }
                            for (index, layer) in layers.iter().enumerate().take(last).skip(first) {
                                let cell = egui::Rect::from_min_size(
                                    rect.min
                                        + egui::vec2(frame_width + index as f32 * column_width, 0.),
                                    egui::vec2(column_width, 25.),
                                );
                                if header {
                                    exposure_cell(
                                        ui,
                                        cell,
                                        ("layer", index),
                                        &layer.name,
                                        index == self.layer,
                                        row,
                                    )
                                    .on_hover_text(&layer.name);
                                    continue;
                                }
                                let Some(marks) = layer.frames.get(frame) else {
                                    continue;
                                };
                                let (symbol, label) = if !layer.key(frame) {
                                    ("—", tr("Held exposure"))
                                } else if marks.is_empty() {
                                    ("○", tr("Blank keyframe"))
                                } else {
                                    ("●", tr("Keyframe"))
                                };
                                let pose = (index == self.layer)
                                    .then(|| self.lip_frame_label(frame))
                                    .flatten();
                                let class =
                                    layer
                                        .options
                                        .drawing_classes
                                        .get(&frame)
                                        .map_or("", |class| match class {
                                            1 => "  K",
                                            2 => "  B",
                                            3 => "  I",
                                            _ => "",
                                        });
                                let text = pose.map_or_else(
                                    || symbol.to_owned(),
                                    |pose| format!("{symbol}  {pose}"),
                                ) + class;
                                let response = exposure_cell(
                                    ui,
                                    cell,
                                    (index, row),
                                    &text,
                                    (index == self.layer && frame == self.frame)
                                        || selected.is_some_and(|(l, a, b)| {
                                            l == index && (a.min(b)..=a.max(b)).contains(&frame)
                                        }),
                                    row,
                                )
                                .on_hover_text(format!(
                                    "{} · {:03} · {label}",
                                    layer.name,
                                    frame + 1
                                ));
                                if response.clicked() {
                                    next = Some((index, frame));
                                }
                                if crate::frame_workflow::editable(layer) {
                                    response.context_menu(|ui| {
                                        for (icon, title, action) in [
                                            (Icon::KeyAdd, "Insert keyframe", 0),
                                            (Icon::BlankKeyAdd, "Insert blank keyframe", 1),
                                            (Icon::KeyRemove, "Delete keyframe", 4),
                                        ] {
                                            if button(ui, icon, title, "", false, true).clicked() {
                                                edit = Some((index, frame, action));
                                                ui.close();
                                            }
                                        }
                                        ui.menu_button(tr("Drawing classification"), |ui| {
                                            for (code, name) in [
                                                (0, "Clear classification"),
                                                (1, "Key drawing"),
                                                (2, "Breakdown"),
                                                (3, "In-between"),
                                            ] {
                                                if ui.button(tr(name)).clicked() {
                                                    label_edit = Some((index, frame, code));
                                                    ui.close();
                                                }
                                            }
                                        });
                                    });
                                    let id = ui.scope_id().with(("xsheet-edge", index, frame));
                                    let edge = egui::Rect::from_min_max(
                                        egui::pos2(cell.left() + 8., cell.bottom() - 5.),
                                        cell.right_bottom(),
                                    );
                                    let current = ui.data(|d| d.get_temp::<(usize, usize)>(id));
                                    if current.is_some()
                                        || frame + 1 == layer.frames.len()
                                        || layer.key(frame + 1)
                                    {
                                        let r = ui
                                            .interact(edge, id, egui::Sense::drag())
                                            .on_hover_cursor(egui::CursorIcon::ResizeVertical);
                                        if r.drag_started() {
                                            let span = layer.exposure(frame);
                                            ui.data_mut(|d| {
                                                d.insert_temp(id, (span.start, span.end))
                                            });
                                        }
                                        if let Some((start, end)) = current {
                                            let delta = (r.drag_delta().y / 25.).round() as isize;
                                            if r.dragged() {
                                                let y = cell.bottom() + delta as f32 * 25.;
                                                ui.painter().hline(
                                                    cell.x_range(),
                                                    y,
                                                    egui::Stroke::new(2., crate::themes::accent()),
                                                );
                                            }
                                            if r.drag_stopped() {
                                                resize = Some((
                                                    index,
                                                    start,
                                                    end,
                                                    end.saturating_add_signed(delta),
                                                ));
                                                ui.data_mut(|d| d.remove::<(usize, usize)>(id));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    },
                );
                ui.add_space(8.);
                ui.label(
                    egui::RichText::new(format!(
                        "● {}     ○ {}     — {}",
                        tr("Keyframe"),
                        tr("Blank keyframe"),
                        tr("Held exposure")
                    ))
                    .color(colors.text),
                );
            });
        self.animation.exposure_sheet = open;
        if let Some((layer, frame)) = next {
            self.playing = false;
            self.layer = layer;
            self.frame = frame.min(self.doc.layers[layer].frames.len() - 1);
            let anchor = if ctx.input(|i| i.modifiers.shift) {
                selected
                    .filter(|s| s.0 == layer)
                    .map_or(self.frame, |s| s.1)
            } else {
                self.frame
            };
            self.feature.frame_selection = Some((layer, anchor, self.frame));
            self.canvas.clear_selection();
        }
        if let Some((layer, frame, action)) = edit {
            self.timeline_cell_action(layer, frame, action);
            self.feature.frame_selection = Some((layer, self.frame, self.frame));
        }
        if let Some((layer, start, end, target)) = resize {
            self.resize_exposure(layer, start, end, target);
        }
        if let Some(step) = cadence {
            self.set_exposure_step(step);
        }
        if transform {
            self.open_batch_transform();
        }
        if let Some((layer, frame, code)) = label_edit {
            let frame = self.doc.layers[layer].exposure(frame).start;
            self.checkpoint();
            let classes = &mut self.doc.layers[layer].options.drawing_classes;
            if code == 0 {
                classes.remove(&frame);
            } else {
                classes.insert(frame, code);
            }
            self.recover();
        }
    }

    pub fn sequencer(&mut self, ui: &mut egui::Ui) {
        self.depth_window(ui.ctx());
        self.folder_dialog(ui.ctx());
        let previous_frame = self.frame;
        self.exposure_sheet(ui.ctx());
        self.frame_workflow_ui(ui.ctx());
        egui::Panel::bottom("timeline")
            .default_size(300.)
            .min_size(150.)
            .max_size(700.)
            .resizable(true)
            .show(ui, |ui| {
                if self.in_character_symbol() {
                    self.character_timeline(ui);
                    return;
                }
                let viewport_id = ui.scope_id().with("timeline-viewport");
                let mut viewport = ui.data(|data| data.get_temp::<TimelineViewport>(viewport_id)).unwrap_or_default();
                let previous_cell = viewport.cell;
                let available_width = ui.available_width();
                let mut fit = false;
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(tr("TIMELINE")).size(10.).color(crate::themes::muted()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if button(ui, Icon::CommandHelper, "COMMAND HELPER", "Show tool shortcuts", self.studio.command_helper, true).clicked() {
                            self.studio.command_helper = !self.studio.command_helper;
                        }
                        ui.separator();
                        let zoom_label = ui.label(tr("Zoom"));
                        ui.spacing_mut().slider_width = 85.;
                        ui.add(egui::Slider::new(&mut viewport.cell, 6.0..=66.0).logarithmic(true).show_value(false))
                            .labelled_by(zoom_label.id);
                        if button(ui, Icon::Fit, "", "Fit timeline to scene", false, true).clicked() {
                            let width = (available_width - 324.).max(1.);
                            viewport.cell = (width / self.doc.layers[0].frames.len() as f32).clamp(6., 66.);
                            fit = true;
                        }
                        let prepare = button(ui, Icon::Film, "Prepare preview", "Prepare preview", self.preparing_preview(), true);
                        egui::Popup::menu(&prepare).show(|ui| {
                            if button(ui, Icon::Film, "Prepare scene preview", "", false, true).clicked() {
                                self.prepare_preview(None);
                                ui.close();
                            }
                            let selection = self.feature.frame_selection.map(|(_, a, b)| (a.min(b), a.max(b)));
                            if button(ui, Icon::Select, "Prepare selected frames", "", false, selection.is_some()).clicked() {
                                self.prepare_preview(selection);
                                ui.close();
                            }
                            let range = self.active_tag_range();
                            if button(ui, Icon::Loop, "Prepare active tag", "", false, range.is_some()).clicked() {
                                self.prepare_preview(range);
                                ui.close();
                            }
                            if button(ui, Icon::Minus, "Stop preparing preview", "", false, self.preparing_preview()).clicked() {
                                self.stop_preparing_preview();
                                ui.close();
                            }
                        });
                    });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if button(
                        ui,
                        Icon::First,
                        "",
                        "Primer fotograma",
                        false,
                        self.frame > 0,
                    )
                    .clicked()
                    {
                        self.frame = 0;
                        self.playing = false;
                    }
                    if button(
                        ui,
                        Icon::Previous,
                        "",
                        "Fotograma anterior",
                        false,
                        self.frame > 0,
                    )
                    .clicked()
                    {
                        self.frame -= 1;
                        self.playing = false;
                    }
                    if button(
                        ui,
                        if self.playing {
                            Icon::Pause
                        } else {
                            Icon::Play
                        },
                        "",
                        "Reproducir",
                        self.playing,
                        true,
                    )
                    .clicked()
                    {
                        self.toggle_timeline_play();
                    }
                    if button(
                        ui,
                        Icon::Next,
                        "",
                        "Fotograma siguiente",
                        false,
                        self.frame + 1 < self.doc.layers[0].frames.len(),
                    )
                    .clicked()
                    {
                        self.frame += 1;
                        self.playing = false;
                    }
                    if button(
                        ui,
                        Icon::Last,
                        "",
                        "Último fotograma",
                        false,
                        self.frame + 1 < self.doc.layers[0].frames.len(),
                    )
                    .clicked()
                    {
                        self.frame = self.doc.layers[0].frames.len() - 1;
                        self.playing = false;
                    }
                    ui.separator();
                    let total = self.doc.layers[0].frames.len();
                    let mut frame = self.frame + 1;
                    if ui.add(egui::DragValue::new(&mut frame).range(1..=total).update_while_editing(false)).on_hover_text(tr("Go to frame")).changed() {
                        self.frame = frame - 1;
                        self.playing = false;
                    }
                    ui.label(format!("/ {total}"));
                    if button(ui, Icon::Layers, "", "Exposure sheet", self.animation.exposure_sheet, true).clicked() {
                        self.animation.exposure_sheet = !self.animation.exposure_sheet;
                    }
                    let mut fps = self.doc.fps;
                    if ui
                        .add(
                            egui::DragValue::new(&mut fps)
                                .range(1..=240)
                                .suffix(crate::i18n::tr(" fps"))
                                .update_while_editing(false),
                        )
                        .changed()
                    {
                        self.set_fps(fps);
                    }
                    if !self.duration_editing {
                        self.timeline_duration =
                            self.doc.layers[0].frames.len() as f32 / self.doc.fps as f32;
                    }
                    let response = ui.add(
                        egui::DragValue::new(&mut self.timeline_duration)
                            .range(1. / self.doc.fps as f32..=10000. / self.doc.fps as f32)
                            .speed(0.05)
                            .suffix(crate::i18n::tr(" s"))
                            .update_while_editing(false),
                    );
                    let editing = response.dragged() || response.has_focus();
                    if response.drag_stopped()
                        || response.lost_focus()
                        || (response.changed() && !editing)
                    {
                        self.set_duration(self.timeline_duration);
                    }
                    self.duration_editing = editing;
                    ui.separator();
                    if button(ui, Icon::Reference, "", "Shift & Trace", self.animation.frame_workflow.tracing(), true).clicked() {
                        self.toggle_shift_trace();
                    }
                    if button(ui, Icon::Onion, "", "Papel cebolla", self.onion, true).clicked() {
                        self.onion = !self.onion;
                    }
                    ui.add(
                        egui::DragValue::new(&mut self.studio.onion_before)
                            .range(0..=24)
                            .prefix(crate::i18n::tr("< ")),
                    );
                    ui.add(
                        egui::DragValue::new(&mut self.studio.onion_after)
                            .range(0..=24)
                            .suffix(crate::i18n::tr(" >")),
                    );
                    ui.separator();
                    if button(ui,Icon::Loop,"","Loop animation tag",self.animation.loop_tag,!self.doc.extras.tags.is_empty() && self.feature.symbol_stack.is_empty()).clicked(){
                        self.animation.loop_tag = !self.animation.loop_tag;
                        if self.playing && let Some((start,end))=self.loop_range() && !(start..=end).contains(&self.frame) {
                            self.frame=start;
                            self.restart_media_audio();
                        }
                    }
                });
                ui.separator();
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.x = 3.;
                        ui.set_width(150.);
                        ui.label(egui::RichText::new(tr("CAPAS")).small().color(crate::themes::muted()));
                        ui.separator();
                        ui.horizontal(|ui| {
                            let add = button(ui, Icon::LayerAdd, "", "Añadir capa", false, self.doc.layers.len() < 100);
                            egui::Popup::menu(&add).show(|ui| {
                                if button(ui, Icon::LayerAdd, "New vector layer", "New vector layer", false, true).clicked() {
                                    self.add_layer_advanced(false);
                                    ui.close();
                                }
                                if button(ui, Icon::LayerAdd, "New raster layer", "New raster layer", false, true).clicked() {
                                    self.add_layer_advanced(true);
                                    ui.close();
                                }
                            });
                            if button(
                                ui,
                                Icon::LayerRemove,
                                "",
                                "Eliminar capa",
                                false,
                                (self.doc.layers.len() > 1 || self.adjustment_active()) && !self.doc.layers[self.layer].options.locked,
                            )
                            .clicked()
                            {
                                self.timeline_layer_action(self.layer,1);
                            }
                            if button(
                                ui,
                                Icon::Mask,
                                "",
                                "Máscara de recorte",
                                self.doc.layers[self.layer].options.clip,
                                !self.adjustment_active() && (self.layer > 0 || self.doc.layers[self.layer].options.clip),
                            )
                            .clicked()
                            {
                                self.checkpoint();
                                self.doc.layers[self.layer].options.clip =
                                    !self.doc.layers[self.layer].options.clip;
                                self.recover();
                            }
                            if button(ui,Icon::Depth,"","Profundidad de capas",self.depth.open,true).clicked() { self.depth.open = !self.depth.open; }
                            if button(ui, Icon::Layers, "", "Layer objects", self.studio.layer_objects_open, true).clicked() {
                                self.studio.layer_objects_open = !self.studio.layer_objects_open;
                                ui.ctx().request_repaint();
                            }
                        });
                    });
                    let (divider, _) = ui.allocate_exact_size(Vec2::new(1., 49.), Sense::hover());
                    ui.painter().line_segment(
                        [divider.left_top(), divider.left_bottom()],
                        Stroke::new(1., crate::themes::line()),
                    );
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.x = 3.;
                        ui.set_width(67.);
                        ui.label(
                            egui::RichText::new(tr("CARPETAS"))
                                .small()
                                .color(crate::themes::muted()),
                        );
                        ui.separator();
                        ui.horizontal(|ui| {
                            if button(
                                ui,
                                Icon::FolderPlus,
                                "",
                                "Crear carpeta",
                                false,
                                self.doc.extras.folders.len() < 200,
                            )
                            .clicked()
                            {
                                self.checkpoint();
                                let id = id();
                                let number = self.doc.extras.folders.len() + 1;
                                self.doc.extras.folders.push(Folder {
                                    id,
                                    name: format!("Folder {}", number),
                                    parent: self.feature.selected_folder,
                                    collapsed: false,
                                    timeline_slot: None,
                                transform:Default::default(), range:None, source_shift:0,
});
                                self.feature.selected_folder = Some(id);
                                self.recover();
                            }
                            if button(
                                ui,
                                Icon::FolderRemove,
                                "",
                                "Eliminar carpeta",
                                false,
                                self.feature.selected_folder.is_some(),
                            )
                            .clicked()
                            {
                                self.delete_folder();
                            }
                        });
                    });
                    let (divider, _) = ui.allocate_exact_size(Vec2::new(1., 49.), Sense::hover());
                    ui.painter().line_segment(
                        [divider.left_top(), divider.left_bottom()],
                        Stroke::new(1., crate::themes::line()),
                    );
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.x = 3.;
                        ui.set_width(275.);
                        ui.label(
                            egui::RichText::new(tr("FOTOGRAMAS"))
                                .small()
                                .color(crate::themes::muted()),
                        );
                        ui.separator();
                        ui.horizontal(|ui| {
                            if button(ui, Icon::AutoKey, "", "Auto keyframe when painting", self.studio.prefs.auto_key_paint, true).clicked() {
                                self.studio.prefs.auto_key_paint = !self.studio.prefs.auto_key_paint;
                                if let Err(error) = self.studio.save() { self.status = error; }
                            }
                            for (action, icon, tip) in [
                                (0, Icon::KeyAdd, "Fotograma clave"),
                                (1, Icon::BlankKeyAdd, "Fotograma clave en blanco"),
                                (2, Icon::FrameExtend, "Ampliar duración de fotograma"),
                                (3, Icon::FrameShorten, "Reducir duración de fotograma"),
                                (4, Icon::KeyRemove, "Eliminar fotograma clave"),
                            ] {
                                if button(
                                    ui,
                                    icon,
                                    "",
                                    tip,
                                    false,
                                    !self.doc.layers[self.layer].options.locked && !self.doc.layers[self.layer].options.animation_track && !self.adjustment_active(),
                                )
                                .clicked()
                                {
                                    self.timeline_edit(action);
                                }
                            }
                            if button(ui, Icon::Tween, "", "Motion tween", false, !self.doc.layers[self.layer].options.animation_track && !self.doc.layers[self.layer].options.raster).clicked() {
                                self.create_motion();
                            }
                            if button(ui,Icon::Film,"","Create Animation Clip…",false,true).clicked(){self.begin_animation_clip();}
                        });
                    });
                    let (divider, _) = ui.allocate_exact_size(Vec2::new(1., 49.), Sense::hover());
                    ui.painter().line_segment([divider.left_top(),divider.left_bottom()],Stroke::new(1.,crate::themes::line()));
                    ui.vertical(|ui| {
                        ui.set_min_width(140.);
                        ui.label(egui::RichText::new(tr("POST-PROCESSING")).small().color(crate::themes::muted()));
                        ui.separator();
                        ui.horizontal(|ui| {
                        if button(ui,Icon::Fx,"","New adjustment layer",false,self.doc.layers.len()<100 && self.feature.symbol_stack.is_empty()).clicked(){
                            self.add_layer_advanced(false);
                            let layer=&mut self.doc.layers[self.layer];
                            layer.name=tr("Adjustment layer").into();
                            layer.options.adjustment=Some(Default::default());
                            self.recover();
                        }
                        if !self.feature.symbol_stack.is_empty()
                            && button(ui, Icon::Depth, "", "Add relief guide", false, self.doc.layers.len() < 100).clicked()
                        {
                            self.add_layer_advanced(false);
                            self.doc.layers[self.layer].name = tr("Relief guide").into();
                            self.doc.layers[self.layer].options.height_guide = Some(0.5);
                            self.recover();
                        }
                        if !self.feature.symbol_stack.is_empty()
                            && button(ui, Icon::Mask, "", "Add light mask", false, self.doc.layers.len() < 100).clicked()
                        {
                            self.open_symbol_light_mask();
                        }
                        let lights = button(ui, Icon::Bulb, "", "Add light", false, self.feature.symbol_stack.is_empty());
                        egui::Popup::menu(&lights).show(|ui| {
                            for kind in [crate::light::Kind::Point, crate::light::Kind::Directional, crate::light::Kind::Ambient, crate::light::Kind::Rim, crate::light::Kind::RimShadow, crate::light::Kind::Spot] {
                                if button(ui, Icon::Bulb, kind.name(), "", false, self.doc.layers.len() < 100).clicked() {
                                    self.add_layer_advanced(false);
                                    let light = crate::light::Light::new(self.doc.width, self.doc.height, kind);
                                    let layer = &mut self.doc.layers[self.layer];
                                    layer.name = tr(kind.name()).into();
                                    layer.options.light = Some(light);
                                    self.recover();
                                    ui.close();
                                }
                            }
                            ui.separator();
                            self.light_presets_ui(ui);
                        });
                        });
                    });
                });
                ui.separator();
                let selection_context = (self.layer, self.doc.layers.len(), self.doc.extras.active_scene,
                    self.feature.symbol_stack.last().map(|entry| entry.3));
                if self.feature.layer_selection.context != Some(selection_context) {
                    self.feature.layer_selection = LayerSelection {
                        context: Some(selection_context),
                        ..Default::default()
                    };
                }
                let mut all = vec![];
                if !self.doc.extras.cameras.is_empty() {
                    all.push(Row::Cameras);
                }
                if !self.doc.extras.rig.is_empty() {
                    all.push(Row::Rigging);
                    if self.animation.rig_channels_open {
                        let turns = &self.doc.extras.rig.character_turns;
                        if turns.head.root.is_some() { all.push(Row::Turn(true)); }
                        if turns.body.root.is_some() { all.push(Row::Turn(false)); }
                    }
                }
                rows(self, &mut all);
                for (index, track) in self.doc.extras.media.tracks.iter().enumerate().rev() {
                    let slot = track.slot.min(self.doc.layers.len());
                    let at = all.iter().position(|row| matches!(row, Row::Layer(layer, _) if *layer < slot)).unwrap_or(all.len());
                    all.insert(at, Row::Media(index));
                }
                let mut layer_menu_action = None;
                let mut delete_rigging=false;
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    self.feature.frame_drag = None;
                    self.cancel_adjustment_drag();
                }
                if let Some(drag) = &mut self.feature.frame_drag { drag.target = None; }
                let mut target = None;
                let mut camera_target = None;
                let label_w = 300.;
                let mut scroll = egui::ScrollArea::both().id_salt("timeline-scroll").auto_shrink([false, false])
                    .scroll_source(egui::scroll_area::ScrollSource::SCROLL_BAR | egui::scroll_area::ScrollSource::MOUSE_WHEEL);
                let mut offset = viewport.offset;
                let (hover, delta, middle_pressed, middle_down) = ui.input(|i| (
                    i.pointer.hover_pos(), i.pointer.delta().x,
                    i.pointer.button_pressed(egui::PointerButton::Middle),
                    i.pointer.button_down(egui::PointerButton::Middle),
                ));
                let over = hover.is_some_and(|pos| viewport.area.contains(pos)) && ui.rect_contains_pointer(viewport.area);
                if middle_pressed { viewport.panning = over; }
                viewport.panning &= middle_down;
                if viewport.panning {
                    offset -= delta;
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                }
                if viewport.cell != previous_cell {
                    offset *= viewport.cell / previous_cell;
                }
                if fit { offset = 0.; }
                if fit || viewport.panning || viewport.cell != previous_cell {
                    scroll = scroll.horizontal_scroll_offset(offset.max(0.));
                }
                let original_style = ui.style().clone();
                let cell = viewport.cell;
                let output = ui.scope(|ui| {
                    ui.style_mut().spacing.scroll = egui::style::ScrollStyle {
                        bar_width: 12.,
                        ..egui::style::ScrollStyle::solid()
                    };
                    let accent = crate::themes::accent();
                    let visuals = ui.visuals_mut();
                    visuals.widgets.inactive.bg_fill = accent.gamma_multiply(0.75);
                    visuals.widgets.hovered.bg_fill = accent;
                    visuals.widgets.active.bg_fill = accent;
                    scroll.show(ui, |ui| {
                        ui.set_style(original_style);
                        let count = self.doc.layers[0].frames.len();
                        let row_h = 28.;
                        let tag_height = if self.doc.extras.tags.is_empty() || !self.feature.symbol_stack.is_empty() { 0. } else { 20. };
                        let header_height = 24. + tag_height;
                        let (r, _) = ui.allocate_exact_size(
                            Vec2::new(
                                label_w + (count + self.feature.frame_drag.map_or(0, |d| d.end-d.start+1)).max(self.feature.exposure_drag.map_or(count,|d|d.target)).max(self.folder_groups.timeline_end()).min(10000) as f32 * cell,
                                header_height + (all.len()+1) as f32 * row_h,
                            ),
                            Sense::hover(),
                        );
                        let painter = ui.painter().clone();
                        let viewport_clip = ui.clip_rect();
                        let header_top = viewport_clip.top();
                        let header_bottom = (header_top + header_height).min(viewport_clip.bottom());
                        let header_clip = viewport_clip.with_max_y(header_bottom);
                        let clip = viewport_clip.with_min_y(header_bottom);
                        let label_left = clip.left();
                        let labels_clip = Rect::from_min_max(clip.min, Pos2::new((label_left + label_w).min(clip.right()), clip.bottom()));
                        let grid_clip = Rect::from_min_max(Pos2::new((label_left + label_w).min(clip.right()), clip.top()), clip.max);
                        let frames = crate::workspace::visible_range(
                            r.left() + label_w,
                            cell,
                            count,
                            grid_clip.left(),
                            grid_clip.right(),
                        );
                        let visible = crate::workspace::visible_range(
                            r.top() + header_height,
                            row_h,
                            all.len(),
                            clip.top(),
                            clip.bottom(),
                        );
                        {
                            let header = Rect::from_min_max(header_clip.min, Pos2::new(r.right().max(grid_clip.left()), header_bottom));
                            let ruler_clip = header_clip.with_min_x(grid_clip.left());
                            let painter = painter.with_clip_rect(header_clip);
                            painter.rect_filled(
                                header,
                                0.,
                                crate::themes::color(Color32::from_gray(31)),
                            );
                            painter.text(
                                Pos2::new(label_left + 60., header_top + 12.),
                                egui::Align2::LEFT_CENTER,
                                tr("Nombre de capa"),
                                egui::FontId::proportional(10.),
                                crate::themes::muted(),
                            );
                            if Rect::from_min_size(Pos2::new(label_left, header_top), Vec2::new(label_w, 24.))
                                .contains(ui.input(|i| i.pointer.interact_pos().unwrap_or(Pos2::ZERO)))
                            {
                                target = Some((false, usize::MAX, DropSide::Above));
                            }
                            ui.set_clip_rect(ruler_clip);
                            let scrub = ui.interact(
                                Rect::from_min_max(Pos2::new(ruler_clip.left(), header_top), Pos2::new(header.right(), header_top+24.)),
                                ui.scope_id().with("scrub"),
                                Sense::click_and_drag(),
                            );
                            if scrub.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_down()) {
                                if let Some(p) = scrub.interact_pointer_pos() {
                                    self.frame = (((p.x - r.left() - label_w) / cell).floor().max(0.)
                                        as usize)
                                        .min(count - 1);
                                    self.playing = false;
                                }
                            }
                            self.timeline_marker_menu(ui, &scrub, r.left() + label_w, cell, count);
                            if tag_height > 0. {
                                self.draw_animation_tags(ui,r.left()+label_w,header_top+24.,cell,ruler_clip,count);
                            }
                            for marker in self.doc.extras.markers.iter().filter(|marker| frames.contains(&marker.frame)) {
                                let x = r.left() + label_w + (marker.frame as f32 + 0.5) * cell;
                                let y = header_top;
                                let [red,green,blue] = marker.color;
                                let color = Color32::from_rgb(red,green,blue);
                                painter.with_clip_rect(ruler_clip).add(egui::Shape::convex_polygon(
                                    vec![Pos2::new(x - 4., y + 1.), Pos2::new(x + 4., y + 1.), Pos2::new(x, y + 7.)],
                                    color, Stroke::NONE,
                                ));
                                if cell >= 20. {
                                    painter.with_clip_rect(ruler_clip).text(Pos2::new(x + 5., y + 1.), egui::Align2::LEFT_TOP,
                                        &marker.name, egui::FontId::proportional(9.), color);
                                }
                            }
                            for f in self.depth.ready_frames(&self.doc).filter(|f| frames.contains(f)) {
                                let x = r.left() + label_w + f as f32 * cell;
                                let strip = Rect::from_min_size(Pos2::new(x, header_top + 22.), Vec2::new(cell, 2.));
                                painter.with_clip_rect(ruler_clip).rect_filled(strip, 0., Color32::from_rgb(78, 192, 130));
                            }
                            for f in frames.clone() {
                                let pose_label = self.lip_frame_label(f);
                                if f == 0 || (f + 1) % 5 == 0 || pose_label.is_some() {
                                    let label_clip = if pose_label.is_some() {
                                        let left = r.left() + label_w + f as f32 * cell;
                                        ruler_clip.intersect(Rect::from_min_max(Pos2::new(left, header_top), Pos2::new(left + cell, header_top+24.)))
                                    } else {
                                        ruler_clip
                                    };
                                    let size = pose_label.as_ref().map_or(10., |name| (cell / (name.chars().count().max(1) as f32 * 0.62)).clamp(6., 10.));
                                    painter.with_clip_rect(label_clip).text(
                                        Pos2::new(
                                            r.left() + label_w + (f as f32 + 0.5) * cell,
                                            header_top + 12.,
                                        ),
                                        egui::Align2::CENTER_CENTER,
                                        pose_label.unwrap_or_else(|| (f + 1).to_string()),
                                        egui::FontId::monospace(size),
                                        crate::themes::muted(),
                                    );
                                }
                            }
                        }
                        let painter = painter.with_clip_rect(clip);
                        for row in visible {
                            ui.set_clip_rect(labels_clip);
                            let painter = ui.painter().with_clip_rect(labels_clip);
                            let y = r.top() + header_height + row as f32 * row_h;
                            let label = Rect::from_min_size(
                                Pos2::new(label_left, y),
                                Vec2::new(label_w, row_h),
                            );
                            match all[row] {
                                Row::Folder(index, depth) => {
                                    let folder = self.doc.extras.folders[index].clone();
                                    painter.rect_filled(
                                        label,
                                        0.,
                                        if self.folder_groups.selected == Some(folder.id) {
                                            crate::themes::color(Color32::from_rgb(50, 70, 85))
                                        } else {
                                            crate::themes::color(Color32::from_gray(39))
                                        },
                                    );
                                    draw(
                                        &painter,
                                        Rect::from_min_size(
                                            label.min + Vec2::new(8. + depth as f32 * 12., 4.),
                                            Vec2::splat(20.),
                                        ),
                                        Icon::Folder,
                                        crate::themes::muted(),
                                    );
                                    let response = ui.interact(
                                        label,
                                        ui.scope_id().with(("folder", folder.id)),
                                        Sense::click_and_drag(),
                                    );
                                    painter.text(
                                        label.min + Vec2::new(35. + depth as f32 * 12., 14.),
                                        egui::Align2::LEFT_CENTER,
                                        &folder.name,
                                        egui::FontId::proportional(11.),
                                        crate::themes::muted(),
                                    );
                                    if response.clicked() {
                                        self.select_folder_group(folder.id);
                                    }
                                    if response.double_clicked() {
                                        self.doc.extras.folders[index].collapsed =
                                            !folder.collapsed;
                                    }
                                    response.context_menu(|ui| {
                                        let mut name = folder.name.clone();
                                        if ui.text_edit_singleline(&mut name).changed() {
                                            self.checkpoint();
                                            self.doc.extras.folders[index].name = name;
                                            self.recover();
                                        }
                                        self.folder_context(ui,folder.id);
                                    });
                                    if response.drag_started_by(egui::PointerButton::Primary) {
                                        self.feature.row_drag = Some((true, index));
                                    }
                                    if label.intersect(labels_clip).contains(
                                        ui.input(|i| {
                                            i.pointer.interact_pos().unwrap_or(Pos2::ZERO)
                                        }),
                                    ) {
                                        let y = ui.input(|i| i.pointer.interact_pos().unwrap().y);
                                        target = Some((true, index, DropSide::for_row(label, y, true)));
                                    }
                                    ui.set_clip_rect(grid_clip);
                                    self.folder_track(ui,index,Rect::from_min_max(Pos2::new(grid_clip.left(),y),Pos2::new(grid_clip.right(),y+row_h)),r.left()+label_w,cell);
                                }
                                Row::Media(index) => {
                                    let track_at = |pos: Pos2| {
                                        if pos.x < grid_clip.left() || !clip.contains(pos) { return None; }
                                        let index = ((pos.y - r.top() - header_height) / row_h).floor();
                                        if index < 0. { return None; }
                                        match all.get(index as usize) {
                                            Some(Row::Media(track)) => Some((*track, r.top() + header_height + index * row_h)),
                                            _ => None,
                                        }
                                    };
                                    ui.set_clip_rect(clip);
                                    self.media_track_ui(ui, index, Rect::from_min_size(Pos2::new(r.left(), y), Vec2::new(r.width(), row_h)), label_w, cell, count, track_at);
                                }
                                Row::Layer(l, depth) => {
                                    let drop = ui.interact(label, ui.scope_id().with(("media-layer-drop", l)), Sense::hover());
                                    if drop.container_contains_pointer() && ui.input(|i| i.pointer.primary_released())
                                        && egui::DragAndDrop::has_payload_of_type::<crate::media::MediaTrackDrag>(ui.ctx())
                                        && let Some(track) = egui::DragAndDrop::take_payload::<crate::media::MediaTrackDrag>(ui.ctx())
                                    {
                                        let above = ui.input(|i| i.pointer.interact_pos()).is_some_and(|pos| pos.y < label.center().y);
                                        self.move_media_track(track.0, l + usize::from(above), None);
                                    }
                                    painter.rect_filled(
                                        label,
                                        0.,
                                        if l == self.layer {
                                            crate::themes::color(Color32::from_rgb(43, 65, 94))
                                        } else if self.doc.layers[l].options.light.is_some() && !self.feature.layer_selection.indices.contains(&l) {
                                            Color32::GOLD.gamma_multiply(0.20)
                                        } else if self.feature.layer_selection.indices.contains(&l) {
                                            crate::themes::accent().gamma_multiply(0.24)
                                        } else if self.doc.layers[l].options.mannequin.is_some() {
                                            crate::themes::color(Color32::from_rgb(69, 48, 91))
                                        } else {
                                            crate::themes::color(Color32::from_gray(42))
                                        },
                                    );
                                    let eye = Rect::from_min_size(
                                        label.min + Vec2::new(3., 3.),
                                        Vec2::splat(22.),
                                    );
                                    draw(
                                        &painter,
                                        eye,
                                        if self.doc.layers[l].visible {
                                            Icon::Eye
                                        } else {
                                            Icon::EyeOff
                                        },
                                        crate::themes::muted(),
                                    );
                                    if ui
                                        .interact(
                                            eye,
                                            ui.scope_id().with(("eye", l)),
                                            Sense::click(),
                                        )
                                        .clicked()
                                    {
                                        self.checkpoint();
                                        self.finish_pending_pen();
                                        self.doc.layers[l].visible = !self.doc.layers[l].visible;
                                        self.recover();
                                    }
                                    let lock = eye.translate(Vec2::new(25., 0.));
                                    draw(
                                        &painter,
                                        lock,
                                        Icon::Lock,
                                        if self.doc.layers[l].options.locked {
                                            crate::themes::accent()
                                        } else {
                                            crate::themes::color(Color32::from_gray(85))
                                        },
                                    );
                                    if ui
                                        .interact(
                                            lock,
                                            ui.scope_id().with(("lock", l)),
                                            Sense::click(),
                                        )
                                        .clicked()
                                    {
                                        self.checkpoint();
                                        self.finish_pending_pen();
                                        self.doc.layers[l].options.locked =
                                            !self.doc.layers[l].options.locked;
                                        self.recover();
                                    }
                                    if self.doc.layers[l].options.animation_track {
                                        let trash=Rect::from_min_size(Pos2::new(label.right()-83.,y+3.),Vec2::splat(22.));
                                        let response=ui.interact(trash,ui.scope_id().with(("delete-animation-track",l)),Sense::click()).on_hover_text(tr("Delete animation track"));
                                        draw(&painter,trash,Icon::Trash,if response.hovered(){Color32::LIGHT_RED}else{crate::themes::muted()});
                                        if response.clicked()&&!self.doc.layers[l].options.locked{layer_menu_action=Some((l,1));}
                                    }
                                    let name_rect = Rect::from_min_max(
                                        label.min + Vec2::new(57. + depth as f32 * 10., 2.),
                                        Pos2::new(label.right() - if self.doc.layers[l].options.animation_track {86.}else{60.}, label.bottom() - 2.),
                                    );
                                    if self.renaming_layer == Some(l) {
                                        let mut name = self.doc.layers[l].name.clone();
                                        let response = ui
                                            .put(name_rect, egui::TextEdit::singleline(&mut name));
                                        if self.rename_focus {
                                            response.request_focus();
                                            self.rename_focus = false;
                                        }
                                        if response.changed() {
                                            self.checkpoint();
                                            self.doc.layers[l].name = name;
                                            self.recover();
                                        }
                                        if response.lost_focus() {
                                            self.renaming_layer = None;
                                        }
                                    } else {
                                        let masked = self.doc.layers[l].options.clip || self.doc.layers[l].options.masked_by.is_some();
                                        let special = self.doc.layers[l].options.animation_track || crate::post::is_adjustment(&self.doc.layers[l]);
                                        use crate::studio::LayerColor;
                                        let color = self.studio.prefs.layer_color(if self.doc.layers[l].options.animation_track {
                                            LayerColor::Animation
                                        } else if crate::post::is_adjustment(&self.doc.layers[l]) {
                                            LayerColor::Adjustment
                                        } else if self.doc.layers[l].options.raster {
                                            LayerColor::Raster
                                        } else {
                                            LayerColor::Vector
                                        });
                                        let drawing = !special && self.doc.layers[l].options.mannequin.is_none();
                                        let reserved = if drawing { if masked { 34. } else { 17. } } else { 0. };
                                        painter.with_clip_rect(Rect::from_min_max(name_rect.min, name_rect.max - Vec2::new(reserved, 0.)).intersect(labels_clip)).text(
                                            name_rect.left_center(),
                                            egui::Align2::LEFT_CENTER,
                                            self.doc.layers[l].name.clone(),
                                            egui::FontId::proportional(11.),
                                            if self.doc.layers[l].options.mannequin.is_some()
                                                && crate::themes::current()
                                                    != crate::themes::Theme::Studio
                                            {
                                                Color32::from_rgb(236, 224, 248)
                                            } else {
                                                crate::themes::color(Color32::from_gray(210))
                                            },
                                        );
                                        let response = ui.interact(
                                            name_rect,
                                            ui.scope_id().with(("layer-row", l)),
                                            Sense::click_and_drag(),
                                        );
                                        let response = if self.doc.layers[l].options.animation_track { response } else {
                                            response.on_hover_text(tr(if self.doc.layers[l].options.raster { "Raster layer" } else { "Vector layer" }))
                                        };
                                        if drawing {
                                            let center = name_rect.right_center() - Vec2::new(if masked { 25. } else { 8. }, 0.);
                                            painter.rect_filled(Rect::from_center_size(center, Vec2::splat(14.)), 3., crate::themes::palette().raised);
                                            painter.text(center, egui::Align2::CENTER_CENTER, if self.doc.layers[l].options.raster { "R" } else { "V" }, egui::FontId::monospace(10.), color);
                                        }
                                        if special {
                                            let icon=if self.doc.layers[l].options.animation_track {Icon::Symbol} else if self.doc.layers[l].options.light.is_some() { Icon::Bulb } else {Icon::Fx};
                                            draw(&painter,Rect::from_center_size(name_rect.right_center()-Vec2::new(8.,0.),Vec2::splat(16.)),icon,if self.doc.layers[l].options.light.is_some() { Color32::GOLD } else { color });
                                        }
                                        if masked {
                                            draw(&painter, Rect::from_center_size(name_rect.right_center() - Vec2::new(8., 0.), Vec2::splat(15.)), Icon::Mask, crate::themes::accent());
                                        }
                                        let modifiers = ui.input(|i| i.modifiers);
                                        let toggle = modifiers.command || modifiers.ctrl;
                                        if response.clicked() {
                                            self.folder_groups.selected=None;
                                            self.layer = self.feature.layer_selection.click(&all, self.layer, l, modifiers.shift, toggle);
                                            self.feature.layer_selection.context = Some((self.layer, selection_context.1, selection_context.2, selection_context.3));
                                            self.feature.selected_folder =
                                                self.doc.layers[self.layer].options.folder;
                                            self.canvas.clear_selection();
                                            for &selected in &self.feature.layer_selection.indices {
                                                let layer = &self.doc.layers[selected];
                                                if layer.visible && !layer.options.locked && !crate::post::is_adjustment(layer) {
                                                    let count = layer.frames[self.frame].len().min(crate::animation::marks(layer, self.frame).len());
                                                    self.canvas.selected.extend((0..count).map(|mark| (selected, mark)));
                                                }
                                            }
                                            ui.ctx().request_repaint();
                                        }
                                        if response.double_clicked() && !toggle && !modifiers.shift {
                                            self.renaming_layer = Some(l);
                                            self.rename_focus = true;
                                        }
                                        if !toggle && !modifiers.shift && response.drag_started_by(egui::PointerButton::Primary) {
                                            self.feature.row_drag = Some((false, l));
                                        }
                                        if response.secondary_clicked() { self.layer = l; }
                                        response.context_menu(|ui| {
                                            ui.label(&self.doc.layers[l].name);
                                            ui.separator();
                                            for (action, icon, label, enabled) in [
                                                (0, Icon::Rename, "Rename layer", true),
                                                (2, Icon::Eye, if self.doc.layers[l].visible { "Hide layer" } else { "Show layer" }, true),
                                                (3, Icon::Lock, if self.doc.layers[l].options.locked { "Unlock layer" } else { "Lock layer" }, true),
                                                (4, Icon::Film, "Select all frames", !special),
                                                (1, Icon::Trash, if self.doc.layers[l].options.animation_track {"Delete animation track"}else{"Delete layer"}, (self.doc.layers.len() > 1 || special) && !self.doc.layers[l].options.locked),
                                            ] {
                                                if button(ui, icon, label, label, false, enabled).clicked() {
                                                    layer_menu_action = Some((l, action));
                                                    ui.close();
                                                }
                                            }
                                        });

                                    }
                                    let mut opacity = self.doc.layers[l].options.opacity * 100.;
                                    if ui
                                        .put(
                                            Rect::from_min_max(
                                                Pos2::new(label.right() - 56., y + 2.),
                                                label.max - Vec2::splat(2.),
                                            ),
                                            egui::DragValue::new(&mut opacity)
                                                .range(0. ..=100.)
                                                .suffix(crate::i18n::tr("%"))
                                                .update_while_editing(false),
                                        )
                                        .changed()
                                    {
                                        self.checkpoint();
                                        self.doc.layers[l].options.opacity = opacity / 100.;
                                        self.recover();
                                    }
                                    if label.intersect(labels_clip).contains(
                                        ui.input(|i| {
                                            i.pointer.interact_pos().unwrap_or(Pos2::ZERO)
                                        }),
                                    ) {
                                        let y = ui.input(|i| i.pointer.interact_pos().unwrap().y);
                                        target = Some((false, l, DropSide::for_row(label, y, false)));
                                    }
                                    ui.set_clip_rect(grid_clip);
                                    let painter = ui.painter().with_clip_rect(grid_clip);
                                    let animation_row = Rect::from_min_max(Pos2::new(r.left()+label_w,y),Pos2::new(r.right(),y+row_h));
                                    if crate::post::is_adjustment(&self.doc.layers[l]) {
                                        let band = animation_row.shrink2(Vec2::new(1.,4.));
                                        let selected = l == self.layer;
                                        if self.doc.layers[l].options.light.is_some() {
                                            let color = Color32::GOLD;
                                            painter.rect_filled(band, 4., color.gamma_multiply(if selected { 0.35 } else { 0.19 }));
                                            let origin = r.left() + label_w;
                                            let keyed = self.doc.layers[l].options.light.clone().unwrap();
                                            let locked = self.doc.layers[l].options.locked;
                                            let mut edit = None;
                                            for f in frames.clone() {
                                                let rect = Rect::from_min_size(Pos2::new(origin + f as f32 * cell, animation_row.top()), Vec2::new(cell, row_h));
                                                let has_key = keyed.keyed(f);
                                                if has_key {
                                                    let p = rect.center();
                                                    painter.add(egui::Shape::convex_polygon(
                                                        vec![p - Vec2::Y * 5., p + Vec2::X * 5., p + Vec2::Y * 5., p - Vec2::X * 5.],
                                                        color, Stroke::NONE));
                                                }
                                                let response = ui.interact(rect, ui.scope_id().with(("light-key", l, f)),
                                                    if has_key && !locked { Sense::click_and_drag() } else { Sense::click() });
                                                if response.clicked() || response.secondary_clicked() {
                                                    self.layer = l;
                                                    self.frame = f;
                                                    self.playing = false;
                                                }
                                                if response.dragged_by(egui::PointerButton::Primary) {
                                                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                                                }
                                                if response.drag_stopped_by(egui::PointerButton::Primary) && has_key
                                                    && let Some(pos) = response.interact_pointer_pos()
                                                    && animation_row.intersect(grid_clip).contains(pos)
                                                {
                                                    let dest = ((pos.x - origin) / cell).floor() as usize;
                                                    if dest < count { edit = Some((f, Some(dest))); }
                                                }
                                                response.context_menu(|ui| {
                                                    let (icon, label, dest) = if has_key {
                                                        (Icon::Minus, "Delete light key", None)
                                                    } else {
                                                        (Icon::Plus, "Add light key", Some(f))
                                                    };
                                                    if button(ui, icon, label, "", false, !locked).clicked() {
                                                        edit = Some((f, dest));
                                                        ui.close();
                                                    }
                                                });
                                            }
                                            if let Some((source, dest)) = edit {
                                                self.edit_light_key(l, source, dest);
                                            }
                                        } else {
                                            self.adjustment_track_ui(ui, l, animation_row, cell, count);
                                        }
                                        continue;
                                    }
                                    self.animation_drop_target(ui,animation_row,r.left()+label_w,cell,Some(l));
                                    if self.doc.layers[l].options.animation_track {
                                        self.animation_track_ui(ui,l,animation_row,r.left()+label_w,cell);
                                        continue;
                                    }
                                    let layer_color = self.studio.prefs.layer_color(if self.doc.layers[l].options.mannequin.is_some() {
                                        crate::studio::LayerColor::Mannequin
                                    } else if self.doc.layers[l].options.raster {
                                        crate::studio::LayerColor::Raster
                                    } else {
                                        crate::studio::LayerColor::Vector
                                    });
                                    for f in frames.clone() {
                                        let rect = Rect::from_min_size(
                                            Pos2::new(r.left() + label_w + f as f32 * cell, y),
                                            Vec2::new(cell, row_h),
                                        );
                                        let key = self.doc.layers[l].key(f);
                                        let color =
                                            if self.doc.layers[l].options.mannequin.is_some() {
                                                layer_color.gamma_multiply(0.36)
                                            } else if crate::animation::span(&self.doc.layers[l], f)
                                                .is_some()
                                                && f != self.frame
                                            {
                                                crate::themes::color(Color32::from_rgb(95, 61, 145))
                                            } else if f == self.frame {
                                                crate::themes::color(Color32::from_rgb(50, 88, 130))
                                            } else if key {
                                                layer_color.gamma_multiply(0.26)
                                            } else {
                                                layer_color.gamma_multiply(0.14)
                                            };
                                        painter.rect_filled(rect, 0., color);
                                        let has_mouth_key = self.doc.layers[l].frames[f].iter()
                                            .any(|mark| crate::lip::has_mouth_key(mark, f));
                                        if has_mouth_key {
                                            let p = rect.center_top() + Vec2::Y * 5.;
                                            painter.add(egui::Shape::convex_polygon(
                                                vec![p - Vec2::Y * 3., p + Vec2::X * 4., p + Vec2::Y * 3., p - Vec2::X * 4.],
                                                Color32::LIGHT_GREEN, Stroke::NONE
                                            ));
                                        }
                                        painter.rect_stroke(
                                            rect,
                                            0.,
                                            Stroke::new(
                                                0.5,
                                                crate::themes::color(Color32::from_gray(30)),
                                            ),
                                            egui::StrokeKind::Inside,
                                        );
                                        if self.doc.layers[l].options.mannequin.is_some() {
                                            if f == 0 {
                                                draw(
                                                    &painter,
                                                    rect.shrink(4.),
                                                    Icon::Mannequin,
                                                    crate::mannequin::COLOR,
                                                );
                                            }
                                        } else if key {
                                            if self.doc.layers[l].frames[f].is_empty() {
                                                painter.circle_stroke(
                                                    rect.center(),
                                                    3.,
                                                    Stroke::new(1., crate::themes::muted()),
                                                );
                                            } else {
                                                painter.circle_filled(
                                                    rect.center(),
                                                    3.5,
                                                    if f == self.frame {
                                                        crate::themes::accent()
                                                    } else {
                                                        crate::themes::muted()
                                                    },
                                                );
                                            }
                                        } else {
                                            painter.line_segment(
                                                [rect.left_center(), rect.right_center()],
                                                Stroke::new(
                                                    1.,
                                                    crate::themes::color(Color32::from_gray(85)),
                                                ),
                                            );
                                        }
                                        let response = ui.interact(
                                            rect,
                                            ui.scope_id().with(("cell", l, f)),
                                            Sense::click_and_drag(),
                                        );
                                        let editable = !self.doc.layers[l].options.locked
                                            && self.doc.layers[l].options.mannequin.is_none();
                                        if egui::DragAndDrop::has_payload_of_type::<crate::media::MediaDrag>(ui.ctx()) && let Some(asset) = response.dnd_release_payload::<crate::media::MediaDrag>() {
                                            if editable {
                                                self.layer = l;
                                                self.place_media(asset.0, f, None);
                                            }
                                        }

                                        if response.clicked() || response.secondary_clicked() {
                                            self.layer = l;
                                            self.frame = f;
                                            self.playing = false;
                                            let anchor = self.feature.frame_selection
                                                .filter(|s| s.0 == l).map_or(f, |s| s.1.min(count - 1));
                                            if response.clicked() || !self.feature.frame_selection.is_some_and(|(layer,a,b)| layer==l && (a.min(b)..=a.max(b)).contains(&f)) {
                                                self.feature.frame_selection = Some((l,
                                                    if response.clicked() && ui.input(|i| i.modifiers.shift) { anchor } else { f }, f));
                                            }
                                            if has_mouth_key && response.clicked() {
                                                self.feature.library = false;
                                                self.clips.open = false;
                                                ui.ctx().request_repaint();
                                            }
                                        }
                                        if response.drag_started_by(egui::PointerButton::Primary) && editable && self.feature.exposure_drag.is_none() {
                                            let (mut a, mut b) = self.feature.frame_selection
                                                .filter(|s| s.0 == l && (s.1.min(s.2)..=s.1.max(s.2)).contains(&f))
                                                .map_or((f, f), |s| (s.1.min(s.2), s.1.max(s.2)));
                                            if a == b {
                                                let exposure=self.doc.layers[l].exposure(f); a=exposure.start; b=exposure.end-1;
                                            }
                                            self.layer = l; self.frame = f;
                                            self.feature.frame_drag = Some(crate::features::FrameDrag {
                                                layer: l, start: a, end: b.min(count - 1), grab: f, target: None,
                                            });
                                            self.feature.frame_selection = Some((l, a, b.min(count - 1)));
                                            self.playing = false;
                                        }
                                        if let Some(drag) = &mut self.feature.frame_drag {
                                            if drag.layer == l && rect.contains(ui.input(|i| i.pointer.interact_pos().unwrap_or(Pos2::ZERO)))
                                                && clip.contains(ui.input(|i| i.pointer.interact_pos().unwrap_or(Pos2::ZERO))) {
                                                drag.target = Some(f.saturating_sub(drag.grab - drag.start)
                                                    .min(10000 - (drag.end - drag.start + 1)));
                                            }
                                        }
                                        if self.feature.frame_drag.is_none() && self.feature.frame_selection.is_some_and(|s|
                                            s.0 == l && (s.1.min(s.2)..=s.1.max(s.2)).contains(&f)) {
                                            painter.rect_stroke(rect.shrink(1.), 1.,
                                                Stroke::new(1.5, crate::themes::accent()), egui::StrokeKind::Inside);
                                        }

                                        response
                                            .on_hover_text(crate::i18n::message(
                                                "Frame {frame} · Shift-click: select range · drag: move · right-click: edit",
                                                &[("frame", (f + 1).to_string())],
                                            ))
                                            .context_menu(|ui| {
                                                ui.label(format!(
                                                    "{} · {}",
                                                    self.doc.layers[l].name,
                                                    f + 1
                                                ));
                                                ui.separator();
                                                if has_mouth_key {
                                                    self.mouth_timeline_menu(ui, l, f, false);
                                                    ui.separator();
                                                }
                                                if button(ui,Icon::Transform,"Transform keyframes","Transform keyframes",false,crate::frame_workflow::editable(&self.doc.layers[l])).clicked(){
                                                    if !self.feature.frame_selection.is_some_and(|(layer,a,b)| layer==l && (a.min(b)..=a.max(b)).contains(&f)) {self.feature.frame_selection=Some((l,f,f));}
                                                    self.open_batch_transform();
                                                    ui.close();
                                                }
                                                if button(ui,Icon::Copy,"Copy frames","Copy frames",false,true).clicked(){
                                                    self.frame_clipboard_context(l,f);self.copy_frames_to_clipboard(ui.ctx());ui.close();
                                                }
                                                if button(ui,Icon::Paste,"Paste frames","Paste frames",false,self.frame_clipboard.has_frames()&&!self.doc.layers[l].options.locked).clicked(){
                                                    self.frame_clipboard_context(l,f);self.paste_timeline_frames();ui.close();
                                                }
                                                ui.separator();
                                                if button(ui,Icon::Film,"Create Animation Clip…","Create Animation Clip…",false,true).clicked(){
                                                    if !self.feature.frame_selection.is_some_and(|s|s.0==l && (s.1..=s.2).contains(&f)) {self.feature.frame_selection=Some((l,f,f));}
                                                    self.layer=l;self.begin_animation_clip();ui.close();
                                                }
                                                let editable = !self.doc.layers[l].options.locked
                                                    && self.doc.layers[l]
                                                        .options
                                                        .mannequin
                                                        .is_none();
                                                for (action, label) in [
                                                    (0, "Insertar fotograma clave aquí"),
                                                    (1, "Insertar fotograma clave vacío aquí"),
                                                    (2, "Prolongar exposición"),
                                                    (3, "Acortar exposición"),
                                                    (4, "Borrar este fotograma clave"),
                                                    (5, "Aplicar interpolación a la capa"),
                                                    (6, "Quitar interpolación"),
                                                ] {
                                                    let enabled = editable
                                                        && match action {
                                                            3 => {
                                                                self.doc.layers[l].exposure(f).len()
                                                                    > 1
                                                            }
                                                            5 => {
                                                                !self.doc.layers[l].frames[f]
                                                                    .is_empty()
                                                                    && f + 1
                                                                        < self.doc.layers[l]
                                                                            .frames
                                                                            .len()
                                                            }
                                                            6 => crate::animation::span(
                                                                &self.doc.layers[l],
                                                                f,
                                                            )
                                                            .is_some(),
                                                            _ => true,
                                                        };
                                                    if button(
                                                        ui,
                                                        match action {
                                                            0 => Icon::KeyAdd,
                                                            1 => Icon::BlankKeyAdd,
                                                            2 => Icon::FrameExtend,
                                                            3 => Icon::FrameShorten,
                                                            4 => Icon::KeyRemove,
                                                            _ => Icon::Tween,
                                                        },
                                                        label,
                                                        label,
                                                        false,
                                                        enabled,
                                                    )
                                                    .clicked()
                                                    {
                                                        self.timeline_cell_action(l, f, action);
                                                        ui.close();
                                                    }
                                                }
                                            });
                                    }
                                    self.exposure_handles_ui(ui,l,animation_row,r.left()+label_w,cell,frames.clone());
                                    if let Some(drag) = &mut self.feature.frame_drag {
                                        if drag.layer == l {
                                            if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
                                                if clip.contains(p) && p.y >= y && p.y < y+row_h && p.x >= r.left()+label_w && p.x <= r.right() {
                                                    let frame=((p.x-r.left()-label_w)/cell).floor().max(0.) as usize;
                                                    drag.target=Some(frame.saturating_sub(drag.grab-drag.start).min(10000-(drag.end-drag.start+1)));
                                                }
                                            }
                                        }
                                    }
                                    if let Some(drag) = self.feature.frame_drag.filter(|d| d.layer == l) {
                                        if let Some(at) = drag.target {
                                            let color = if self.frame_drop_valid(drag, at) {
                                                crate::themes::accent() } else { Color32::LIGHT_RED };
                                            let x = r.left()+label_w+(at+drag.grab-drag.start) as f32*cell;
                                            let destination = Rect::from_min_size(Pos2::new(x,y),Vec2::new(cell,row_h));
                                            painter.rect_stroke(destination.shrink(1.),1.,Stroke::new(2.,color),egui::StrokeKind::Inside);
                                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                                        }
                                    }
                                }
                                Row::Rigging => {
                                    let trash=Rect::from_min_size(Pos2::new(label.right()-27.,y+3.),Vec2::splat(22.));
                                    let header=ui.interact(Rect::from_min_max(label.min,Pos2::new(trash.left()-3.,label.bottom())),ui.scope_id().with("rigging-track-header"),Sense::click());
                                    if header.clicked() { self.animation.rig_channels_open ^= true; }
                                    header.context_menu(|ui|{
                                        ui.label(tr("Deleting this track removes its bones, weights and keys. Artwork is kept; Undo restores the rig."));
                                        if button(ui,Icon::Trash,"Delete rigging track","Delete rigging track",false,true).clicked(){delete_rigging=true;ui.close();}
                                    });
                                    let delete=ui.interact(trash,ui.scope_id().with("delete-rigging-track"),Sense::click()).on_hover_text(tr("Delete rigging track"));
                                    if delete.clicked(){delete_rigging=true;}
                                    let color =
                                        self.studio.prefs.layer_color(crate::studio::LayerColor::Rigging);
                                    painter.rect_filled(
                                        label,
                                        0.,
                                        color.gamma_multiply(0.27),
                                    );
                                    draw(
                                        &painter,
                                        Rect::from_min_size(
                                            label.min + Vec2::new(8., 4.),
                                            Vec2::splat(20.),
                                        ),
                                        Icon::Bones,
                                        color,
                                    );
                                    painter.text(
                                        label.min + Vec2::new(36., 14.),
                                        egui::Align2::LEFT_CENTER,
                                        format!(
                                            "{} {} · {}",
                                            if self.animation.rig_channels_open { "▾" } else { "▸" },
                                            tr("Rigging"),
                                            tr("Bone pose"),
                                        ),
                                        egui::FontId::proportional(11.),
                                        color,
                                    );
                                    draw(&painter,trash,Icon::Trash,if delete.hovered(){Color32::LIGHT_RED}else{crate::themes::muted()});
                                    ui.set_clip_rect(grid_clip);
                                    let painter = ui.painter().with_clip_rect(grid_clip);
                                    for f in frames.clone() {
                                        let x = r.left() + label_w + (f as f32 + 0.5) * cell;
                                        let q = Pos2::new(x, y + row_h / 2.);
                                        let keyed = self
                                            .doc
                                            .extras
                                            .rig
                                            .bones
                                            .iter()
                                            .any(|b| b.keys.iter().any(|k| k.frame == f))
                                            || self.doc.extras.rig.bindings.iter().any(|b| {
                                                b.pins
                                                    .iter()
                                                    .any(|p| p.keys.iter().any(|k| k.frame == f))
                                            });
                                        if keyed {
                                            painter.add(egui::Shape::convex_polygon(
                                                vec![
                                                    q - Vec2::Y * 5.,
                                                    q + Vec2::X * 4.,
                                                    q + Vec2::Y * 5.,
                                                    q - Vec2::X * 4.,
                                                ],
                                                color,
                                                Stroke::NONE,
                                            ));
                                        } else {
                                            painter.circle_filled(
                                                q,
                                                1.5,
                                                color.gamma_multiply(0.5),
                                            );
                                        }
                                        if self.feature.frame_selection.is_some_and(|(_,a,b)|(a..=b).contains(&f)) {
                                            painter.rect_filled(Rect::from_center_size(q,Vec2::new(cell,row_h)).shrink(1.),1.,color.gamma_multiply(0.2));
                                        }
                                        let response = ui.interact(
                                            Rect::from_center_size(q, Vec2::new(cell, row_h)),
                                            ui.scope_id().with(("rig-frame", f)),
                                            if keyed { Sense::click_and_drag() } else { Sense::click() },
                                        );
                                        if response.clicked() || response.secondary_clicked() {
                                            if ui.input(|i|i.modifiers.shift) {
                                                let anchor=self.feature.frame_selection.map_or(self.frame,|s|s.1);
                                                self.feature.frame_selection=Some((self.layer,anchor.min(f),anchor.max(f)));
                                            } else if response.clicked() {self.feature.frame_selection=Some((self.layer,f,f));}
                                            self.frame = f;
                                            self.playing = false;
                                        }
                                        if self.frame == f {
                                            painter.rect_stroke(response.rect.shrink(1.), 2., Stroke::new(2., color), egui::StrokeKind::Inside);
                                        }
                                        if response.dragged_by(egui::PointerButton::Primary) && f > 0 {
                                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                                            if let Some(pos) = response.interact_pointer_pos()
                                                && (pos.y - q.y).abs() < row_h / 2. && pos.x >= r.left() + label_w
                                            {
                                                let to = ((pos.x - r.left() - label_w) / cell) as usize;
                                                if to < count {
                                                    let dest = Rect::from_min_size(Pos2::new(r.left() + label_w + to as f32 * cell, y), Vec2::new(cell, row_h));
                                                    timeline_drop_marker(&painter, dest, self.rig_key_drop_valid(f, to));
                                                }
                                            }
                                        }
                                        if response.drag_stopped_by(egui::PointerButton::Primary) && let Some(pos) = response.interact_pointer_pos()
                                            && (pos.y - q.y).abs() < row_h / 2. && pos.x >= r.left() + label_w
                                        {
                                            let to = ((pos.x - r.left() - label_w) / cell) as usize;
                                            self.move_rig_keys(f, to);
                                        }
                                        response.context_menu(|ui| {
                                            if button(ui,Icon::Film,"Create Animation Clip…","Create Animation Clip…",false,true).clicked(){self.begin_animation_clip();ui.close();}
                                            for (action, icon, label) in [
                                                (0, Icon::Keyframe, "Clave"),
                                                (4, Icon::Trash, "Eliminar clave actual"),
                                            ] {
                                                let enabled = if action == 4 { keyed && f > 0 } else {
                                                    (self.tool == crate::canvas::Tool::Bones && self.rigging.bone.is_some())
                                                        || (self.tool == crate::canvas::Tool::Puppet && self.rigging.pin.is_some())
                                                };
                                                if button(ui, icon, label, label, false, enabled).clicked() {
                                                    self.frame = f;
                                                    if action == 4 { self.delete_rig_keys(f); } else { self.rig_key_action(action); }
                                                    ui.close();
                                                }
                                            }
                                        });
                                    }
                                }
                                Row::Turn(head) => {
                                    let color = self.studio.prefs.layer_color(crate::studio::LayerColor::Rigging);
                                    painter.rect_filled(label, 0., color.gamma_multiply(0.18));
                                    draw(&painter, Rect::from_min_size(label.min + Vec2::new(25., 4.), Vec2::splat(18.)), if head { Icon::Head } else { Icon::Bones }, color);
                                    painter.text(label.min + Vec2::new(51., row_h / 2.), egui::Align2::LEFT_CENTER, tr(if head { "Head turn" } else { "Body turn" }), egui::FontId::proportional(11.), color);
                                    ui.set_clip_rect(grid_clip);
                                    let painter = ui.painter().with_clip_rect(grid_clip);
                                    let turns = &self.doc.extras.rig.character_turns;
                                    let keys = if head { turns.head.keys.clone() } else { turns.body.keys.clone() };
                                    for f in frames.clone() {
                                        let x = r.left() + label_w + f as f32 * cell;
                                        let rect = Rect::from_min_size(Pos2::new(x,y),Vec2::new(cell,row_h));
                                        let key = keys.binary_search_by_key(&f, |key| key.frame).ok();
                                        let response = ui.interact(rect, ui.scope_id().with(("turn",head,f)), if key.is_some() { Sense::click_and_drag() } else { Sense::click() });
                                        if key.is_some() {
                                            let q = rect.center();
                                            painter.add(egui::Shape::convex_polygon(vec![q - Vec2::Y * 5., q + Vec2::X * 4., q + Vec2::Y * 5., q - Vec2::X * 4.],color,Stroke::NONE));
                                        }
                                        if self.frame == f {
                                            painter.rect_stroke(rect.shrink(1.), 2., Stroke::new(1., color), egui::StrokeKind::Inside);
                                        }
                                        if response.clicked() || response.secondary_clicked() {
                                            self.frame = f;
                                            self.playing = false;
                                        }
                                        let destination = response.interact_pointer_pos().filter(|p| p.y >= y && p.y < y + row_h && p.x >= r.left() + label_w)
                                            .map(|p| ((p.x - r.left() - label_w) / cell) as usize)
                                            .filter(|&to| to < count && to != f);
                                        if let Some(to) = destination && let Some(index) = key {
                                            let valid = keys.binary_search_by_key(&to, |key| key.frame).is_err();
                                            if response.dragged() {
                                                timeline_drop_marker(&painter, rect.translate(Vec2::X * (to as f32 - f as f32) * cell), valid);
                                            }
                                            if response.drag_stopped() && valid {
                                                self.checkpoint();
                                                let turns = &mut self.doc.extras.rig.character_turns;
                                                let channel = if head { &mut turns.head } else { &mut turns.body };
                                                let value = channel.keys.remove(index).value;
                                                channel.set(to, value);
                                                self.frame = to;
                                                self.playing = false;
                                                self.recover();
                                            }
                                        }
                                        response.context_menu(|ui| {
                                            if button(ui, Icon::KeyAdd, "Insert keyframe", "", false, key.is_none()).clicked() {
                                                self.checkpoint();
                                                let turns = &mut self.doc.extras.rig.character_turns;
                                                let channel = if head { &mut turns.head } else { &mut turns.body };
                                                channel.set(f, channel.value(f));
                                                self.recover();
                                                ui.close();
                                            }
                                            if button(ui, Icon::KeyRemove, "Eliminar fotograma clave", "", false, key.is_some()).clicked() {
                                                self.checkpoint();
                                                let turns = &mut self.doc.extras.rig.character_turns;
                                                let channel = if head { &mut turns.head } else { &mut turns.body };
                                                channel.keys.retain(|key| key.frame != f);
                                                self.recover();
                                                ui.close();
                                            }
                                        });
                                    }
                                }
                                Row::Cameras => {
                                    let orange =
                                        self.studio.prefs.layer_color(crate::studio::LayerColor::Camera);
                                    painter.rect_filled(
                                        label,
                                        0.,
                                        orange.gamma_multiply(0.3),
                                    );
                                    draw(
                                        &painter,
                                        Rect::from_min_size(
                                            label.min + Vec2::new(8., 4.),
                                            Vec2::splat(20.),
                                        ),
                                        Icon::Camera,
                                        orange,
                                    );
                                    painter.text(
                                        label.min + Vec2::new(36., 14.),
                                        egui::Align2::LEFT_CENTER,
                                        tr("Cámaras"),
                                        egui::FontId::proportional(11.),
                                        orange,
                                    );
                                    ui.set_clip_rect(grid_clip);
                                    let painter = ui.painter().with_clip_rect(grid_clip);
                                    for (index, c) in
                                        self.doc.extras.cameras.clone().iter().enumerate()
                                    {
                                        let rect = Rect::from_min_size(
                                            Pos2::new(
                                                r.left() + label_w + c.start as f32 * cell,
                                                y + 1.,
                                            ),
                                            Vec2::new(c.duration as f32 * cell, row_h - 2.),
                                        );
                                        if !rect.intersects(clip) {
                                            continue;
                                        }
                                        painter.rect_filled(
                                            rect,
                                            3.,
                                            if self.feature.selected_camera == Some(c.id) {
                                                orange.gamma_multiply(0.68)
                                            } else {
                                                orange.gamma_multiply(0.45)
                                            },
                                        );
                                        painter.rect_stroke(
                                            rect,
                                            3.,
                                            Stroke::new(1., orange),
                                            egui::StrokeKind::Inside,
                                        );
                                        painter.text(
                                            rect.left_center() + Vec2::new(6., 0.),
                                            egui::Align2::LEFT_CENTER,
                                            &c.name,
                                            egui::FontId::proportional(11.),
                                            orange,
                                        );
                                        let response = ui.interact(
                                            rect,
                                            ui.scope_id().with(("camera-cut", c.id)),
                                            Sense::click_and_drag(),
                                        );
                                        for f in frames
                                            .clone()
                                            .filter(|f| *f >= c.start && *f < c.start + c.duration)
                                        {
                                            let x = r.left() + label_w + f as f32 * cell;
                                            painter.line_segment(
                                                [
                                                    Pos2::new(x, rect.top()),
                                                    Pos2::new(x, rect.bottom()),
                                                ],
                                                Stroke::new(0.5, orange.gamma_multiply(0.35)),
                                            );
                                        }
                                        for key in &c.animation.keys {
                                            let f = c.start + key.frame;
                                            if !frames.contains(&f) {
                                                continue;
                                            }
                                            let center = Pos2::new(
                                                r.left() + label_w + (f as f32 + 0.5) * cell,
                                                rect.bottom() - 4.,
                                            );
                                            painter.add(egui::Shape::convex_polygon(
                                                vec![
                                                    center + Vec2::new(-3., 0.),
                                                    center + Vec2::new(0., -3.),
                                                    center + Vec2::new(3., 0.),
                                                    center + Vec2::new(0., 3.),
                                                ],
                                                orange,
                                                Stroke::NONE,
                                            ));
                                        }
                                        let key_drag = response.id.with("key-drag");
                                        let local_at = |pos: Pos2| {
                                            ((pos.x - rect.left()) / cell).floor() as usize
                                        };
                                        if response.drag_started_by(egui::PointerButton::Primary) && self.feature.camera_trim.is_none() {
                                            let source = ui.input(|i| i.pointer.press_origin())
                                                .filter(|pos| pos.y >= rect.bottom() - 9.)
                                                .map(local_at)
                                                .filter(|frame| c.animation.keys.iter().any(|key| key.frame == *frame));
                                            if let Some(source) = source {
                                                ui.data_mut(|data| data.insert_temp(key_drag, source));
                                            } else {
                                                self.feature.camera_drag = Some(index);
                                            }
                                        }
                                        if response.dragged_by(egui::PointerButton::Primary)
                                            && let Some(from) = ui.data(|data| data.get_temp::<usize>(key_drag))
                                            && let Some(pos) = response.interact_pointer_pos()
                                            && rect.intersect(grid_clip).contains(pos)
                                        {
                                            let to = local_at(pos);
                                            let dest = Rect::from_min_size(Pos2::new(rect.left() + to as f32 * cell, rect.top()), Vec2::new(cell, rect.height()));
                                            let valid = from > 0 && to > 0 && to != from && !c.animation.keys.iter().any(|key| key.frame == to);
                                            timeline_drop_marker(&painter, dest, valid);
                                        }
                                        if response.drag_stopped_by(egui::PointerButton::Primary) {
                                            let source = ui.data_mut(|data| data.remove_temp::<usize>(key_drag));
                                            if let (Some(from), Some(pos)) = (source, response.interact_pointer_pos())
                                                && rect.intersect(grid_clip).contains(pos)
                                            {
                                                self.move_camera_key(c.id, from, local_at(pos));
                                            }
                                        }
                                        if rect.intersect(grid_clip).contains(ui.input(|i| {
                                            i.pointer.interact_pos().unwrap_or(Pos2::ZERO)
                                        })) {
                                            camera_target = Some(index);
                                            if let Some(source) = self.feature.camera_drag.filter(|&source| source != index) {
                                                let x = if source < index { rect.right() } else { rect.left() };
                                                painter.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(3., crate::themes::accent()));
                                            }
                                        }
                                        if response.clicked() || response.secondary_clicked() {
                                            self.feature.selected_camera = Some(c.id);
                                            if let Some(p) = response.interact_pointer_pos() {
                                                self.frame = (((p.x - r.left() - label_w) / cell)
                                                    .floor()
                                                    .max(0.)
                                                    as usize)
                                                    .clamp(c.start, c.start + c.duration - 1);
                                            }
                                            self.activate_tool(crate::canvas::Tool::Camera);
                                            self.playing = false;
                                        }
                                        response.context_menu(|ui| {
                                            ui.label(format!("{} · {}", c.name, self.frame + 1));
                                            for (action, icon, label) in [
                                                (
                                                    0,
                                                    Icon::Keyframe,
                                                    "Insertar clave de cámara aquí",
                                                ),
                                                (1, Icon::Trash, "Borrar clave de cámara"),
                                                (2, Icon::Plus, "Insertar fotograma de cámara"),
                                                (3, Icon::Previous, "Eliminar fotograma de cámara"),
                                                (4, Icon::Cut, "Dividir cámara aquí"),
                                                (5, Icon::Trash, "Eliminar cámara"),
                                            ] {
                                                let local = self.frame.saturating_sub(c.start);
                                                let enabled = match action {
                                                    1 => {
                                                        local > 0
                                                            && c.animation
                                                                .keys
                                                                .iter()
                                                                .any(|k| k.frame == local)
                                                    }
                                                    3 => c.duration > 1,
                                                    4 => local > 0 && local < c.duration,
                                                    _ => true,
                                                };
                                                if button(ui, icon, label, label, false, enabled)
                                                    .clicked()
                                                {
                                                    self.camera_timeline_action(
                                                        c.id, self.frame, action,
                                                    );
                                                    ui.close();
                                                }
                                            }
                                        });
                                        if index + 1 < self.doc.extras.cameras.len() {
                                            let edge = Rect::from_center_size(
                                                rect.right_center(),
                                                Vec2::new((cell * 0.4).clamp(3., 10.), row_h),
                                            );
                                            let handle = ui.interact(
                                                edge,
                                                ui.scope_id().with(("camera-edge", c.id)),
                                                Sense::drag(),
                                            );
                                            if handle.hovered() || handle.dragged_by(egui::PointerButton::Primary) {
                                                ui.ctx().set_cursor_icon(
                                                    egui::CursorIcon::ResizeHorizontal,
                                                );
                                                painter.line_segment(
                                                    [rect.right_top(), rect.right_bottom()],
                                                    Stroke::new(3., crate::themes::accent()),
                                                );
                                            }
                                            if handle.drag_started_by(egui::PointerButton::Primary) {
                                                self.checkpoint();
                                                self.feature.camera_drag = None;
                                                self.feature.camera_trim =
                                                    Some(crate::features::CameraTrim {
                                                        index,
                                                        pointer_x: handle
                                                            .interact_pointer_pos()
                                                            .map_or(rect.right(), |p| p.x),
                                                        left: c.clone(),
                                                        right: self.doc.extras.cameras[index + 1]
                                                            .clone(),
                                                    });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        ui.set_clip_rect(clip);
                        if let Some(pos) = ui.input(|i| i.pointer.interact_pos())
                            && clip.contains(pos) && r.contains(pos)
                        {
                            let local_row = ((pos.y - r.top() - header_height) / row_h).floor();
                            let row = (local_row >= 0.).then(|| all.get(local_row as usize)).flatten();
                            let top = r.top() + header_height + local_row * row_h;
                            let x = grid_clip.left();
                            if let Some(Row::Layer(layer, _)) = row
                                && pos.x >= x
                                && egui::DragAndDrop::has_payload_of_type::<crate::media::MediaDrag>(ui.ctx())
                            {
                                let frame = ((pos.x - r.left() - label_w) / cell).floor() as usize;
                                if frame < count {
                                    let dest = Rect::from_min_size(Pos2::new(r.left() + label_w + frame as f32 * cell, top), Vec2::new(cell, row_h));
                                    let options = &self.doc.layers[*layer].options;
                                    timeline_drop_marker(&painter, dest, !options.locked && options.mannequin.is_none());
                                }
                            }
                            if pos.x < x && let Some(row) = row {
                                if matches!(row, Row::Layer(..) | Row::Media(..))
                                    && egui::DragAndDrop::has_payload_of_type::<crate::media::MediaTrackDrag>(ui.ctx())
                                {
                                    let y = if pos.y < top + row_h / 2. || matches!(row, Row::Media(..)) { top } else { top + row_h };
                                    painter.line_segment([Pos2::new(label_left, y), Pos2::new(r.right(), y)], Stroke::new(3., crate::themes::accent()));
                                } else if self.feature.row_drag.is_some()
                                    && let Some((_, _, side)) = target
                                    && matches!(row, Row::Layer(..) | Row::Folder(..))
                                {
                                    if side == DropSide::Inside {
                                        let dest = Rect::from_min_size(Pos2::new(label_left, top), Vec2::new(label_w, row_h));
                                        timeline_drop_marker(&painter, dest, true);
                                    } else {
                                        let y = if side == DropSide::Above { top } else { top + row_h };
                                        painter.line_segment([Pos2::new(label_left, y), Pos2::new(r.right(), y)], Stroke::new(3., crate::themes::accent()));
                                    }
                                }
                            }
                        }
                        if let Some(trim) = self.feature.camera_trim.clone() {
                            if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
                                let delta = ((p.x - trim.pointer_x) / cell).round() as isize;
                                if trim.index + 1 < self.doc.extras.cameras.len() {
                                    self.doc.extras.cameras[trim.index] = trim.left.clone();
                                    self.doc.extras.cameras[trim.index + 1] = trim.right.clone();
                                    self.roll_camera_boundary(
                                        trim.index,
                                        trim.left.duration as isize + delta,
                                    );
                                }
                            }
                        }
                        let empty_top=r.top()+header_height+all.len() as f32*row_h;
                        self.animation_drop_target(ui,Rect::from_min_max(Pos2::new(r.left()+label_w,empty_top),r.max),r.left()+label_w,cell,None);
                        let x = r.left() + label_w + (self.frame as f32 + 0.5) * cell;
                        ui.painter().with_clip_rect(viewport_clip.with_min_x(grid_clip.left())).line_segment(
                            [Pos2::new(x, header_top), Pos2::new(x, r.bottom())],
                            Stroke::new(1., crate::themes::accent()),
                        );
                        ui.set_clip_rect(viewport_clip);
                    })
                }).inner;
                if let Some(pos)=ui.input(|i|if i.pointer.any_pressed(){i.pointer.interact_pos()}else{None}) {
                    self.frame_clipboard.focus=output.inner_rect.contains(pos)&&!ui.ctx().egui_wants_keyboard_input();
                }
                viewport.area = output.inner_rect;
                viewport.offset = output.state.clamped_offset().x;
                ui.data_mut(|data| data.insert_temp(viewport_id, viewport));
                if delete_rigging {self.delete_rigging_track();}
                self.finish_exposure_resize(ui.ctx());
                self.finish_folder_ui(ui.ctx());
                if let Some((layer, action)) = layer_menu_action { self.timeline_layer_action(layer, action); }
                if ui.input(|i| i.pointer.primary_released()) {
                    self.cancel_adjustment_drag();
                    if let Some(drag) = self.feature.frame_drag.take() {
                        if let Some(at) = drag.target { self.move_timeline_frames(drag, at); }
                    }
                    if self.feature.camera_trim.take().is_some() {
                        self.feature.camera_drag = None;
                        self.recover();
                    }
                    if let (Some(source), Some(target)) =
                        (self.feature.camera_drag.take(), camera_target)
                    {
                        self.move_camera(source, target);
                    }
                    if let (Some(source), Some(target)) = (self.feature.row_drag.take(), target) {
                        if source != (target.0, target.1) {
                            self.reorder_row(source, target);
                        }
                    }
                }
            });
        if self.frame != previous_frame
            && !self.playing
            && ui.input(|i| i.pointer.primary_down() || i.pointer.primary_released())
        {
            self.scrub_media_audio(ui.ctx());
        }
    }
    pub fn move_camera(&mut self, source: usize, target: usize) {
        if source == target
            || source >= self.doc.extras.cameras.len()
            || target >= self.doc.extras.cameras.len()
        {
            return;
        }
        self.checkpoint();
        let camera = self.doc.extras.cameras.remove(source);
        self.doc.extras.cameras.insert(target, camera);
        self.normalize_cameras();
        self.recover();
    }
    fn reorder_row(&mut self, source: (bool, usize), target: (bool, usize, DropSide)) {
        let (target_folder, target_index, side) = target;
        let (parent, boundary) = if target_index == usize::MAX {
            (None, self.doc.layers.len())
        } else if target_folder {
            let folder = &self.doc.extras.folders[target_index];
            let range = folder_range(&self.doc, target_index);
            match side {
                DropSide::Inside => (Some(folder.id), range.end),
                DropSide::Above => (folder.parent, range.end),
                DropSide::Below => (folder.parent, range.start),
            }
        } else {
            (
                self.doc.layers[target_index].options.folder,
                target_index + usize::from(side == DropSide::Above),
            )
        };
        let folder_id = source.0.then(|| self.doc.extras.folders[source.1].id);
        if folder_id.is_some_and(|id| in_folder(&self.doc, parent, id)) {
            return;
        }

        let (moved, mut remaining): (Vec<_>, Vec<_>) = (0..self.doc.layers.len()).partition(|&i| {
            folder_id.map_or(i == source.1, |id| {
                in_folder(&self.doc, self.doc.layers[i].options.folder, id)
            })
        });
        let slot = remaining.partition_point(|&index| index < boundary);
        let map_slot = |previous: usize| {
            let base = remaining.partition_point(|&index| index < previous);
            base + if previous > boundary { moved.len() } else { 0 }
        };
        self.finish_pending_pen();
        self.checkpoint();
        self.feature.layer_selection = Default::default();
        self.feature.frame_selection = None;
        self.renaming_layer = None;
        for track in &mut self.doc.extras.media.tracks {
            track.slot = map_slot(track.slot);
        }
        let folder_slots: Vec<_> = self
            .doc
            .extras
            .folders
            .iter()
            .map(|folder| {
                folder.timeline_slot.map(|previous| {
                    if folder_id.is_some_and(|id| in_folder(&self.doc, folder.parent, id)) {
                        slot + moved.partition_point(|&index| index < previous)
                    } else {
                        map_slot(previous)
                    }
                })
            })
            .collect();
        for (folder, position) in self.doc.extras.folders.iter_mut().zip(folder_slots) {
            folder.timeline_slot = position;
        }
        if let Some(id) = folder_id {
            let folder = &mut self.doc.extras.folders[source.1];
            folder.parent = parent;
            folder.timeline_slot = Some(slot);
            if target_folder && side != DropSide::Inside && target_index != source.1 {
                let folder = self.doc.extras.folders.remove(source.1);
                let index = target_index - usize::from(source.1 < target_index);
                self.doc
                    .extras
                    .folders
                    .insert(index + usize::from(side == DropSide::Below), folder);
            }
            self.feature.selected_folder = Some(id);
        } else {
            self.doc.layers[source.1].options.folder = parent;
            self.layer = source.1;
            self.feature.selected_folder = parent;
        }
        remaining.splice(slot..slot, moved);
        self.layer = remaining
            .iter()
            .position(|&index| index == self.layer)
            .unwrap_or(0);
        self.doc.layers = remaining
            .into_iter()
            .map(|index| self.doc.layers[index].clone())
            .collect::<Vec<_>>()
            .into();
        self.recover();
    }
}

impl Editor {
    pub fn roll_camera_boundary(&mut self, index: usize, duration: isize) {
        if index + 1 >= self.doc.extras.cameras.len() {
            return;
        }
        let left = self.doc.extras.cameras[index].clone();
        let right = self.doc.extras.cameras[index + 1].clone();
        let total = left.duration + right.duration;
        let duration = duration.clamp(1, total as isize - 1) as usize;
        let new_start = left.start + duration;
        let pose = crate::animation::camera_at(&right, new_start);
        let shift = new_start as isize - right.start as isize;
        let l = &mut self.doc.extras.cameras[index];
        l.duration = duration;
        l.animation.keys.retain(|k| k.frame < duration);
        let r = &mut self.doc.extras.cameras[index + 1];
        r.duration = total - duration;
        r.start = new_start;
        r.center = pose.center;
        r.zoom = pose.zoom;
        r.angle = pose.angle;
        r.optics = pose.optics(&right);
        let mut keys = vec![];
        for key in &right.animation.keys {
            let at = key.frame as isize - shift;
            if at >= 0 && at < r.duration as isize {
                let mut k = key.clone();
                k.frame = at as usize;
                keys.push(k);
            }
        }
        r.animation.keys = keys.into();
    }
    fn rig_key_drop_valid(&self, from: usize, to: usize) -> bool {
        if from == 0 || to == 0 || from == to || to >= self.doc.layers[0].frames.len() {
            return false;
        }
        let rig = &self.doc.extras.rig;
        let tracks = rig
            .bones
            .iter()
            .map(|bone| key_slots(bone.keys.iter().map(|key| key.frame), from, to))
            .chain(
                rig.bindings
                    .iter()
                    .flat_map(|binding| binding.pins.iter())
                    .map(|pin| key_slots(pin.keys.iter().map(|key| key.frame), from, to)),
            );
        let mut has_source = false;
        for (source, destination) in tracks {
            if source && destination {
                return false;
            }
            has_source |= source;
        }
        has_source
    }

    fn move_rig_keys(&mut self, from: usize, to: usize) {
        if !self.rig_key_drop_valid(from, to) {
            return;
        }
        self.checkpoint();
        let rig = &mut self.doc.extras.rig;
        for bone in &mut rig.bones {
            if let Some(key) = bone.keys.iter_mut().find(|key| key.frame == from) {
                key.frame = to;
                bone.keys.sort_by_key(|key| key.frame);
            }
        }
        for binding in &mut rig.bindings {
            for pin in &mut binding.pins {
                if let Some(key) = pin.keys.iter_mut().find(|key| key.frame == from) {
                    key.frame = to;
                    pin.keys.sort_by_key(|key| key.frame);
                }
            }
        }
        self.frame = to;
        self.playing = false;
        self.recover();
    }

    fn delete_rig_keys(&mut self, frame: usize) {
        if frame == 0 {
            return;
        }
        self.checkpoint();
        let rig = &mut self.doc.extras.rig;
        for bone in &mut rig.bones {
            bone.keys.retain(|key| key.frame != frame);
        }
        for binding in &mut rig.bindings {
            for pin in &mut binding.pins {
                pin.keys.retain(|key| key.frame != frame);
            }
        }
        self.recover();
    }

    fn move_camera_key(&mut self, id: u64, from: usize, to: usize) {
        let Some(index) = self
            .doc
            .extras
            .cameras
            .iter()
            .position(|camera| camera.id == id)
        else {
            return;
        };
        let camera = &self.doc.extras.cameras[index];
        if from == 0
            || to == 0
            || from == to
            || to >= camera.duration
            || key_slots(camera.animation.keys.iter().map(|key| key.frame), from, to)
                != (true, false)
        {
            return;
        }
        self.checkpoint();
        let camera = &mut self.doc.extras.cameras[index];
        if let Some(key) = camera
            .animation
            .keys
            .iter_mut()
            .find(|key| key.frame == from)
        {
            key.frame = to;
            camera.animation.keys.sort_by_key(|key| key.frame);
        }
        self.frame = camera.start + to;
        self.feature.selected_camera = Some(id);
        self.playing = false;
        self.recover();
    }

    pub fn camera_timeline_action(&mut self, id: u64, frame: usize, action: usize) {
        let Some(index) = self.doc.extras.cameras.iter().position(|c| c.id == id) else {
            return;
        };
        let camera = self.doc.extras.cameras[index].clone();
        let local = frame.saturating_sub(camera.start).min(camera.duration - 1);
        if (action == 1 && local == 0)
            || (action == 3 && camera.duration <= 1)
            || (action == 4 && local == 0)
        {
            return;
        }
        self.checkpoint();
        self.feature.selected_camera = Some(id);
        self.playing = false;
        match action {
            0 => {
                let mut key = crate::animation::camera_at(&camera, frame);
                key.frame = local;
                crate::animation::camera_record(&mut self.doc.extras.cameras[index], key);
            }
            1 => self.doc.extras.cameras[index]
                .animation
                .keys
                .retain(|k| k.frame != local),
            2 => {
                if self
                    .doc
                    .extras
                    .cameras
                    .iter()
                    .map(|c| c.duration)
                    .sum::<usize>()
                    >= 10000
                {
                    return;
                }
                let c = &mut self.doc.extras.cameras[index];
                c.duration += 1;
                for k in &mut c.animation.keys {
                    if k.frame >= local {
                        k.frame += 1;
                    }
                }
            }
            3 => {
                let c = &mut self.doc.extras.cameras[index];
                c.duration -= 1;
                c.animation.keys.retain(|k| k.frame != local);
                for k in &mut c.animation.keys {
                    if k.frame > local {
                        k.frame -= 1;
                    }
                }
            }
            4 => {
                let pose = crate::animation::camera_at(&camera, frame);
                let mut right = camera.clone();
                right.id = crate::features::id();
                right.name = format!("{} · 2", camera.name);
                right.duration -= local;
                right.center = pose.center;
                right.zoom = pose.zoom;
                right.angle = pose.angle;
                right.optics = pose.optics(&camera);
                right.animation.keys.retain(|k| k.frame >= local);
                for k in &mut right.animation.keys {
                    k.frame -= local;
                }
                self.doc.extras.cameras[index].duration = local;
                self.doc.extras.cameras[index]
                    .animation
                    .keys
                    .retain(|k| k.frame < local);
                self.feature.selected_camera = Some(right.id);
                self.doc.extras.cameras.insert(index + 1, right);
            }
            5 => {
                self.doc.extras.cameras.remove(index);
                self.feature.selected_camera = self
                    .doc
                    .extras
                    .cameras
                    .get(index.min(self.doc.extras.cameras.len().saturating_sub(1)))
                    .map(|c| c.id);
            }
            _ => return,
        }
        self.normalize_cameras();
        self.frame = frame.min(self.doc.layers[0].frames.len() - 1);
        self.recover();
    }
}

fn key_slots(frames: impl Iterator<Item = usize>, from: usize, to: usize) -> (bool, bool) {
    frames.fold((false, false), |(source, destination), frame| {
        (source || frame == from, destination || frame == to)
    })
}

#[cfg(test)]
mod special_track_tests {
    use super::*;
    use crate::{
        animation::{CameraAnimation, CameraKey},
        features::Camera,
        rig::BoneKey,
    };

    #[test]
    fn moving_special_track_keys_preserves_values_and_protects_occupied_frames() {
        let mut editor = Editor::new(&eframe::CreationContext::_new_kittest(
            egui::Context::default(),
        ));
        editor.recovery = std::env::temp_dir().join(format!(
            "libretoon-special-tracks-{}.json",
            std::process::id()
        ));
        editor.set_duration(2.);
        editor.add_bone(Pos2::ZERO, Pos2::new(50., 0.), None);
        let id = editor.doc.extras.rig.bones[0].id;
        editor.doc.extras.rig.set_bone_key(
            id,
            BoneKey {
                frame: 3,
                translation: [15., -4.],
                ..Default::default()
            },
        );
        editor.doc.extras.rig.set_bone_key(
            id,
            BoneKey {
                frame: 8,
                ..Default::default()
            },
        );
        editor.move_rig_keys(3, 8);
        assert_eq!(editor.doc.extras.rig.bones[0].keys[1].frame, 3);
        editor.move_rig_keys(3, 5);
        let key = &editor.doc.extras.rig.bones[0].keys[1];
        assert_eq!((key.frame, key.translation), (5, [15., -4.]));
        editor.delete_rig_keys(5);
        assert_eq!(editor.doc.extras.rig.bones[0].keys.len(), 2);

        editor.doc.extras.cameras.push(Camera {
            optics: Default::default(),
            id: 9,
            name: "Camera".into(),
            start: 0,
            duration: 12,
            center: [10., 20.],
            zoom: 1.,
            angle: 0.,
            animation: CameraAnimation {
                keys: vec![
                    CameraKey {
                        optics: None,
                        frame: 0,
                        center: [10., 20.],
                        zoom: 1.,
                        angle: 0.,
                    },
                    CameraKey {
                        optics: None,
                        frame: 3,
                        center: [20., 40.],
                        zoom: 2.,
                        angle: 0.,
                    },
                ]
                .into(),
                ..Default::default()
            },
        });
        editor.move_camera_key(9, 3, 12);
        assert_eq!(editor.doc.extras.cameras[0].animation.keys[1].frame, 3);
        editor.move_camera_key(9, 3, 7);
        let camera = &editor.doc.extras.cameras[0];
        assert_eq!(camera.duration, 12);
        assert_eq!(
            (
                camera.animation.keys[1].frame,
                camera.animation.keys[1].zoom
            ),
            (7, 2.)
        );
        assert_eq!(editor.frame, 7);
        editor.history(false);
        assert_eq!(editor.doc.extras.cameras[0].animation.keys[1].frame, 3);
    }
}

pub(crate) fn timeline_drop_marker(painter: &egui::Painter, rect: Rect, valid: bool) {
    let color = if valid {
        crate::themes::accent()
    } else {
        Color32::LIGHT_RED
    };
    painter.rect_filled(rect, 2., color.gamma_multiply(0.23));
    painter.rect_stroke(
        rect.shrink(1.),
        2.,
        Stroke::new(2., color),
        egui::StrokeKind::Inside,
    );
}

#[cfg(test)]
mod viewport_input_tests {
    use super::*;

    #[test]
    fn slider_zooms_middle_drag_pans_and_wheel_scrolls_below_fixed_ruler() {
        let ctx = egui::Context::default();
        let mut editor = Editor::new(&eframe::CreationContext::_new_kittest(ctx.clone()));
        editor.doc.layers[0].frames.resize(100, Default::default());
        for number in 2..=20 {
            let mut layer = editor.doc.layers[0].clone();
            layer.name = format!("Layer {number}");
            editor.doc.layers.push(layer);
        }
        let mut time = 0.;
        let mut render = |editor: &mut Editor, events| {
            time += 1. / 60.;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200., 760.))),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| editor.sequencer(ui),
            );
            output.textures_delta.clear();
            let label = |number: &str| {
                output
                    .shapes
                    .iter()
                    .find_map(|shape| {
                        if let egui::Shape::Text(text) = &shape.shape
                            && text.galley.job.text == number
                            && (number == "Zoom" || text.pos.y > 520.)
                        {
                            return Some(text.visual_bounding_rect().center());
                        }
                        None
                    })
                    .unwrap_or_else(|| panic!("timeline ruler label {number} missing"))
            };
            [label("5"), label("10"), label("Layer 17"), label("Zoom")]
        };
        render(&mut editor, vec![]);
        let before = render(&mut editor, vec![]);
        let scrolled = render(
            &mut editor,
            vec![
                egui::Event::PointerMoved(before[2]),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Unknown,
                    delta: Vec2::new(0., -45.),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let settled = render(&mut editor, vec![]);
        assert_eq!(scrolled[0], before[0]);
        assert_eq!(settled[0], before[0]);
        assert!(settled[2].y < before[2].y);
        let pointer = before[0];
        let press = egui::Event::PointerButton {
            pos: pointer,
            button: egui::PointerButton::Middle,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        };
        let before_pan = render(&mut editor, vec![press]);
        let panned = render(
            &mut editor,
            vec![egui::Event::PointerMoved(pointer - Vec2::new(40., 0.))],
        );
        assert!(panned[1].x < before_pan[1].x - 20.);
        assert!(
            (panned[2].x - before_pan[2].x).abs() < 1.,
            "layer labels must stay pinned while frames move"
        );
        render(
            &mut editor,
            vec![egui::Event::PointerButton {
                pos: pointer - Vec2::new(40., 0.),
                button: egui::PointerButton::Middle,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        let zoom = before[3] - Vec2::new(35., 0.);
        render(&mut editor, vec![egui::Event::PointerMoved(zoom)]);
        let slider_press = |pressed| egui::Event::PointerButton {
            pos: zoom,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        render(&mut editor, vec![slider_press(true)]);
        let zoomed = render(&mut editor, vec![slider_press(false)]);
        assert!(((zoomed[1].x - zoomed[0].x) - (panned[1].x - panned[0].x)).abs() > 5.);
        assert_eq!(editor.frame, 0);
    }
}

#[cfg(test)]
mod layer_selection_tests {
    use super::*;

    #[test]
    fn folder_label_drop_reorders_nested_layers_as_one_block_and_can_be_undone() {
        let mut e = Editor::new(&eframe::CreationContext::_new_kittest(
            egui::Context::default(),
        ));
        e.doc = crate::fresh_document();
        let mut layer = e.doc.layers[0].clone();
        for (name, folder) in [
            ("Back", None),
            ("Inside", Some(10)),
            ("Nested", Some(11)),
            ("Front", None),
        ] {
            layer.name = name.into();
            layer.options.folder = folder;
            if name == "Back" {
                e.doc.layers[0] = layer.clone();
            } else {
                e.doc.layers.push(layer.clone());
            }
        }
        for (id, parent) in [(10, None), (11, Some(10)), (12, None)] {
            e.doc.extras.folders.push(Folder {
                id,
                parent,
                name: id.to_string(),
                collapsed: false,
                timeline_slot: None,
                transform: Default::default(),
                range: None,
                source_shift: 0,
            });
        }
        e.recovery = std::env::temp_dir().join(format!("folder-order-{}.json", std::process::id()));
        e.reorder_row((true, 0), (false, 0, DropSide::Below));
        assert_eq!(
            e.doc
                .layers
                .iter()
                .map(|l| l.name.as_str())
                .collect::<Vec<_>>(),
            ["Inside", "Nested", "Back", "Front"]
        );
        assert_eq!(e.doc.layers[1].options.folder, Some(11));
        let mut visible = vec![];
        rows(&e, &mut visible);
        assert!(matches!(visible[3], Row::Folder(0, 0)));
        e.history(false);
        assert_eq!(e.doc.layers[0].name, "Back");
        e.reorder_row((true, 2), (false, 0, DropSide::Below));
        assert_eq!(e.doc.extras.folders[2].timeline_slot, Some(0));
        let mut visible = vec![];
        rows(&e, &mut visible);
        assert!(matches!(visible.last(), Some(Row::Folder(2, 0))));
        let saved = crate::native_project::encode(&e.doc).unwrap();
        let reopened = crate::native_project::decode_if_native(&saved)
            .unwrap()
            .unwrap();
        assert_eq!(reopened.extras.folders[2].timeline_slot, Some(0));
    }

    #[test]
    fn toggles_individual_layers_and_extends_in_visible_row_order() {
        let rows = [
            Row::Layer(3, 0),
            Row::Folder(0, 0),
            Row::Layer(0, 1),
            Row::Rigging,
            Row::Layer(2, 0),
        ];
        let mut selection = LayerSelection::default();
        let active = selection.click(&rows, 3, 2, false, true);
        assert_eq!(active, 2);
        assert_eq!(selection.indices, [3, 2]);
        let active = selection.click(&rows, active, 2, false, true);
        assert_eq!(active, 3);
        assert_eq!(selection.indices, [3]);
        let active = selection.click(&rows, active, 2, true, false);
        assert_eq!(active, 2);
        assert_eq!(selection.indices, [3, 0, 2]);
        assert_eq!(selection.click(&rows, active, 0, true, false), 0);
        assert_eq!(selection.indices, [3, 0]);
        assert_eq!(selection.click(&rows, 0, 2, false, false), 2);
        assert_eq!(selection.indices, [2]);
    }
}
