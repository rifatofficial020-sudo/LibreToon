//! Timeline folders are static composition parents, not automatically animated layers.
use crate::{
    Document, Editor, Mark,
    canvas::View,
    design::{Icon, button, draw},
    document::Layer,
    features::Folder,
    i18n::tr,
    shared::SharedVec,
};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Transform {
    pub position: [f32; 2],
    pub pivot: [f32; 2],
    pub scale: f32,
    pub angle: f32,
}
impl Default for Transform {
    fn default() -> Self {
        Self {
            position: [0.; 2],
            pivot: [0.; 2],
            scale: 1.,
            angle: 0.,
        }
    }
}
impl Transform {
    pub fn matrix(self) -> crate::rig::Matrix {
        let (s, c) = self.angle.sin_cos();
        let a = c * self.scale;
        let b = s * self.scale;
        [
            a,
            b,
            -b,
            a,
            self.position[0] + self.pivot[0] - a * self.pivot[0] + b * self.pivot[1],
            self.position[1] + self.pivot[1] - b * self.pivot[0] - a * self.pivot[1],
        ]
    }
    pub fn valid(self) -> bool {
        self.position
            .iter()
            .chain(self.pivot.iter())
            .all(|v| v.is_finite() && v.abs() <= 1e6)
            && self.scale.is_finite()
            && (0.000001..=1000000.).contains(&self.scale)
            && self.angle.is_finite()
            && self.angle.abs() <= 1000.
    }
    fn around(mut self, pivot: Pos2) -> Self {
        let old = self.matrix();
        self.pivot = [pivot.x, pivot.y];
        let new = self.matrix();
        self.position[0] += old[4] - new[4];
        self.position[1] += old[5] - new[5];
        self
    }
}
pub fn inverse(m: crate::rig::Matrix) -> Option<crate::rig::Matrix> {
    let d = m[0] as f64 * m[3] as f64 - m[1] as f64 * m[2] as f64;
    if !d.is_finite() || d == 0. {
        return None;
    }
    let result = [
        (m[3] as f64 / d) as f32,
        (-m[1] as f64 / d) as f32,
        (-m[2] as f64 / d) as f32,
        (m[0] as f64 / d) as f32,
        ((m[2] as f64 * m[5] as f64 - m[3] as f64 * m[4] as f64) / d) as f32,
        ((m[1] as f64 * m[4] as f64 - m[0] as f64 * m[5] as f64) / d) as f32,
    ];
    result
        .iter()
        .all(|v| v.is_finite() && v.abs() <= 1e18)
        .then_some(result)
}
pub fn belongs(folders: &[Folder], mut folder: Option<u64>, root: u64) -> bool {
    for _ in 0..=32 {
        let Some(id) = folder else {
            return false;
        };
        if id == root {
            return true;
        }
        folder = folders.iter().find(|f| f.id == id).and_then(|f| f.parent);
    }
    false
}
pub fn matrix(folders: &[Folder], mut folder: Option<u64>) -> crate::rig::Matrix {
    let mut result = crate::rig::IDENTITY;
    for _ in 0..=32 {
        let Some(f) = folder.and_then(|id| folders.iter().find(|f| f.id == id)) else {
            break;
        };
        result = crate::rig::multiply(f.transform.matrix(), result);
        folder = f.parent;
    }
    result
}
pub fn active(folders: &[Folder], mut folder: Option<u64>, frame: usize) -> bool {
    for _ in 0..=32 {
        let Some(f) = folder.and_then(|id| folders.iter().find(|f| f.id == id)) else {
            return true;
        };
        if f.range.is_some_and(|r| frame < r[0] || frame >= r[1]) {
            return false;
        }
        folder = f.parent;
    }
    true
}
pub fn view(base: View, m: crate::rig::Matrix) -> View {
    if m == crate::rig::IDENTITY {
        return base;
    }
    View {
        center: base.screen(egui::pos2(m[4], m[5])),
        stage_center: Pos2::ZERO,
        angle: base.angle + m[1].atan2(m[0]),
        scale: base.scale * m[0].hypot(m[1]),
    }
}
pub fn mark(mark: &Mark, m: crate::rig::Matrix) -> Mark {
    if m == crate::rig::IDENTITY {
        return mark.clone();
    }
    let mut result = crate::features::affine(mark, m);
    result.width *= m[0].hypot(m[1]);
    result
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LayerGroup {
    pub matrix: crate::rig::Matrix,
    pub range: Option<[usize; 2]>,
    pub source_shift: i32,
}
impl Default for LayerGroup {
    fn default() -> Self {
        Self {
            matrix: crate::rig::IDENTITY,
            range: None,
            source_shift: 0,
        }
    }
}
impl LayerGroup {
    pub fn valid(&self) -> bool {
        self.matrix.iter().all(|v| v.is_finite() && v.abs() <= 1e12)
            && inverse(self.matrix).is_some()
            && self.range.is_none_or(|r| r[0] <= r[1] && r[1] <= 10000)
            && self.source_shift.unsigned_abs() <= 1000000
    }
}
pub fn layer_matrix(doc: &Document, layer: &Layer) -> crate::rig::Matrix {
    crate::rig::multiply(
        matrix(&doc.extras.folders, layer.options.folder),
        layer.options.composition.matrix,
    )
}
pub fn layer_active(doc: &Document, layer: &Layer, frame: usize) -> bool {
    active(&doc.extras.folders, layer.options.folder, frame)
        && layer
            .options
            .composition
            .range
            .is_none_or(|r| r[0] <= frame && frame < r[1])
}
pub(crate) fn inherited_layer(doc: &Document, layer: &Layer) -> Layer {
    let mut result = layer.clone();
    result.options.composition.matrix = layer_matrix(doc, layer);
    let mut parent = layer.options.folder;
    let mut shift = layer.options.composition.source_shift;
    let mut span = layer.options.composition.range;
    for _ in 0..=32 {
        let Some(f) = parent.and_then(|id| doc.extras.folders.iter().find(|f| f.id == id)) else {
            break;
        };
        shift += f.source_shift;
        span = intersect(span, f.range);
        parent = f.parent;
    }
    result.options.composition.source_shift = shift;
    result.options.composition.range = span;
    result.options.folder = None;
    result
}
pub fn rig_frame(doc: &Document, layer: &Layer, frame: usize) -> usize {
    let mut shift = layer.options.composition.source_shift as i64;
    let mut parent = layer.options.folder;
    for _ in 0..=32 {
        let Some(f) = parent.and_then(|id| doc.extras.folders.iter().find(|f| f.id == id)) else {
            break;
        };
        shift += f.source_shift as i64;
        parent = f.parent;
    }
    (frame as i64 + shift).clamp(0, 9999) as usize
}
fn intersect(a: Option<[usize; 2]>, b: Option<[usize; 2]>) -> Option<[usize; 2]> {
    match (a, b) {
        (Some(a), Some(b)) => {
            let r = [a[0].max(b[0]), a[1].min(b[1])];
            Some(if r[0] <= r[1] { r } else { [b[0], b[0]] })
        }
        (a, None) => a,
        (None, b) => b,
    }
}
fn transform_from_matrix(m: crate::rig::Matrix) -> Transform {
    Transform {
        position: [m[4], m[5]],
        pivot: [0., 0.],
        scale: m[0].hypot(m[1]),
        angle: m[1].atan2(m[0]),
    }
}
pub(crate) fn preserve_ungroup(doc: &mut Document, id: u64) {
    let Some(folder) = doc.extras.folders.iter().find(|f| f.id == id).cloned() else {
        return;
    };
    for child in &mut doc.extras.folders {
        if child.parent == Some(id) {
            child.transform = transform_from_matrix(crate::rig::multiply(
                folder.transform.matrix(),
                child.transform.matrix(),
            ));
            child.source_shift += folder.source_shift;
            child.range = intersect(child.range, folder.range);
        }
    }
    for layer in &mut doc.layers {
        if layer.options.folder == Some(id) {
            layer.options.composition.matrix =
                crate::rig::multiply(folder.transform.matrix(), layer.options.composition.matrix);
            layer.options.composition.source_shift += folder.source_shift;
            layer.options.composition.range =
                intersect(layer.options.composition.range, folder.range);
        }
    }
}

#[derive(Clone)]
struct CanvasDrag {
    id: u64,
    original: Transform,
    pointer: Pos2,
    pivot: Pos2,
    parent_inverse: crate::rig::Matrix,
    mode: i8,
    bounds: Rect,
}
struct TimeDrag {
    id: u64,
    from: [usize; 2],
    pointer: f32,
    target: usize,
}
#[derive(Default)]
pub struct State {
    pub selected: Option<u64>,
    pub(crate) dialog: Option<(u64, Transform)>,
    canvas: Option<CanvasDrag>,
    timeline: Option<TimeDrag>,
    bounds: Option<(
        SharedVec<Layer>,
        SharedVec<Folder>,
        SharedVec<crate::features::Symbol>,
        SharedVec<crate::rig::Bone>,
        SharedVec<crate::rig::Binding>,
        crate::rig::CharacterTurns,
        usize,
        u64,
        Rect,
    )>,
}
impl State {
    pub(crate) fn timeline_end(&self) -> usize {
        self.timeline
            .as_ref()
            .map_or(0, |d| d.target + d.from[1] - d.from[0])
    }
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

struct RangeCache {
    layers: SharedVec<Layer>,
    folders: SharedVec<Folder>,
    signature: Vec<(u64, Option<u64>, Option<[usize; 2]>)>,
    ranges: HashMap<u64, [usize; 2]>,
}
thread_local! {static RANGES:std::cell::RefCell<Option<RangeCache>>=const{std::cell::RefCell::new(None)};}
#[cfg(test)]
thread_local! {static RANGE_BUILDS:std::cell::Cell<usize>=const{std::cell::Cell::new(0)};}
fn range(doc: &Document, folder: u64) -> Option<[usize; 2]> {
    RANGES.with(|cell| {
        let mut cache = cell.borrow_mut();
        if let Some(c) = cache.as_mut() {
            if c.layers.identity() == doc.layers.identity() {
                if c.folders.identity() == doc.extras.folders.identity() {
                    return c.ranges.get(&folder).copied().filter(|r| r[0] < r[1]);
                }
                let signature: Vec<_> = doc
                    .extras
                    .folders
                    .iter()
                    .map(|f| (f.id, f.parent, f.range))
                    .collect();
                if signature == c.signature {
                    c.folders = doc.extras.folders.clone();
                    return c.ranges.get(&folder).copied().filter(|r| r[0] < r[1]);
                }
            }
        }
        #[cfg(test)]
        RANGE_BUILDS.with(|count| count.set(count.get() + 1));
        let mut ranges: HashMap<_, _> = doc
            .extras
            .folders
            .iter()
            .filter_map(|f| {
                f.range.map(|r| {
                    let mut effective = Some(r);
                    let mut parent = f.parent;
                    for _ in 0..=32 {
                        let Some(p) =
                            parent.and_then(|id| doc.extras.folders.iter().find(|f| f.id == id))
                        else {
                            break;
                        };
                        effective = intersect(effective, p.range);
                        parent = p.parent;
                    }
                    (f.id, effective.unwrap())
                })
            })
            .collect();
        for layer in &doc.layers {
            let extent = if layer.options.mannequin.is_some() {
                Some([0, layer.frames.len()])
            } else {
                layer
                    .frames
                    .iter()
                    .position(|f| !f.is_empty())
                    .map(|first| {
                        [
                            first,
                            layer.frames.iter().rposition(|f| !f.is_empty()).unwrap() + 1,
                        ]
                    })
            };
            let Some(mut r) = extent else {
                continue;
            };
            let mut clipping = layer.options.folder;
            let mut effective = intersect(Some(r), layer.options.composition.range);
            for _ in 0..=32 {
                let Some(f) =
                    clipping.and_then(|id| doc.extras.folders.iter().find(|f| f.id == id))
                else {
                    break;
                };
                effective = intersect(effective, f.range);
                clipping = f.parent;
            }
            r = effective.unwrap();
            if r[0] == r[1] {
                continue;
            }
            let mut parent = layer.options.folder;
            for _ in 0..=32 {
                let Some(f) = parent.and_then(|id| doc.extras.folders.iter().find(|f| f.id == id))
                else {
                    break;
                };
                if f.range.is_none() {
                    ranges
                        .entry(f.id)
                        .and_modify(|v| {
                            v[0] = v[0].min(r[0]);
                            v[1] = v[1].max(r[1]);
                        })
                        .or_insert(r);
                }
                parent = f.parent;
            }
        }
        let found = ranges.get(&folder).copied().filter(|r| r[0] < r[1]);
        *cache = Some(RangeCache {
            layers: doc.layers.clone(),
            folders: doc.extras.folders.clone(),
            signature: doc
                .extras
                .folders
                .iter()
                .map(|f| (f.id, f.parent, f.range))
                .collect(),
            ranges,
        });
        found
    })
}

fn parent_range(doc: &Document, id: u64) -> [usize; 2] {
    let mut parent = doc
        .extras
        .folders
        .iter()
        .find(|f| f.id == id)
        .and_then(|f| f.parent);
    let mut result = Some([0, 10000]);
    for _ in 0..=32 {
        let Some(f) = parent.and_then(|id| doc.extras.folders.iter().find(|f| f.id == id)) else {
            break;
        };
        result = intersect(result, f.range);
        parent = f.parent;
    }
    result.unwrap()
}
fn cropped(layer: &Layer, span: [usize; 2], doc: &Document, destination: usize) -> Layer {
    let mut result = layer.clone();
    let count = doc.layers[0]
        .frames
        .len()
        .max(destination + span[1] - span[0]);
    result.frames = vec![SharedVec::default(); count].into();
    result.options.keys = vec![false; count].into();
    result.options.keys[0] = true;
    let crossed: Vec<_> = layer
        .options
        .tweens
        .iter()
        .filter(|t| {
            t.start < span[1] && t.end >= span[0] && (t.start < span[0] || t.end >= span[1])
        })
        .cloned()
        .collect();
    result.options.tweens = layer
        .options
        .tweens
        .iter()
        .filter(|t| t.start >= span[0] && t.end < span[1])
        .cloned()
        .map(|mut t| {
            t.start = destination + t.start - span[0];
            t.end = destination + t.end - span[0];
            t
        })
        .collect();
    let mut cache = HashMap::<usize, (SharedVec<Mark>, SharedVec<Mark>)>::new();
    for source in span[0]..span[1] {
        if !layer_active(doc, layer, source) {
            continue;
        }
        let sampled = crossed.iter().any(|t| t.start <= source && source <= t.end);
        let marks = if sampled {
            crate::animation::marks(layer, source)
        } else {
            layer.frames[source].clone()
        };
        let frame = if let Some((_, frame)) = cache.get(&marks.identity()) {
            frame.clone()
        } else {
            let frame: SharedVec<Mark> = marks
                .iter()
                .cloned()
                .map(|mut m| {
                    crate::frame_clipboard::retime(&mut m, span[0], destination, doc);
                    m
                })
                .collect();
            cache.insert(marks.identity(), (marks, frame.clone()));
            frame
        };
        let at = destination + source - span[0];
        result.frames[at] = frame;
        result.options.keys[at] = layer.key(source) || sampled;
    }
    if let Some(r) = result.options.composition.range {
        let delta = destination as isize - span[0] as isize;
        let r = [r[0].max(span[0]), r[1].min(span[1])];
        result.options.composition.range = if r[0] < r[1] {
            Some([
                (r[0] as isize + delta) as usize,
                (r[1] as isize + delta) as usize,
            ])
        } else {
            Some([destination, destination])
        };
    }
    result.options.keys[destination] = true;
    let end = destination + span[1] - span[0];
    if end < count {
        result.options.keys[end] = true;
    }
    result
}

impl Editor {
    fn folder_transform_allowed(&self, id: u64, transform: Transform) -> bool {
        if !transform.valid() {
            return false;
        }
        let mut folders = self.doc.extras.folders.clone();
        let Some(folder) = folders.iter_mut().find(|f| f.id == id) else {
            return false;
        };
        folder.transform = transform;
        folders.iter().all(|f| {
            let m = matrix(&folders, Some(f.id));
            m.iter().all(|v| v.is_finite() && v.abs() <= 1e12) && inverse(m).is_some()
        })
    }
    pub(crate) fn group_rig_frame(&self) -> usize {
        self.doc
            .layers
            .get(self.layer)
            .map_or(self.frame, |l| rig_frame(&self.doc, l, self.frame))
    }
    pub(crate) fn select_folder_group(&mut self, id: u64) {
        self.folder_groups.selected = Some(id);
        self.feature.selected_folder = Some(id);
        if let Some(layer) = self
            .doc
            .layers
            .iter()
            .position(|l| belongs(&self.doc.extras.folders, l.options.folder, id))
        {
            self.layer = layer;
        }
        if let Some(r) = range(&self.doc, id) {
            if self.frame < r[0] || self.frame >= r[1] {
                self.frame = r[0];
            }
        }
        self.canvas.clear_selection();
        self.clips.clear_track_selection();
        self.activate_tool(crate::canvas::Tool::Transform);
    }
    fn folder_bounds(&mut self, id: u64) -> Option<Rect> {
        if let Some((layers, folders, symbols, bones, bindings, turns, frame, folder, bounds)) =
            &self.folder_groups.bounds
        {
            if layers.identity() == self.doc.layers.identity()
                && folders.identity() == self.doc.extras.folders.identity()
                && symbols.identity() == self.doc.extras.symbols.identity()
                && bones.identity() == self.doc.extras.rig.bones.identity()
                && bindings.identity() == self.doc.extras.rig.bindings.identity()
                && turns.identity() == self.doc.extras.rig.character_turns.identity()
                && *frame == self.frame
                && *folder == id
            {
                return Some(*bounds);
            }
        }
        let mut bounds = Rect::NOTHING;
        for (layer, _, mark) in crate::features::resolve_export(&self.doc, self.frame, false) {
            if belongs(
                &self.doc.extras.folders,
                self.doc.layers[layer].options.folder,
                id,
            ) {
                bounds = bounds.union(crate::vector::control_bounds(&mark));
            }
        }
        if !bounds.is_finite() {
            return None;
        }
        self.folder_groups.bounds = Some((
            self.doc.layers.clone(),
            self.doc.extras.folders.clone(),
            self.doc.extras.symbols.clone(),
            self.doc.extras.rig.bones.clone(),
            self.doc.extras.rig.bindings.clone(),
            self.doc.extras.rig.character_turns.clone(),
            self.frame,
            id,
            bounds,
        ));
        Some(bounds)
    }
    pub(crate) fn folder_context(&mut self, ui: &mut egui::Ui, id: u64) {
        ui.separator();
        if button(
            ui,
            Icon::Transform,
            "Transform folder…",
            "Transform folder…",
            false,
            true,
        )
        .clicked()
        {
            let mut t = self
                .doc
                .extras
                .folders
                .iter()
                .find(|f| f.id == id)
                .unwrap()
                .transform;
            if t == Transform::default() {
                if let Some(bounds) = self.folder_bounds(id) {
                    let parent = self
                        .doc
                        .extras
                        .folders
                        .iter()
                        .find(|f| f.id == id)
                        .and_then(|f| f.parent);
                    if let Some(inv) = inverse(matrix(&self.doc.extras.folders, parent)) {
                        t = t.around(crate::rig::point(inv, bounds.center()));
                    }
                }
            }
            self.folder_groups.dialog = Some((id, t));
            self.select_folder_group(id);
            ui.close();
        }
        if button(
            ui,
            Icon::Cut,
            "Split folder at playhead",
            "Split folder at playhead",
            false,
            range(&self.doc, id).is_some_and(|r| r[0] < self.frame && self.frame < r[1]),
        )
        .clicked()
        {
            if let Err(e) = self.split_folder_group(id, self.frame) {
                self.status = e;
            }
            ui.close();
        }
    }
    pub(crate) fn folder_dialog(&mut self, ctx: &egui::Context) {
        let Some((id, mut draft)) = self.folder_groups.dialog.take() else {
            return;
        };
        let mut apply = false;
        let mut cancel = false;
        egui::Modal::new(egui::Id::unique("folder-transform")).show(ctx, |ui| {
            ui.heading(tr("Transform folder"));
            ui.small(tr("Affects all child layers without creating keyframes."));
            egui::Grid::new("folder-transform-fields")
                .num_columns(2)
                .show(ui, |ui| {
                    button(ui, Icon::Pan, "Position", "Position", false, false);
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut draft.position[0])
                                .prefix("X ")
                                .range(-1e6..=1e6),
                        );
                        ui.add(
                            egui::DragValue::new(&mut draft.position[1])
                                .prefix("Y ")
                                .range(-1e6..=1e6),
                        );
                    });
                    ui.end_row();
                    button(ui, Icon::Transform, "Scale", "Scale", false, false);
                    let mut percent = draft.scale * 100.;
                    ui.add(
                        egui::DragValue::new(&mut percent)
                            .suffix(" %")
                            .range(0.0001..=100000000.),
                    );
                    draft.scale = percent / 100.;
                    ui.end_row();
                    button(ui, Icon::Rotate, "Rotation", "Rotation", false, false);
                    let mut degrees = draft.angle.to_degrees();
                    ui.add(
                        egui::DragValue::new(&mut degrees)
                            .suffix("°")
                            .range(-3600. ..=3600.),
                    );
                    draft.angle = degrees.to_radians();
                    ui.end_row();
                    button(ui, Icon::Focus, "Pivot", "Pivot", false, false);
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut draft.pivot[0])
                                .prefix("X ")
                                .range(-1e6..=1e6),
                        );
                        ui.add(
                            egui::DragValue::new(&mut draft.pivot[1])
                                .prefix("Y ")
                                .range(-1e6..=1e6),
                        );
                    });
                    ui.end_row();
                });
            ui.horizontal(|ui| {
                if button(ui, Icon::Focus, "Center pivot", "Center pivot", false, true).clicked() {
                    if let Some(b) = self.folder_bounds(id) {
                        let parent = self
                            .doc
                            .extras
                            .folders
                            .iter()
                            .find(|f| f.id == id)
                            .and_then(|f| f.parent);
                        let parent_inverse =
                            inverse(matrix(&self.doc.extras.folders, parent)).unwrap();
                        let parent_point = crate::rig::point(parent_inverse, b.center());
                        let original = self
                            .doc
                            .extras
                            .folders
                            .iter()
                            .find(|f| f.id == id)
                            .unwrap()
                            .transform;
                        let pivot = crate::rig::point(
                            inverse(original.matrix()).unwrap_or(crate::rig::IDENTITY),
                            parent_point,
                        );
                        draft = draft.around(pivot);
                    }
                }
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
                    draft = Transform::default();
                }
            });
            ui.horizontal(|ui| {
                apply =
                    button(ui, Icon::Save, "Aplicar", "Aplicar", false, draft.valid()).clicked();
                cancel = button(ui, Icon::Close, "Cancelar", "Cancelar", false, true).clicked();
            });
        });
        if apply {
            if let Some(index) = self.doc.extras.folders.iter().position(|f| f.id == id) {
                self.checkpoint();
                self.doc.extras.folders[index].transform = draft;
                self.recover();
            }
        } else if !cancel {
            self.folder_groups.dialog = Some((id, draft));
        }
    }
    pub(crate) fn move_folder_group(&mut self, id: u64, destination: usize) -> Result<(), String> {
        let span = range(&self.doc, id).ok_or("Empty folder")?;
        let length = span[1] - span[0];
        if destination + length > 10000 {
            return Err(tr("The folder would exceed the timeline limit.").into());
        }
        let parent = parent_range(&self.doc, id);
        if destination < parent[0] || destination + length > parent[1] {
            return Err(
                tr("Keep this block inside its parent folder, or move the parent folder.").into(),
            );
        }
        if destination == span[0] {
            return Ok(());
        }
        let members: Vec<_> = self
            .doc
            .layers
            .iter()
            .enumerate()
            .filter(|(_, l)| belongs(&self.doc.extras.folders, l.options.folder, id))
            .map(|(i, _)| i)
            .collect();
        if members.iter().any(|&i| self.doc.layers[i].options.locked) {
            return Err(
                tr("Unlock the child layers before moving or splitting the folder.").into(),
            );
        }
        let replacements: Vec<_> = members
            .iter()
            .map(|&i| {
                (
                    i,
                    cropped(&self.doc.layers[i], span, &self.doc, destination),
                )
            })
            .collect();
        let folders: Vec<_> = self
            .doc
            .extras
            .folders
            .iter()
            .enumerate()
            .filter(|(_, f)| belongs(&self.doc.extras.folders, Some(f.id), id))
            .map(|(i, f)| (i, range(&self.doc, f.id)))
            .collect();
        self.finish_pending_pen();
        self.checkpoint();
        let count = self.doc.layers[0].frames.len().max(destination + length);
        for layer in &mut self.doc.layers {
            if crate::animation_clips::resize_track(layer, count) {
                continue;
            }
            let held = layer.frames.last().cloned().unwrap_or_default();
            layer.frames.resize(count, held);
            layer.init_keys();
            layer.options.keys.resize(count, false);
        }
        for (index, mut layer) in replacements {
            layer.frames.resize(count, Default::default());
            layer.options.keys.resize(count, false);
            self.doc.layers[index] = layer;
        }
        let delta = destination as isize - span[0] as isize;
        for (index, old) in folders {
            let clipped = intersect(old, Some(span));
            self.doc.extras.folders[index].range = clipped.map(|r| {
                [
                    (r[0] as isize + delta) as usize,
                    (r[1] as isize + delta) as usize,
                ]
            });
        }
        let root = self
            .doc
            .extras
            .folders
            .iter_mut()
            .find(|f| f.id == id)
            .unwrap();
        root.source_shift -= delta as i32;
        root.range = Some([destination, destination + length]);
        self.frame = destination;
        self.folder_groups.bounds = None;
        self.clips.clear_track_selection();
        self.fit_camera_duration();
        self.recover();
        Ok(())
    }
    pub(crate) fn split_folder_group(&mut self, id: u64, cut: usize) -> Result<u64, String> {
        let span = range(&self.doc, id).ok_or("Empty folder")?;
        if cut <= span[0] || cut >= span[1] {
            return Err(tr("Place the playhead inside the folder to split it.").into());
        }
        let members: HashSet<_> = self
            .doc
            .layers
            .iter()
            .enumerate()
            .filter(|(_, l)| belongs(&self.doc.extras.folders, l.options.folder, id))
            .map(|(i, _)| i)
            .collect();
        let originals: Vec<_> = self
            .doc
            .extras
            .folders
            .iter()
            .filter(|f| belongs(&self.doc.extras.folders, Some(f.id), id))
            .cloned()
            .collect();
        if self.doc.layers.len() + members.len() > 100
            || self.doc.extras.folders.len() + originals.len() > 200
        {
            return Err(tr("Splitting would exceed the layer or folder limit.").into());
        }
        if members.iter().any(|&i| self.doc.layers[i].options.locked) {
            return Err(
                tr("Unlock the child layers before moving or splitting the folder.").into(),
            );
        }
        let ids: HashMap<_, _> = originals
            .iter()
            .map(|f| (f.id, crate::features::id()))
            .collect();
        let right = ids[&id];
        let mut layers = vec![];
        for (index, layer) in self.doc.layers.iter().enumerate() {
            if members.contains(&index) {
                let left = cropped(layer, [span[0], cut], &self.doc, span[0]);
                let mut rhs = cropped(layer, [cut, span[1]], &self.doc, cut);
                rhs.options.folder = rhs.options.folder.and_then(|f| ids.get(&f).copied());
                layers.push(left);
                layers.push(rhs);
            } else {
                layers.push(layer.clone());
            }
        }
        self.finish_pending_pen();
        self.checkpoint();
        for track in &mut self.doc.extras.media.tracks {
            track.slot += members.iter().filter(|&&i| i < track.slot).count();
        }
        self.doc.layers = layers.into();
        for mut f in originals {
            let original = f.id;
            f.id = ids[&original];
            if original == id {
                f.name = format!("{} (2)", f.name);
                f.range = Some([cut, span[1]]);
            } else {
                f.range = None;
            }
            f.parent = f.parent.map(|p| ids.get(&p).copied().unwrap_or(p));
            self.doc.extras.folders.push(f);
        }
        self.doc
            .extras
            .folders
            .iter_mut()
            .find(|f| f.id == id)
            .unwrap()
            .range = Some([span[0], cut]);
        self.layer = self.layer.min(self.doc.layers.len() - 1);
        self.select_folder_group(right);
        self.folder_groups.bounds = None;
        self.recover();
        Ok(right)
    }
    pub(crate) fn folder_track(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        row: Rect,
        left: f32,
        cell: f32,
    ) {
        let folder = self.doc.extras.folders[index].clone();
        let Some(span) = range(&self.doc, folder.id) else {
            return;
        };
        let rect = Rect::from_min_max(
            egui::pos2(left + span[0] as f32 * cell, row.top() + 3.),
            egui::pos2(left + span[1] as f32 * cell, row.bottom() - 3.),
        );
        let response = ui
            .interact(
                rect,
                ui.scope_id().with(("folder-composition", folder.id)),
                Sense::click_and_drag(),
            )
            .on_hover_cursor(egui::CursorIcon::Grab)
            .on_hover_text(tr(
                "Drag to move the folder contents · right-click to transform or split",
            ));
        ui.painter()
            .rect_filled(rect, 3., Color32::from_rgb(78, 85, 120));
        draw(
            ui.painter(),
            Rect::from_min_size(rect.min + Vec2::new(4., 2.), Vec2::splat(16.)),
            Icon::Folder,
            Color32::WHITE,
        );
        if self.folder_groups.selected == Some(folder.id) {
            ui.painter().rect_stroke(
                rect,
                3.,
                Stroke::new(1.5, crate::themes::accent()),
                egui::StrokeKind::Inside,
            );
        }
        if span[0] <= self.frame && self.frame < span[1] {
            let x = left + (self.frame as f32 + 0.5) * cell;
            ui.painter().line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                Stroke::new(1.5, crate::themes::accent()),
            );
        }
        if rect.width() > 70. {
            ui.painter().with_clip_rect(rect.shrink(2.)).text(
                rect.left_center() + Vec2::new(25., 0.),
                egui::Align2::LEFT_CENTER,
                &folder.name,
                egui::FontId::proportional(11.),
                Color32::WHITE,
            );
        }
        if response.clicked() {
            self.frame = response
                .interact_pointer_pos()
                .map(|p| ((p.x - left) / cell).floor().max(0.) as usize)
                .unwrap_or(span[0])
                .clamp(span[0], span[1] - 1);
            self.select_folder_group(folder.id);
        }
        response.context_menu(|ui| self.folder_context(ui, folder.id));
        if response.drag_started() {
            self.select_folder_group(folder.id);
            if let Some(p) = ui.input(|i| i.pointer.press_origin()) {
                self.folder_groups.timeline = Some(TimeDrag {
                    id: folder.id,
                    from: span,
                    pointer: p.x,
                    target: span[0],
                });
            }
        }
        if let Some(d) = self
            .folder_groups
            .timeline
            .as_mut()
            .filter(|d| d.id == folder.id)
        {
            if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
                d.target = (d.from[0] as isize + ((p.x - d.pointer) / cell).round() as isize)
                    .clamp(0, (10000 - (d.from[1] - d.from[0])) as isize)
                    as usize;
                let x = left + d.target as f32 * cell;
                let parent = parent_range(&self.doc, d.id);
                let valid = d.target >= parent[0] && d.target + d.from[1] - d.from[0] <= parent[1];
                ui.painter().rect_stroke(
                    Rect::from_min_size(
                        egui::pos2(x, row.top() + 1.),
                        Vec2::new((d.from[1] - d.from[0]) as f32 * cell, row.height() - 2.),
                    ),
                    2.,
                    Stroke::new(
                        2.,
                        if valid {
                            crate::themes::accent()
                        } else {
                            Color32::LIGHT_RED
                        },
                    ),
                    egui::StrokeKind::Inside,
                );
                if p.x > ui.clip_rect().right() - 24. {
                    ui.scroll_to_rect(
                        Rect::from_min_size(
                            egui::pos2(x + (d.from[1] - d.from[0]) as f32 * cell, row.top()),
                            Vec2::new(cell, row.height()),
                        ),
                        Some(egui::Align::Max),
                    );
                }
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
        }
    }
    pub(crate) fn finish_folder_ui(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.folder_groups.timeline = None;
        }
        if ctx.input(|i| i.pointer.primary_released() || !i.pointer.primary_down()) {
            if let Some(d) = self.folder_groups.timeline.take() {
                if let Err(e) = self.move_folder_group(d.id, d.target) {
                    self.status = e;
                }
            }
        }
    }
    pub(crate) fn folder_canvas_input(&mut self, ui: &egui::Ui, area: Rect, base: View) -> bool {
        let Some(id) = self.folder_groups.selected else {
            return false;
        };
        if !matches!(
            self.tool,
            crate::canvas::Tool::Select
                | crate::canvas::Tool::Object
                | crate::canvas::Tool::Transform
        ) {
            self.folder_groups.selected = None;
            return false;
        }
        if self.playing
            || self.folder_groups.dialog.is_some()
            || ui.input(|i| i.key_down(egui::Key::Space))
        {
            return false;
        }
        if !self.doc.extras.folders.iter().any(|f| f.id == id) {
            self.folder_groups.clear();
            return false;
        }
        let Some(pointer) = ui.input(|i| i.pointer.interact_pos()) else {
            if self.folder_groups.canvas.is_some() && !ui.input(|i| i.pointer.primary_down()) {
                self.folder_groups.canvas = None;
                self.folder_groups.bounds = None;
                self.recover();
            }
            return false;
        };
        if let Some(d) = self.folder_groups.canvas.clone() {
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.doc
                    .extras
                    .folders
                    .iter_mut()
                    .find(|f| f.id == id)
                    .unwrap()
                    .transform = d.original;
                self.folder_groups.canvas = None;
                self.recover();
                return true;
            }
            let current = crate::rig::point(d.parent_inverse, base.world(pointer));
            let pivot = d.pivot;
            let local_pivot = crate::rig::point(
                inverse(d.original.matrix()).unwrap_or(crate::rig::IDENTITY),
                pivot,
            );
            let mut t = d.original.around(local_pivot);
            match d.mode {
                0 => {
                    let delta = current - d.pointer;
                    t.position[0] += delta.x;
                    t.position[1] += delta.y;
                }
                1 => {
                    t.scale = (t.scale * current.distance(pivot)
                        / d.pointer.distance(pivot).max(0.01))
                    .clamp(0.000001, 1000000.);
                }
                _ => {
                    t.angle += (current - pivot).angle() - (d.pointer - pivot).angle();
                }
            }
            if self.folder_transform_allowed(d.id, t) {
                self.doc
                    .extras
                    .folders
                    .iter_mut()
                    .find(|f| f.id == d.id)
                    .unwrap()
                    .transform = t;
            }
            if ui.input(|i| i.pointer.primary_released()) {
                self.folder_groups.canvas = None;
                self.folder_groups.bounds = None;
                self.recover();
            }
            return true;
        }
        if !area.contains(pointer) || !ui.rect_contains_pointer(area) {
            return false;
        }
        let Some(bounds) = self.folder_bounds(id) else {
            return false;
        };
        let screen = Rect::from_points(
            &[
                bounds.left_top(),
                bounds.right_top(),
                bounds.right_bottom(),
                bounds.left_bottom(),
            ]
            .map(|p| base.screen(p)),
        );
        let rotate_at = screen.center_top() - Vec2::new(0., 24.);
        let scale_at = screen.right_bottom();
        let mode = if pointer.distance(rotate_at) < 10. {
            2
        } else if pointer.distance(scale_at) < 10. {
            1
        } else if screen.contains(pointer) {
            0
        } else {
            -1
        };
        if mode >= 0 {
            ui.ctx().set_cursor_icon(match mode {
                1 => egui::CursorIcon::ResizeNwSe,
                2 => egui::CursorIcon::Alias,
                _ => egui::CursorIcon::Move,
            });
        }
        if ui.input(|i| i.pointer.primary_pressed()) {
            if mode < 0 {
                self.folder_groups.selected = None;
                return false;
            }
            let folder = self.doc.extras.folders.iter().find(|f| f.id == id).unwrap();
            let original = folder.transform;
            let parent_inverse = inverse(matrix(&self.doc.extras.folders, folder.parent)).unwrap();
            let d = CanvasDrag {
                id,
                original,
                parent_inverse,
                pointer: crate::rig::point(parent_inverse, base.world(pointer)),
                pivot: if original.pivot != [0., 0.] {
                    egui::pos2(
                        original.pivot[0] + original.position[0],
                        original.pivot[1] + original.position[1],
                    )
                } else {
                    crate::rig::point(parent_inverse, bounds.center())
                },
                mode,
                bounds,
            };
            self.checkpoint();
            self.folder_groups.canvas = Some(d);
            return true;
        }
        false
    }
    pub(crate) fn paint_folder_gizmo(&mut self, painter: &egui::Painter, base: View) {
        if self.playing {
            return;
        }
        let Some(id) = self.folder_groups.selected else {
            return;
        };
        let bounds = if let Some(d) = self.folder_groups.canvas.as_ref() {
            let folder = self.doc.extras.folders.iter().find(|f| f.id == id).unwrap();
            let parent = matrix(&self.doc.extras.folders, folder.parent);
            let old = crate::rig::multiply(parent, d.original.matrix());
            let new = crate::rig::multiply(parent, folder.transform.matrix());
            let delta = crate::rig::multiply(new, inverse(old).unwrap());
            Rect::from_points(
                &[
                    d.bounds.left_top(),
                    d.bounds.right_top(),
                    d.bounds.right_bottom(),
                    d.bounds.left_bottom(),
                ]
                .map(|p| crate::rig::point(delta, p)),
            )
        } else {
            let Some(b) = self.folder_bounds(id) else {
                return;
            };
            b
        };
        let points = [
            bounds.left_top(),
            bounds.right_top(),
            bounds.right_bottom(),
            bounds.left_bottom(),
        ]
        .map(|p| base.screen(p));
        painter.add(egui::Shape::closed_line(
            points.to_vec(),
            Stroke::new(1.5, crate::themes::accent()),
        ));
        let rect = Rect::from_points(&points);
        let top = rect.center_top() - Vec2::new(0., 24.);
        painter.line_segment(
            [rect.center_top(), top],
            Stroke::new(1., crate::themes::accent()),
        );
        draw(
            painter,
            Rect::from_center_size(top, Vec2::splat(18.)),
            Icon::Rotate,
            crate::themes::accent(),
        );
        painter.rect_filled(
            Rect::from_center_size(rect.right_bottom(), Vec2::splat(8.)),
            1.,
            crate::themes::accent(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn folder(id: u64, parent: Option<u64>) -> Folder {
        Folder {
            id,
            name: format!("Folder {id}"),
            parent,
            collapsed: false,
            timeline_slot: None,
            transform: Default::default(),
            range: None,
            source_shift: 0,
        }
    }
    fn fixture() -> Editor {
        let mut e = Editor::new(&eframe::CreationContext::_new_kittest(
            egui::Context::default(),
        ));
        e.doc = crate::fresh_document();
        e.recovery = std::env::temp_dir().join(format!(
            "folder-groups-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ));
        e.doc.extras.folders.push(folder(10, None));
        e.doc.layers[0].options.folder = Some(10);
        for f in 0..24 {
            e.doc.layers[0].frames[f] = vec![Mark {
                points: vec![[10. + f as f32, 20.], [30. + f as f32, 40.]].into(),
                rectangle: true,
                fill: Some([90, 100, 220, 255]),
                ..Default::default()
            }]
            .into();
        }
        e.doc.layers[0].options.keys = vec![true; 24].into();
        e
    }
    fn bounds(doc: &Document, frame: usize) -> Rect {
        let mut r = Rect::NOTHING;
        for (_, _, m) in crate::features::resolve_export(doc, frame, false) {
            r = r.union(crate::vector::control_bounds(&m));
        }
        r
    }
    fn close(a: Pos2, b: Pos2) {
        assert!(a.distance(b) < 0.002, "{a:?} != {b:?}");
    }
    #[test]
    fn static_folder_transform_changes_preview_and_export_without_touching_keys_or_geometry() {
        let mut e = fixture();
        let frames = e.doc.layers[0].frames.clone();
        let keys = e.doc.layers[0].options.keys.clone();
        let t = Transform {
            position: [60., 20.],
            pivot: [20., 30.],
            scale: 2.,
            angle: 0.7,
        };
        e.doc.extras.folders[0].transform = t;
        let p = egui::pos2(10., 20.);
        let base = View {
            center: egui::pos2(100., 100.),
            stage_center: Pos2::ZERO,
            angle: 0.3,
            scale: 1.7,
        };
        close(
            e.camera_depth_view(base, 0).screen(p),
            base.screen(crate::rig::point(t.matrix(), p)),
        );
        close(
            e.camera_depth_view(base, 0)
                .world(base.screen(crate::rig::point(t.matrix(), p))),
            p,
        );
        let exported = crate::features::resolve_export(&e.doc, 0, false);
        let wanted = mark(&frames[0][0], t.matrix());
        assert_eq!(
            crate::vector::control_bounds(&exported[0].2),
            crate::vector::control_bounds(&wanted)
        );
        assert_eq!(e.doc.layers[0].frames.identity(), frames.identity());
        assert_eq!(e.doc.layers[0].options.keys.identity(), keys.identity());
        assert!(e.doc.validate().is_ok());
    }
    #[test]
    fn splitting_and_moving_preserves_every_frame_and_can_be_undone() {
        let mut e = fixture();
        let original = e.doc.clone();
        let right = e.split_folder_group(10, 12).unwrap();
        assert_eq!(e.doc.layers.len(), 2);
        assert_eq!(e.doc.extras.folders.len(), 2);
        for f in 0..24 {
            assert_eq!(bounds(&e.doc, f), bounds(&original, f));
        }
        e.move_folder_group(right, 30).unwrap();
        assert!(e.doc.layers.iter().all(|l| l.frames.len() == 42));
        for f in 0..12 {
            assert_eq!(bounds(&e.doc, f), bounds(&original, f));
        }
        for f in 12..30 {
            assert!(crate::features::resolve_export(&e.doc, f, false).is_empty());
        }
        for f in 30..42 {
            assert_eq!(bounds(&e.doc, f), bounds(&original, f - 18));
        }
        assert!(e.doc.validate().is_ok());
        e.history(false);
        for f in 0..24 {
            assert_eq!(bounds(&e.doc, f), bounds(&original, f));
        }
        e.history(false);
        assert_eq!(e.doc.layers.len(), 1);
        assert_eq!(e.doc.extras.folders.len(), 1);
    }
    #[test]
    fn nested_folders_compose_and_ungroup_keeps_the_stage_unchanged() {
        let mut e = fixture();
        e.doc.extras.folders.push(folder(11, Some(10)));
        e.doc.layers[0].options.folder = Some(11);
        e.doc.extras.folders[0].transform = Transform {
            position: [80., 10.],
            scale: 2.,
            angle: 0.4,
            ..Default::default()
        };
        e.doc.extras.folders[1].transform = Transform {
            position: [5., 20.],
            scale: 0.5,
            angle: -0.2,
            ..Default::default()
        };
        let before: Vec<_> = (0..24).map(|f| bounds(&e.doc, f)).collect();
        preserve_ungroup(&mut e.doc, 10);
        e.doc.extras.folders[1].parent = None;
        e.doc.extras.folders.remove(0);
        for f in 0..24 {
            let after = bounds(&e.doc, f);
            close(after.min, before[f].min);
            close(after.max, before[f].max);
        }
        assert!(e.doc.validate().is_ok());
        preserve_ungroup(&mut e.doc, 11);
        e.doc.layers[0].options.folder = None;
        e.doc.extras.folders.clear();
        for f in 0..24 {
            let after = bounds(&e.doc, f);
            close(after.min, before[f].min);
            close(after.max, before[f].max);
        }
        assert!(e.doc.validate().is_ok());
    }
    #[test]
    fn folder_composition_round_trips_native_and_fla_with_separate_layers() {
        let mut e = fixture();
        e.split_folder_group(10, 12).unwrap();
        e.doc.extras.folders[0].transform.position = [30., 15.];
        e.doc.extras.folders[1].transform.angle = 0.4;
        let json = serde_json::to_vec(&e.doc).unwrap();
        let native: Document = serde_json::from_slice(&json).unwrap();
        assert!(native.validate().is_ok());
        let bytes = crate::fla::encode(&e.doc).unwrap();
        let loaded = crate::fla::decode(&bytes).unwrap();
        assert_eq!(loaded.layers.len(), 2);
        assert_eq!(loaded.extras.folders.len(), 2);
        for f in 0..24 {
            let a = bounds(&e.doc, f);
            let b = bounds(&loaded, f);
            close(a.min, b.min);
            close(a.max, b.max);
        }
    }
    #[test]
    fn old_folder_json_keeps_identity_and_locked_split_is_atomic() {
        let old: Folder =
            serde_json::from_str(r#"{"id":1,"name":"old","parent":null,"collapsed":false}"#)
                .unwrap();
        assert!(old.transform == Transform::default());
        assert!(old.range.is_none());
        let mut e = fixture();
        e.doc.layers[0].options.locked = true;
        let before = serde_json::to_vec(&e.doc).unwrap();
        assert!(e.split_folder_group(10, 12).is_err());
        assert_eq!(before, serde_json::to_vec(&e.doc).unwrap());
    }
    #[test]
    fn crossing_tween_split_and_relocation_keep_the_sampled_animation() {
        let mut e = fixture();
        let first = e.doc.layers[0].frames[0].clone();
        for frame in &mut e.doc.layers[0].frames {
            *frame = first.clone();
        }
        e.doc.layers[0].frames[23][0].points = vec![[100., 20.], [120., 40.]].into();
        e.doc.layers[0].options.keys = vec![false; 24].into();
        e.doc.layers[0].options.keys[0] = true;
        e.doc.layers[0].options.keys[23] = true;
        e.doc.layers[0].options.tweens = vec![crate::animation::Tween {
            start: 0,
            end: 23,
            curve: Default::default(),
        }]
        .into();
        let original = e.doc.clone();
        let right = e.split_folder_group(10, 12).unwrap();
        for f in 0..24 {
            let a = bounds(&e.doc, f);
            let b = bounds(&original, f);
            close(a.min, b.min);
            close(a.max, b.max);
        }
        e.move_folder_group(right, 30).unwrap();
        for f in 30..42 {
            let a = bounds(&e.doc, f);
            let b = bounds(&original, f - 18);
            close(a.min, b.min);
            close(a.max, b.max);
        }
        assert!(e.doc.validate().is_ok());
    }
    #[test]
    fn canvas_group_drag_moves_or_scales_once_without_creating_keys() {
        for scale in [false, true] {
            let mut e = fixture();
            e.select_folder_group(10);
            let original = e.doc.layers[0].frames.clone();
            let keys = e.doc.layers[0].options.keys.clone();
            let ctx = egui::Context::default();
            let base = View {
                center: Pos2::ZERO,
                stage_center: Pos2::ZERO,
                scale: 1.,
                angle: 0.,
            };
            let tick = |e: &mut Editor, events: Vec<egui::Event>| {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(400.))),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        let (response, painter) =
                            ui.allocate_painter(Vec2::splat(400.), Sense::click_and_drag());
                        e.folder_canvas_input(ui, response.rect, base);
                        e.paint_folder_gizmo(&painter, base);
                    },
                );
                output.textures_delta.clear();
            };
            tick(&mut e, vec![]);
            let from = if scale {
                egui::pos2(30., 40.)
            } else {
                egui::pos2(20., 30.)
            };
            let to = if scale {
                egui::pos2(40., 50.)
            } else {
                egui::pos2(60., 70.)
            };
            let press = |pos, pressed| egui::Event::PointerButton {
                pos,
                pressed,
                button: egui::PointerButton::Primary,
                modifiers: Default::default(),
            };
            tick(&mut e, vec![egui::Event::PointerMoved(from)]);
            tick(&mut e, vec![press(from, true)]);
            tick(&mut e, vec![egui::Event::PointerMoved(to)]);
            tick(&mut e, vec![press(to, false)]);
            let transform = e.doc.extras.folders[0].transform;
            if scale {
                assert!((transform.scale - 2.).abs() < 0.01);
                close(
                    crate::rig::point(transform.matrix(), egui::pos2(20., 30.)),
                    egui::pos2(20., 30.),
                );
            } else {
                assert_eq!(transform.position, [40., 40.]);
            }
            assert_eq!(e.undo.len(), 1);
            assert_eq!(e.doc.layers[0].frames.identity(), original.identity());
            assert_eq!(e.doc.layers[0].options.keys.identity(), keys.identity());
            e.history(false);
            assert!(e.doc.extras.folders[0].transform == Transform::default());
        }
    }
    #[test]
    fn timeline_folder_block_drag_moves_the_group_with_preview() {
        let mut e = fixture();
        let ctx = egui::Context::default();
        let tick = |e: &mut Editor, events: Vec<egui::Event>| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000., 250.))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    e.folder_track(
                        ui,
                        0,
                        Rect::from_min_max(egui::pos2(100., 60.), egui::pos2(900., 88.)),
                        100.,
                        10.,
                    )
                },
            );
            output.textures_delta.clear();
            e.finish_folder_ui(&ctx);
        };
        tick(&mut e, vec![]);
        let from = egui::pos2(180., 74.);
        let to = egui::pos2(230., 74.);
        let press = |pos, pressed| egui::Event::PointerButton {
            pos,
            pressed,
            button: egui::PointerButton::Primary,
            modifiers: Default::default(),
        };
        tick(&mut e, vec![egui::Event::PointerMoved(from)]);
        tick(&mut e, vec![press(from, true)]);
        tick(&mut e, vec![egui::Event::PointerMoved(to)]);
        assert_eq!(e.folder_groups.timeline.as_ref().unwrap().target, 5);
        tick(&mut e, vec![press(to, false)]);
        assert_eq!(range(&e.doc, 10), Some([5, 29]));
        assert!(crate::features::resolve_export(&e.doc, 0, false).is_empty());
        assert!(e.doc.validate().is_ok());
    }
    #[test]
    #[ignore = "Manual UI render for folder composition"]
    fn render_folder_workspace_preview() {
        let mut e = fixture();
        e.add_document(e.doc.clone(), None);
        e.select_folder_group(10);
        let ctx = egui::Context::default();
        crate::design::theme(&ctx);
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1440., 900.))),
                ..Default::default()
            },
            |ui| {
                e.workspace(ui);
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().inner_margin(0.))
                    .show(ui, |ui| e.canvas_ui(ui));
            },
        );
        if let Some(path) = std::env::var_os("LIBRETOON_FOLDER_PREVIEW") {
            let pixels = crate::exporter::raster_ui_test(&ctx, &mut output, 1440, 900);
            image::save_buffer(path, &pixels, 1440, 900, image::ColorType::Rgb8).unwrap();
        }
        output.textures_delta.clear();
    }
    #[test]
    fn changing_static_transform_never_rescans_timeline_frames() {
        let mut e = fixture();
        assert_eq!(range(&e.doc, 10), Some([0, 24]));
        let builds = RANGE_BUILDS.with(|n| n.get());
        let frames = e.doc.layers[0].frames.identity();
        for i in 0..100 {
            e.doc.extras.folders[0].transform.position[0] = i as f32;
            assert_eq!(range(&e.doc, 10), Some([0, 24]));
        }
        assert_eq!(RANGE_BUILDS.with(|n| n.get()), builds);
        assert_eq!(frames, e.doc.layers[0].frames.identity());
    }
    #[test]
    fn nested_time_move_cannot_silently_hide_content_outside_parent() {
        let mut e = fixture();
        e.doc.extras.folders[0].range = Some([0, 12]);
        e.doc.extras.folders.push(folder(11, Some(10)));
        e.doc.layers[0].options.folder = Some(11);
        assert_eq!(range(&e.doc, 11), Some([0, 12]));
        let before = serde_json::to_vec(&e.doc).unwrap();
        assert!(e.move_folder_group(11, 30).is_err());
        assert_eq!(before, serde_json::to_vec(&e.doc).unwrap());
    }
}
