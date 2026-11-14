//! Non-destructive scene adjustment layers, applied to the accumulated image below each row.
use crate::{
    Document, Editor,
    design::{self, Icon, button},
    i18n::tr,
};
use eframe::egui::{self, Color32, Pos2, Vec2};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Hue,
    Levels,
    Curves,
    Glow,
    Vignette,
    Harmony,
    Gaussian,
    Lens,
    Sharpen,
    Exposure,
    Posterize,
    Outline,
    InnerShadow,
    DirectionalBlur,
    Grain,
    RgbSplit,
}
impl Kind {
    const ALL: [Self; 16] = [
        Self::Hue,
        Self::Levels,
        Self::Curves,
        Self::Glow,
        Self::Vignette,
        Self::Harmony,
        Self::Gaussian,
        Self::Lens,
        Self::Sharpen,
        Self::Exposure,
        Self::Posterize,
        Self::Outline,
        Self::InnerShadow,
        Self::DirectionalBlur,
        Self::Grain,
        Self::RgbSplit,
    ];
    fn name(self) -> &'static str {
        match self {
            Self::Hue => "Hue / Saturation",
            Self::Levels => "Color levels",
            Self::Curves => "Color curves",
            Self::Glow => "Bloom / Glow",
            Self::Vignette => "Vignette",
            Self::Harmony => "Color integration",
            Self::Gaussian => "Gaussian blur",
            Self::Lens => "Lens blur",
            Self::Sharpen => "Sharpen",
            Self::Exposure => "Exposure / Contrast",
            Self::Posterize => "Posterize",
            Self::Outline => "Outline",
            Self::InnerShadow => "Inner shadow",
            Self::DirectionalBlur => "Directional blur",
            Self::Grain => "Grain / Paper texture",
            Self::RgbSplit => "RGB separation",
        }
    }
    fn icon(self) -> Icon {
        match self {
            Self::Hue => Icon::SwapColors,
            Self::Levels => Icon::Adjust,
            Self::Curves => Icon::Tween,
            Self::Glow => Icon::Wand,
            Self::Vignette => Icon::Ellipse,
            Self::Harmony => Icon::Eyedropper,
            Self::Gaussian => Icon::Blur,
            Self::Lens => Icon::Camera,
            Self::Sharpen => Icon::Focus,
            Self::Exposure => Icon::LightBrush,
            Self::Posterize => Icon::Layers,
            Self::Outline => Icon::Ellipse,
            Self::InnerShadow => Icon::ShadeBrush,
            Self::DirectionalBlur => Icon::Blur,
            Self::Grain => Icon::Layers,
            Self::RgbSplit => Icon::SwapColors,
        }
    }
}

const MAX_CURVE_NODES: usize = 32;

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
struct CurveNode {
    point: [f32; 2],
    incoming: [f32; 2],
    outgoing: [f32; 2],
}

fn empty_curves(curves: &[Vec<CurveNode>; 4]) -> bool {
    curves.iter().all(Vec::is_empty)
}

fn valid_curve(nodes: &[CurveNode]) -> bool {
    nodes.is_empty()
        || ((2..=MAX_CURVE_NODES).contains(&nodes.len())
            && nodes.first().is_some_and(|n| n.point[0] == 0.)
            && nodes.last().is_some_and(|n| n.point[0] == 1.)
            && nodes
                .windows(2)
                .all(|pair| pair[0].point[0] < pair[1].point[0])
            && nodes.iter().all(|n| {
                n.point.iter().all(|v| (0. ..=1.).contains(v))
                    && n.incoming
                        .iter()
                        .chain(&n.outgoing)
                        .all(|v| (-1. ..=1.).contains(v))
            }))
}

fn linear_nodes(values: &[f32]) -> Vec<CurveNode> {
    let step = 1. / (values.len() - 1) as f32;
    values
        .iter()
        .enumerate()
        .map(|(i, &y)| {
            let y = y.clamp(0., 1.);
            CurveNode {
                point: [
                    if i + 1 == values.len() {
                        1.
                    } else {
                        i as f32 * step
                    },
                    y,
                ],
                incoming: [
                    -step / 3.,
                    (values[i.saturating_sub(1)].clamp(0., 1.) - y) / 3.,
                ],
                outgoing: [
                    step / 3.,
                    (values[(i + 1).min(values.len() - 1)].clamp(0., 1.) - y) / 3.,
                ],
            }
        })
        .collect()
}

fn curve_segment(a: CurveNode, b: CurveNode) -> crate::vector::Cubic {
    let [x0, y0] = a.point;
    let [x1, y1] = b.point;
    let mid = (x0 + x1) * 0.5;
    crate::vector::Cubic([
        Pos2::new(x0, y0),
        Pos2::new(
            (x0 + a.outgoing[0]).clamp(x0, mid),
            (y0 + a.outgoing[1]).clamp(0., 1.),
        ),
        Pos2::new(
            (x1 + b.incoming[0]).clamp(mid, x1),
            (y1 + b.incoming[1]).clamp(0., 1.),
        ),
        Pos2::new(x1, y1),
    ])
}

fn evaluate_curve(nodes: &[CurveNode], x: f32) -> f32 {
    let index = nodes
        .partition_point(|n| n.point[0] <= x)
        .clamp(1, nodes.len() - 1);
    let segment = curve_segment(nodes[index - 1], nodes[index]);
    let [a, _, _, b] = segment.0;
    if x <= a.x {
        return a.y;
    }
    if x >= b.x {
        return b.y;
    }
    let (mut low, mut high) = (0., 1.);
    for _ in 0..16 {
        let t = (low + high) * 0.5;
        if segment.at(t).x < x {
            low = t;
        } else {
            high = t;
        }
    }
    segment.at((low + high) * 0.5).y
}

fn blend_curve(
    a: &[CurveNode],
    legacy_a: [f32; 5],
    b: &[CurveNode],
    legacy_b: [f32; 5],
    t: f32,
) -> Vec<CurveNode> {
    if a.is_empty() && b.is_empty() {
        return Vec::new();
    }
    let fallback_a;
    let fallback_b;
    let a = if a.is_empty() {
        fallback_a = linear_nodes(&legacy_a);
        &fallback_a
    } else {
        a
    };
    let b = if b.is_empty() {
        fallback_b = linear_nodes(&legacy_b);
        &fallback_b
    } else {
        b
    };
    let mix = |a, b| egui::lerp(a..=b, t);
    if a.len() == b.len() {
        let pair = |a: [f32; 2], b: [f32; 2]| [mix(a[0], b[0]), mix(a[1], b[1])];
        return a
            .iter()
            .zip(b)
            .map(|(a, b)| CurveNode {
                point: pair(a.point, b.point),
                incoming: pair(a.incoming, b.incoming),
                outgoing: pair(a.outgoing, b.outgoing),
            })
            .collect();
    }
    let values: [f32; MAX_CURVE_NODES] = std::array::from_fn(|i| {
        let x = i as f32 / (MAX_CURVE_NODES - 1) as f32;
        mix(evaluate_curve(a, x), evaluate_curve(b, x))
    });
    linear_nodes(&values)
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Effect {
    kind: Kind,
    enabled: bool,
    values: [f32; 3],
    color: [u8; 3],
    channels: [[f32; 5]; 4],
    #[serde(default, skip_serializing_if = "empty_curves")]
    curves: [Vec<CurveNode>; 4],
    #[serde(skip)]
    grain_seed: u32,
}
impl Effect {
    pub(crate) fn is_outline(&self) -> bool {
        self.kind == Kind::Outline
    }

    pub(crate) fn enabled_outline(&self) -> bool {
        self.enabled && self.is_outline()
    }

    pub(crate) fn new(kind: Kind) -> Self {
        let values = match kind {
            Kind::Glow => [0.68, 18., 0.5],
            Kind::Vignette => [0.55, 0.58, 0.5],
            Kind::Harmony => [0.35, 0.75, 0.],
            Kind::Gaussian | Kind::Lens => [12., 0., 0.],
            Kind::Sharpen => [0.6, 3., 0.],
            Kind::Posterize => [8., 0., 0.],
            Kind::Outline => [4., 1., 0.85],
            Kind::InnerShadow => [5., 135., 0.55],
            Kind::DirectionalBlur => [12., 0., 0.],
            Kind::Grain => [0.14, 2., 0.],
            Kind::RgbSplit => [4., 0., 1.],
            _ => [0.; 3],
        };
        let channels = if kind == Kind::Levels {
            [[0., 1., 1., 0., 1.]; 4]
        } else {
            [[0., 0.25, 0.5, 0.75, 1.]; 4]
        };
        Self {
            kind,
            enabled: true,
            values,
            channels,
            curves: Default::default(),
            grain_seed: 0,
            color: if kind == Kind::Harmony {
                [93, 148, 117]
            } else {
                [0; 3]
            },
        }
    }
}
pub fn valid(effects: &[Effect]) -> bool {
    effects.len() <= 16
        && effects.iter().all(|e| {
            e.values
                .iter()
                .chain(e.channels.iter().flatten())
                .all(|x| x.is_finite() && x.abs() <= 360.)
                && e.curves.iter().all(|nodes| valid_curve(nodes))
        })
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct Key {
    frame: i32,
    effects: crate::shared::SharedVec<Effect>,
}

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Band {
    start: usize,
    end: Option<usize>,
    #[serde(default)]
    keys: crate::shared::SharedVec<Key>,
}

impl Band {
    fn range(&self, count: usize) -> std::ops::Range<usize> {
        self.start.min(count)..self.end.unwrap_or(count).min(count)
    }

    fn set_key(&mut self, frame: usize, effects: crate::shared::SharedVec<Effect>) {
        let frame = frame as i32;
        match self.keys.binary_search_by_key(&frame, |key| key.frame) {
            Ok(index) => self.keys[index].effects = effects,
            Err(index) => self.keys.insert(index, Key { frame, effects }),
        }
    }

    fn evaluate<'a>(&'a self, base: &'a [Effect], frame: usize) -> std::borrow::Cow<'a, [Effect]> {
        let frame = frame as i32;
        let next = self.keys.partition_point(|key| key.frame <= frame);
        match (
            next.checked_sub(1).and_then(|i| self.keys.get(i)),
            self.keys.get(next),
        ) {
            (None, None) => base.into(),
            (None, Some(key)) | (Some(key), None) => key.effects.as_slice().into(),
            (Some(a), Some(b)) => {
                let t = (frame - a.frame) as f32 / (b.frame - a.frame) as f32;
                if t == 0. {
                    return a.effects.as_slice().into();
                }
                let lerp = |a: f32, b: f32| a + (b - a) * t;
                a.effects
                    .iter()
                    .zip(b.effects.iter())
                    .map(|(a, b)| Effect {
                        kind: a.kind,
                        enabled: a.enabled,
                        values: std::array::from_fn(|i| lerp(a.values[i], b.values[i])),
                        color: std::array::from_fn(|i| {
                            lerp(a.color[i] as f32, b.color[i] as f32).round() as u8
                        }),
                        grain_seed: 0,
                        curves: std::array::from_fn(|i| {
                            blend_curve(&a.curves[i], a.channels[i], &b.curves[i], b.channels[i], t)
                        }),
                        channels: std::array::from_fn(|i| {
                            std::array::from_fn(|j| lerp(a.channels[i][j], b.channels[i][j]))
                        }),
                    })
                    .collect::<Vec<_>>()
                    .into()
            }
        }
    }
}

pub(crate) fn valid_bands(layer: &crate::document::Layer) -> bool {
    layer.options.adjustment_bands.as_ref().is_none_or(|bands| {
        let Some(base) = &layer.options.adjustment else {
            return false;
        };
        bands.len() <= 10000
            && bands.iter().enumerate().all(|(i, band)| {
                band.start < 10000
                    && band
                        .end
                        .map_or(i + 1 == bands.len(), |end| end > band.start && end <= 10000)
                    && (i == 0 || bands[i - 1].end.is_some_and(|end| end <= band.start))
                    && band.keys.len() <= 10000
                    && band
                        .keys
                        .windows(2)
                        .all(|pair| pair[0].frame < pair[1].frame)
                    && band.keys.iter().all(|key| {
                        (-1_000_000..=1_000_000).contains(&key.frame)
                            && valid(&key.effects)
                            && key.effects.len() == base.len()
                            && key
                                .effects
                                .iter()
                                .zip(base.iter())
                                .all(|(key, base)| key.kind == base.kind)
                    })
            })
    })
}

pub(crate) fn effects_at(
    layer: &crate::document::Layer,
    frame: usize,
) -> Option<std::borrow::Cow<'_, [Effect]>> {
    let base = layer.options.adjustment.as_ref()?;
    if let Some(bands) = &layer.options.adjustment_bands {
        let band = bands
            .iter()
            .find(|band| band.range(layer.frames.len()).contains(&frame))?;
        Some(band.evaluate(base, frame))
    } else {
        Some(base.as_slice().into())
    }
}

pub(crate) fn visible_effects_at(layer: &crate::document::Layer, frame: usize) -> bool {
    layer.visible
        && (layer.options.light.is_some()
            || effects_at(layer, frame)
                .is_some_and(|effects| effects.iter().any(|effect| effect.enabled)))
}

pub fn is_adjustment(layer: &crate::document::Layer) -> bool {
    layer.options.adjustment.is_some() || layer.options.light.is_some()
}
pub fn present(doc: &Document) -> bool {
    doc.layers
        .iter()
        .any(|layer| layer.options.adjustment.is_some())
}
pub fn visible_effects(layer: &crate::document::Layer) -> bool {
    layer.visible
        && (layer.options.light.is_some()
            || layer
                .options
                .adjustment
                .as_ref()
                .is_some_and(|effects| effects.iter().any(|e| e.enabled))
            || layer
                .options
                .adjustment_bands
                .as_ref()
                .is_some_and(|bands| {
                    bands.iter().any(|band| {
                        band.keys
                            .iter()
                            .any(|key| key.effects.iter().any(|e| e.enabled))
                    })
                }))
}

fn luma(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}
fn hue_saturation(rgb: [f32; 3], hue: f32, saturation: f32, light: f32) -> [f32; 3] {
    let max = rgb.into_iter().fold(0., f32::max);
    let min = rgb.into_iter().fold(1., f32::min);
    let delta = max - min;
    let old_l = (max + min) * 0.5;
    let s = if delta > 0.00001 {
        delta / (1. - (2. * old_l - 1.).abs()).max(0.00001)
    } else {
        0.
    };
    let h = if delta <= 0.00001 {
        0.
    } else if max == rgb[0] {
        (rgb[1] - rgb[2]) / delta
    } else if max == rgb[1] {
        2. + (rgb[2] - rgb[0]) / delta
    } else {
        4. + (rgb[0] - rgb[1]) / delta
    };
    let h = (h + hue / 60.).rem_euclid(6.);
    let s = (s * (1. + saturation)).clamp(0., 1.);
    let l = (old_l + light).clamp(0., 1.);
    let chroma = (1. - (2. * l - 1.).abs()) * s;
    let x = chroma * (1. - (h.rem_euclid(2.) - 1.).abs());
    let rgb = match h as u32 {
        0 => [chroma, x, 0.],
        1 => [x, chroma, 0.],
        2 => [0., chroma, x],
        3 => [0., x, chroma],
        4 => [x, 0., chroma],
        _ => [chroma, 0., x],
    };
    rgb.map(|v| v + l - chroma / 2.)
}
fn channel_value(kind: Kind, v: f32, p: [f32; 5]) -> f32 {
    if kind == Kind::Levels {
        let t = ((v - p[0]) / (p[1] - p[0]).max(0.001))
            .clamp(0., 1.)
            .powf(1. / p[2].clamp(0.1, 5.));
        (p[3] + (p[4] - p[3]) * t).clamp(0., 1.)
    } else {
        let x = v.clamp(0., 1.) * 4.;
        let i = (x as usize).min(3);
        (p[i] + (p[i + 1] - p[i]) * (x - i as f32)).clamp(0., 1.)
    }
}
fn smooth(x: f32) -> f32 {
    let t = x.clamp(0., 1.);
    t * t * (3. - 2. * t)
}

// Retain one adjustment input and its recent prefixes. Comparing exact bytes
// keeps reuse valid across edits, undo, animated layers and preview resolutions.
#[derive(Default)]
pub(crate) struct EffectCache {
    signature: Option<([u32; 2], f32)>,
    input: Vec<u8>,
    effects: Vec<Effect>,
    prefixes: std::collections::VecDeque<(usize, Vec<u8>)>,
}

impl EffectCache {
    pub(crate) fn apply(
        &mut self,
        pixels: &mut Vec<u8>,
        w: u32,
        h: u32,
        effects: &[Effect],
        scale: f32,
    ) {
        const LIMIT: usize = 40 * 1024 * 1024;
        let bytes = pixels.len();
        if bytes == 0
            || bytes > LIMIT / 2
            || !effects.iter().any(|e| {
                e.enabled
                    && matches!(
                        e.kind,
                        Kind::Gaussian | Kind::Lens | Kind::Glow | Kind::Sharpen
                    )
            })
        {
            apply(pixels, w, h, effects, scale);
            return;
        }
        let signature = Some(([w, h], scale));
        if self.signature != signature || self.input != *pixels {
            self.signature = signature;
            self.input.clone_from(pixels);
            self.effects.clear();
            self.prefixes.clear();
        }
        let common = self
            .effects
            .iter()
            .zip(effects)
            .take_while(|(a, b)| a == b)
            .count();
        self.prefixes.retain(|(end, _)| *end <= common);
        let start = self.prefixes.back().map_or(0, |(end, output)| {
            pixels.clone_from(output);
            *end
        });
        for (index, effect) in effects.iter().enumerate().skip(start) {
            apply(pixels, w, h, std::slice::from_ref(effect), scale);
            if effect.enabled {
                let capacity = (LIMIT / bytes).saturating_sub(1).min(4);
                while self.prefixes.len() >= capacity {
                    self.prefixes.pop_front();
                }
                self.prefixes.push_back((index + 1, pixels.clone()));
            }
        }
        self.effects.clear();
        self.effects.extend_from_slice(effects);
    }
}

pub(crate) fn animate_grain(effects: &mut [Effect], frame: usize) {
    for effect in effects
        .iter_mut()
        .filter(|effect| effect.kind == Kind::Grain && effect.values[2] >= 1.)
    {
        effect.grain_seed = (frame / effect.values[2].round().clamp(1., 12.) as usize) as u32;
    }
}

fn apply_new_effect(pixels: &mut Vec<u8>, w: u32, h: u32, effect: &Effect, scale: f32) -> bool {
    let [a, b, c] = effect.values;
    if !matches!(
        effect.kind,
        Kind::Outline | Kind::InnerShadow | Kind::DirectionalBlur | Kind::Grain | Kind::RgbSplit
    ) {
        return false;
    }
    if w == 0 || h == 0 {
        return true;
    }
    if effect.kind == Kind::Grain {
        let cell = (b * scale).max(0.001);
        let hash = |x: u32, y: u32| {
            let mut n = x.wrapping_mul(0x9e3779b9)
                ^ y.wrapping_mul(0x85ebca6b)
                ^ effect.grain_seed.wrapping_mul(0x27d4eb2d);
            n ^= n >> 16;
            n = n.wrapping_mul(0x7feb352d);
            n ^= n >> 15;
            ((n >> 16) as f32 / 65535.) * 2. - 1.
        };
        for (index, pixel) in pixels.chunks_exact_mut(4).enumerate() {
            let alpha = f32::from(pixel[3]);
            let (x, y) = (index as u32 % w, index as u32 / w);
            let noise = hash((x as f32 / cell) as u32, (y as f32 / cell) as u32) * a * alpha * 0.35;
            for value in &mut pixel[..3] {
                *value = (f32::from(*value) + noise).round().clamp(0., alpha) as u8;
            }
        }
        return true;
    }
    let source = pixels.clone();
    let sample = |image: &[u8], x: i32, y: i32, channel: usize| -> f32 {
        if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
            0.
        } else {
            f32::from(image[(y as usize * w as usize + x as usize) * 4 + channel])
        }
    };
    let direction = Vec2::angled(b.to_radians());
    let distance = (a * scale).clamp(0., 128.);
    let softness = if effect.kind == Kind::InnerShadow {
        distance * 0.5
    } else {
        0.
    };
    let blurred = (softness > 0.5).then(|| {
        crate::depth::blur(
            source.clone(),
            w,
            h,
            softness,
            &crate::depth::Optics::default(),
        )
    });
    let matte = blurred.as_deref().unwrap_or(&source);
    let opacity = c.clamp(0., 1.);
    let outline_distance = (effect.kind == Kind::Outline).then(|| {
        let inverse_coverage: Vec<_> = source
            .chunks_exact(4)
            .map(|pixel| 1. - f32::from(pixel[3]) / 255.)
            .collect();
        crate::light::silhouette_distances(&inverse_coverage, w as usize, h as usize)
    });
    let feather = (b * scale).clamp(1., 32.);
    for (index, pixel) in pixels.chunks_exact_mut(4).enumerate() {
        let (x, y) = ((index as u32 % w) as i32, (index as u32 / w) as i32);
        let alpha = f32::from(pixel[3]);
        let at = |image: &[u8], delta: Vec2, channel| {
            sample(
                image,
                x + delta.x.round() as i32,
                y + delta.y.round() as i32,
                channel,
            )
        };
        match effect.kind {
            Kind::DirectionalBlur => {
                for (channel, value) in pixel.iter_mut().enumerate() {
                    let sum: f32 = (-4..=4)
                        .map(|step| at(&source, direction * (distance * step as f32 / 8.), channel))
                        .sum();
                    *value = (sum / 9.).round() as u8;
                }
            }
            Kind::RgbSplit => {
                for (channel, value) in pixel[..3].iter_mut().enumerate() {
                    let delta = direction * (distance * (channel as f32 - 1.));
                    let source_alpha = at(&source, delta, 3);
                    let shifted = if source_alpha > 0. {
                        at(&source, delta, channel) * alpha / source_alpha
                    } else {
                        0.
                    };
                    *value = egui::lerp(f32::from(*value)..=shifted, opacity)
                        .clamp(0., alpha)
                        .round() as u8;
                }
            }
            Kind::InnerShadow => {
                let depth = (1. - at(matte, direction * distance, 3) / 255.) * opacity;
                for (channel, value) in pixel[..3].iter_mut().enumerate() {
                    let tint = f32::from(effect.color[channel]) / 255.;
                    *value = (f32::from(*value) * (1. - depth * (1. - tint))).round() as u8;
                }
            }
            Kind::Outline => {
                let field_index = (y as usize + 1) * (w as usize + 2) + x as usize + 1;
                let edge_distance = outline_distance.as_ref().unwrap()[field_index].sqrt();
                let added = (255. - alpha)
                    * opacity
                    * ((distance + 0.5 - edge_distance) / feather).clamp(0., 1.);
                for (channel, value) in pixel[..3].iter_mut().enumerate() {
                    *value = (f32::from(*value) + added * f32::from(effect.color[channel]) / 255.)
                        .round() as u8;
                }
                pixel[3] = (alpha + added).round() as u8;
            }
            _ => unreachable!(),
        }
    }
    true
}

pub fn apply(pixels: &mut Vec<u8>, w: u32, h: u32, effects: &[Effect], scale: f32) {
    // Stored in application order. The inspector displays the top effect first.
    for effect in effects.iter().filter(|e| e.enabled) {
        let [a, b, c] = effect.values;
        let kind = effect.kind;
        if apply_new_effect(pixels, w, h, effect, scale) {
            continue;
        }
        let blur = |input: Vec<u8>, radius: f32, method| {
            crate::depth::blur(
                input,
                w,
                h,
                (radius * scale).clamp(0., 128.),
                &crate::depth::Optics {
                    method,
                    quality: crate::depth::Quality::High,
                    ..Default::default()
                },
            )
        };
        if matches!(kind, Kind::Gaussian | Kind::Lens) {
            *pixels = blur(
                std::mem::take(pixels),
                a,
                if kind == Kind::Lens {
                    crate::depth::BlurMethod::Lens
                } else {
                    crate::depth::BlurMethod::Gaussian
                },
            );
            continue;
        }
        if kind == Kind::Glow {
            if c <= 0. {
                continue;
            }
            let mut bright = pixels.clone();
            for p in bright.chunks_exact_mut(4) {
                let alpha = f32::from(p[3]).max(1.);
                let strength = smooth(
                    (luma([
                        f32::from(p[0]) / alpha,
                        f32::from(p[1]) / alpha,
                        f32::from(p[2]) / alpha,
                    ]) - a)
                        / (1. - a).max(0.01),
                );
                for v in p {
                    *v = (f32::from(*v) * strength).round() as u8;
                }
            }
            let glow = blur(bright, b, crate::depth::BlurMethod::Gaussian);
            for (p, g) in pixels.chunks_exact_mut(4).zip(glow.chunks_exact(4)) {
                let ga = f32::from(g[3]) * c.clamp(0., 2.);
                let alpha = (f32::from(p[3]) + ga * (1. - f32::from(p[3]) / 255.)).min(255.);
                for i in 0..3 {
                    p[i] = (f32::from(p[i]) + f32::from(g[i]) * c.clamp(0., 2.))
                        .min(alpha)
                        .round() as u8;
                }
                p[3] = alpha.round() as u8;
            }
            continue;
        }
        if kind == Kind::Sharpen && a == 0. {
            continue;
        }
        let exposure = if kind == Kind::Exposure {
            2f32.powf(a)
        } else {
            1.
        };
        let tint_luma = luma(effect.color.map(|v| f32::from(v) / 255.));
        let blurred = (kind == Kind::Sharpen)
            .then(|| blur(pixels.clone(), b, crate::depth::BlurMethod::Gaussian));
        let tint = effect.color.map(|v| f32::from(v) / 255.);
        let curves: [Option<Vec<f32>>; 4] = std::array::from_fn(|channel| {
            let nodes = &effect.curves[channel];
            (kind == Kind::Curves && !nodes.is_empty()).then(|| {
                (0..=1024)
                    .map(|i| evaluate_curve(nodes, i as f32 / 1024.))
                    .collect()
            })
        });
        let mapped = |v: f32, channel: usize| {
            if let Some(lut) = &curves[channel] {
                let x = v.clamp(0., 1.) * 1024.;
                let i = (x as usize).min(1023);
                egui::lerp(lut[i]..=lut[i + 1], x - i as f32)
            } else {
                channel_value(kind, v, effect.channels[channel])
            }
        };
        for (index, p) in pixels.chunks_exact_mut(4).enumerate() {
            let alpha = f32::from(p[3]);
            if alpha == 0. {
                continue;
            }
            let rgb = [
                f32::from(p[0]) / alpha,
                f32::from(p[1]) / alpha,
                f32::from(p[2]) / alpha,
            ];
            let out = match kind {
                Kind::Hue => hue_saturation(rgb, a, b, c),
                Kind::Levels | Kind::Curves => {
                    std::array::from_fn(|i| mapped(mapped(rgb[i], i + 1), 0))
                }
                Kind::Vignette => {
                    let x = 2. * (index as u32 % w) as f32 / w as f32 - 1.;
                    let y = 2. * (index as u32 / w) as f32 / h as f32 - 1.;
                    let edge = x.hypot(y) / std::f32::consts::SQRT_2;
                    let strength = smooth((edge - b) / (c.max(0.01))) * a;
                    std::array::from_fn(|i| rgb[i] + (tint[i] - rgb[i]) * strength)
                }
                Kind::Harmony => {
                    let y = luma(rgb);
                    std::array::from_fn(|i| {
                        let target = (tint[i] + (y - tint_luma) * b).clamp(0., 1.);
                        rgb[i] + (target - rgb[i]) * a
                    })
                }
                Kind::Sharpen => {
                    let low = &blurred.as_ref().unwrap()[index * 4..index * 4 + 4];
                    let low_alpha = f32::from(low[3]).max(1.);
                    std::array::from_fn(|i| rgb[i] + a * (rgb[i] - f32::from(low[i]) / low_alpha))
                }
                Kind::Exposure => rgb.map(|v| (v * exposure - 0.5) * (1. + b) + 0.5),
                Kind::Posterize => {
                    let steps = a.round().clamp(2., 32.) - 1.;
                    rgb.map(|v| (v * steps).round() / steps)
                }
                _ => rgb,
            };
            for i in 0..3 {
                p[i] = (out[i].clamp(0., 1.) * alpha).round() as u8;
            }
        }
    }
}

fn slider(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(18.), egui::Sense::hover());
        design::draw(ui.painter(), rect, icon, crate::themes::muted());
        ui.add(egui::Slider::new(value, range).text(tr(label)));
    });
}
fn curve(ui: &mut egui::Ui, values: &[f32; 5], stored: &mut Vec<CurveNode>, color: Color32) {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width().min(310.), 160.),
        egui::Sense::click_and_drag(),
    );
    let graph = rect.shrink(8.);
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 4., crate::themes::palette().canvas);
    let grid = egui::Stroke::new(1., crate::themes::line());
    for i in 0..5 {
        let t = i as f32 / 4.;
        painter.vline(egui::lerp(graph.x_range(), t), graph.y_range(), grid);
        painter.hline(graph.x_range(), egui::lerp(graph.y_range(), t), grid);
    }
    let screen = |p: Pos2| {
        Pos2::new(
            egui::lerp(graph.x_range(), p.x),
            egui::lerp(graph.y_range(), 1. - p.y),
        )
    };
    let input = |p: Pos2| {
        [
            (p.x - graph.left()) / graph.width(),
            (graph.bottom() - p.y) / graph.height(),
        ]
    };
    let mut nodes = if stored.is_empty() {
        linear_nodes(values)
    } else {
        stored.clone()
    };
    let state = response.id.with("curve-control");
    let (mut selected, mut handle) = ui
        .data(|data| data.get_temp::<(usize, i8)>(state))
        .unwrap_or((usize::MAX, 0));
    let mut changed = false;
    let point = |node: CurveNode| Pos2::new(node.point[0], node.point[1]);
    let nearest = |nodes: &[CurveNode], pos: Pos2| {
        nodes
            .iter()
            .position(|node| screen(point(*node)).distance(pos) <= 6.)
    };
    let grab = if response.drag_started() {
        ui.input(|input| input.pointer.press_origin())
    } else if response.clicked() {
        response.interact_pointer_pos()
    } else {
        None
    };
    if let Some(pos) = grab {
        response.request_focus();
        handle = 0;
        if let Some(index) = nearest(&nodes, pos) {
            selected = index;
        } else if selected > 0
            && selected < nodes.len()
            && screen(curve_segment(nodes[selected - 1], nodes[selected]).0[2]).distance(pos) <= 7.
        {
            handle = -1;
        } else if selected < nodes.len() - 1
            && screen(curve_segment(nodes[selected], nodes[selected + 1]).0[1]).distance(pos) <= 7.
        {
            handle = 1;
        } else if graph.contains(pos) && nodes.len() < MAX_CURVE_NODES {
            let [x, y] = input(pos);
            let index = nodes.partition_point(|node| node.point[0] < x);
            if index > 0
                && index < nodes.len()
                && x - nodes[index - 1].point[0] >= 0.004
                && nodes[index].point[0] - x >= 0.004
            {
                nodes.insert(
                    index,
                    CurveNode {
                        point: [x, y],
                        incoming: [(nodes[index - 1].point[0] - x) / 3., 0.],
                        outgoing: [(nodes[index].point[0] - x) / 3., 0.],
                    },
                );
                selected = index;
                changed = true;
            }
        }
    }
    if response.dragged()
        && selected < nodes.len()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let [x, y] = input(pos);
        let previous = selected.checked_sub(1).map(|i| nodes[i].point[0]);
        let next = nodes.get(selected + 1).map(|n| n.point[0]);
        let node = &mut nodes[selected];
        let y = y.clamp(0., 1.);
        match handle {
            -1 if previous.is_some() => {
                let offset =
                    (x - node.point[0]).clamp((previous.unwrap() - node.point[0]) * 0.5, 0.);
                node.incoming = [offset, y - node.point[1]];
            }
            1 if next.is_some() => {
                let offset = (x - node.point[0]).clamp(0., (next.unwrap() - node.point[0]) * 0.5);
                node.outgoing = [offset, y - node.point[1]];
            }
            _ => {
                if let (Some(left), Some(right)) = (previous, next) {
                    let margin = 0.001f32.min((right - left) / 4.);
                    node.point[0] = x.clamp(left + margin, right - margin);
                }
                node.point[1] = y;
            }
        }
        changed = true;
    }
    let remove = if response.secondary_clicked() {
        response
            .interact_pointer_pos()
            .and_then(|pos| nearest(&nodes, pos))
    } else if response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Delete)) {
        Some(selected)
    } else {
        None
    };
    if let Some(index) = remove.filter(|i| *i > 0 && *i < nodes.len() - 1) {
        nodes.remove(index);
        selected = usize::MAX;
        changed = true;
    }
    let mut path = vec![screen(point(nodes[0]))];
    for pair in nodes.windows(2) {
        let segment = curve_segment(pair[0], pair[1]);
        path.extend((1..=24).map(|i| screen(segment.at(i as f32 / 24.))));
    }
    painter.add(egui::Shape::line(path, egui::Stroke::new(2., color)));
    for (i, node) in nodes.iter().enumerate() {
        let p = screen(point(*node));
        if i == selected {
            for control in [
                i.checked_sub(1)
                    .map(|prev| curve_segment(nodes[prev], *node).0[2]),
                nodes
                    .get(i + 1)
                    .map(|next| curve_segment(*node, *next).0[1]),
            ]
            .into_iter()
            .flatten()
            {
                let control = screen(control);
                painter.line_segment([p, control], egui::Stroke::new(1., color));
                painter.circle_filled(control, 3., color);
            }
            painter.circle_stroke(p, 5., egui::Stroke::new(1., color));
        }
        painter.circle_filled(p, 3., color);
    }
    ui.data_mut(|data| data.insert_temp(state, (selected, handle)));
    if changed {
        *stored = nodes;
    }
    ui.small(tr("Click to add a point. Drag its handles to shape the curve. Right-click a point to remove it."));
}

fn settings(ui: &mut egui::Ui, effect: &mut Effect, channel: &mut usize) {
    let [a, b, c] = &mut effect.values;
    match effect.kind {
        Kind::Hue => {
            slider(ui, Icon::Rotate, "Hue", a, -180. ..=180.);
            slider(ui, Icon::SwapColors, "Saturation", b, -1. ..=1.);
            slider(ui, Icon::LightBrush, "Lightness", c, -1. ..=1.);
        }
        Kind::Levels | Kind::Curves => {
            ui.horizontal_wrapped(|ui| {
                for (i, name) in ["Master", "Red", "Green", "Blue"].into_iter().enumerate() {
                    ui.selectable_value(channel, i, tr(name));
                }
            });
            let p = &mut effect.channels[*channel];
            if effect.kind == Kind::Curves {
                curve(
                    ui,
                    p,
                    &mut effect.curves[*channel],
                    [
                        crate::themes::accent(),
                        Color32::LIGHT_RED,
                        Color32::LIGHT_GREEN,
                        Color32::LIGHT_BLUE,
                    ][*channel],
                );
            } else {
                for (value, name, range) in [
                    (0, "Input black", 0. ..=0.99),
                    (1, "Input white", 0.01..=1.),
                    (2, "Gamma", 0.1..=5.),
                    (3, "Output black", 0. ..=1.),
                    (4, "Output white", 0. ..=1.),
                ] {
                    slider(ui, Icon::Adjust, name, &mut p[value], range);
                }
                p[1] = p[1].max(p[0] + 0.01).min(1.);
            }
        }
        Kind::Glow => {
            slider(ui, Icon::LightBrush, "Brightness threshold", a, 0. ..=1.);
            slider(ui, Icon::Blur, "Radius", b, 1. ..=100.);
            slider(ui, Icon::Wand, "Intensity", c, 0. ..=2.);
        }
        Kind::Vignette => {
            slider(ui, Icon::Ellipse, "Intensity", a, 0. ..=1.);
            slider(ui, Icon::Focus, "Start radius", b, 0. ..=1.);
            slider(ui, Icon::Blur, "Softness", c, 0.01..=0.9);
            ui.color_edit_button_srgb(&mut effect.color)
                .on_hover_text(tr("Vignette color"));
        }
        Kind::Harmony => {
            ui.color_edit_button_srgb(&mut effect.color)
                .on_hover_text(tr("Dominant color"));
            slider(ui, Icon::Eyedropper, "Tint amount", a, 0. ..=1.);
            slider(ui, Icon::LightBrush, "Preserve lighting", b, 0. ..=1.);
        }
        Kind::Gaussian | Kind::Lens => slider(ui, effect.kind.icon(), "Radius", a, 0. ..=100.),
        Kind::Sharpen => {
            slider(ui, Icon::Focus, "Intensity", a, 0. ..=2.);
            slider(ui, Icon::Blur, "Radius", b, 0.5..=12.);
        }
        Kind::Exposure => {
            slider(ui, Icon::LightBrush, "Exposure", a, -3. ..=3.);
            slider(ui, Icon::Adjust, "Contrast", b, -1. ..=1.);
        }
        Kind::Posterize => slider(ui, Icon::Layers, "Color steps", a, 2. ..=32.),
        Kind::Outline | Kind::InnerShadow => {
            if effect.kind == Kind::Outline {
                ui.small(tr(
                    "Outline follows the nearest artwork below, or the chosen effect target.",
                ));
            }
            slider(ui, Icon::Width, "Distance", a, 1. ..=100.);
            if effect.kind == Kind::Outline {
                slider(ui, Icon::Blur, "Edge softness", b, 0. ..=20.);
            } else {
                slider(ui, Icon::Rotate, "Direction", b, -180. ..=180.);
            }
            slider(ui, Icon::Adjust, "Opacity", c, 0. ..=1.);
            ui.color_edit_button_srgb(&mut effect.color);
        }
        Kind::DirectionalBlur | Kind::RgbSplit => {
            slider(ui, Icon::Width, "Distance", a, 0. ..=100.);
            slider(ui, Icon::Rotate, "Direction", b, -180. ..=180.);
            if effect.kind == Kind::RgbSplit {
                slider(ui, Icon::Adjust, "Opacity", c, 0. ..=1.);
            }
        }
        Kind::Grain => {
            slider(ui, Icon::Adjust, "Intensity", a, 0. ..=1.);
            slider(ui, Icon::Width, "Grain size", b, 1. ..=12.);
            slider(
                ui,
                Icon::Film,
                "Grain frame interval (0 = fixed)",
                c,
                0. ..=12.,
            );
        }
    }
}
#[derive(Clone, Copy)]
struct EffectDrag(usize);

#[derive(Clone, Copy)]
enum BandHandle {
    Start,
    End,
    Move,
    Key(usize),
}

#[derive(Clone, Copy)]
struct BandDrag {
    layer: usize,
    band: usize,
    x: f32,
    handle: BandHandle,
}

fn bands_for(layer: &crate::document::Layer) -> crate::shared::SharedVec<Band> {
    layer
        .options
        .adjustment_bands
        .clone()
        .unwrap_or_else(|| vec![Band::default()].into())
}

#[derive(Clone, Copy)]
enum BandAction {
    Split,
    Remove,
    AddKey,
    RemoveKey,
}

#[derive(Clone)]
enum StackEdit {
    Add(Effect),
    Remove(usize),
    Move(usize, usize),
}

impl StackEdit {
    fn apply(&self, effects: &mut crate::shared::SharedVec<Effect>) {
        match self {
            Self::Add(effect) => effects.push(effect.clone()),
            Self::Remove(index) => {
                effects.remove(*index);
            }
            Self::Move(from, to) => {
                let effect = effects.remove(*from);
                effects.insert(*to, effect);
            }
        }
    }
}

#[derive(Default)]
pub(crate) struct State {
    pub hidden: bool,
    pub hide_lights: bool,
    pub solo_light: bool,
    pub preset_name: String,
    selected: usize,
    channel: usize,
    transaction: bool,
    band_drag: Option<BandDrag>,
}
impl Editor {
    pub(crate) fn cancel_adjustment_drag(&mut self) {
        self.post.band_drag = None;
        self.post.transaction = false;
    }

    fn edit_adjustment_band(&mut self, layer: usize, frame: usize, action: BandAction) {
        let data = &self.doc.layers[layer];
        if data.options.locked {
            return;
        }
        let Some(base) = &data.options.adjustment else {
            return;
        };
        let mut bands = bands_for(data);
        let Some(index) = bands
            .iter()
            .position(|band| band.range(data.frames.len()).contains(&frame))
        else {
            return;
        };
        match action {
            BandAction::Split if frame > bands[index].start => {
                let mut right = bands[index].clone();
                right.start = frame;
                let first_at_or_after = right.keys.partition_point(|key| key.frame < frame as i32);
                // Keep the neighbouring keys on each side for a continuous split.
                right.keys.drain(..first_at_or_after.saturating_sub(1));
                bands[index].keys.truncate(first_at_or_after + 1);
                bands[index].end = Some(frame);
                bands.insert(index + 1, right);
            }
            BandAction::Remove => {
                bands.remove(index);
            }
            BandAction::AddKey => {
                let value = bands[index].evaluate(base, frame).into_owned().into();
                bands[index].set_key(frame, value);
            }
            BandAction::RemoveKey => {
                let Ok(key) = bands[index]
                    .keys
                    .binary_search_by_key(&(frame as i32), |key| key.frame)
                else {
                    return;
                };
                bands[index].keys.remove(key);
            }
            _ => return,
        }
        self.checkpoint();
        self.doc.layers[layer].options.adjustment_bands = Some(bands);
        self.recover();
    }

    pub(crate) fn adjustment_track_ui(
        &mut self,
        ui: &mut egui::Ui,
        layer: usize,
        rect: egui::Rect,
        cell: f32,
        count: usize,
    ) {
        let data = &self.doc.layers[layer];
        let locked = data.options.locked;
        let mut bands = bands_for(data);
        let response = ui.interact(
            rect,
            ui.scope_id().with(("fx-band", layer)),
            egui::Sense::click_and_drag(),
        );
        let painter = ui.painter().with_clip_rect(ui.clip_rect());
        let x = |frame: usize| rect.left() + frame as f32 * cell;
        let frame_at = |pos: Pos2| (((pos.x - rect.left()) / cell).floor() as usize).min(count - 1);
        let hit = |pos: Pos2| {
            let frame = frame_at(pos);
            let (index, band) = bands
                .iter()
                .enumerate()
                .find(|(_, band)| band.range(count).contains(&frame))?;
            let range = band.range(count);
            let grip = (cell * 0.28).clamp(2., 6.);
            let handle = if pos.x - x(range.start) <= grip {
                BandHandle::Start
            } else if x(range.end) - pos.x <= grip {
                BandHandle::End
            } else if band
                .keys
                .binary_search_by_key(&(frame as i32), |key| key.frame)
                .is_ok()
                && (pos.x - x(frame) - cell * 0.5).abs() <= 6.
            {
                BandHandle::Key(frame)
            } else {
                BandHandle::Move
            };
            Some((index, handle))
        };
        if !locked
            && let Some(pos) = response.hover_pos()
            && let Some((_, handle)) = hit(pos)
        {
            ui.ctx().set_cursor_icon(match handle {
                BandHandle::Start | BandHandle::End => egui::CursorIcon::ResizeHorizontal,
                BandHandle::Move | BandHandle::Key(_) => egui::CursorIcon::Grab,
            });
        }
        let pointer = ui.input(|input| input.pointer.interact_pos());
        if (response.clicked() || response.secondary_clicked())
            && let Some(pos) = pointer
        {
            self.layer = layer;
            self.frame = frame_at(pos);
            self.playing = false;
        }
        let mut action = None;
        response.context_menu(|ui| {
            let frame = self.frame;
            let band = bands.iter().find(|band| band.range(count).contains(&frame));
            let keyed = band.is_some_and(|band| {
                band.keys
                    .binary_search_by_key(&(frame as i32), |key| key.frame)
                    .is_ok()
            });
            for (icon, label, command, enabled) in [
                (
                    Icon::Cut,
                    "Split FX band here",
                    BandAction::Split,
                    band.is_some_and(|band| frame > band.start),
                ),
                (
                    Icon::KeyAdd,
                    "Add FX keyframe",
                    BandAction::AddKey,
                    band.is_some() && !keyed,
                ),
                (
                    Icon::KeyRemove,
                    "Delete FX keyframe",
                    BandAction::RemoveKey,
                    keyed,
                ),
                (
                    Icon::Trash,
                    "Delete FX band",
                    BandAction::Remove,
                    band.is_some() && bands.len() > 1,
                ),
            ] {
                if button(ui, icon, label, "", false, !locked && enabled).clicked() {
                    action = Some((frame, command));
                    ui.close();
                }
            }
        });
        if !locked
            && response.drag_started_by(egui::PointerButton::Primary)
            && let Some(pos) = ui.input(|input| input.pointer.press_origin())
            && let Some((band, handle)) = hit(pos)
        {
            self.layer = layer;
            self.playing = false;
            self.post.band_drag = Some(BandDrag {
                layer,
                band,
                x: pos.x,
                handle,
            });
        }
        let mut changed = false;
        if !locked
            && let Some(drag) = self.post.band_drag.filter(|drag| drag.layer == layer)
            && let Some(pos) = pointer
            && drag.band < bands.len()
        {
            let delta = ((pos.x - drag.x) / cell).round() as i32;
            let low = drag
                .band
                .checked_sub(1)
                .and_then(|i| bands[i].end)
                .unwrap_or(0);
            let high = bands
                .get(drag.band + 1)
                .map_or(count, |next| next.start.min(count));
            let band = &mut bands[drag.band];
            let range = band.range(count);
            if !range.is_empty() && delta != 0 {
                let offset = |frame: usize| frame.saturating_add_signed(delta as isize);
                match drag.handle {
                    BandHandle::Start => band.start = offset(range.start).clamp(low, range.end - 1),
                    BandHandle::End => {
                        let end = offset(range.end).clamp(range.start + 1, high);
                        if end != range.end {
                            band.end = Some(end);
                        }
                    }
                    BandHandle::Move => {
                        let start = offset(range.start).clamp(low, high - range.len());
                        let shift = start as i32 - range.start as i32;
                        if shift != 0 {
                            band.start = start;
                            band.end = Some(start + range.len());
                            for key in &mut band.keys {
                                key.frame += shift;
                            }
                        }
                    }
                    BandHandle::Key(frame) => {
                        let to = offset(frame).clamp(range.start, range.end - 1) as i32;
                        if band
                            .keys
                            .binary_search_by_key(&to, |key| key.frame)
                            .is_err()
                            && let Ok(index) = band
                                .keys
                                .binary_search_by_key(&(frame as i32), |key| key.frame)
                        {
                            band.keys[index].frame = to;
                            band.keys.sort_by_key(|key| key.frame);
                            if response.drag_stopped() {
                                self.frame = to as usize;
                            }
                        }
                    }
                }
            }
            changed = bands != bands_for(&self.doc.layers[layer]);
            if matches!(drag.handle, BandHandle::Move | BandHandle::Key(_)) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
        }
        let color = self
            .studio
            .prefs
            .layer_color(crate::studio::LayerColor::Adjustment);
        let opacity = if layer == self.layer { 0.42 } else { 0.24 };
        for band in &bands {
            let range = band.range(count);
            if range.is_empty() {
                continue;
            }
            let band_rect = egui::Rect::from_min_max(
                Pos2::new(x(range.start) + 1., rect.top() + 4.),
                Pos2::new(x(range.end) - 1., rect.bottom() - 4.),
            );
            painter.rect_filled(band_rect, 4., color.gamma_multiply(opacity));
            for key in &band.keys {
                if key.frame < 0 || !range.contains(&(key.frame as usize)) {
                    continue;
                }
                let center = Pos2::new(x(key.frame as usize) + cell / 2., band_rect.center().y);
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        center - Vec2::Y * 5.,
                        center + Vec2::X * 5.,
                        center + Vec2::Y * 5.,
                        center - Vec2::X * 5.,
                    ],
                    color,
                    egui::Stroke::NONE,
                ));
            }
        }
        if response.drag_stopped_by(egui::PointerButton::Primary) {
            self.post.band_drag = None;
            if changed {
                self.checkpoint();
                self.doc.layers[layer].options.adjustment_bands = Some(bands);
                self.recover();
            }
        }
        if let Some((frame, action)) = action {
            self.edit_adjustment_band(layer, frame, action);
        }
    }

    pub(crate) fn adjustment_active(&self) -> bool {
        self.doc.layers.get(self.layer).is_some_and(is_adjustment)
    }

    pub(crate) fn adjustment_properties(&mut self, ui: &mut egui::Ui) {
        let layer = self.layer;
        let frame = self.frame;
        let data = &self.doc.layers[layer];
        let Some(mut base) = data.options.adjustment.clone() else {
            return;
        };
        let locked = data.options.locked;
        let mut stored_bands = data.options.adjustment_bands.clone();
        let active = bands_for(data)
            .iter()
            .find(|band| band.range(data.frames.len()).contains(&frame))
            .cloned();
        let keyed = active.as_ref().is_some_and(|band| {
            band.keys
                .binary_search_by_key(&(frame as i32), |key| key.frame)
                .is_ok()
        });
        let animated = active.as_ref().is_some_and(|band| !band.keys.is_empty());
        let before: crate::shared::SharedVec<Effect> = if animated {
            active
                .as_ref()
                .unwrap()
                .evaluate(&base, frame)
                .into_owned()
                .into()
        } else {
            base.clone()
        };
        let mut effects = before.clone();
        let mut stack_edits = Vec::new();
        ui.heading(tr("Adjustment layer"));
        let mut target = data.options.fx_target;
        let mut target_layer = None;
        ui.add_enabled_ui(!locked, |ui| {
            ui.menu_button(tr("Effect target"), |ui| {
                if ui
                    .selectable_label(target.is_none(), tr("All artwork below"))
                    .clicked()
                {
                    target = None;
                    ui.close();
                }
                for (index, candidate) in self.doc.layers.iter().enumerate().take(layer).rev() {
                    if candidate.options.adjustment.is_some()
                        || candidate.options.light.is_some()
                        || candidate.options.height_guide.is_some()
                        || candidate.options.light_mask
                    {
                        continue;
                    }
                    let selected = candidate
                        .options
                        .light_receiver
                        .is_some_and(|id| target == Some(crate::light::Receiver::Layer(id)));
                    if ui.selectable_label(selected, &candidate.name).clicked() {
                        target_layer = Some(index);
                        ui.close();
                    }
                }
                for folder in &self.doc.extras.folders {
                    if ui
                        .selectable_label(
                            target == Some(crate::light::Receiver::Folder(folder.id)),
                            &folder.name,
                        )
                        .clicked()
                    {
                        target = Some(crate::light::Receiver::Folder(folder.id));
                        ui.close();
                    }
                }
            });
        });
        if target.is_some() {
            ui.small(tr("Effects apply to the selected layer or folder below."));
        } else {
            ui.small(tr("Effects apply to all visible layers below this layer."));
        }
        ui.small(tr("Effects run from bottom to top. Drag to reorder."));
        ui.horizontal(|ui| {
            for (icon, label, action, enabled) in [
                (
                    Icon::KeyAdd,
                    "Add FX keyframe",
                    BandAction::AddKey,
                    active.is_some() && !keyed,
                ),
                (
                    Icon::KeyRemove,
                    "Delete FX keyframe",
                    BandAction::RemoveKey,
                    keyed,
                ),
                (
                    Icon::Cut,
                    "Split FX band here",
                    BandAction::Split,
                    active.as_ref().is_some_and(|band| frame > band.start),
                ),
            ] {
                if button(ui, icon, "", label, false, !locked && enabled).clicked() {
                    self.edit_adjustment_band(layer, frame, action);
                }
            }
            if keyed {
                ui.small(tr("FX keyframe"));
            }
            if active.is_none() {
                ui.small(tr("Outside FX band"));
            }
        });
        ui.separator();
        ui.add_enabled_ui(!locked, |ui| {
            ui.horizontal(|ui| {
                let add = button(ui, Icon::Plus, "", "Add effect", false, effects.len() < 16);
                egui::Popup::menu(&add).show(|ui| {
                    for kind in Kind::ALL {
                        if button(ui, kind.icon(), kind.name(), "", false, effects.len() < 16)
                            .clicked()
                        {
                            let effect = Effect::new(kind);
                            effects.push(effect.clone());
                            stack_edits.push(StackEdit::Add(effect));
                            self.post.selected = effects.len() - 1;
                            self.post.channel = 0;
                            ui.close();
                        }
                    }
                });
                if button(
                    ui,
                    Icon::Minus,
                    "",
                    "Remove selected effect",
                    false,
                    !effects.is_empty(),
                )
                .clicked()
                {
                    let selected = self.post.selected.min(effects.len() - 1);
                    effects.remove(selected);
                    stack_edits.push(StackEdit::Remove(selected));
                    self.post.selected = self.post.selected.saturating_sub(1);
                }
            });
            let mut reorder = None;
            for (i, effect) in effects.iter_mut().enumerate().rev() {
                let row = ui
                    .horizontal(|ui| {
                        if button(
                            ui,
                            if effect.enabled {
                                Icon::Eye
                            } else {
                                Icon::EyeOff
                            },
                            "",
                            "Show / Hide effect",
                            effect.enabled,
                            true,
                        )
                        .clicked()
                        {
                            effect.enabled = !effect.enabled;
                        }
                        let selected = i == self.post.selected;
                        let response = button(
                            ui,
                            effect.kind.icon(),
                            effect.kind.name(),
                            "",
                            selected,
                            true,
                        );
                        let drag = response
                            .interact(egui::Sense::drag())
                            .on_hover_cursor(egui::CursorIcon::Grab);
                        if drag.clicked() || drag.drag_started() {
                            self.post.selected = i;
                            self.post.channel = 0;
                        }
                        if drag.drag_started() {
                            drag.dnd_set_drag_payload(EffectDrag(i));
                        }
                    })
                    .response;
                if !locked
                    && let Some(source) = row.dnd_hover_payload::<EffectDrag>()
                    && let Some(pos) = ui.ctx().pointer_interact_pos()
                {
                    let before = pos.y < row.rect.center().y;
                    let y = if before {
                        row.rect.top()
                    } else {
                        row.rect.bottom()
                    };
                    ui.painter().hline(
                        row.rect.x_range(),
                        y,
                        egui::Stroke::new(2., crate::themes::accent()),
                    );
                    if row.dnd_release_payload::<EffectDrag>().is_some() {
                        reorder = Some((source.0, i + usize::from(before)));
                    }
                }
            }
            if let Some((from, slot)) = reorder
                && from < effects.len()
            {
                let to = slot - usize::from(from < slot);
                if from != to {
                    let effect = effects.remove(from);
                    effects.insert(to, effect);
                    stack_edits.push(StackEdit::Move(from, to));
                }
                self.post.selected = to;
            }
            let len = effects.len();
            if let Some(effect) = effects.get_mut(self.post.selected.min(len.saturating_sub(1))) {
                ui.separator();
                settings(ui, effect, &mut self.post.channel);
                if button(ui, Icon::ResetColors, "Reset effect", "", false, true).clicked() {
                    *effect = Effect {
                        enabled: effect.enabled,
                        ..Effect::new(effect.kind)
                    };
                }
            }
        });
        if target_layer.is_some() || target != self.doc.layers[layer].options.fx_target {
            self.checkpoint();
            if let Some(index) = target_layer {
                let id = *self.doc.layers[index]
                    .options
                    .light_receiver
                    .get_or_insert_with(crate::features::id);
                target = Some(crate::light::Receiver::Layer(id));
            }
            self.doc.layers[layer].options.fx_target = target;
            self.recover();
        }
        if effects != before || !stack_edits.is_empty() {
            if !self.post.transaction {
                self.checkpoint();
                self.post.transaction = true;
            }
            let mut reordered = before;
            for edit in &stack_edits {
                edit.apply(&mut base);
                edit.apply(&mut reordered);
                if let Some(bands) = &mut stored_bands {
                    for band in bands {
                        for key in &mut band.keys {
                            edit.apply(&mut key.effects);
                        }
                    }
                }
            }
            if animated {
                if effects != reordered
                    && let Some(band) = stored_bands.as_mut().and_then(|bands| {
                        bands.iter_mut().find(|band| {
                            band.range(self.doc.layers[layer].frames.len())
                                .contains(&frame)
                        })
                    })
                {
                    band.set_key(frame, effects);
                }
            } else {
                base = effects;
            }
            self.doc.layers[layer].options.adjustment = Some(base);
            self.doc.layers[layer].options.adjustment_bands = stored_bands;
            self.recover();
        }
        if !ui.input(|i| i.pointer.primary_down()) {
            self.post.transaction = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outline_targets_artwork_over_an_opaque_background() {
        let mut doc = Document {
            width: 64,
            height: 64,
            ..Default::default()
        };
        let mut art = doc.layers[0].clone();
        let mut adjustment = art.clone();
        let square = |a, b, fill| crate::Mark {
            points: vec![[a, a], [b, b]].into(),
            rectangle: true,
            closed: true,
            stroke: false,
            fill: Some(fill),
            ..Default::default()
        };
        doc.layers[0].frames[0].push(square(0., 64., [255, 255, 255, 255]));
        art.frames[0].push(square(20., 44., [220, 30, 30, 255]));
        let mut edge = Effect::new(Kind::Outline);
        edge.values = [5., 0., 1.];
        adjustment.options.adjustment = Some(vec![edge].into());
        doc.layers.push(art);
        doc.layers.push(adjustment);
        let rgb = crate::exporter::raster(
            &doc,
            0,
            64,
            64,
            &egui::Context::default(),
            &mut Default::default(),
        );
        let pixel = |x: usize| &rgb[(32 * 64 + x) * 3..][..3];
        assert_eq!(pixel(17), &[0, 0, 0]);
        assert_eq!(pixel(32), &[220, 30, 30]);
        assert_eq!(pixel(10), &[255, 255, 255]);
    }

    #[test]
    fn outline_extends_alpha_and_inner_shadow_stays_inside() {
        let mut source = vec![0; 21 * 21 * 4];
        for y in 7..14 {
            for x in 7..14 {
                source[(y * 21 + x) * 4..][..4].copy_from_slice(&[180, 150, 120, 255]);
            }
        }
        let at = |x: usize, y: usize| (y * 21 + x) * 4;
        let mut outline = source.clone();
        let mut edge = Effect::new(Kind::Outline);
        edge.values = [2., 0., 1.];
        apply(&mut outline, 21, 21, &[edge], 1.);
        assert_eq!(outline[at(6, 10) + 3], 255);
        assert_eq!(&outline[at(10, 10)..][..4], &source[at(10, 10)..][..4]);
        let mut shadow = source.clone();
        apply(&mut shadow, 21, 21, &[Effect::new(Kind::InnerShadow)], 1.);
        assert_eq!(shadow[at(6, 10) + 3], 0);
        assert_eq!(shadow[at(7, 10) + 3], 255);
        assert!(shadow[at(7, 10)] < source[at(7, 10)]);
    }

    #[test]
    fn bezier_curves_preserve_legacy_points_and_apply_handles_to_pixels() {
        let legacy = [0., 0.12, 0.5, 0.93, 1.];
        let linear = linear_nodes(&legacy);
        for i in 0..=100 {
            let x = i as f32 / 100.;
            assert!(
                (evaluate_curve(&linear, x) - channel_value(Kind::Curves, x, legacy)).abs()
                    < 0.00003
            );
        }
        let mut effect = Effect::new(Kind::Curves);
        effect.channels[0] = legacy;
        let before = serde_json::to_value(&effect).unwrap();
        assert!(before.get("curves").is_none());
        let previous: Effect = serde_json::from_value(before).unwrap();
        assert!(previous.curves[0].is_empty());
        effect.curves[0] = linear;
        effect.curves[0][0].outgoing = [0.1, 0.5];
        assert!(valid(std::slice::from_ref(&effect)));
        let restored: Effect =
            serde_json::from_value(serde_json::to_value(&effect).unwrap()).unwrap();
        assert!(effect == restored);
        let mut old = vec![32, 32, 32, 255];
        let mut new = old.clone();
        apply(&mut old, 1, 1, &[previous], 1.);
        apply(&mut new, 1, 1, &[restored], 1.);
        assert!(new[0] > old[0] + 20);
        assert_eq!(new[3], old[3]);
    }

    #[test]
    fn fx_band_limits_rendering_and_interpolates_across_a_split() {
        let ctx = egui::Context::default();
        let mut editor = Editor::new(&eframe::CreationContext::_new_kittest(ctx.clone()));
        editor.doc = Document {
            width: 32,
            height: 32,
            ..Default::default()
        };
        editor.recovery =
            std::env::temp_dir().join(format!("fx-band-test-{}.json", std::process::id()));
        let mut adjustment = editor.doc.layers[0].clone();
        let mut first = Effect::new(Kind::Hue);
        first.values[0] = 0.;
        let mut last = first.clone();
        last.values[0] = 120.;
        let mut band = Band {
            start: 2,
            end: Some(9),
            ..Default::default()
        };
        band.set_key(2, vec![first.clone()].into());
        band.set_key(8, vec![last].into());
        adjustment.options.adjustment = Some(vec![first].into());
        adjustment.options.adjustment_bands = Some(vec![band].into());
        editor.doc.layers.push(adjustment);
        let art = crate::Mark {
            points: vec![[2., 2.], [30., 30.]].into(),
            rectangle: true,
            stroke: false,
            fill: Some([255, 0, 0, 255]),
            ..Default::default()
        };
        for frame in &mut editor.doc.layers[0].frames {
            frame.push(art.clone());
        }
        editor.edit_adjustment_band(1, 5, BandAction::Split);
        let layer = &editor.doc.layers[1];
        assert_eq!(layer.options.adjustment_bands.as_ref().unwrap().len(), 2);
        assert_eq!(effects_at(layer, 4).unwrap()[0].values[0], 40.);
        assert_eq!(effects_at(layer, 5).unwrap()[0].values[0], 60.);
        assert!(effects_at(layer, 9).is_none());
        let bytes = crate::native_project::encode(&editor.doc).unwrap();
        let doc = crate::native_project::decode_if_native(&bytes)
            .unwrap()
            .unwrap();
        let render = |frame| {
            let pixels =
                crate::exporter::raster(&doc, frame, 32, 32, &ctx, &mut Default::default());
            let offset = (16 * 32 + 16) * 3;
            [pixels[offset], pixels[offset + 1], pixels[offset + 2]]
        };
        assert_eq!(render(1), [255, 0, 0]);
        assert_eq!(render(5), [255, 255, 0]);
        assert_eq!(render(8), [0, 255, 0]);
        assert_eq!(render(9), [255, 0, 0]);
        editor.history(false);
        assert_eq!(
            editor.doc.layers[1]
                .options
                .adjustment_bands
                .as_ref()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn fx_band_edges_trim_and_body_moves_keys_with_the_band() {
        let ctx = egui::Context::default();
        let mut editor = Editor::new(&eframe::CreationContext::_new_kittest(ctx.clone()));
        editor.doc = Document::default();
        editor.recovery =
            std::env::temp_dir().join(format!("fx-drag-test-{}.json", std::process::id()));
        let effects: crate::shared::SharedVec<_> = vec![Effect::new(Kind::Gaussian)].into();
        let mut band = Band {
            start: 2,
            end: Some(9),
            ..Default::default()
        };
        band.set_key(3, effects.clone());
        editor.doc.layers[0].options.adjustment = Some(effects);
        editor.doc.layers[0].options.adjustment_bands = Some(vec![band].into());
        let rect = egui::Rect::from_min_size(Pos2::new(10., 30.), Vec2::new(480., 28.));
        let tick = |editor: &mut Editor, events| {
            ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(600., 200.))),
                    events,
                    ..Default::default()
                },
                |ui| editor.adjustment_track_ui(ui, 0, rect, 20., 24),
            )
            .textures_delta
            .clear();
        };
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            pressed,
            button: egui::PointerButton::Primary,
            modifiers: egui::Modifiers::NONE,
        };
        let drag = |editor: &mut Editor, from: Pos2, to: Pos2| {
            tick(editor, vec![egui::Event::PointerMoved(from)]);
            tick(editor, vec![button(from, true)]);
            tick(editor, vec![egui::Event::PointerMoved(to)]);
            tick(editor, vec![button(to, false)]);
        };
        tick(&mut editor, vec![]);
        drag(&mut editor, Pos2::new(188., 44.), Pos2::new(148., 44.));
        assert_eq!(bands_for(&editor.doc.layers[0])[0].range(24), 2..7);
        drag(&mut editor, Pos2::new(101., 44.), Pos2::new(141., 44.));
        let bands = bands_for(&editor.doc.layers[0]);
        assert_eq!(bands[0].range(24), 4..9);
        assert_eq!(bands[0].keys[0].frame, 5);
        drag(&mut editor, Pos2::new(120., 44.), Pos2::new(140., 44.));
        let bands = bands_for(&editor.doc.layers[0]);
        assert_eq!(bands[0].range(24), 4..9);
        assert_eq!(bands[0].keys[0].frame, 6);
    }

    #[test]
    fn adjustment_roundtrips_and_only_affects_layers_below_it() {
        let mut doc = Document {
            width: 64,
            height: 64,
            ..Default::default()
        };
        let mut upper = doc.layers[0].clone();
        let mut adjustment = upper.clone();
        let red_rectangle = |left, right| crate::Mark {
            points: vec![[left, 10.], [right, 10.], [right, 54.], [left, 54.]].into(),
            fill: Some([255, 0, 0, 255]),
            stroke: false,
            closed: true,
            ..Default::default()
        };
        doc.layers[0].frames[0].push(red_rectangle(4., 30.));
        upper.frames[0].push(red_rectangle(34., 60.));
        let mut hue = Effect::new(Kind::Hue);
        hue.values[0] = 120.;
        adjustment.options.adjustment = Some(vec![hue].into());
        doc.layers.push(adjustment);
        doc.layers.push(upper);

        let bytes = crate::native_project::encode(&doc).unwrap();
        let mut doc = crate::native_project::decode_if_native(&bytes)
            .unwrap()
            .unwrap();
        let render = |doc: &Document| {
            crate::exporter::raster(
                doc,
                0,
                64,
                64,
                &egui::Context::default(),
                &mut Default::default(),
            )
        };
        let left = (32 * 64 + 16) * 3;
        let right = (32 * 64 + 48) * 3;
        let pixels = render(&doc);
        assert_eq!(&pixels[left..left + 3], &[0, 255, 0]);
        assert_eq!(&pixels[right..right + 3], &[255, 0, 0]);

        doc.layers[0].options.light_receiver = Some(100);
        doc.layers[1].options.fx_target = Some(crate::light::Receiver::Layer(100));
        assert_eq!(&render(&doc)[left..left + 3], &[0, 255, 0]);
        doc.layers[1].options.fx_target = Some(crate::light::Receiver::Layer(101));
        assert_eq!(&render(&doc)[left..left + 3], &[255, 0, 0]);
        doc.layers[1].visible = false;
        let pixels = render(&doc);
        assert_eq!(&pixels[left..left + 3], &[255, 0, 0]);
    }
}

#[cfg(test)]
#[test]
fn effect_cache_matches_uncached_render_after_parameter_and_input_changes() {
    let mut cache = EffectCache::default();
    let mut effects = [Effect::new(Kind::Gaussian), Effect::new(Kind::Glow)];
    let mut input = vec![0; 16 * 16 * 4];
    input[512..516].copy_from_slice(&[255; 4]);
    for index in 0..3 {
        if index == 1 {
            effects[1].values[2] = 0.5;
        }
        if index == 2 {
            input[513] = 190;
        }
        let mut expected = input.clone();
        let mut actual = input.clone();
        apply(&mut expected, 16, 16, &effects, 1.);
        cache.apply(&mut actual, 16, 16, &effects, 1.);
        assert_eq!(actual, expected);
    }
}
