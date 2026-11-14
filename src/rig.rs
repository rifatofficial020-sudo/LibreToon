//! Native, vector-only deformation. A context owns bones and bindings; symbols
//! retain their original editable paths. Weight lattices interpolate influences,
//! not pixels. FK/IK and puppet pins share the same deterministic export path.
use crate::{
    animation::Curve,
    document::Mark,
    shared::SharedVec,
    vector::{self, Cubic},
};
use eframe::egui::{Pos2, Rect, Vec2};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
pub const GRID: usize = 17;
pub const CELLS: usize = GRID * GRID;
pub type Matrix = [f32; 6];
pub const IDENTITY: Matrix = [1., 0., 0., 1., 0., 0.];
pub fn point(m: Matrix, p: Pos2) -> Pos2 {
    Pos2::new(
        m[0] * p.x + m[2] * p.y + m[4],
        m[1] * p.x + m[3] * p.y + m[5],
    )
}
pub fn inverse(m: Matrix) -> Option<Matrix> {
    let d = m[0] * m[3] - m[1] * m[2];
    if !d.is_finite() || d.abs() < 1e-9 {
        return None;
    }
    Some([
        m[3] / d,
        -m[1] / d,
        -m[2] / d,
        m[0] / d,
        (m[2] * m[5] - m[3] * m[4]) / d,
        (m[1] * m[4] - m[0] * m[5]) / d,
    ])
}
pub fn multiply(a: Matrix, b: Matrix) -> Matrix {
    let o = point(a, Pos2::new(b[4], b[5]));
    [
        a[0] * b[0] + a[2] * b[1],
        a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3],
        a[1] * b[2] + a[3] * b[3],
        o.x,
        o.y,
    ]
}
fn identity(m: Matrix) -> bool {
    m.iter().zip(IDENTITY).all(|(a, b)| (*a - b).abs() < 1e-5)
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Rig {
    pub bones: SharedVec<Bone>,
    pub bindings: SharedVec<Binding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) character_symbol: Option<u64>,
    #[serde(skip_serializing_if = "CharacterTurns::is_empty")]
    pub(crate) character_turns: CharacterTurns,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct CharacterTurns {
    pub head: TurnChannel,
    pub body: TurnChannel,
}

impl CharacterTurns {
    fn is_empty(&self) -> bool {
        self.head.root.is_none() && self.body.root.is_none()
    }
    pub(crate) fn identity(&self) -> [usize; 14] {
        let mut ids = [0; 14];
        for (channel, slots) in [&self.head, &self.body]
            .into_iter()
            .zip(ids.chunks_exact_mut(7))
        {
            slots[0] = channel.root.unwrap_or_default() as usize;
            slots[1] = channel.keys.identity();
            for (slot, view) in slots[2..].iter_mut().zip(&channel.views) {
                *slot = view
                    .as_ref()
                    .map_or(0, |view| std::sync::Arc::as_ptr(view) as usize);
            }
        }
        ids
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct TurnChannel {
    pub root: Option<u64>,
    pub views: [Option<std::sync::Arc<SavedPose>>; 5],
    pub keys: SharedVec<TurnKey>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct TurnKey {
    pub frame: usize,
    pub value: f32,
}

impl Rig {
    pub(crate) fn character_angle(&self, track: u64, frame: usize) -> Option<usize> {
        self.character_symbol?;
        let turns = &self.character_turns;
        let affects = |root| {
            self.pose_branch(root, None).into_iter().any(|id| {
                self.bones
                    .iter()
                    .any(|bone| bone.id == id && bone.targets.contains(&track))
            })
        };
        let body = turns
            .body
            .root
            .filter(|&root| affects(root))
            .map_or(0., |_| turns.body.value(frame));
        let head = turns
            .head
            .root
            .filter(|&root| affects(root))
            .map_or(0., |_| turns.head.value(frame));
        Some(((body + head).round() as i32).rem_euclid(8) as usize)
    }
}

impl TurnChannel {
    pub fn value(&self, frame: usize) -> f32 {
        let index = self.keys.partition_point(|key| key.frame <= frame);
        let Some(a) = index.checked_sub(1).and_then(|i| self.keys.get(i)) else {
            return 0.;
        };
        self.keys.get(index).map_or(a.value, |b| {
            a.value + (b.value - a.value) * (frame - a.frame) as f32 / (b.frame - a.frame) as f32
        })
    }

    pub fn set(&mut self, frame: usize, value: f32) {
        if self.keys.is_empty() && frame != 0 {
            self.keys.push(TurnKey {
                frame: 0,
                value: 0.,
            });
        }
        match self.keys.binary_search_by_key(&frame, |key| key.frame) {
            Ok(index) => self.keys[index].value = value,
            Err(index) => self.keys.insert(index, TurnKey { frame, value }),
        }
    }

    fn offsets(&self, branch: Vec<u64>, frame: usize, output: &mut HashMap<u64, BoneKey>) {
        let Some(neutral) = &self.views[2] else {
            return;
        };
        let value = self.value(frame) + 2.;
        let mut views = self
            .views
            .iter()
            .enumerate()
            .filter_map(|(i, p)| p.as_ref().map(|p| (i as f32, p)));
        let Some(mut left) = views.next() else { return };
        let mut right = left;
        for view in views {
            if value <= right.0 {
                break;
            }
            left = right;
            right = view;
        }
        let blend = if right.0 == left.0 {
            0.
        } else {
            ((value - left.0) / (right.0 - left.0)).clamp(0., 1.)
        };
        for (id, base) in &neutral.bones {
            if !branch.contains(id) {
                continue;
            }
            let at = |pose: &SavedPose| {
                pose.bones
                    .iter()
                    .find(|(bone, _)| bone == id)
                    .map(|(_, key)| key.clone())
                    .unwrap_or_else(|| base.clone())
            };
            let mut offset =
                blend_pose_key(&at(left.1), &at(right.1), BoneKey::default(), frame, blend);
            offset.angle = (offset.angle - base.angle + std::f32::consts::PI)
                .rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;
            for axis in 0..2 {
                offset.translation[axis] -= base.translation[axis];
            }
            offset.scale /= base.scale;
            offset.stretch /= base.stretch;
            output.insert(*id, offset);
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct SavedPose {
    pub(crate) name: String,
    pub(crate) bones: Vec<(u64, BoneKey)>,
}

pub(crate) fn blend_pose_key(
    from: &BoneKey,
    to: &BoneKey,
    mut key: BoneKey,
    frame: usize,
    blend: f32,
) -> BoneKey {
    let mix = |a: f32, b: f32| a + (b - a) * blend;
    let turn = (to.angle - from.angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;
    key.frame = frame;
    key.angle = from.angle + turn * blend;
    key.translation = std::array::from_fn(|axis| mix(from.translation[axis], to.translation[axis]));
    key.scale = mix(from.scale, to.scale);
    key.stretch = mix(from.stretch, to.stretch);
    key
}

impl Rig {
    fn turn_offsets(&self, frame: usize) -> HashMap<u64, BoneKey> {
        let turns = &self.character_turns;
        let mut offsets = HashMap::new();
        for (channel, excluded) in [(&turns.body, turns.head.root), (&turns.head, None)] {
            if let Some(root) = channel.root {
                channel.offsets(self.pose_branch(root, excluded), frame, &mut offsets);
            }
        }
        offsets
    }
    /// Includes the root while stopping at an optional independently posed branch.
    pub(crate) fn pose_branch(&self, root: u64, excluded: Option<u64>) -> Vec<u64> {
        let mut branch = vec![root];
        let mut cursor = 0;
        while let Some(&parent) = branch.get(cursor) {
            for bone in self.bones.iter().filter(|bone| bone.parent == Some(parent)) {
                if Some(bone.id) != excluded && !branch.contains(&bone.id) {
                    branch.push(bone.id);
                }
            }
            cursor += 1;
        }
        branch
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Bone {
    pub id: u64,
    pub name: String,
    pub parent: Option<u64>,
    pub start: [f32; 2],
    pub end: [f32; 2],
    pub radius: f32,
    pub ik: bool,
    pub targets: SharedVec<u64>,
    pub keys: SharedVec<BoneKey>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct BoneKey {
    pub frame: usize,
    pub angle: f32,
    pub translation: [f32; 2],
    pub scale: f32,
    #[serde(
        default = "neutral_stretch",
        skip_serializing_if = "is_neutral_stretch"
    )]
    pub stretch: f32,
    pub curve: Curve,
}
fn neutral_stretch() -> f32 {
    1.
}
fn is_neutral_stretch(value: &f32) -> bool {
    *value == neutral_stretch()
}
impl Default for BoneKey {
    fn default() -> Self {
        Self {
            frame: 0,
            angle: 0.,
            translation: [0., 0.],
            scale: 1.,
            stretch: 1.,
            curve: Default::default(),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Pin {
    pub id: u64,
    pub name: String,
    pub rest: [f32; 2],
    pub keys: SharedVec<PinKey>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct PinKey {
    pub frame: usize,
    pub position: [f32; 2],
    pub curve: Curve,
}
#[derive(Clone, Copy, Default, Serialize, Deserialize)]
pub struct Influence {
    pub bone: u64,
    pub weight: f32,
}
#[derive(Clone, Copy, Default, Serialize, Deserialize)]
pub struct Cell {
    pub influences: [Influence; 4],
}
impl Cell {
    pub fn weight(&self, bone: u64) -> f32 {
        self.influences
            .iter()
            .filter(|i| i.bone == bone)
            .map(|i| i.weight)
            .sum()
    }
    pub fn set(&mut self, bone: u64, weight: f32) {
        // Painting edits only this bone. Unused weight stays in the rest pose.
        let slot = self
            .influences
            .iter()
            .position(|i| i.bone == bone)
            .or_else(|| self.influences.iter().position(|i| i.weight <= 1e-8));
        let Some(slot) = slot else {
            return;
        };
        let other = self
            .influences
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != slot)
            .map(|(_, i)| i.weight)
            .sum::<f32>();
        self.influences[slot] = Influence {
            bone,
            weight: weight.clamp(0., (1. - other).max(0.)),
        };
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Binding {
    pub target: u64,
    pub symbol: u64,
    pub matrix: Matrix,
    pub bounds: [f32; 4],
    pub cells: SharedVec<Cell>,
    pub pins: SharedVec<Pin>,
}
impl Binding {
    pub fn new(target: u64, symbol: u64, matrix: Matrix, bounds: Rect) -> Self {
        let bounds = bounds.expand(1.);
        Self {
            target,
            symbol,
            matrix,
            bounds: [bounds.left(), bounds.top(), bounds.right(), bounds.bottom()],
            cells: vec![Cell::default(); CELLS].into(),
            pins: Default::default(),
        }
    }
    pub fn rect(&self) -> Rect {
        Rect::from_min_max(
            Pos2::new(self.bounds[0], self.bounds[1]),
            Pos2::new(self.bounds[2], self.bounds[3]),
        )
    }
    pub fn grid_point(&self, i: usize) -> Pos2 {
        let r = self.rect();
        r.min
            + r.size()
                * Vec2::new(
                    (i % GRID) as f32 / (GRID - 1) as f32,
                    (i / GRID) as f32 / (GRID - 1) as f32,
                )
    }
    pub fn sample(&self, p: Pos2) -> Vec<Influence> {
        if self.cells.len() != CELLS {
            return vec![];
        }
        let r = self.rect();
        let q = ((p - r.min) / r.size().max(Vec2::splat(0.001))).clamp(Vec2::ZERO, Vec2::splat(1.))
            * (GRID - 1) as f32;
        let x = (q.x.floor() as usize).min(GRID - 2);
        let y = (q.y.floor() as usize).min(GRID - 2);
        let tx = q.x - x as f32;
        let ty = q.y - y as f32;
        let mut out: Vec<Influence> = vec![];
        for (index, factor) in [
            (y * GRID + x, (1. - tx) * (1. - ty)),
            (y * GRID + x + 1, tx * (1. - ty)),
            ((y + 1) * GRID + x, (1. - tx) * ty),
            ((y + 1) * GRID + x + 1, tx * ty),
        ] {
            for i in &self.cells[index].influences {
                if i.bone == 0 || i.weight * factor < 1e-7 {
                    continue;
                }
                if let Some(v) = out.iter_mut().find(|v| v.bone == i.bone) {
                    v.weight += i.weight * factor;
                } else {
                    out.push(Influence {
                        bone: i.bone,
                        weight: i.weight * factor,
                    });
                }
            }
        }
        out
    }
    pub fn weight(&self, p: Pos2, bone: u64) -> f32 {
        self.sample(p)
            .iter()
            .filter(|i| i.bone == bone)
            .map(|i| i.weight)
            .sum::<f32>()
            .clamp(0., 1.)
    }
}
#[derive(Clone)]
pub struct Pose {
    pub targets: SharedVec<u64>,
    pub start: Pos2,
    pub end: Pos2,
    pub angle: f32,
    pub scale: f32,
    pub matrix: Matrix,
}
pub fn bone_value(bone: &Bone, frame: usize) -> BoneKey {
    let index = bone.keys.partition_point(|k| k.frame <= frame);
    if index == 0 {
        return bone.keys.first().cloned().unwrap_or_default();
    }
    let a = &bone.keys[index - 1];
    let Some(b) = bone.keys.get(index) else {
        return a.clone();
    };
    let t = a
        .curve
        .time((frame - a.frame) as f32 / (b.frame - a.frame) as f32);
    BoneKey {
        frame,
        angle: a.angle * (1. - t) + b.angle * t,
        translation: vector::xy(a.curve.point(
            vector::pos(a.translation),
            vector::pos(b.translation),
            t,
        )),
        scale: a.scale * (1. - t) + b.scale * t,
        stretch: a.stretch * (1. - t) + b.stretch * t,
        curve: a.curve.clone(),
    }
}
pub fn pin_value(pin: &Pin, frame: usize) -> Pos2 {
    let index = pin.keys.partition_point(|k| k.frame <= frame);
    if index == 0 {
        return pin
            .keys
            .first()
            .map_or(vector::pos(pin.rest), |k| vector::pos(k.position));
    }
    let a = &pin.keys[index - 1];
    let Some(b) = pin.keys.get(index) else {
        return vector::pos(a.position);
    };
    let t = a
        .curve
        .time((frame - a.frame) as f32 / (b.frame - a.frame) as f32);
    a.curve
        .point(vector::pos(a.position), vector::pos(b.position), t)
}
impl Rig {
    pub fn snapshot_targets(&self, targets: &HashSet<u64>, frame: usize) -> Self {
        let mut result = self.clone();
        result.bindings.retain(|b| targets.contains(&b.target));
        if result.bindings.is_empty() {
            return Self::default();
        }
        for b in &mut result.bones {
            b.targets.retain(|t| targets.contains(t));
            let mut key = bone_value(b, frame);
            key.frame = 0;
            b.keys = vec![key].into();
        }
        for channel in [
            &mut result.character_turns.head,
            &mut result.character_turns.body,
        ] {
            let value = channel.value(frame);
            channel.keys.clear();
            channel.set(0, value);
        }
        for b in &mut result.bindings {
            for p in &mut b.pins {
                p.keys = vec![PinKey {
                    frame: 0,
                    position: vector::xy(pin_value(p, frame)),
                    curve: Default::default(),
                }]
                .into();
            }
        }
        result
    }
    pub fn trim_keys(&mut self, count: usize) {
        for channel in [
            &mut self.character_turns.head,
            &mut self.character_turns.body,
        ] {
            channel.keys.retain(|key| key.frame < count);
        }
        for b in &mut self.bones {
            b.keys.retain(|k| k.frame < count);
        }
        for b in &mut self.bindings {
            for p in &mut b.pins {
                p.keys.retain(|k| k.frame < count);
            }
        }
    }
    pub fn is_empty(&self) -> bool {
        self.bones.is_empty() && self.bindings.is_empty()
    }
    pub fn poses(&self, frame: usize) -> HashMap<u64, Pose> {
        fn solve(
            index: usize,
            rig: &Rig,
            lookup: &HashMap<u64, usize>,
            cache: &mut HashMap<u64, Pose>,
            visiting: &mut HashSet<u64>,
            turns: &HashMap<u64, BoneKey>,
            frame: usize,
        ) -> Option<Pose> {
            let b = &rig.bones[index];
            if let Some(p) = cache.get(&b.id) {
                return Some(p.clone());
            }
            if visiting.len() >= 64 || !visiting.insert(b.id) {
                return None;
            }
            let parent = b
                .parent
                .and_then(|id| lookup.get(&id))
                .and_then(|&i| solve(i, rig, lookup, cache, visiting, turns, frame));
            let mut value = bone_value(b, frame);
            if let Some(offset) = turns.get(&b.id) {
                value.angle += offset.angle;
                for axis in 0..2 {
                    value.translation[axis] += offset.translation[axis];
                }
                value.scale = (value.scale * offset.scale).clamp(0.01, 100.);
                value.stretch = (value.stretch * offset.stretch).clamp(0.1, 10.);
            }
            let rest = vector::pos(b.start);
            let rest_angle = (vector::pos(b.end) - rest).angle();
            let angle = rest_angle
                + parent.as_ref().map_or(0., |p| {
                    p.angle
                        - (vector::pos(rig.bones[*lookup.get(&b.parent.unwrap()).unwrap()].end)
                            - vector::pos(
                                rig.bones[*lookup.get(&b.parent.unwrap()).unwrap()].start,
                            ))
                        .angle()
                })
                + value.angle;
            let inherited = parent.as_ref().map_or(IDENTITY, |p| p.matrix);
            let start = point(inherited, rest)
                + crate::canvas::rotate(
                    vector::pos(value.translation).to_vec2(),
                    parent.as_ref().map_or(0., |p| p.angle),
                );
            let scale = value.scale * parent.as_ref().map_or(1., |p| p.scale);
            let rotation = angle - rest_angle;
            let (s, c) = rotation.sin_cos();
            let mut matrix = [
                c * scale,
                s * scale,
                -s * scale,
                c * scale,
                start.x - (c * rest.x - s * rest.y) * scale,
                start.y - (s * rest.x + c * rest.y) * scale,
            ];
            if !is_neutral_stretch(&value.stretch) {
                let (sin, cos) = rest_angle.sin_cos();
                let across = value.stretch.recip();
                let difference = value.stretch - across;
                let xx = across + difference * cos * cos;
                let xy = difference * cos * sin;
                let yy = across + difference * sin * sin;
                matrix = multiply(
                    matrix,
                    [
                        xx,
                        xy,
                        xy,
                        yy,
                        rest.x - xx * rest.x - xy * rest.y,
                        rest.y - xy * rest.x - yy * rest.y,
                    ],
                );
            }
            let pose = Pose {
                targets: b.targets.clone(),
                start,
                end: point(matrix, vector::pos(b.end)),
                angle,
                scale,
                matrix,
            };
            visiting.remove(&b.id);
            cache.insert(b.id, pose.clone());
            Some(pose)
        }
        let lookup = self
            .bones
            .iter()
            .enumerate()
            .map(|(i, b)| (b.id, i))
            .collect();
        let mut cache = HashMap::new();
        let mut visiting = HashSet::new();
        let turns = self.turn_offsets(frame);
        for i in 0..self.bones.len() {
            solve(i, self, &lookup, &mut cache, &mut visiting, &turns, frame);
        }
        cache
    }
    pub fn set_bone_key(&mut self, id: u64, key: BoneKey) {
        if let Some(b) = self.bones.iter_mut().find(|b| b.id == id) {
            if let Some(k) = b.keys.iter_mut().find(|k| k.frame == key.frame) {
                *k = key;
            } else {
                b.keys.push(key);
                b.keys.sort_by_key(|k| k.frame);
            }
        }
    }
    pub fn set_pin_key(&mut self, target: u64, id: u64, key: PinKey) {
        if let Some(p) = self
            .bindings
            .iter_mut()
            .find(|b| b.target == target)
            .and_then(|b| b.pins.iter_mut().find(|p| p.id == id))
        {
            if let Some(k) = p.keys.iter_mut().find(|k| k.frame == key.frame) {
                *k = key;
            } else {
                p.keys.push(key);
                p.keys.sort_by_key(|k| k.frame);
            }
        }
    }
    pub fn can_parent(&self, id: u64, mut parent: Option<u64>) -> bool {
        let mut seen = HashSet::new();
        while let Some(p) = parent {
            if p == id || !seen.insert(p) || seen.len() > 64 {
                return false;
            }
            let Some(b) = self.bones.iter().find(|b| b.id == p) else {
                return false;
            };
            parent = b.parent;
        }
        true
    }
    pub fn detach(&mut self, bone: u64, target: u64) {
        if let Some(b) = self.bones.iter_mut().find(|b| b.id == bone) {
            b.targets.retain(|t| *t != target);
        }
        if let Some(binding) = self.bindings.iter_mut().find(|b| b.target == target) {
            for cell in &mut binding.cells {
                for influence in &mut cell.influences {
                    if influence.bone == bone {
                        *influence = Influence::default();
                    }
                }
            }
        }
    }
    pub fn auto_weights(&mut self, target: u64) {
        let Some(i) = self.bindings.iter().position(|b| b.target == target) else {
            return;
        };
        let binding = &self.bindings[i];
        let bones: Vec<_> = self
            .bones
            .iter()
            .filter(|b| b.targets.contains(&target))
            .collect();
        let cells = (0..CELLS)
            .map(|n| {
                let p = binding.grid_point(n);
                let mut values: Vec<_> = bones
                    .iter()
                    .map(|b| {
                        let d = segment_distance(p, vector::pos(b.start), vector::pos(b.end));
                        Influence {
                            bone: b.id,
                            weight: 1. / (d * d + b.radius * b.radius * 0.08 + 0.01).powi(2),
                        }
                    })
                    .collect();
                values.sort_by(|a, b| b.weight.total_cmp(&a.weight));
                values.truncate(4);
                let sum = values.iter().map(|v| v.weight).sum::<f32>();
                let mut cell = Cell::default();
                for (slot, v) in cell.influences.iter_mut().zip(values) {
                    *slot = Influence {
                        bone: v.bone,
                        weight: if sum > 0. { v.weight / sum } else { 0. },
                    };
                }
                cell
            })
            .collect();
        self.bindings[i].cells = cells;
    }
    pub fn solve_ik(&mut self, id: u64, target: Pos2, frame: usize) {
        let mut chain = vec![];
        let mut current = Some(id);
        while let Some(id) = current {
            if chain.contains(&id) || chain.len() >= 32 {
                break;
            }
            chain.push(id);
            current = self
                .bones
                .iter()
                .find(|b| b.id == id)
                .and_then(|b| b.parent);
        }
        for _ in 0..32 {
            let poses = self.poses(frame);
            let Some(tip) = poses.get(&id).map(|p| p.end) else {
                return;
            };
            if tip.distance(target) < 0.05 {
                break;
            }
            for joint in &chain {
                let poses = self.poses(frame);
                let Some(p) = poses.get(joint) else {
                    continue;
                };
                let tip = poses.get(&id).unwrap().end;
                let a = tip - p.start;
                let b = target - p.start;
                if a.length_sq() < 1e-8 || b.length_sq() < 1e-8 {
                    continue;
                }
                let bone = self.bones.iter().find(|b| b.id == *joint).unwrap();
                let mut key = bone_value(bone, frame);
                key.frame = frame;
                key.angle += crate::animation::shortest(b.angle() - a.angle());
                self.set_bone_key(*joint, key);
            }
        }
    }
    pub fn remove_bone(&mut self, id: u64) {
        for channel in [
            &mut self.character_turns.head,
            &mut self.character_turns.body,
        ] {
            if channel.root == Some(id) {
                channel.root = None;
            }
            for pose in channel.views.iter_mut().flatten() {
                std::sync::Arc::make_mut(pose)
                    .bones
                    .retain(|(bone, _)| *bone != id);
            }
        }
        let parent = self
            .bones
            .iter()
            .find(|b| b.id == id)
            .and_then(|b| b.parent);
        self.bones.retain(|b| b.id != id);
        for b in &mut self.bones {
            if b.parent == Some(id) {
                b.parent = parent;
            }
        }
        for b in &mut self.bindings {
            for cell in &mut b.cells {
                for i in &mut cell.influences {
                    if i.bone == id {
                        *i = Influence::default();
                    }
                }
            }
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.bones.len() > 128 || self.bindings.len() > 512 {
            return Err("Rig exceeds resource limits".into());
        }
        let finite = |v: f32| v.is_finite() && v.abs() <= 1e7;
        for channel in [&self.character_turns.head, &self.character_turns.body] {
            if channel.keys.len() > 10000
                || channel.keys.windows(2).any(|w| w[0].frame >= w[1].frame)
                || channel
                    .keys
                    .iter()
                    .any(|key| key.frame >= 10000 || !(-4.0..=4.0).contains(&key.value))
                || channel.views.iter().flatten().any(|pose| {
                    pose.name.len() > 256
                        || pose.bones.len() > 128
                        || pose.bones.iter().any(|(_, key)| {
                            !finite(key.angle)
                                || key.translation.iter().any(|v| !finite(*v))
                                || !(0.01..=100.).contains(&key.scale)
                                || !(0.1..=10.).contains(&key.stretch)
                        })
                })
            {
                return Err("Invalid character turn".into());
            }
        }
        let mut ids = HashSet::new();
        for b in &self.bones {
            if b.id == 0
                || !ids.insert(b.id)
                || b.name.len() > 1024
                || !self.can_parent(b.id, b.parent)
                || b.start.iter().chain(b.end.iter()).any(|v| !finite(*v))
                || vector::pos(b.start).distance(vector::pos(b.end)) < 0.001
                || !b.radius.is_finite()
                || !(0.1..=10000.).contains(&b.radius)
                || b.targets.len() > 512
                || b.keys.len() > 10000
                || b.keys.windows(2).any(|w| w[0].frame >= w[1].frame)
                || b.keys.iter().any(|k| {
                    k.frame >= 10000
                        || !finite(k.angle)
                        || k.translation.iter().any(|v| !finite(*v))
                        || !k.scale.is_finite()
                        || !(0.01..=100.).contains(&k.scale)
                        || !(0.1..=10.).contains(&k.stretch)
                        || !k.curve.valid()
                })
            {
                return Err("Invalid bone hierarchy or animation".into());
            }
        }
        let mut targets = HashSet::new();
        for b in &self.bindings {
            if b.target == 0
                || !targets.insert(b.target)
                || b.cells.len() != CELLS
                || b.matrix.iter().any(|v| !finite(*v))
                || inverse(b.matrix).is_none()
                || b.bounds.iter().any(|v| !finite(*v))
                || b.bounds[0] >= b.bounds[2]
                || b.bounds[1] >= b.bounds[3]
                || b.pins.len() > 32
            {
                return Err("Invalid deformation binding".into());
            }
            for cell in &b.cells {
                let mut seen = HashSet::new();
                let sum = cell.influences.iter().map(|i| i.weight).sum::<f32>();
                if sum > 1.0001
                    || cell.influences.iter().any(|i| {
                        !i.weight.is_finite()
                            || !(0. ..=1.).contains(&i.weight)
                            || (i.weight > 0. && (!ids.contains(&i.bone) || !seen.insert(i.bone)))
                    })
                {
                    return Err("Invalid bone weights".into());
                }
            }
            let mut pins = HashSet::new();
            for p in &b.pins {
                if p.id == 0
                    || !pins.insert(p.id)
                    || p.name.len() > 1024
                    || p.rest.iter().any(|v| !finite(*v))
                    || p.keys.len() > 10000
                    || p.keys.windows(2).any(|w| w[0].frame >= w[1].frame)
                    || p.keys.iter().any(|k| {
                        k.frame >= 10000
                            || k.position.iter().any(|v| !finite(*v))
                            || !k.curve.valid()
                    })
                {
                    return Err("Invalid puppet pins or animation".into());
                }
            }
        }
        Ok(())
    }
}
pub fn segment_distance(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let d = b - a;
    p.distance(a + d * ((p - a).dot(d) / d.length_sq().max(1e-9)).clamp(0., 1.))
}
pub fn skin_point(binding: &Binding, poses: &HashMap<u64, Pose>, p: Pos2) -> Pos2 {
    let mut sum = 0.;
    let mut out = Vec2::ZERO;
    for i in binding.sample(p) {
        if let Some(b) = poses
            .get(&i.bone)
            .filter(|b| b.targets.contains(&binding.target))
        {
            out += point(b.matrix, p).to_vec2() * i.weight;
            sum += i.weight;
        }
    }
    Pos2::ZERO + out + p.to_vec2() * (1. - sum).max(0.)
}
pub struct Deformer<'a> {
    pub binding: &'a Binding,
    pub poses: &'a HashMap<u64, Pose>,
    warp: Warp,
    pub active: bool,
}
impl<'a> Deformer<'a> {
    pub fn new(binding: &'a Binding, poses: &'a HashMap<u64, Pose>, frame: usize) -> Self {
        let source: Vec<_> = binding
            .pins
            .iter()
            .map(|p| skin_point(binding, poses, vector::pos(p.rest)))
            .collect();
        let target: Vec<_> = binding
            .pins
            .iter()
            .map(|p| skin_point(binding, poses, pin_value(p, frame)))
            .collect();
        let warp = Warp::new(source, target);
        let active = !warp.is_identity()
            || binding.cells.iter().flat_map(|c| c.influences).any(|i| {
                i.weight > 1e-7
                    && poses
                        .get(&i.bone)
                        .is_some_and(|p| p.targets.contains(&binding.target) && !identity(p.matrix))
            });
        Self {
            binding,
            poses,
            warp,
            active,
        }
    }
    pub fn point(&self, p: Pos2) -> Pos2 {
        self.warp.apply(skin_point(self.binding, self.poses, p))
    }
    pub fn inverse_point(&self, p: Pos2) -> Pos2 {
        inverse_field(p, |q| self.point(q))
    }
    pub fn inverse_skin(&self, p: Pos2) -> Pos2 {
        inverse_field(p, |q| skin_point(self.binding, self.poses, q))
    }
    pub fn mark(&self, mark: &Mark, current: Matrix) -> Vec<Mark> {
        if !self.active {
            return vec![crate::features::affine(mark, current)];
        }
        let delta = multiply(current, inverse(self.binding.matrix).unwrap_or(IDENTITY));
        let f = |p: Pos2| point(delta, self.point(point(self.binding.matrix, p)));
        let mut out = vec![];
        if mark.fill.is_some() {
            let mut fill = mark.clone();
            fill.stroke = false;
            out.push(deform_path(&fill, &f));
        }
        if mark.stroke {
            for m in vector::polygons_to_marks(crate::width::outline(mark), mark.color) {
                out.push(deform_path(&m, &f));
            }
        }
        if mark.fill.is_none() && !mark.stroke {
            out.push(deform_path(mark, &f));
        }
        out
    }
}
fn inverse_field(p: Pos2, f: impl Fn(Pos2) -> Pos2) -> Pos2 {
    let mut q = p;
    for _ in 0..20 {
        let d = p - f(q);
        if d.length_sq() < 1e-6 {
            break;
        }
        let h = 0.02;
        let x = (f(q + Vec2::X * h) - f(q)) / h;
        let y = (f(q + Vec2::Y * h) - f(q)) / h;
        let det = x.x * y.y - x.y * y.x;
        if det.abs() < 1e-7 {
            q += d * 0.5;
        } else {
            q += Vec2::new((d.x * y.y - d.y * y.x) / det, (x.x * d.y - x.y * d.x) / det)
                .clamp(Vec2::splat(-1000.), Vec2::splat(1000.));
        }
    }
    q
}
pub fn deform_path(mark: &Mark, f: &impl Fn(Pos2) -> Pos2) -> Mark {
    fn curved(c: Cubic, f: &impl Fn(Pos2) -> Pos2, depth: u32, out: &mut Vec<Cubic>) {
        let mapped = Cubic(c.0.map(f));
        let error = [0.25, 0.5, 0.75]
            .into_iter()
            .map(|t| mapped.at(t).distance(f(c.at(t))))
            .fold(0., f32::max);
        if error > 0.02 && depth < 12 && out.len() < 16384 {
            let (a, b) = c.split(0.5);
            curved(a, f, depth + 1, out);
            curved(b, f, depth + 1, out);
        } else {
            out.push(mapped);
        }
    }
    let mut curves = vec![];
    for c in vector::cubics(mark) {
        curved(c, f, 0, &mut curves);
    }
    let mut out = if curves.is_empty() {
        vector::transform(mark, f)
    } else {
        vector::from_cubics(&curves, mark)
    };
    out.closed = mark.closed || mark.rectangle;
    out.holes = mark
        .holes
        .iter()
        .map(|ring| {
            let m = Mark {
                points: ring.clone(),
                closed: true,
                ..Default::default()
            };
            let mut curves = vec![];
            for c in vector::cubics(&m) {
                curved(c, f, 0, &mut curves);
            }
            let path = vector::from_cubics(&curves, &m);
            vector::flatten(&path, 0.02)
                .into_iter()
                .map(vector::xy)
                .collect()
        })
        .collect();
    if let Some(g) = out.style.gradient.as_mut() {
        g.start = vector::xy(f(vector::pos(g.start)));
        g.end = vector::xy(f(vector::pos(g.end)));
    }
    out
}
enum Warp {
    Identity,
    Shift(Vec2),
    Similarity {
        a: Pos2,
        c: Pos2,
        x: f32,
        y: f32,
    },
    ThinPlate {
        origin: Pos2,
        unit: f32,
        source: Vec<[f64; 2]>,
        coeff: Vec<[f64; 2]>,
    },
    Shepard {
        source: Vec<Pos2>,
        target: Vec<Pos2>,
    },
}
fn radial(r2: f64) -> f64 {
    if r2 < 1e-16 { 0. } else { 0.5 * r2 * r2.ln() }
}
impl Warp {
    fn new(source: Vec<Pos2>, target: Vec<Pos2>) -> Self {
        if source.is_empty()
            || source
                .iter()
                .zip(&target)
                .all(|(a, b)| a.distance(*b) < 1e-5)
        {
            return Self::Identity;
        }
        if source.len() == 1 {
            return Self::Shift(target[0] - source[0]);
        }
        if source.len() == 2 {
            let u = source[1] - source[0];
            let v = target[1] - target[0];
            let d = u.length_sq();
            if d > 1e-8 {
                return Self::Similarity {
                    a: source[0],
                    c: target[0],
                    x: u.dot(v) / d,
                    y: (u.x * v.y - u.y * v.x) / d,
                };
            }
        }
        let origin = source[0];
        let unit = source.iter().map(|p| p.distance(origin)).fold(1., f32::max);
        let n = source.len();
        let size = n + 3;
        let normalized: Vec<_> = source
            .iter()
            .map(|p| {
                let q = (*p - origin) / unit;
                [q.x as f64, q.y as f64]
            })
            .collect();
        let mut a = vec![vec![0.; size + 2]; size];
        for i in 0..n {
            for j in 0..n {
                let dx = normalized[i][0] - normalized[j][0];
                let dy = normalized[i][1] - normalized[j][1];
                a[i][j] = radial(dx * dx + dy * dy);
            }
            a[i][i] += 1e-10;
            a[i][n] = 1.;
            a[i][n + 1] = normalized[i][0];
            a[i][n + 2] = normalized[i][1];
            a[n][i] = 1.;
            a[n + 1][i] = normalized[i][0];
            a[n + 2][i] = normalized[i][1];
            let delta = (target[i] - source[i]) / unit;
            a[i][size] = delta.x as f64;
            a[i][size + 1] = delta.y as f64;
        }
        for col in 0..size {
            let pivot = (col..size)
                .max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))
                .unwrap();
            if a[pivot][col].abs() < 1e-12 {
                return Self::Shepard { source, target };
            }
            a.swap(col, pivot);
            let v = a[col][col];
            for j in col..size + 2 {
                a[col][j] /= v;
            }
            for i in 0..size {
                if i == col {
                    continue;
                }
                let factor = a[i][col];
                for j in col..size + 2 {
                    a[i][j] -= factor * a[col][j];
                }
            }
        }
        let coeff = a.iter().map(|r| [r[size], r[size + 1]]).collect();
        Self::ThinPlate {
            origin,
            unit,
            source: normalized,
            coeff,
        }
    }
    fn is_identity(&self) -> bool {
        matches!(self, Self::Identity)
    }
    fn apply(&self, p: Pos2) -> Pos2 {
        match self {
            Self::Identity => p,
            Self::Shift(d) => p + *d,
            Self::Similarity { a, c, x, y } => {
                let q = p - *a;
                *c + Vec2::new(x * q.x - y * q.y, y * q.x + x * q.y)
            }
            Self::ThinPlate {
                origin,
                unit,
                source,
                coeff,
            } => {
                let q = (p - *origin) / *unit;
                let n = source.len();
                let mut d = [
                    coeff[n][0] + coeff[n + 1][0] * q.x as f64 + coeff[n + 2][0] * q.y as f64,
                    coeff[n][1] + coeff[n + 1][1] * q.x as f64 + coeff[n + 2][1] * q.y as f64,
                ];
                for (s, c) in source.iter().zip(coeff) {
                    let dx = q.x as f64 - s[0];
                    let dy = q.y as f64 - s[1];
                    let w = radial(dx * dx + dy * dy);
                    d[0] += c[0] * w;
                    d[1] += c[1] * w;
                }
                p + Vec2::new(d[0] as f32, d[1] as f32) * *unit
            }
            Self::Shepard { source, target } => {
                let mut sum = 0.;
                let mut delta = Vec2::ZERO;
                for (a, b) in source.iter().zip(target) {
                    let d = p.distance_sq(*a);
                    if d < 1e-8 {
                        return *b;
                    }
                    let w = 1. / d;
                    sum += w;
                    delta += (*b - *a) * w;
                }
                p + delta / sum.max(1e-9)
            }
        }
    }
}
