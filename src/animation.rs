use crate::{
    Document, Editor,
    document::{Layer, Mark},
    features::{Camera, Folder, Perspective},
    shared::SharedVec,
};
use eframe::egui::{Pos2, Vec2};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Curve {
    pub pace: [f32; 2],
    pub ease: [[f32; 2]; 2],
    pub path: [[f32; 2]; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timing: Option<Vec<[f32; 2]>>,
}
impl Default for Curve {
    fn default() -> Self {
        Self {
            pace: [1. / 3., 1. / 3.],
            ease: [[1. / 3., 1. / 3.], [2. / 3., 2. / 3.]],
            path: [[0., 0.], [0., 0.]],
            timing: None,
        }
    }
}
impl Curve {
    pub fn time(&self, t: f32) -> f32 {
        if let Some(points) = &self.timing {
            return self.direct_time(t, points);
        }
        let t = self.ease_time(t);
        3. * (1. - t).powi(2) * t * self.pace[0]
            + 3. * (1. - t) * t * t * (1. - self.pace[1])
            + t * t * t
    }

    /// Explicit graph editing uses a monotone Hermite curve. Old documents keep
    /// their original composed easing until the timing curve is edited.
    pub fn edit_timing(&mut self) {
        if self.timing.is_none() {
            let points = if self.ease != Self::default().ease {
                (1..8)
                    .map(|i| {
                        let x = i as f32 / 8.;
                        [x, self.time(x).clamp(0., 1.)]
                    })
                    .collect()
            } else {
                Vec::new()
            };
            self.timing = Some(points);
        }
    }
    fn direct_time(&self, t: f32, points: &[[f32; 2]]) -> f32 {
        let t = t.clamp(0., 1.);
        let n = points.len() + 2;
        let point = |i: usize| {
            if i == 0 {
                [0., 0.]
            } else if i == n - 1 {
                [1., 1.]
            } else {
                points[i - 1]
            }
        };
        let secant = |i: usize| {
            let a = point(i);
            let b = point(i + 1);
            (b[1] - a[1]) / (b[0] - a[0]).max(1e-6)
        };
        let tangent = |i: usize| {
            if i == 0 {
                return self.pace[0] * 3.;
            }
            if i == n - 1 {
                return self.pace[1] * 3.;
            }
            let (left, right) = (secant(i - 1), secant(i));
            if left <= 0. || right <= 0. {
                return 0.;
            }
            let h0 = point(i)[0] - point(i - 1)[0];
            let h1 = point(i + 1)[0] - point(i)[0];
            let (w0, w1) = (2. * h1 + h0, h1 + 2. * h0);
            (w0 + w1) / (w0 / left + w1 / right)
        };
        let i = (0..n - 1).find(|&i| t <= point(i + 1)[0]).unwrap_or(n - 2);
        let (a, b) = (point(i), point(i + 1));
        let h = b[0] - a[0];
        // Limit each shared tangent using both adjacent segments, so the
        // derivative stays continuous when a point is pulled near a flat section.
        let limited = |index: usize| {
            let mut factor = 1.0_f32;
            for segment in [index.checked_sub(1), (index + 1 < n).then_some(index)]
                .into_iter()
                .flatten()
            {
                let slope = secant(segment);
                if slope <= 1e-6 {
                    return 0.;
                }
                let norm = ((tangent(segment) / slope).powi(2)
                    + (tangent(segment + 1) / slope).powi(2))
                .sqrt();
                if norm > 3. {
                    factor = factor.min(3. / norm);
                }
            }
            tangent(index) * factor
        };
        let (m0, m1) = (limited(i), limited(i + 1));
        let u = ((t - a[0]) / h.max(1e-6)).clamp(0., 1.);
        ((2. * u.powi(3) - 3. * u * u + 1.) * a[1]
            + (u.powi(3) - 2. * u * u + u) * h * m0
            + (-2. * u.powi(3) + 3. * u * u) * b[1]
            + (u.powi(3) - u * u) * h * m1)
            .clamp(a[1], b[1])
    }
    fn ease_time(&self, t: f32) -> f32 {
        if self.ease[0][0] == self.ease[0][1] && self.ease[1][0] == self.ease[1][1] {
            return t.clamp(0., 1.);
        }
        if t <= 0. {
            return 0.;
        }
        if t >= 1. {
            return 1.;
        }
        let cubic = |t: f32, a: f32, b: f32| {
            3. * (1. - t).powi(2) * t * a + 3. * (1. - t) * t * t * b + t * t * t
        };
        let (mut lo, mut hi) = (0., 1.);
        for _ in 0..18 {
            let mid = (lo + hi) / 2.;
            if cubic(mid, self.ease[0][0], self.ease[1][0]) < t {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        cubic((lo + hi) / 2., self.ease[0][1], self.ease[1][1])
    }

    pub fn bend_path(&mut self, t: f32, delta: Vec2, original: [[f32; 2]; 2]) {
        let t = t.clamp(0.02, 0.98);
        let w0 = 3. * (1. - t).powi(2) * t;
        let w1 = 3. * (1. - t) * t * t;
        let sum = w0 * w0 + w1 * w1;
        for (i, weight) in [w0, w1].into_iter().enumerate() {
            self.path[i] = std::array::from_fn(|axis| {
                (original[i][axis] + delta[axis] * weight / sum).clamp(-1e6, 1e6)
            });
        }
    }
    pub fn point(&self, a: Pos2, b: Pos2, t: f32) -> Pos2 {
        let p = a + (b - a) / 3. + crate::vector::pos(self.path[0]).to_vec2();
        let q = a + (b - a) * 2. / 3. + crate::vector::pos(self.path[1]).to_vec2();
        a + (p - a) * (3. * (1. - t).powi(2) * t)
            + (q - a) * (3. * (1. - t) * t * t)
            + (b - a) * (t * t * t)
    }
    pub fn valid(&self) -> bool {
        self.timing.as_ref().is_none_or(|points| {
            points.len() <= 32
                && points.iter().all(|p| {
                    p.iter().all(|v| v.is_finite())
                        && p[0] > 0.
                        && p[0] < 1.
                        && (0. ..=1.).contains(&p[1])
                })
                && points
                    .windows(2)
                    .all(|p| p[0][0] < p[1][0] && p[0][1] <= p[1][1])
        }) && self
            .pace
            .iter()
            .all(|v| v.is_finite() && (0. ..=1.).contains(v))
            && self
                .ease
                .iter()
                .flatten()
                .all(|v| v.is_finite() && (0. ..=1.).contains(v))
            && self
                .path
                .iter()
                .flatten()
                .all(|v| v.is_finite() && v.abs() < 1e7)
    }
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Tween {
    pub start: usize,
    pub end: usize,
    pub curve: Curve,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct CameraKey {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub optics: Option<crate::depth::Optics>,
    pub frame: usize,
    pub center: [f32; 2],
    pub zoom: f32,
    pub angle: f32,
}
impl CameraKey {
    pub fn optics(&self, camera: &Camera) -> crate::depth::Optics {
        self.optics.clone().unwrap_or_else(|| camera.optics.clone())
    }
}
pub fn camera_optics(camera: &Camera, frame: usize) -> crate::depth::Optics {
    camera_at(camera, frame).optics(camera)
}
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraAnimation {
    pub keys: SharedVec<CameraKey>,
    pub curve: Curve,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Scene {
    #[serde(default)]
    pub markers: SharedVec<crate::dialogue::Marker>,
    #[serde(default)]
    pub tags: SharedVec<crate::dialogue::Tag>,
    #[serde(default)]
    pub(crate) media_tracks: SharedVec<crate::media::Track>,
    #[serde(default)]
    pub rig: crate::rig::Rig,
    pub id: u64,
    pub name: String,
    pub layers: SharedVec<Layer>,
    pub folders: SharedVec<Folder>,
    pub cameras: SharedVec<Camera>,
    pub perspective: Perspective,
}
#[derive(Clone, Copy)]
pub struct MotionBend {
    pub layer: usize,
    pub span: usize,
    pub start: Pos2,
    pub original: [[f32; 2]; 2],
    pub t: f32,
}
#[derive(Default)]
pub struct State {
    pub clock: usize,
    pub(crate) annotation: Option<crate::dialogue::Draft>,
    pub(crate) loop_tag: bool,
    pub(crate) selected_tag: Option<u64>,
    pub(crate) tag_drag: Option<crate::dialogue::TagDrag>,
    pub(crate) space_press: Option<(f64, bool)>,
    pub(crate) exposure_sheet: bool,
    pub(crate) rig_channels_open: bool,
    pub(crate) frame_workflow: crate::frame_workflow::State,
    pub scenes_open: bool,
    pub scene_rename: Option<u64>,
    pub scene_delete: Option<u64>,
    pub scene_drag: Option<usize>,
    pub play_all: bool,
    pub picker: Option<(usize, usize, usize)>,
    pub stop: usize,
    pub gradient_drag: Option<usize>,
    pub ease_handle: Option<usize>,
    pub path_handle: Option<usize>,
    pub path_bend: Option<MotionBend>,
    pub timing_transaction: bool,
    pub camera_mode: usize,
    pub thumbnail_cache: crate::paint::RenderCache,
}
pub fn scene_from(doc: &Document, id: u64, name: String) -> Scene {
    Scene {
        markers: doc.extras.markers.clone(),
        tags: doc.extras.tags.clone(),
        rig: doc.extras.rig.clone(),
        media_tracks: doc.extras.media.tracks.clone(),
        id,
        name,
        layers: doc.layers.clone(),
        folders: doc.extras.folders.clone(),
        cameras: doc.extras.cameras.clone(),
        perspective: doc.extras.perspective.clone(),
    }
}
pub fn store_scene(doc: &mut Document) {
    if !doc.extras.scenes.is_empty() {
        let index = doc.extras.active_scene.min(doc.extras.scenes.len() - 1);
        let old = &doc.extras.scenes[index];
        let scene = scene_from(doc, old.id, old.name.clone());
        doc.extras.scenes[index] = scene;
    }
}
pub fn scene_doc(doc: &Document, index: usize) -> Document {
    let mut result = doc.clone();
    if !doc.extras.scenes.is_empty() && index != doc.extras.active_scene {
        let s = &doc.extras.scenes[index];
        result.layers = s.layers.clone();
        result.extras.markers = s.markers.clone();
        result.extras.tags = s.tags.clone();
        result.extras.rig = s.rig.clone();
        result.extras.media.tracks = s.media_tracks.clone();
        result.extras.folders = s.folders.clone();
        result.extras.cameras = s.cameras.clone();
        result.extras.perspective = s.perspective.clone();
    }
    result.extras.scenes = Default::default();
    result.extras.active_scene = 0;
    result
}
pub fn movie_count(doc: &Document) -> usize {
    if doc.extras.scenes.is_empty() {
        return doc.layers[0].frames.len();
    }
    doc.extras
        .scenes
        .iter()
        .enumerate()
        .map(|(i, s)| {
            if i == doc.extras.active_scene {
                doc.layers[0].frames.len()
            } else {
                s.layers[0].frames.len()
            }
        })
        .sum()
}
pub fn locate(doc: &Document, mut frame: usize) -> (usize, usize) {
    if doc.extras.scenes.is_empty() {
        return (0, frame.min(doc.layers[0].frames.len() - 1));
    }
    for (i, s) in doc.extras.scenes.iter().enumerate() {
        let count = if i == doc.extras.active_scene {
            doc.layers[0].frames.len()
        } else {
            s.layers[0].frames.len()
        };
        if frame < count {
            return (i, frame);
        }
        frame -= count;
    }
    let i = doc.extras.scenes.len() - 1;
    (i, scene_doc(doc, i).layers[0].frames.len() - 1)
}
pub fn movie_offset(doc: &Document, local: usize) -> usize {
    doc.extras
        .scenes
        .iter()
        .take(doc.extras.active_scene)
        .map(|s| s.layers[0].frames.len())
        .sum::<usize>()
        + local
}
pub fn resolve_movie(doc: &Document, f: usize, camera: bool) -> Vec<(usize, usize, Mark)> {
    let (i, f) = locate(doc, f);
    crate::features::resolve_export(&scene_doc(doc, i), f, camera)
}
pub fn movie_svg(doc: &Document, f: usize) -> String {
    let (i, f) = locate(doc, f);
    scene_doc(doc, i).svg(f)
}
fn control(mark: &Mark) -> Vec<Pos2> {
    if mark.rectangle {
        return crate::vector::flatten(mark, 0.1);
    }
    if !mark.nodes.is_empty() {
        mark.nodes
            .iter()
            .take(86)
            .flat_map(|n| [n.position, n.in_handle, n.out_handle])
            .map(crate::vector::pos)
            .collect()
    } else {
        mark.points
            .iter()
            .take(256)
            .copied()
            .map(crate::vector::pos)
            .collect()
    }
}
pub fn delta(a: &Mark, b: &Mark) -> [f32; 4] {
    let p = control(a);
    let q = control(b);
    if p.len() != q.len() || p.len() < 2 {
        let a = crate::vector::control_bounds(a).size();
        let b = crate::vector::control_bounds(b).size();
        return [b.x / a.x.max(0.001), 0., 0., b.y / a.y.max(0.001)];
    }
    let origin = p[0];
    let i = (1..p.len())
        .max_by(|&i, &j| {
            p[i].distance_sq(origin)
                .total_cmp(&p[j].distance_sq(origin))
        })
        .unwrap();
    let u = p[i] - origin;
    let v = q[i] - q[0];
    let cross = |a: Vec2, b: Vec2| a.x * b.y - a.y * b.x;
    let j = (0..p.len())
        .max_by(|&i, &j| {
            cross(u, p[i] - origin)
                .abs()
                .total_cmp(&cross(u, p[j] - origin).abs())
        })
        .unwrap();
    let w = p[j] - origin;
    let z = q[j] - q[0];
    let det = cross(u, w);
    if det.abs() > 0.001 {
        [
            (v.x * w.y - z.x * u.y) / det,
            (v.y * w.y - z.y * u.y) / det,
            (z.x * u.x - v.x * w.x) / det,
            (z.y * u.x - v.y * w.x) / det,
        ]
    } else {
        let len = u.length_sq().max(0.001);
        let x = u.dot(v) / len;
        let y = cross(u, v) / len;
        [x, y, -y, x]
    }
}
pub fn shortest(angle: f32) -> f32 {
    (angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}
pub fn interpolate(a: &Mark, b: &Mark, t: f32, curve: &Curve) -> Mark {
    let t = curve.time(t);
    let center = crate::vector::control_bounds(a).center();
    let target = crate::vector::control_bounds(b).center();
    let position = curve.point(center, target, t);
    let m = delta(a, b);
    let sx = Vec2::new(m[0], m[1]).length().max(0.00001);
    let sy = (m[0] * m[3] - m[1] * m[2]) / sx;
    let angle = shortest(m[1].atan2(m[0])) * t;
    let scale = Vec2::new(1. + (sx - 1.) * t, 1. + (sy - 1.) * t);
    let mut out = crate::vector::transform(a, |p| {
        position + crate::canvas::rotate((p - center) * scale, angle)
    });
    if let (Some(first), Some(last)) = (out.style.instance.as_mut(), b.style.instance.as_ref()) {
        first.first = (first.first as f32 * (1. - t) + last.first as f32 * t).round() as usize;
    }
    let identity = [1., 1., 1., 1., 0., 0., 0., 0.];
    if a.style.instance_color.is_some() || b.style.instance_color.is_some() {
        let first = a.style.instance_color.unwrap_or(identity);
        let last = b.style.instance_color.unwrap_or(identity);
        out.style.instance_color = Some(std::array::from_fn(|i| first[i] * (1. - t) + last[i] * t));
    }
    out
}
pub fn span(layer: &Layer, frame: usize) -> Option<&Tween> {
    layer
        .options
        .tweens
        .iter()
        .find(|s| frame >= s.start && frame <= s.end)
}
pub fn segment(layer: &Layer, frame: usize) -> Option<(usize, usize, &Tween)> {
    let span = span(layer, frame)?;
    let a = (span.start..=frame)
        .rev()
        .find(|&f| layer.key(f))
        .unwrap_or(span.start);
    let b = (frame + 1..=span.end)
        .find(|&f| layer.key(f))
        .unwrap_or(span.end);
    (a < b).then_some((a, b, span))
}
pub fn marks(layer: &Layer, frame: usize) -> SharedVec<Mark> {
    if layer.frames.is_empty() {
        return Default::default();
    }
    let frame = frame.min(layer.frames.len() - 1);
    if layer.key(frame) {
        return layer.frames[frame].clone();
    }
    let Some((a, b, span)) = segment(layer, frame) else {
        return layer.frames[frame].clone();
    };
    let t = (frame - a) as f32 / (b - a) as f32;
    let targets: std::collections::HashMap<_, _> = layer.frames[b]
        .iter()
        .filter(|m| m.style.track != 0)
        .map(|m| (m.style.track, m))
        .collect();
    layer.frames[a]
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let target = targets
                .get(&m.style.track)
                .copied()
                .or_else(|| layer.frames[b].get(i));
            target.map_or_else(|| m.clone(), |n| interpolate(m, n, t, &span.curve))
        })
        .collect()
}
pub fn camera_at(camera: &Camera, frame: usize) -> CameraKey {
    let local = frame.saturating_sub(camera.start).min(camera.duration - 1);
    let base = CameraKey {
        optics: Some(camera.optics.clone()),
        frame: 0,
        center: camera.center,
        zoom: camera.zoom,
        angle: camera.angle,
    };
    let a = camera
        .animation
        .keys
        .iter()
        .rev()
        .find(|k| k.frame <= local)
        .unwrap_or(&base);
    let Some(b) = camera.animation.keys.iter().find(|k| k.frame > local) else {
        let mut key = a.clone();
        key.optics = Some(a.optics(camera));
        return key;
    };
    let t = camera
        .animation
        .curve
        .time((local - a.frame) as f32 / (b.frame - a.frame) as f32);
    CameraKey {
        optics: Some(a.optics(camera).interpolate(&b.optics(camera), t)),
        frame: local,
        center: crate::vector::xy(camera.animation.curve.point(
            crate::vector::pos(a.center),
            crate::vector::pos(b.center),
            t,
        )),
        zoom: a.zoom * (1. - t) + b.zoom * t,
        angle: a.angle + shortest(b.angle - a.angle) * t,
    }
}
pub fn camera_record(camera: &mut Camera, key: CameraKey) {
    if key.frame == 0 {
        camera.center = key.center;
        camera.zoom = key.zoom;
        camera.angle = key.angle;
        if let Some(optics) = &key.optics {
            camera.optics = optics.clone();
        }
    }
    if let Some(k) = camera
        .animation
        .keys
        .iter_mut()
        .find(|k| k.frame == key.frame)
    {
        *k = key;
    } else {
        camera.animation.keys.push(key);
        camera.animation.keys.sort_by_key(|k| k.frame);
    }
}
pub fn validate(doc: &Document) -> Result<(), String> {
    for layer in &doc.layers {
        for s in &layer.options.tweens {
            if s.start >= s.end || s.end >= layer.frames.len() || !s.curve.valid() {
                return Err("Invalid motion span".into());
            }
        }
        if layer.options.tweens.len() > 10000 {
            return Err("Too many motion spans".into());
        }
    }
    for c in &doc.extras.cameras {
        if !c.animation.curve.valid()
            || c.animation.keys.len() > 10000
            || c.animation
                .keys
                .windows(2)
                .any(|k| k[0].frame >= k[1].frame)
            || c.animation.keys.iter().any(|k| {
                k.frame >= c.duration
                    || k.optics.as_ref().is_some_and(|o| !o.valid())
                    || !k.zoom.is_finite()
                    || !(0.01..=100.).contains(&k.zoom)
                    || !k.angle.is_finite()
                    || k.center.iter().any(|v| !v.is_finite() || v.abs() > 1e7)
            })
        {
            return Err("Invalid camera animation".into());
        }
    }
    if doc.extras.scenes.len() > 100
        || (!doc.extras.scenes.is_empty() && doc.extras.active_scene >= doc.extras.scenes.len())
    {
        return Err("Invalid scene catalogue".into());
    }
    let mut ids = std::collections::HashSet::new();
    for (i, s) in doc.extras.scenes.iter().enumerate() {
        if !ids.insert(s.id) || s.layers.is_empty() {
            return Err("Invalid scene".into());
        }
        if i != doc.extras.active_scene {
            scene_doc(doc, i).validate()?;
        }
    }
    if !doc.extras.scenes.is_empty() && movie_count(doc) > 100000 {
        return Err("Movie exceeds 100000 frames".into());
    }
    Ok(())
}
impl Editor {
    // Transform gestures may materialize an explicitly enabled tween, but must
    // never create a new tween merely because artwork is moved.
    pub fn autokey_motion(&mut self, ids: &[(usize, usize)]) {
        self.materialize_motion(ids);
    }
    pub fn materialize_motion(&mut self, ids: &[(usize, usize)]) {
        let mut layers: Vec<_> = ids.iter().map(|i| i.0).collect();
        layers.sort_unstable();
        layers.dedup();
        for l in layers {
            if span(&self.doc.layers[l], self.frame).is_some()
                && !self.doc.layers[l].key(self.frame)
            {
                let marks = marks(&self.doc.layers[l], self.frame);
                let layer = &mut self.doc.layers[l];
                layer.init_keys();
                layer.frames[self.frame] = marks;
                layer.options.keys[self.frame] = true;
            }
        }
    }
    pub fn create_motion(&mut self) {
        let l = self.layer;
        if self.doc.layers[l].options.raster || self.adjustment_active() {
            return;
        }
        if self.doc.layers[l].options.animation_track {
            return;
        }
        if self.doc.layers[l].options.locked
            || self.doc.layers[l].options.mannequin.is_some()
            || self.doc.layers[l].frames[self.frame].is_empty()
        {
            return;
        }
        let ids = self.canvas.selected.clone();
        self.checkpoint();
        let start = self.frame;
        let count = self.doc.layers[l].frames.len();
        let end = (start + 1..count)
            .find(|&f| self.doc.layers[l].key(f) && !self.doc.layers[l].frames[f].is_empty())
            .unwrap_or((start + 9).min(count - 1));
        if end <= start {
            self.canvas.selected = ids;
            return;
        }
        let layer = &mut self.doc.layers[l];
        layer.init_keys();
        layer.options.keys[start] = true;
        let mut source = layer.frames[start].clone();
        for m in &mut source {
            if m.style.track == 0 {
                m.style.track = crate::features::id();
            }
        }
        layer.frames[start] = source.clone();
        if layer.frames[end].is_empty() {
            layer.frames[end] = source.clone();
        } else {
            for (i, m) in layer.frames[end].iter_mut().enumerate() {
                if m.style.track == 0 {
                    m.style.track = source
                        .get(i)
                        .map_or_else(crate::features::id, |m| m.style.track);
                }
            }
        }
        layer.options.keys[end] = true;
        for f in start + 1..end {
            if layer.frames[f].is_empty() || !layer.key(f) {
                layer.options.keys[f] = false;
                layer.frames[f] = source.clone();
            }
        }
        layer
            .options
            .tweens
            .retain(|s| s.end <= start || s.start >= end);
        layer.options.tweens.push(Tween {
            start,
            end,
            curve: Default::default(),
        });
        layer.options.tweens.sort_by_key(|s| s.start);
        self.canvas.selected = ids;
        self.recover();
    }
    pub fn advance_animation(&mut self, steps: usize) {
        self.animation.clock = self.animation.clock.saturating_add(steps);
        if let Some((start, end)) = self.loop_range() {
            let offset = self.frame.clamp(start, end) - start;
            let length = end - start + 1;
            self.frame = start + (offset + steps) % length;
            if offset + steps >= length {
                self.animation.clock = self.frame;
                self.restart_media_audio();
            }
        } else if self.animation.play_all && !self.doc.extras.scenes.is_empty() {
            let global = (movie_offset(&self.doc, self.frame) + steps) % movie_count(&self.doc);
            let (index, local) = locate(&self.doc, global);
            if index != self.doc.extras.active_scene {
                self.switch_scene(index);
            }
            self.frame = local;
        } else {
            self.frame = (self.frame + steps) % self.doc.layers[0].frames.len();
        }
    }
}
