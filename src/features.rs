use crate::{
    document::{Document, Layer, Mark},
    shared::SharedVec,
};
use eframe::egui::{Pos2, Vec2};
use geo::BooleanOps;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Cap {
    #[default]
    Round,
    Flat,
    Square,
    Triangle,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stop {
    pub t: f32,
    pub color: [u8; 4],
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gradient {
    pub radial: bool,
    pub start: [f32; 2],
    pub end: [f32; 2],
    pub stops: SharedVec<Stop>,
}
impl Default for Gradient {
    fn default() -> Self {
        Self {
            radial: false,
            start: [0., 0.],
            end: [100., 0.],
            stops: vec![
                Stop {
                    t: 0.,
                    color: [89, 72, 220, 255],
                },
                Stop {
                    t: 1.,
                    color: [255, 255, 255, 255],
                },
            ]
            .into(),
        }
    }
}
impl Gradient {
    pub fn color(&self, p: Pos2) -> [u8; 4] {
        let a = crate::vector::pos(self.start);
        let d = crate::vector::pos(self.end) - a;
        let t = if self.radial {
            p.distance(a) / d.length().max(0.001)
        } else {
            (p - a).dot(d) / d.length_sq().max(0.001)
        }
        .clamp(0., 1.);
        let first = &self.stops[0];
        if t <= first.t {
            return first.color;
        }
        for pair in self.stops.windows(2) {
            if t <= pair[1].t {
                let f = (t - pair[0].t) / (pair[1].t - pair[0].t).max(0.00001);
                return std::array::from_fn(|i| {
                    (pair[0].color[i] as f32 * (1. - f) + pair[1].color[i] as f32 * f).round() as u8
                });
            }
        }
        self.stops.last().unwrap().color
    }
    pub fn fit(&mut self, b: eframe::egui::Rect) {
        let direction =
            (crate::vector::pos(self.end) - crate::vector::pos(self.start)).normalized();
        let direction = if direction.length_sq() < 0.5 {
            Vec2::X
        } else {
            direction
        };
        let radius = (direction.x.abs() * b.width() + direction.y.abs() * b.height()) / 2.;
        self.start = crate::vector::xy(if self.radial {
            b.center()
        } else {
            b.center() - direction * radius
        });
        self.end = crate::vector::xy(b.center() + direction * radius.max(1.));
    }
    pub fn valid(&self) -> bool {
        self.stops.len() >= 2
            && self.stops.len() <= 32
            && self
                .start
                .iter()
                .chain(self.end.iter())
                .all(|v| v.is_finite() && v.abs() < 1e7)
            && self
                .stops
                .iter()
                .all(|s| s.t.is_finite() && (0. ..=1.).contains(&s.t))
            && self.stops.windows(2).all(|s| s[0].t <= s[1].t)
    }
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Playback {
    #[default]
    Loop,
    Once,
    Single,
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolClock {
    #[default]
    Scene,
    Independent,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Instance {
    #[serde(default)]
    pub clock: SymbolClock,
    pub symbol: u64,
    pub matrix: [f32; 6],
    pub mode: Playback,
    pub first: usize,
    pub born: usize,
}
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MarkStyle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) palette_fill: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) palette_stroke: Option<u64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub primitive: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_pivot: Option<[f32; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_folder: Option<[u64; 2]>,
    #[serde(skip_serializing_if = "SharedVec::is_empty")]
    pub mouth_keys: SharedVec<crate::lip::Key>,
    #[serde(skip_serializing_if = "SharedVec::is_empty")]
    pub mouth_routes: SharedVec<crate::lip::Route>,
    #[serde(skip)]
    pub surface_height: Option<f32>,
    #[serde(skip)]
    pub light_mask: bool,
    #[serde(skip)]
    pub light_mask_target: Option<u64>,
    #[serde(skip)]
    pub surface_piece: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation_clip: Option<crate::animation_clips::Timing>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_image: Option<u64>,
    pub raster_tile: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texture_basis: Option<[[f32; 2]; 3]>,
    pub instance_color: Option<[f32; 8]>,
    pub guide: bool,
    pub text: Option<Box<crate::typography::TextSpec>>,
    pub track: u64,
    pub gradient: Option<Gradient>,
    pub shade: u8,
    pub light: u8,
    pub start_cap: Cap,
    pub end_cap: Cap,
    pub instance: Option<Instance>,
}
#[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub enum ObjectFolderMode {
    #[default]
    Normal,
    Switch,
    Solo,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct ObjectFolder {
    pub id: u64,
    pub name: String,
    pub mode: ObjectFolderMode,
    pub choices: SharedVec<(usize, u64)>,
}

impl ObjectFolder {
    pub fn choice(&self, frame: usize) -> Option<u64> {
        match self.mode {
            ObjectFolderMode::Normal => None,
            ObjectFolderMode::Solo => self.choices.last().map(|choice| choice.1),
            ObjectFolderMode::Switch => self
                .choices
                .iter()
                .rev()
                .find(|choice| choice.0 <= frame)
                .or_else(|| self.choices.first())
                .map(|choice| choice.1),
        }
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LayerOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) character_piece: Option<crate::character::Piece>,
    pub composition: crate::folder_groups::LayerGroup,
    pub animation_track: bool,
    pub raster: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adjustment: Option<SharedVec<crate::post::Effect>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adjustment_bands: Option<SharedVec<crate::post::Band>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) fx_target: Option<crate::light::Receiver>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light: Option<crate::light::Light>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light_receiver: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height_guide: Option<f32>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub light_mask: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light_mask_target: Option<u64>,
    pub depth: crate::depth::LayerDepth,
    pub mannequin: Option<crate::mannequin::Mannequin>,
    pub tweens: SharedVec<crate::animation::Tween>,
    pub locked: bool,
    pub opacity: f32,
    pub clip: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mask_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub masked_by: Option<u64>,
    pub folder: Option<u64>,
    pub keys: SharedVec<bool>,
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub(crate) drawing_classes: std::collections::BTreeMap<usize, u8>,
    #[serde(skip_serializing_if = "SharedVec::is_empty")]
    pub object_folders: SharedVec<ObjectFolder>,
}
impl Default for LayerOptions {
    fn default() -> Self {
        Self {
            character_piece: None,
            composition: Default::default(),
            animation_track: false,
            raster: false,
            adjustment: None,
            adjustment_bands: None,
            fx_target: None,
            light: None,
            light_receiver: None,
            height_guide: None,
            light_mask: false,
            light_mask_target: None,
            depth: Default::default(),
            mannequin: None,
            locked: false,
            tweens: Default::default(),
            opacity: 1.,
            clip: false,
            mask_id: None,
            masked_by: None,
            folder: None,
            keys: Default::default(),
            drawing_classes: Default::default(),
            object_folders: Default::default(),
        }
    }
}
pub(crate) fn object_visible(layer: &Layer, mark: &Mark, frame: usize) -> bool {
    let Some([folder, object]) = mark.style.object_folder else {
        return true;
    };
    layer
        .options
        .object_folders
        .iter()
        .find(|item| item.id == folder)
        .and_then(|item| item.choice(frame))
        .is_none_or(|choice| choice == object)
}

pub fn default_style(style: &MarkStyle) -> bool {
    *style == MarkStyle::default()
}
pub fn default_layer_options(options: &LayerOptions) -> bool {
    *options == LayerOptions::default()
}
pub fn default_extras(extras: &ProjectExtras) -> bool {
    extras.palette.is_empty()
        && extras.main_palettes.is_empty()
        && extras.rig_poses.is_empty()
        && extras.markers.is_empty()
        && extras.tags.is_empty()
        && extras.references.is_empty()
        && extras.animation_library.is_empty()
        && extras.animation_clips.is_empty()
        && !extras.imported
        && extras.import_warnings.is_empty()
        && extras.rig.is_empty()
        && extras.library.is_empty()
        && extras.media.assets.is_empty()
        && extras.media.tracks.is_empty()
        && extras.scenes.is_empty()
        && extras.active_scene == 0
        && extras.folders.is_empty()
        && extras.symbols.is_empty()
        && extras.cameras.is_empty()
        && extras.perspective == Perspective::default()
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Folder {
    pub id: u64,
    pub name: String,
    pub parent: Option<u64>,
    pub collapsed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeline_slot: Option<usize>,
    #[serde(default)]
    pub transform: crate::folder_groups::Transform,
    #[serde(default)]
    pub range: Option<[usize; 2]>,
    #[serde(default)]
    pub source_shift: i32,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Symbol {
    #[serde(default, skip_serializing_if = "SharedVec::is_empty")]
    pub mouth_poses: SharedVec<crate::lip::Pose>,
    #[serde(default)]
    pub rig: crate::rig::Rig,
    #[serde(default)]
    pub library_folder: Option<u64>,
    #[serde(default)]
    pub folders: SharedVec<Folder>,
    #[serde(default)]
    pub perspective: Perspective,
    pub id: u64,
    pub name: String,
    pub layers: SharedVec<Layer>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Camera {
    #[serde(default)]
    pub optics: crate::depth::Optics,
    #[serde(default)]
    pub animation: crate::animation::CameraAnimation,
    pub id: u64,
    pub name: String,
    pub start: usize,
    pub duration: usize,
    pub center: [f32; 2],
    pub zoom: f32,
    pub angle: f32,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Perspective {
    pub enabled: bool,
    pub horizon: f32,
    pub points: SharedVec<[f32; 2]>,
    pub rays: u32,
}
impl Default for Perspective {
    fn default() -> Self {
        Self {
            enabled: false,
            horizon: 180.,
            points: vec![[480., 180.]].into(),
            rays: 12,
        }
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectExtras {
    #[serde(skip_serializing_if = "SharedVec::is_empty")]
    pub(crate) palette: SharedVec<crate::palette::Swatch>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub(crate) main_palettes: std::collections::BTreeMap<usize, SharedVec<[u8; 4]>>,
    #[serde(default, skip_serializing_if = "SharedVec::is_empty")]
    pub(crate) rig_poses: SharedVec<crate::rig::SavedPose>,
    pub markers: SharedVec<crate::dialogue::Marker>,
    pub tags: SharedVec<crate::dialogue::Tag>,
    pub references: SharedVec<crate::reftool::Reference>,
    pub animation_library: crate::animation_clips::Collection,
    pub animation_clips: SharedVec<crate::animation_clips::Asset>,
    pub imported: bool,
    pub import_warnings: SharedVec<String>,
    pub rig: crate::rig::Rig,
    pub library: crate::library::Library,
    pub media: crate::media::MediaLibrary,
    pub scenes: SharedVec<crate::animation::Scene>,
    pub active_scene: usize,
    pub folders: SharedVec<Folder>,
    pub symbols: SharedVec<Symbol>,
    pub cameras: SharedVec<Camera>,
    pub perspective: Perspective,
}
#[derive(Clone)]
pub struct CameraTrim {
    pub index: usize,
    pub pointer_x: f32,
    pub left: Camera,
    pub right: Camera,
}
#[derive(Clone, Copy, Debug)]
pub struct FrameDrag {
    pub layer: usize,
    pub start: usize,
    pub end: usize,
    pub grab: usize,
    pub target: Option<usize>,
}
pub struct FeatureState {
    pub(crate) pen_edit_only: bool,
    pub toon_levels: u8,
    pub gradient_on: bool,
    pub gradient: Gradient,
    pub start_cap: Cap,
    pub end_cap: Cap,
    pub library: bool,
    pub selected_symbol: Option<u64>,
    pub selected_folder: Option<u64>,
    pub selected_camera: Option<u64>,
    pub symbol_stack: Vec<(Document, usize, usize, u64)>,
    pub frame_selection: Option<(usize, usize, usize)>,
    pub(crate) bucket_range_armed: bool,
    pub(crate) bucket_next: bool,
    pub(crate) bucket_next_count: usize,
    pub(crate) bucket_range: Option<crate::canvas::BucketRange>,
    pub frame_drag: Option<FrameDrag>,
    pub exposure_drag: Option<crate::exposure_resize::Drag>,
    pub row_drag: Option<(bool, usize)>,
    pub(crate) layer_selection: crate::sequencer::LayerSelection,
    pub camera_drag: Option<usize>,
    pub camera_trim: Option<CameraTrim>,
    pub guide_drag: Option<usize>,
    pub last_pointer: Option<Pos2>,
}
impl Default for FeatureState {
    fn default() -> Self {
        Self {
            pen_edit_only: false,
            toon_levels: 1,
            gradient_on: false,
            gradient: Default::default(),
            start_cap: Cap::Round,
            end_cap: Cap::Round,
            library: false,
            selected_symbol: None,
            selected_folder: None,
            selected_camera: None,
            symbol_stack: vec![],
            frame_selection: None,
            bucket_range_armed: false,
            bucket_next: false,
            bucket_next_count: 2,
            bucket_range: None,
            frame_drag: None,
            exposure_drag: None,
            row_drag: None,
            layer_selection: Default::default(),
            camera_drag: None,
            camera_trim: None,
            guide_drag: None,
            last_pointer: None,
        }
    }
}
impl Layer {
    pub fn key(&self, f: usize) -> bool {
        self.options.keys.len() != self.frames.len() || self.options.keys[f]
    }
    pub fn exposure(&self, f: usize) -> std::ops::Range<usize> {
        let start = (0..=f).rev().find(|&i| self.key(i)).unwrap_or(0);
        let end = (f + 1..self.frames.len())
            .find(|&i| self.key(i))
            .unwrap_or(self.frames.len());
        start..end
    }
    pub fn init_keys(&mut self) {
        if self.options.keys.len() != self.frames.len() {
            self.options.keys = vec![true; self.frames.len()].into();
        }
    }
}
pub fn id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64;
    time.wrapping_add(NEXT.fetch_add(1, Ordering::Relaxed))
}
pub fn styled(polys: geo::MultiPolygon<f64>, color: [u8; 4], style: &MarkStyle) -> Vec<Mark> {
    let mut marks = crate::vector::polygons_to_marks(polys, color);
    for m in &mut marks {
        m.style = style.clone();
        m.style.instance = None;
    }
    marks
}
pub fn clip_to(m: &Mark, area: &geo::MultiPolygon<f64>) -> Vec<Mark> {
    if m.style.raster_tile && m.style.media_image.is_some() {
        let points = crate::canvas::vertices(m);
        let mut style = m.style.clone();
        if style.texture_basis.is_none() && points.len() >= 4 {
            style.texture_basis = Some([points[0], points[1], points[3]].map(crate::vector::xy));
        }
        let mut parts = styled(
            crate::vector::polygon(m).intersection(area),
            [0, 0, 0, 0],
            &style,
        );
        for part in &mut parts {
            part.color = m.color;
        }
        return parts;
    }
    if m.style.text.is_some() {
        let mut style = m.style.clone();
        style.text = None;
        let mut out = vec![];
        if let Some(color) = m.fill {
            out.extend(styled(
                crate::vector::text_ink(m).intersection(area),
                color,
                &style,
            ));
        }
        if m.stroke {
            let mut line = m.clone();
            line.style.text = None;
            line.fill = None;
            for ring in std::iter::once(crate::vector::flatten(m, 0.04)).chain(
                m.holes
                    .iter()
                    .map(|r| r.iter().copied().map(crate::vector::pos).collect()),
            ) {
                line.nodes = Default::default();
                line.holes = Default::default();
                line.points = ring.into_iter().map(crate::vector::xy).collect();
                out.extend(styled(
                    crate::width::outline(&line).intersection(area),
                    m.color,
                    &style,
                ));
            }
        }
        return out;
    }
    let mut out = vec![];
    for polygon in &area.0 {
        let (_, parts) = crate::editing::split_area(m, polygon);
        out.extend(parts);
    }
    out
}

pub fn affine(m: &Mark, matrix: [f32; 6]) -> Mark {
    crate::vector::transform(m, |p| {
        Pos2::new(
            matrix[0] * p.x + matrix[2] * p.y + matrix[4],
            matrix[1] * p.x + matrix[3] * p.y + matrix[5],
        )
    })
}
fn expand(
    doc: &Document,
    mark: &Mark,
    frame: usize,
    rig_frame: usize,
    depth: usize,
    out: &mut Vec<Mark>,
    guides: bool,
    clock: usize,
    poses: &std::collections::HashMap<u64, crate::rig::Pose>,
    mouth: Option<crate::lip::RouteScope<'_>>,
    surfaces: bool,
    piece: u64,
) {
    if mark.style.guide && !guides {
        return;
    }
    if depth > 16 {
        return;
    }
    if let Some(instance) = &mark.style.instance {
        let Some(symbol) = doc.extras.symbols.iter().find(|s| s.id == instance.symbol) else {
            return;
        };
        let count = symbol.layers.first().map_or(1, |l| l.frames.len()).max(1);
        let parent_frame = rig_frame;
        let mouth = if mouth.is_none() && !mark.style.mouth_routes.is_empty() {
            Some(crate::lip::RouteScope::new(&mark.style.mouth_routes, frame))
        } else {
            mouth
        };
        let frame = mark
            .style
            .animation_clip
            .as_ref()
            .and_then(|timing| {
                crate::animation_clips::source_frame(doc, timing, instance.born, frame)
            })
            .or_else(|| {
                mouth
                    .as_ref()
                    .and_then(crate::lip::RouteScope::pose)
                    .or_else(|| crate::lip::pose_at(&mark.style.mouth_keys, frame))
                    .filter(|&pose| pose < count)
            })
            .unwrap_or_else(|| {
                if symbol.mouth_poses.is_empty() {
                    symbol_frame(instance, count, frame, clock)
                } else {
                    instance.first.min(count - 1)
                }
            });
        let mut sub = doc.clone();
        sub.layers = symbol.layers.clone();
        sub.extras.cameras = Default::default();
        sub.extras.rig = symbol.rig.clone();
        sub.extras.folders = symbol.folders.clone();
        let binding = doc
            .extras
            .rig
            .bindings
            .iter()
            .find(|b| b.target == mark.style.track && b.symbol == instance.symbol);
        let deform = binding.map(|b| crate::rig::Deformer::new(b, poses, parent_frame));
        let replacement = crate::character::artwork(
            doc,
            mark.style.track,
            parent_frame,
            (!symbol.mouth_poses.is_empty()).then_some(frame),
        );
        let resolved = if let Some((layers, view)) = replacement {
            sub.layers = layers;
            sub.extras.rig = Default::default();
            sub.extras.folders = Default::default();
            resolve_internal(
                &sub,
                view,
                depth + 1,
                guides,
                clock,
                mouth.as_ref(),
                surfaces,
                piece,
            )
        } else {
            resolve_internal(
                &sub,
                frame,
                depth + 1,
                guides,
                if mark.style.animation_clip.is_some() {
                    frame
                } else {
                    clock
                        .saturating_sub(instance.born)
                        .saturating_add(instance.first)
                },
                mouth.as_ref(),
                surfaces,
                piece,
            )
        };
        for (_, _, mut m) in resolved {
            if let Some(c) = mark.style.instance_color {
                apply_color(&mut m, c);
            }
            if let Some(d) = &deform {
                out.extend(d.mark(&m, instance.matrix));
            } else {
                out.push(affine(&m, instance.matrix));
            }
        }
    } else {
        out.push(mark.clone());
    }
}
pub fn ink(marks: &[Mark]) -> geo::MultiPolygon<f64> {
    let mut pieces = vec![];
    for mark in marks {
        if mark.fill.is_some_and(|c| c[3] > 0)
            || mark
                .style
                .gradient
                .as_ref()
                .is_some_and(|g| g.stops.iter().any(|s| s.color[3] > 0))
        {
            pieces.push(if mark.style.text.is_some() {
                crate::vector::text_ink(mark)
            } else {
                geo::MultiPolygon(vec![crate::vector::polygon(mark)])
            });
        }
        if mark.stroke && mark.color[3] > 0 {
            pieces.push(crate::width::outline(mark));
        }
    }
    geo::algorithm::bool_ops::unary_union(&pieces)
}
fn resolve_internal(
    doc: &Document,
    frame: usize,
    depth: usize,
    guides: bool,
    clock: usize,
    mouth: Option<&crate::lip::RouteScope<'_>>,
    surfaces: bool,
    scope: u64,
) -> Vec<(usize, usize, Mark)> {
    let mut pose_cache = std::collections::HashMap::new();
    let mut previous_folder = crate::rig::IDENTITY;
    let mut explicit_masks = std::collections::HashMap::new();
    let mut output = vec![];
    let mut below = vec![];
    for (l, layer) in doc.layers.iter().enumerate() {
        let guide = layer.options.height_guide.is_some() || layer.options.light_mask;
        if guide && !(surfaces || guides && depth == 0 && layer.options.light_mask_target.is_none())
        {
            continue;
        }
        if layer.options.mannequin.is_some()
            || layer.options.mask_id.is_some()
            || crate::post::is_adjustment(layer)
        {
            continue;
        }
        if !layer.visible || !crate::folder_groups::layer_active(doc, layer, frame) {
            below.clear();
            continue;
        }
        let folder = crate::folder_groups::layer_matrix(doc, layer);
        let rig_frame = crate::folder_groups::rig_frame(doc, layer, frame);
        let character_poses = |frame| {
            let rig = &doc.extras.rig;
            let joints = crate::character::view_joints(doc, rig, frame);
            if joints.is_empty() {
                return rig.poses(frame);
            }
            let mut rig = rig.clone();
            for bone in &mut rig.bones {
                if let Some((_, [start, end])) = joints.iter().find(|(id, _)| *id == bone.id)
                    && crate::vector::pos(*start).distance(crate::vector::pos(*end)) > 0.01
                {
                    bone.start = *start;
                    bone.end = *end;
                }
            }
            rig.poses(frame)
        };
        let poses = pose_cache
            .entry(rig_frame)
            .or_insert_with(|| character_poses(rig_frame));
        let mut current = vec![];
        let mask = if let Some(id) = layer.options.masked_by {
            let source = explicit_masks.entry(id).or_insert_with(|| {
                let source = doc
                    .layers
                    .iter()
                    .find(|layer| layer.options.mask_id == Some(id))?;
                let mut marks = Vec::new();
                if crate::folder_groups::layer_active(doc, source, frame) {
                    let rig_frame = crate::folder_groups::rig_frame(doc, source, frame);
                    let poses = character_poses(rig_frame);
                    for mark in
                        crate::animation::marks(source, frame.min(source.frames.len() - 1)).iter()
                    {
                        if !object_visible(source, mark, frame) {
                            continue;
                        }
                        expand(
                            doc, mark, frame, rig_frame, depth, &mut marks, false, clock, &poses,
                            None, false, 0,
                        );
                    }
                }
                Some((marks, crate::folder_groups::layer_matrix(doc, source)))
            });
            source.as_ref().map(|(marks, matrix)| {
                let relative = crate::rig::multiply(
                    crate::folder_groups::inverse(folder).unwrap_or(crate::rig::IDENTITY),
                    *matrix,
                );
                let converted: Vec<_> = marks
                    .iter()
                    .map(|mark| crate::folder_groups::mark(mark, relative))
                    .collect();
                ink(&converted)
            })
        } else if layer.options.clip {
            let relative = crate::rig::multiply(
                crate::folder_groups::inverse(folder).unwrap_or(crate::rig::IDENTITY),
                previous_folder,
            );
            let converted: Vec<_> = below
                .iter()
                .map(|m| crate::folder_groups::mark(m, relative))
                .collect();
            Some(ink(&converted))
        } else {
            None
        };
        let first_output = output.len();
        let marks = crate::animation::marks(layer, frame.min(layer.frames.len() - 1));
        let mut arranged = Vec::new();
        let natural = marks.iter().enumerate();
        let ordered: &mut dyn Iterator<Item = (usize, &Mark)> =
            if doc.extras.rig.character_symbol.is_some() {
                arranged.extend(natural);
                arranged
                    .sort_by_cached_key(|(_, mark)| crate::character::order(doc, mark, rig_frame));
                &mut arranged.drain(..)
            } else {
                &mut marks.iter().enumerate()
            };
        for (m, mark) in ordered {
            if !object_visible(layer, mark, frame) {
                continue;
            }
            let mut expanded = vec![];
            expand(
                doc,
                mark,
                frame,
                rig_frame,
                depth,
                &mut expanded,
                guides,
                clock,
                poses,
                mouth.and_then(|scope| scope.child(l, m, mark)),
                surfaces,
                scope
                    .wrapping_mul(0x100000001b3)
                    .wrapping_add(((l as u64 + 1) << 32) | (m as u64 + 1)),
            );
            for original in expanded {
                let parts = mask
                    .as_ref()
                    .map_or_else(|| vec![original.clone()], |mask| clip_to(&original, mask));
                for mut part in parts {
                    let alpha = layer.options.opacity.clamp(0., 1.);
                    part.color[3] = (part.color[3] as f32 * alpha).round() as u8;
                    if let Some(c) = part.fill.as_mut() {
                        c[3] = (c[3] as f32 * alpha).round() as u8;
                    }
                    if let Some(g) = part.style.gradient.as_mut() {
                        for s in &mut g.stops {
                            s.color[3] = (s.color[3] as f32 * alpha).round() as u8;
                        }
                    }
                    if surfaces {
                        if part.style.surface_piece == 0 {
                            part.style.surface_piece =
                                scope.wrapping_mul(0x100000001b3).wrapping_add(l as u64 + 1);
                        }
                        part.style.surface_height =
                            layer.options.height_guide.or(part.style.surface_height);
                        part.style.light_mask |= layer.options.light_mask;
                        part.style.light_mask_target = layer
                            .options
                            .light_mask_target
                            .or(part.style.light_mask_target);
                    }
                    if part.style.surface_height.is_none() && !part.style.light_mask && !guide {
                        current.push(part.clone());
                    }
                    output.push((
                        l,
                        m,
                        if depth > 0 {
                            crate::folder_groups::mark(&part, folder)
                        } else {
                            part
                        },
                    ));
                }
            }
        }
        if surfaces && layer.options.light_mask && output.len() == first_output {
            // A visible empty mask is still a mask. Erasing the last stroke must
            // leave an empty matte rather than restore unrestricted lighting.
            let mut empty = Mark::default();
            empty.style.light_mask = true;
            empty.style.light_mask_target = layer.options.light_mask_target;
            output.push((l, 0, empty));
        }
        if !guide {
            below = current;
            previous_folder = folder;
        }
    }
    output
}
pub fn resolve(doc: &Document, frame: usize, camera: bool) -> Vec<(usize, usize, Mark)> {
    resolve_mode(doc, frame, camera, true, frame, false)
}
pub fn resolve_export(doc: &Document, frame: usize, camera: bool) -> Vec<(usize, usize, Mark)> {
    resolve_mode(doc, frame, camera, false, frame, false)
}
fn resolve_mode(
    doc: &Document,
    frame: usize,
    camera: bool,
    guides: bool,
    clock: usize,
    surfaces: bool,
) -> Vec<(usize, usize, Mark)> {
    let mut marks = resolve_internal(doc, frame, 0, guides, clock, None, surfaces, 0);
    if camera || !guides {
        for (l, _, mark) in &mut marks {
            let matrix = crate::folder_groups::layer_matrix(doc, &doc.layers[*l]);
            *mark = crate::folder_groups::mark(mark, matrix);
        }
    }
    if camera && let Some(track) = crate::depth::camera(doc, frame) {
        let optics = crate::animation::camera_optics(track, frame);
        marks.retain(|(l, _, _)| crate::depth::visible(&doc.layers[*l].options.depth, &optics));
        project_camera_marks(doc, frame, track, &optics, &mut marks);
    }
    marks
}

pub(crate) fn project_camera_marks(
    doc: &Document,
    frame: usize,
    track: &Camera,
    optics: &crate::depth::Optics,
    marks: &mut [(usize, usize, Mark)],
) {
    let c = crate::animation::camera_at(track, frame);
    let center = Pos2::new(doc.width as f32 / 2., doc.height as f32 / 2.);
    for (l, _, m) in marks {
        *m = crate::depth::project_mark(doc, &c, optics, &doc.layers[*l].options.depth, m);
        *m = crate::vector::transform(m, |p| {
            center + crate::canvas::rotate(p - crate::vector::pos(c.center), -c.angle) * c.zoom
        });
        m.width *= c.zoom;
    }
}

pub fn baked(doc: &Document) -> Document {
    if doc.extras.rig.is_empty()
        && doc
            .extras
            .folders
            .iter()
            .all(|f| f.transform == crate::folder_groups::Transform::default() && f.range.is_none())
        && doc.extras.scenes.is_empty()
        && doc.extras.symbols.is_empty()
        && doc.extras.cameras.is_empty()
        && doc.layers.iter().all(|l| {
            l.options.object_folders.is_empty()
                && l.options.composition == crate::folder_groups::LayerGroup::default()
                && l.options.tweens.is_empty()
                && l.options.tweens.is_empty()
                && l.options.mannequin.is_none()
                && l.options.opacity == 1.
                && !l.options.clip
                && l.options.mask_id.is_none()
                && l.options.masked_by.is_none()
                && l.frames.iter().flatten().all(|m| {
                    !m.style.guide && m.style.gradient.is_none() && m.style.instance.is_none()
                })
        })
    {
        return doc.clone();
    }
    let count = crate::animation::movie_count(doc);
    let mut result = Document::default();
    result.width = doc.width;
    result.height = doc.height;
    result.fps = doc.fps;
    result.layers[0].frames = Vec::with_capacity(count).into();
    for f in 0..count {
        let mut frame = vec![];
        for (_, _, mark) in crate::animation::resolve_movie(doc, f, true) {
            if let Some(g) = &mark.style.gradient {
                let bounds = crate::vector::control_bounds(&mark);
                let source = if mark.style.text.is_some() {
                    crate::vector::text_ink(&mark)
                } else {
                    geo::MultiPolygon(vec![crate::vector::polygon(&mark)])
                };
                for y in 0..24 {
                    for x in 0..24 {
                        let a = bounds.min
                            + Vec2::new(
                                bounds.width() * x as f32 / 24.,
                                bounds.height() * y as f32 / 24.,
                            );
                        let b = a + bounds.size() / 24.;
                        let cutter =
                            crate::editing::area(&[a, Pos2::new(b.x, a.y), b, Pos2::new(a.x, b.y)]);
                        let mut style = mark.style.clone();
                        style.gradient = None;
                        frame.extend(styled(
                            source.intersection(&cutter),
                            g.color(a + (b - a) * 0.5),
                            &style,
                        ));
                    }
                }
                if mark.stroke && mark.color[3] > 0 {
                    let mut stroke = mark.clone();
                    stroke.fill = None;
                    stroke.style.gradient = None;
                    frame.push(stroke);
                }
            } else {
                frame.push(mark);
            }
        }
        result.layers[0].frames.push(frame.into());
    }
    result
}
pub fn validate(doc: &Document) -> Result<(), String> {
    crate::reftool::validate(&doc.extras.references)?;
    crate::dialogue::validate(doc)?;
    for layer in doc
        .layers
        .iter()
        .chain(doc.extras.scenes.iter().flat_map(|s| s.layers.iter()))
        .chain(doc.extras.symbols.iter().flat_map(|s| s.layers.iter()))
    {
        if layer
            .options
            .height_guide
            .is_some_and(|v| !(-1. ..=1.).contains(&v))
        {
            return Err("Invalid height guide".into());
        }
        if !crate::post::valid_bands(layer)
            || (crate::post::is_adjustment(layer)
                && (layer
                    .options
                    .adjustment
                    .as_ref()
                    .is_some_and(|effects| !crate::post::valid(effects))
                    || layer
                        .options
                        .light
                        .as_ref()
                        .is_some_and(|light| !light.valid())
                    || layer.options.raster
                    || layer.options.animation_track
                    || layer.frames.iter().any(|frame| !frame.is_empty())))
        {
            return Err("Invalid effect or light layer".into());
        }
    }
    doc.extras.rig.validate()?;
    doc.extras.library.validate(&doc.extras.symbols)?;
    for symbol in &doc.extras.symbols {
        symbol.rig.validate()?;
        if symbol.mouth_poses.len() > 128 || symbol.mouth_poses.iter().any(|name| !name.valid()) {
            return Err("Invalid Lip Sync poses".into());
        }
    }
    if doc.extras.folders.len() > 200
        || doc.extras.symbols.len() > 500
        || doc.extras.cameras.len() > 1000
    {
        return Err("Too many project resources".into());
    }
    if doc.layers.is_empty() || doc.layers.len() > 100 {
        return Err("Invalid layers".into());
    }
    if doc
        .layers
        .iter()
        .filter(|l| l.options.mannequin.is_some())
        .count()
        > 1
    {
        return Err("Only one mannequin per scene".into());
    }
    validate_resources(doc)?;
    validate_expansion(doc)?;
    crate::animation::validate(doc)?;
    crate::animation_clips::validate(doc)?;
    let mut checked = std::collections::HashSet::new();
    for layer in &doc.layers {
        if !layer.options.depth.valid()
            || !layer.options.opacity.is_finite()
            || !(0. ..=1.).contains(&layer.options.opacity)
        {
            return Err("Invalid layer opacity".into());
        }
        for m in layer
            .frames
            .iter()
            .filter(|f| checked.insert(f.identity()))
            .flatten()
        {
            if m.style.gradient.as_ref().is_some_and(|g| !g.valid()) {
                return Err("Invalid gradient".into());
            }
            if let Some(i) = &m.style.instance {
                if !i.matrix.iter().all(|v| v.is_finite() && v.abs() < 1e7)
                    || !doc.extras.symbols.iter().any(|s| s.id == i.symbol)
                {
                    return Err("Invalid symbol instance".into());
                }
            }
        }
    }
    let mut end = 0;
    for c in &doc.extras.cameras {
        if !c.optics.valid()
            || c.start != end
            || c.duration == 0
            || !c.zoom.is_finite()
            || !(0.01..=100.).contains(&c.zoom)
            || !c.angle.is_finite()
            || c.center.iter().any(|v| !v.is_finite() || v.abs() > 1e7)
        {
            return Err("Invalid camera sequence".into());
        }
        end = c.start + c.duration;
    }
    for folder in &doc.extras.folders {
        let mut parent = folder.parent;
        for depth in 0..=doc.extras.folders.len() {
            if let Some(id) = parent {
                if id == folder.id || depth == doc.extras.folders.len() {
                    return Err("Folder cycle".into());
                }
                parent = doc
                    .extras
                    .folders
                    .iter()
                    .find(|f| f.id == id)
                    .ok_or("Missing folder")?
                    .parent;
            } else {
                break;
            }
        }
    }
    for symbol in &doc.extras.symbols {
        if symbol.layers.is_empty() || symbol.layers.len() > 100 {
            return Err("Invalid symbol".into());
        }
        let count = symbol.layers[0].frames.len();
        if count == 0 || count > 10000 || symbol.layers.iter().any(|l| l.frames.len() != count) {
            return Err("Invalid symbol timeline".into());
        }
        if symbol.layers.iter().any(|layer| {
            layer.options.character_piece.as_ref().is_some_and(|piece| {
                !matches!(count, 8 | crate::character::FACE_FRAMES) || !piece.valid()
            })
        }) {
            return Err("Invalid character view".into());
        }
    }
    Ok(())
}

fn validate_resources(doc: &Document) -> Result<(), String> {
    fn folders(folders: &[Folder], layers: &[Layer]) -> Result<(), String> {
        if folders.len() > 200 {
            return Err("Too many folders".into());
        }
        let mut ids = std::collections::HashSet::new();
        for f in folders {
            if !f.transform.valid()
                || f.range.is_some_and(|r| r[0] > r[1] || r[1] > 10000)
                || f.source_shift.unsigned_abs() > 1000000
            {
                return Err("Invalid folder composition".into());
            }
            if !ids.insert(f.id) {
                return Err("Duplicate folder ID".into());
            }
        }
        for f in folders {
            let m = crate::folder_groups::matrix(folders, Some(f.id));
            if m.iter().any(|v| !v.is_finite() || v.abs() > 1e12)
                || crate::folder_groups::inverse(m).is_none()
            {
                return Err("Invalid combined folder transform".into());
            }
            let mut parent = f.parent;
            for depth in 0..=32 {
                if let Some(id) = parent {
                    if id == f.id || depth == 32 {
                        return Err("Folder cycle or excessive depth".into());
                    }
                    parent = folders
                        .iter()
                        .find(|f| f.id == id)
                        .ok_or("Missing parent folder")?
                        .parent;
                } else {
                    break;
                }
            }
        }
        if layers
            .iter()
            .any(|l| l.options.folder.is_some_and(|id| !ids.contains(&id)))
        {
            return Err("Missing layer folder".into());
        }
        Ok(())
    }
    folders(&doc.extras.folders, &doc.layers)?;
    fn layers(layers: &[Layer], symbols: &[Symbol]) -> Result<(), String> {
        let mut checked = std::collections::HashSet::new();
        for layer in layers {
            let mut folder_ids = std::collections::HashSet::new();
            if layer.options.object_folders.len() > 200
                || layer.options.object_folders.iter().any(|folder| {
                    folder.id == 0
                        || !folder_ids.insert(folder.id)
                        || folder.name.len() > 256
                        || folder.choices.len() > 10000
                        || folder
                            .choices
                            .iter()
                            .any(|&(frame, object)| frame >= 10000 || object == 0)
                        || folder.choices.windows(2).any(|keys| keys[0].0 >= keys[1].0)
                })
            {
                return Err("Invalid object folders".into());
            }
            if !layer.options.composition.valid() {
                return Err("Invalid layer composition".into());
            }
            if layer.options.mannequin.as_ref().is_some_and(|m| !m.valid()) {
                return Err("Invalid mannequin".into());
            }
            if layer
                .options
                .tweens
                .iter()
                .any(|s| s.start >= s.end || s.end >= layer.frames.len() || !s.curve.valid())
            {
                return Err("Invalid symbol motion".into());
            }
            if layer.frames.is_empty()
                || layer.frames.len() > 10000
                || !layer.options.depth.valid()
                || !layer.options.opacity.is_finite()
                || !(0. ..=1.).contains(&layer.options.opacity)
                || (!layer.options.keys.is_empty()
                    && (layer.options.keys.len() != layer.frames.len() || !layer.options.keys[0]))
            {
                return Err("Invalid layer or exposure timeline".into());
            }
            for mark in layer
                .frames
                .iter()
                .filter(|f| checked.insert(f.identity()))
                .flatten()
            {
                if mark
                    .style
                    .instance_color
                    .is_some_and(|c| c.iter().any(|v| !v.is_finite() || v.abs() > 1e7))
                {
                    return Err("Invalid instance color transform".into());
                }
                let keys = &mark.style.mouth_keys;
                if keys.len() > 10_000
                    || keys
                        .iter()
                        .any(|key| key.frame >= 10_000 || key.pose >= 128)
                    || !keys.windows(2).all(|pair| pair[0].frame < pair[1].frame)
                    || !keys.is_empty() && mark.style.instance.is_none()
                {
                    return Err("Invalid Lip Sync keys".into());
                }
                if !crate::lip::valid_routes(&mark.style.mouth_routes) {
                    return Err("Invalid nested Lip Sync channels".into());
                }
                if mark.style.text.as_ref().is_some_and(|t| !t.valid()) {
                    return Err("Invalid text".into());
                }
                if !mark.width.is_finite()
                    || mark.width <= 0.
                    || mark.width > 100.
                    || !crate::width::validate(&mark.width_profile)
                    || mark
                        .points
                        .iter()
                        .flatten()
                        .chain(mark.nodes.iter().flat_map(|n| {
                            n.position
                                .iter()
                                .chain(n.in_handle.iter())
                                .chain(n.out_handle.iter())
                        }))
                        .chain(mark.holes.iter().flat_map(|h| h.iter().flatten()))
                        .any(|v| !v.is_finite() || v.abs() > 1e7)
                    || mark.style.gradient.as_ref().is_some_and(|g| !g.valid())
                {
                    return Err("Invalid symbol or scene geometry".into());
                }
                if let Some(i) = &mark.style.instance {
                    if !symbols.iter().any(|s| s.id == i.symbol)
                        || i.matrix.iter().any(|v| !v.is_finite() || v.abs() > 1e7)
                    {
                        return Err("Invalid symbol reference".into());
                    }
                }
            }
        }
        Ok(())
    }
    layers(&doc.layers, &doc.extras.symbols)?;
    let mut unique = std::collections::HashSet::new();
    for symbol in &doc.extras.symbols {
        if !unique.insert(symbol.id) {
            return Err("Duplicate symbol ID".into());
        }
        layers(&symbol.layers, &doc.extras.symbols)?;
        folders(&symbol.folders, &symbol.layers)?;
    }
    let graph = symbol_graph(&doc.extras.symbols);
    let mut memo = std::collections::HashMap::new();
    for id in graph.keys() {
        symbol_depth(
            *id,
            &graph,
            &mut std::collections::HashSet::new(),
            &mut memo,
        )?;
    }
    unique.clear();
    for folder in &doc.extras.folders {
        if !unique.insert(folder.id) {
            return Err("Duplicate folder ID".into());
        }
    }
    for layer in &doc.layers {
        if layer.options.folder.is_some_and(|id| !unique.contains(&id)) {
            return Err("Missing layer folder".into());
        }
    }
    let p = &doc.extras.perspective;
    if p.points.is_empty()
        || p.points.len() > 3
        || !(4..=32).contains(&p.rays)
        || !p.horizon.is_finite()
        || p.points
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 1e7)
    {
        return Err("Invalid perspective guides".into());
    }
    unique.clear();
    for camera in &doc.extras.cameras {
        if !unique.insert(camera.id)
            || camera.start.saturating_add(camera.duration)
                > doc.layers.first().map_or(0, |l| l.frames.len())
        {
            return Err("Invalid camera cut".into());
        }
    }
    Ok(())
}

pub fn svg(doc: &Document, frame: usize) -> String {
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\"><rect width=\"{}\" height=\"{}\" fill=\"white\"/>",
        doc.width, doc.height, doc.width, doc.height, doc.width, doc.height
    );
    let mut index = 0;
    let mut group = None;
    for (layer, _, original) in resolve_export(doc, frame, true) {
        if group != Some(layer) {
            if group.is_some() {
                out += "</g>";
            }
            group = Some(layer);
            if let Some(camera) = crate::depth::camera(doc, frame) {
                let optics = crate::animation::camera_optics(camera, frame);
                let radius =
                    optics.radius(doc.layers[layer].options.depth.distance - optics.position);
                if radius > 0.1 {
                    let pad = radius * 2. + 3.;
                    let filter = match optics.method {
                        crate::depth::BlurMethod::Gaussian => {
                            format!("<feGaussianBlur stdDeviation=\"{}\"/>", radius * 0.5)
                        }
                        crate::depth::BlurMethod::Lens => {
                            let kernel = (-5i32..=5)
                                .flat_map(|y| {
                                    (-5i32..=5)
                                        .map(move |x| if x * x + y * y <= 25 { "1" } else { "0" })
                                })
                                .collect::<Vec<_>>()
                                .join(" ");
                            format!(
                                "<feConvolveMatrix order=\"11\" kernelMatrix=\"{kernel}\" divisor=\"81\" kernelUnitLength=\"{}\" edgeMode=\"none\"/>",
                                radius / 5.
                            )
                        }
                    };
                    out += &format!(
                        "<defs><filter id=\"dof-{layer}\" filterUnits=\"userSpaceOnUse\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" color-interpolation-filters=\"sRGB\">{filter}</filter></defs><g filter=\"url(#dof-{layer})\">",
                        -pad,
                        -pad,
                        doc.width as f32 + pad * 2.,
                        doc.height as f32 + pad * 2.
                    );
                } else {
                    out += "<g>";
                }
            } else {
                out += "<g>";
            }
        }
        for mark in crate::width::export_marks(&original) {
            let mut fill = mark
                .fill
                .map(|c| format!("rgb({},{},{})", c[0], c[1], c[2]))
                .unwrap_or("none".into());
            let mut alpha = mark.fill.map_or(1., |c| c[3] as f32 / 255.);
            if let Some(g) = mark.style.gradient.as_ref().filter(|_| mark.fill.is_some()) {
                let name = format!("gradient-{index}");
                index += 1;
                let tag = if g.radial {
                    "radialGradient"
                } else {
                    "linearGradient"
                };
                if g.radial {
                    out += &format!(
                        "<defs><{tag} id=\"{name}\" gradientUnits=\"userSpaceOnUse\" cx=\"{}\" cy=\"{}\" r=\"{}\">",
                        g.start[0],
                        g.start[1],
                        crate::vector::pos(g.start)
                            .distance(crate::vector::pos(g.end))
                            .max(0.001)
                    );
                } else {
                    out += &format!(
                        "<defs><{tag} id=\"{name}\" gradientUnits=\"userSpaceOnUse\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\">",
                        g.start[0], g.start[1], g.end[0], g.end[1]
                    );
                }
                for stop in &g.stops {
                    out += &format!(
                        "<stop offset=\"{}\" stop-color=\"rgb({},{},{})\" stop-opacity=\"{}\"/>",
                        stop.t,
                        stop.color[0],
                        stop.color[1],
                        stop.color[2],
                        stop.color[3] as f32 / 255.
                    );
                }
                out += &format!("</{tag}></defs>");
                fill = format!("url(#{name})");
                alpha = 1.;
            }
            let stroke = if mark.stroke {
                format!("rgb({},{},{})", mark.color[0], mark.color[1], mark.color[2])
            } else {
                "none".into()
            };
            if mark.points.len() == 1 && mark.nodes.is_empty() && mark.stroke && !mark.rectangle {
                out += &format!(
                    "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{stroke}\" fill-opacity=\"{}\"/>",
                    mark.points[0][0],
                    mark.points[0][1],
                    mark.width / 2.,
                    mark.color[3] as f32 / 255.
                );
            } else {
                out += &format!(
                    "<path d=\"{}\" fill=\"{fill}\" fill-opacity=\"{alpha}\" fill-rule=\"evenodd\" stroke=\"{stroke}\" stroke-opacity=\"{}\" stroke-width=\"{}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>",
                    mark.svg_path(),
                    mark.color[3] as f32 / 255.,
                    mark.width
                );
            }
        }
    }
    if group.is_some() {
        out += "</g>";
    }
    out + "</svg>"
}

pub fn resolve_preview(
    doc: &Document,
    frame: usize,
    camera: bool,
    clock: usize,
) -> Vec<(usize, usize, Mark)> {
    resolve_mode(doc, frame, camera, false, clock, true)
}
pub fn resolve_at(
    doc: &Document,
    frame: usize,
    camera: bool,
    clock: usize,
) -> Vec<(usize, usize, Mark)> {
    resolve_mode(doc, frame, camera, true, clock, false)
}
pub fn symbol_frame(i: &Instance, count: usize, frame: usize, clock: usize) -> usize {
    let count = count.max(1);
    let source = if i.clock == SymbolClock::Independent {
        clock
    } else {
        frame
    };
    let offset = source.saturating_sub(i.born).saturating_add(i.first);
    match i.mode {
        Playback::Loop => offset % count,
        Playback::Once => offset.min(count - 1),
        Playback::Single => i.first.min(count - 1),
    }
}

pub fn apply_color(m: &mut Mark, c: [f32; 8]) {
    let convert = |color: &mut [u8; 4]| {
        for i in 0..4 {
            color[i] = (color[i] as f32 * c[i] + c[i + 4]).round().clamp(0., 255.) as u8;
        }
    };
    convert(&mut m.color);
    if let Some(f) = m.fill.as_mut() {
        convert(f);
    }
    if let Some(g) = m.style.gradient.as_mut() {
        for s in &mut g.stops {
            convert(&mut s.color);
        }
    }
}

/// Deduplicate held exposures and dependency edges before traversing the DAG.
pub(crate) fn symbol_graph(symbols: &[Symbol]) -> std::collections::HashMap<u64, Vec<u64>> {
    symbols
        .iter()
        .map(|s| {
            let mut frames = std::collections::HashSet::new();
            let mut edges = std::collections::BTreeSet::new();
            for frame in s.layers.iter().flat_map(|l| l.frames.iter()) {
                if frames.insert(frame.identity()) {
                    for i in frame.iter().filter_map(|m| m.style.instance.as_ref()) {
                        edges.insert(i.symbol);
                    }
                }
            }
            (s.id, edges.into_iter().collect())
        })
        .collect()
}
fn symbol_depth(
    id: u64,
    graph: &std::collections::HashMap<u64, Vec<u64>>,
    active: &mut std::collections::HashSet<u64>,
    memo: &mut std::collections::HashMap<u64, usize>,
) -> Result<usize, String> {
    if let Some(d) = memo.get(&id) {
        return Ok(*d);
    }
    if active.len() >= 8 || !active.insert(id) {
        return Err("Recursive symbol or excessive nesting".into());
    }
    let mut depth = 1;
    for child in graph.get(&id).into_iter().flatten() {
        depth = depth.max(1 + symbol_depth(*child, graph, active, memo)?);
    }
    active.remove(&id);
    if depth > 8 {
        return Err("Recursive symbol or excessive nesting".into());
    }
    memo.insert(id, depth);
    Ok(depth)
}

/// Limit expanded geometry rather than counting shared resources only. A tiny
/// recursive-free DAG can otherwise instantiate billions of paths in one frame.
pub(crate) fn validate_expansion(doc: &Document) -> Result<(), String> {
    fn layers_cost(
        layers: &[Layer],
        symbols: &[Symbol],
        memo: &mut std::collections::HashMap<u64, usize>,
    ) -> usize {
        let mut total = 0usize;
        for layer in layers {
            let mut seen = std::collections::HashSet::new();
            let mut max = 0usize;
            for frame in &layer.frames {
                if !seen.insert(frame.identity()) {
                    continue;
                }
                let mut cost = 0usize;
                for mark in frame {
                    let value = if let Some(i) = &mark.style.instance {
                        if let Some(cost) = memo.get(&i.symbol) {
                            *cost
                        } else if let Some(s) = symbols.iter().find(|s| s.id == i.symbol) {
                            let cost = layers_cost(&s.layers, symbols, memo);
                            memo.insert(s.id, cost);
                            cost
                        } else {
                            1
                        }
                    } else {
                        mark.points
                            .len()
                            .saturating_add(mark.nodes.len().saturating_mul(3))
                            .saturating_add(mark.holes.iter().map(|h| h.len()).sum::<usize>())
                            .max(1)
                    };
                    cost = cost.saturating_add(value).min(2_000_001);
                    if cost > 2_000_000 {
                        break;
                    }
                }
                max = max.max(cost);
            }
            total = total.saturating_add(max).min(2_000_001);
        }
        total
    }
    let mut memo = std::collections::HashMap::new();
    for s in &doc.extras.symbols {
        if layers_cost(&s.layers, &doc.extras.symbols, &mut memo) > 2_000_000 {
            return Err("Expanded symbol geometry exceeds 2000000 control points per frame".into());
        }
    }
    if layers_cost(&doc.layers, &doc.extras.symbols, &mut memo) > 2_000_000 {
        return Err("Expanded scene geometry exceeds 2000000 control points per frame".into());
    }
    Ok(())
}
