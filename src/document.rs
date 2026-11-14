use crate::shared::SharedVec;
use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Mark {
    #[serde(default, skip_serializing_if = "crate::features::default_style")]
    pub style: crate::features::MarkStyle,
    pub points: SharedVec<[f32; 2]>,
    pub color: [u8; 4],
    pub width: f32,
    pub rectangle: bool,
    #[serde(default, skip_serializing_if = "empty")]
    pub nodes: SharedVec<Node>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<[u8; 4]>,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub stroke: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub closed: bool,
    #[serde(default, skip_serializing_if = "empty")]
    pub holes: SharedVec<SharedVec<[f32; 2]>>,
    #[serde(default, skip_serializing_if = "empty")]
    pub width_profile: SharedVec<crate::width::WidthStop>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bucket: bool,
}
fn empty<T>(v: &SharedVec<T>) -> bool {
    v.is_empty()
}
fn yes() -> bool {
    true
}
fn is_true(v: &bool) -> bool {
    *v
}
fn is_false(v: &bool) -> bool {
    !*v
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub position: [f32; 2],
    pub in_handle: [f32; 2],
    pub out_handle: [f32; 2],
}
impl Node {
    pub fn corner(position: [f32; 2]) -> Self {
        Self {
            position,
            in_handle: position,
            out_handle: position,
        }
    }
}
impl Default for Mark {
    fn default() -> Self {
        Self {
            style: Default::default(),
            points: Default::default(),
            color: [89, 72, 220, 255],
            width: 4.,
            rectangle: false,
            nodes: Default::default(),
            fill: None,
            stroke: true,
            closed: false,
            holes: Default::default(),
            width_profile: Default::default(),
            bucket: false,
        }
    }
}
impl Mark {
    pub fn svg_path(&self) -> String {
        let mut d = String::new();
        if let Some(first) = self.nodes.first() {
            d += &format!("M{} {}", first.position[0], first.position[1]);
            for c in crate::vector::cubics(self) {
                let [_, b, c, destination] = c.0;
                d += &format!(
                    " C{} {} {} {} {} {}",
                    b.x, b.y, c.x, c.y, destination.x, destination.y
                );
            }
        } else {
            for (i, p) in crate::vector::flatten(self, 0.1).iter().enumerate() {
                d += &format!("{}{} {}", if i == 0 { "M" } else { " L" }, p.x, p.y);
            }
        }
        if self.closed || self.rectangle {
            d.push_str(" Z");
        }
        for hole in &self.holes {
            for (i, p) in hole.iter().enumerate() {
                d += &format!("{}{} {}", if i == 0 { " M" } else { " L" }, p[0], p[1]);
            }
            d.push_str(" Z");
        }
        d
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Layer {
    #[serde(
        default,
        skip_serializing_if = "crate::features::default_layer_options"
    )]
    pub options: crate::features::LayerOptions,
    pub name: String,
    pub visible: bool,
    #[serde(with = "crate::native_project::cels")]
    pub frames: SharedVec<SharedVec<Mark>>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Document {
    #[serde(default, skip_serializing_if = "crate::features::default_extras")]
    pub extras: crate::features::ProjectExtras,
    pub version: u32,
    pub fps: u32,
    #[serde(default = "default_width", skip_serializing_if = "is_default_width")]
    pub width: u32,
    #[serde(default = "default_height", skip_serializing_if = "is_default_height")]
    pub height: u32,
    pub layers: SharedVec<Layer>,
}
fn default_width() -> u32 {
    960
}
fn default_height() -> u32 {
    540
}
fn is_default_width(v: &u32) -> bool {
    *v == 960
}
fn is_default_height(v: &u32) -> bool {
    *v == 540
}
impl Default for Document {
    fn default() -> Self {
        Self {
            extras: Default::default(),
            version: 1,
            fps: 12,
            width: 960,
            height: 540,
            layers: vec![Layer {
                options: Default::default(),
                name: "Layer 1".into(),
                visible: true,
                frames: vec![SharedVec::default(); 24].into(),
            }]
            .into(),
        }
    }
}
impl Document {
    pub fn validate(&self) -> Result<(), String> {
        if !crate::palette::valid_main_palettes(&self.extras.main_palettes) {
            return Err("Invalid main palette".into());
        }
        if self.extras.palette.len() > 256
            || self
                .extras
                .palette
                .iter()
                .any(|swatch| swatch.name.len() > 256)
        {
            return Err("Invalid linked palette".into());
        }
        if self.extras.rig_poses.len() > 100
            || self.extras.rig_poses.iter().any(|pose| {
                let mut ids = std::collections::HashSet::new();
                pose.name.is_empty()
                    || pose.name.len() > 256
                    || pose.bones.is_empty()
                    || pose.bones.len() > 128
                    || pose.bones.iter().any(|(id, key)| {
                        *id == 0
                            || !ids.insert(*id)
                            || key.frame >= 10000
                            || !key.angle.is_finite()
                            || key.angle.abs() > 1e7
                            || key
                                .translation
                                .iter()
                                .any(|v| !v.is_finite() || v.abs() > 1e7)
                            || !(0.01..=100.).contains(&key.scale)
                            || !(0.1..=10.).contains(&key.stretch)
                            || !key.curve.valid()
                    })
            })
        {
            return Err("Invalid saved rig pose".into());
        }
        crate::features::validate(self)?;
        self.extras.media.validate(&self.extras.scenes)?;
        if self.version != 1
            || !(1..=240).contains(&self.fps)
            || !(1..=16384).contains(&self.width)
            || !(1..=16384).contains(&self.height)
            || self.layers.is_empty()
            || self.layers.len() > 100
        {
            return Err("Formato de proyecto no compatible".into());
        }
        let count = self.layers[0].frames.len();
        if count == 0 || count > 10000 {
            return Err("Número de fotogramas no válido".into());
        }
        let mut checked = std::collections::HashSet::new();
        for layer in &self.layers {
            if layer.frames.len() != count {
                return Err("Capas con duraciones diferentes".into());
            }
            for mark in layer
                .frames
                .iter()
                .filter(|f| checked.insert(f.identity()))
                .flatten()
            {
                if mark
                    .style
                    .symbol_pivot
                    .is_some_and(|pivot| pivot.iter().any(|v| !v.is_finite() || v.abs() > 1e7))
                    || !mark.width.is_finite()
                    || mark.width <= 0.0
                    || mark.width > 100.0
                    || !crate::width::validate(&mark.width_profile)
                    || mark
                        .points
                        .iter()
                        .flatten()
                        .any(|v| !v.is_finite() || v.abs() > 1e7)
                    || mark
                        .nodes
                        .iter()
                        .flat_map(|n| [n.position, n.in_handle, n.out_handle])
                        .flatten()
                        .any(|v| !v.is_finite() || v.abs() > 1e7)
                    || mark
                        .holes
                        .iter()
                        .flat_map(|r| r.iter())
                        .flatten()
                        .any(|v| !v.is_finite() || v.abs() > 1e7)
                {
                    return Err("Trazo no válido".into());
                }
            }
        }
        Ok(())
    }
    pub fn svg(&self, frame: usize) -> String {
        crate::features::svg(self, frame)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_round_trip_and_validation() {
        let mut d = Document::default();
        d.layers[0].frames[0].push(Mark {
            points: vec![[20., 30.]].into(),
            color: [0, 0, 0, 255],
            width: 4.,
            rectangle: false,
            ..Mark::default()
        });
        let restored: Document = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert!(restored.validate().is_ok());
        assert!(restored.svg(0).contains("<circle"));
        d.layers[0].frames.clear();
        assert!(d.validate().is_err());
    }
    #[test]
    fn reject_mismatched_timeline() {
        let mut d = Document::default();
        let mut l = d.layers[0].clone();
        l.frames.pop();
        d.layers.push(l);
        assert!(d.validate().is_err());
    }
}
