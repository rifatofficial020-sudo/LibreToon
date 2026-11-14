//! Sparse, editable raster cels. Tile bytes are shared with the document history.
use crate::{
    Editor, Mark,
    canvas::{Tool, View},
    i18n::tr,
};
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};

const SIDE: i32 = 64;
const BYTES: usize = (SIDE * SIDE * 4) as usize;
const MAX_TILES_PER_STROKE: usize = 1024;
const CAPACITY: usize = 256 * 1024 * 1024;
type Cell = (i32, i32);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Preset {
    #[default]
    Ink,
    Soft,
    Airbrush,
    Graphite,
    Dry,
    Chalk,
    Smudge,
}
impl Preset {
    const ALL: [Self; 7] = [
        Self::Ink,
        Self::Soft,
        Self::Airbrush,
        Self::Graphite,
        Self::Dry,
        Self::Chalk,
        Self::Smudge,
    ];
    fn name(self) -> &'static str {
        match self {
            Self::Ink => "Round ink",
            Self::Soft => "Soft round",
            Self::Airbrush => "Soft airbrush",
            Self::Graphite => "Graphite pencil",
            Self::Dry => "Dry bristle",
            Self::Chalk => "Grainy chalk",
            Self::Smudge => "Smudge brush",
        }
    }
    fn icon(self) -> crate::design::Icon {
        use crate::design::Icon;
        match self {
            Self::Ink => Icon::Brush,
            Self::Soft => Icon::CapRound,
            Self::Airbrush => Icon::Airbrush,
            Self::Graphite => Icon::SketchBrush,
            Self::Dry => Icon::ShadeBrush,
            Self::Chalk => Icon::Grid,
            Self::Smudge => Icon::Blur,
        }
    }

    fn brush(self) -> Brush {
        let (size, hardness, opacity, flow, spacing) = match self {
            Self::Ink => (14., 0.95, 1., 1., 0.12),
            Self::Soft => (40., 0.32, 0.85, 0.4, 0.12),
            Self::Airbrush => (82., 0.08, 0.65, 0.18, 0.10),
            Self::Graphite => (6., 0.7, 0.85, 0.55, 0.18),
            Self::Dry => (32., 0.82, 0.88, 0.60, 0.19),
            Self::Chalk => (25., 0.5, 0.88, 0.55, 0.18),
            Self::Smudge => (34., 0.37, 0.85, 0.45, 0.11),
        };
        Brush {
            preset: self,
            size,
            hardness,
            opacity,
            flow,
            spacing,
            pressure_size: true,
            pressure_opacity: false,
            smudge: 0.7,
            eraser_size: 25.,
        }
    }

    fn tip(self, x: f32, y: f32, hardness: f32, radius: f32, px: i32, py: i32) -> f32 {
        let (x, y) = match self {
            Self::Graphite => (x * 0.88, y * 1.35),
            Self::Dry => ((x + y * 0.28) * 0.96, y * 1.23),
            _ => (x, y),
        };
        let distance = x.hypot(y);
        let edge = (1. - hardness).max(0.7 / radius);
        let t = ((1. - distance) / edge).clamp(0., 1.);
        let falloff = t * t * (3. - 2. * t);
        let mut hash =
            (px as u32).wrapping_mul(0x9e37_79b9) ^ (py as u32).wrapping_mul(0x85eb_ca6b);
        hash ^= hash >> 16;
        hash = hash.wrapping_mul(0x7feb_352d);
        let grain = (hash & 255) as f32 / 255.;
        let texture = match self {
            Self::Graphite => 0.22 + 0.78 * grain.powi(2),
            Self::Dry => {
                (0.26 + 0.74 * grain)
                    * (0.36 + 0.64 * (px as f32 * 0.9 + py as f32 * 0.27).sin().abs().powi(3))
            }
            Self::Chalk => {
                if grain > 0.27 {
                    0.35 + 0.65 * grain
                } else {
                    0.04
                }
            }
            _ => 1.,
        };
        falloff * texture
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct Brush {
    pub preset: Preset,
    size: f32,
    hardness: f32,
    opacity: f32,
    flow: f32,
    spacing: f32,
    pressure_size: bool,
    pressure_opacity: bool,
    smudge: f32,
    eraser_size: f32,
}
impl Default for Brush {
    fn default() -> Self {
        Preset::Ink.brush()
    }
}
impl Brush {
    fn diameter(self, eraser: bool) -> f32 {
        let size = if eraser { self.eraser_size } else { self.size };
        if size.is_finite() {
            size.clamp(1., 256.)
        } else {
            14.
        }
    }
}
fn unit(v: f32) -> f32 {
    if v.is_finite() { v.clamp(0., 1.) } else { 1. }
}

struct Tile {
    original: Vec<u8>,
    rgba: Vec<u8>,
    coverage: Vec<f32>,
    id: Option<u64>,
    source: Option<(usize, u64)>,
}
pub(crate) struct Stroke {
    layer: usize,
    frame: usize,
    last: Pos2,
    distance: f32,
    pressure: Option<f32>,
    pending: Vec<Pos2>,
    wait_for_pressure_until: std::time::Instant,
    brush: Brush,
    eraser: bool,
    color: [u8; 4],
    tiles: BTreeMap<Cell, Tile>,
    started: bool,
    bytes: usize,
    carried: Option<[u8; 4]>,
    next_dab: std::time::Instant,
    limit_reached: bool,
}
fn tile_cell(point: Pos2) -> Cell {
    (
        (point.x.floor() as i32).div_euclid(SIDE),
        (point.y.floor() as i32).div_euclid(SIDE),
    )
}
fn tile_mark(cell: Cell, id: u64) -> Mark {
    let [x, y] = [cell.0 as f32 * SIDE as f32, cell.1 as f32 * SIDE as f32];
    let mut mark = Mark {
        points: vec![[x, y], [x + SIDE as f32, y + SIDE as f32]].into(),
        color: [255; 4],
        rectangle: true,
        closed: true,
        stroke: false,
        fill: Some([0; 4]),
        ..Default::default()
    };
    mark.style.raster_tile = true;
    mark.style.media_image = Some(id);
    mark
}

fn blend(base: [u8; 4], ink: [u8; 4], coverage: f32, erase: bool) -> [u8; 4] {
    let old_alpha = f32::from(base[3]) / 255.;
    if erase {
        let alpha = (f32::from(base[3]) * (1. - coverage)).round() as u8;
        return if alpha == 0 {
            [0; 4]
        } else {
            [base[0], base[1], base[2], alpha]
        };
    }
    let a = coverage * f32::from(ink[3]) / 255.;
    let alpha = a + old_alpha * (1. - a);
    if alpha <= 0. {
        return [0; 4];
    }
    let mut out = [0; 4];
    for k in 0..3 {
        out[k] = ((f32::from(ink[k]) * a + f32::from(base[k]) * old_alpha * (1. - a)) / alpha)
            .round() as u8;
    }
    out[3] = (alpha * 255.).round() as u8;
    out
}

impl Stroke {
    fn paint_samples(
        &mut self,
        doc: &crate::Document,
        point: Option<Pos2>,
        force: Option<f32>,
        down: bool,
    ) -> BTreeSet<Cell> {
        let mut changed = BTreeSet::new();
        let time = std::time::Instant::now();
        let force = force
            .filter(|force| force.is_finite())
            .map(|force| force.clamp(0., 1.));
        let spacing =
            (self.brush.diameter(self.eraser) * self.brush.spacing.clamp(0.03, 0.75)).max(0.5);
        if !self.pending.is_empty() {
            if down
                && let Some(point) = point
                && self.pending.last() != Some(&point)
            {
                self.pending.push(point);
            }
            let pressure_sensitive = self.brush.pressure_size || self.brush.pressure_opacity;
            if pressure_sensitive
                && down
                && force.is_none()
                && time < self.wait_for_pressure_until
                && self.pending.len() < 32
            {
                return changed;
            }
            let pressure = force.or(self.pressure).unwrap_or(1.);
            let pending = std::mem::take(&mut self.pending);
            self.dab(doc, pending[0], pressure, &mut changed);
            self.last = pending[0];
            self.distance = spacing;
            self.next_dab = time + std::time::Duration::from_millis(33);
            for point in pending.into_iter().skip(1) {
                self.paint_segment(doc, point, pressure, pressure, spacing, &mut changed);
            }
            self.pressure = force.or(self.pressure);
            return changed;
        }
        if !down {
            return changed;
        }
        if let Some(point) = point {
            let pressure = force.or(self.pressure).unwrap_or(1.);
            if point != self.last {
                // The first real pressure packet must not interpolate from mouse pressure.
                let prior = self.pressure.unwrap_or(pressure);
                self.paint_segment(doc, point, prior, pressure, spacing, &mut changed);
                self.next_dab = time + std::time::Duration::from_millis(33);
            } else if self.brush.preset == Preset::Airbrush && !self.eraser && time >= self.next_dab
            {
                self.dab(doc, point, pressure, &mut changed);
                self.next_dab = time + std::time::Duration::from_millis(33);
            }
            self.pressure = force.or(self.pressure);
        }
        changed
    }

    fn paint_segment(
        &mut self,
        doc: &crate::Document,
        point: Pos2,
        from_force: f32,
        to_force: f32,
        spacing: f32,
        changed: &mut BTreeSet<Cell>,
    ) {
        let delta = point - self.last;
        let length = delta.length();
        if length <= 0. {
            return;
        }
        let mut offset = self.distance.max(0.);
        let step = spacing.max(length / 96.);
        while offset <= length {
            let t = offset / length;
            self.dab(
                doc,
                self.last + delta * t,
                from_force + (to_force - from_force) * t,
                changed,
            );
            offset += step;
        }
        self.distance = offset - length;
        self.last = point;
    }

    fn sample(&self, doc: &crate::Document, point: Pos2) -> [u8; 4] {
        let cell = tile_cell(point);
        let index = ((point.y.floor() as i32).rem_euclid(SIDE) * SIDE
            + (point.x.floor() as i32).rem_euclid(SIDE)) as usize
            * 4;
        if let Some(tile) = self.tiles.get(&cell) {
            return tile.rgba[index..index + 4].try_into().unwrap();
        }
        let layer = &doc.layers[self.layer];
        layer.frames[self.frame]
            .iter()
            .find_map(|mark| {
                let origin = mark.points.first().copied()?;
                if !mark.style.raster_tile || tile_cell(crate::vector::pos(origin)) != cell {
                    return None;
                }
                doc.extras
                    .media
                    .pixels(mark.style.media_image?)
                    .and_then(|(_, bytes)| bytes.get(index..index + 4)?.try_into().ok())
            })
            .unwrap_or([0; 4])
    }
    fn dab(
        &mut self,
        doc: &crate::Document,
        point: Pos2,
        pressure: f32,
        changed: &mut BTreeSet<Cell>,
    ) {
        let size = self.brush.diameter(self.eraser);
        let radius = (size
            * 0.5
            * if self.brush.pressure_size {
                pressure.max(0.07)
            } else {
                1.
            })
        .max(0.5);
        let opacity = unit(self.brush.opacity)
            * if self.brush.pressure_opacity {
                pressure
            } else {
                1.
            };
        let smudge = !self.eraser && self.brush.preset == Preset::Smudge;
        let source = if smudge {
            self.carried.unwrap_or_else(|| self.sample(doc, point))
        } else {
            self.color
        };
        if smudge {
            let next = self.sample(doc, point);
            let pickup = 1. - unit(self.brush.smudge);
            self.carried = Some(std::array::from_fn(|i| {
                (f32::from(source[i]) * (1. - pickup) + f32::from(next[i]) * pickup).round() as u8
            }));
        }
        if (!self.eraser && source[3] == 0) || opacity == 0. {
            return;
        }
        let reach = radius * 1.2 + 1.;
        let bounds = Rect::from_center_size(point, Vec2::splat(reach * 2.));
        let (left, top) = tile_cell(bounds.min);
        let (right, bottom) = tile_cell(bounds.max);
        let brush = if self.eraser {
            Preset::Ink
        } else {
            self.brush.preset
        };
        let flow = unit(self.brush.flow);
        for ty in top..=bottom {
            for tx in left..=right {
                let cell = (tx, ty);
                let frame = &doc.layers[self.layer].frames[self.frame];
                if !self.tiles.contains_key(&cell) {
                    let source = frame.iter().enumerate().find_map(|(index, mark)| {
                        (mark.style.raster_tile
                            && tile_cell(crate::vector::pos(*mark.points.first()?)) == cell)
                            .then_some((index, mark.style.media_image?))
                    });
                    if self.eraser && source.is_none() {
                        continue;
                    }
                    if self.tiles.len() >= MAX_TILES_PER_STROKE
                        || self.bytes.saturating_add((self.tiles.len() + 1) * BYTES) > CAPACITY
                    {
                        self.limit_reached = true;
                        continue;
                    }
                    let original = source
                        .and_then(|(_, id)| doc.extras.media.pixels(id))
                        .filter(|(size, _)| *size == [SIDE as usize; 2])
                        .map_or_else(|| vec![0; BYTES], |(_, data)| data.to_vec());
                    self.tiles.insert(
                        cell,
                        Tile {
                            rgba: original.clone(),
                            original,
                            coverage: vec![0.; BYTES / 4],
                            id: None,
                            source,
                        },
                    );
                }
                let tile = self.tiles.get_mut(&cell).unwrap();
                let (ox, oy) = (tx * SIDE, ty * SIDE);
                let x0 = ox.max(bounds.left().floor() as i32);
                let y0 = oy.max(bounds.top().floor() as i32);
                let x1 = (ox + SIDE).min(bounds.right().ceil() as i32);
                let y1 = (oy + SIDE).min(bounds.bottom().ceil() as i32);
                for py in y0..y1 {
                    for px in x0..x1 {
                        let tip = brush.tip(
                            (px as f32 + 0.5 - point.x) / radius,
                            (py as f32 + 0.5 - point.y) / radius,
                            unit(self.brush.hardness),
                            radius,
                            px,
                            py,
                        );
                        if tip <= 0. {
                            continue;
                        }
                        let index = ((py - oy) * SIDE + px - ox) as usize;
                        let i = index * 4;
                        let value = if smudge {
                            blend(
                                tile.rgba[i..i + 4].try_into().unwrap(),
                                source,
                                opacity * flow * tip * unit(self.brush.smudge),
                                false,
                            )
                        } else {
                            tile.coverage[index] += (1. - tile.coverage[index]) * flow * tip;
                            blend(
                                tile.original[i..i + 4].try_into().unwrap(),
                                source,
                                opacity * tile.coverage[index],
                                self.eraser,
                            )
                        };
                        if tile.rgba[i..i + 4] != value {
                            tile.rgba[i..i + 4].copy_from_slice(&value);
                            changed.insert(cell);
                        }
                    }
                }
            }
        }
    }
}

impl Editor {
    pub(crate) fn pick_raster_color(&mut self, point: Pos2) -> bool {
        let marks = crate::features::resolve(&self.doc, self.frame, false);
        for (_, _, mark) in marks
            .iter()
            .rev()
            .filter(|(layer, _, mark)| *layer == self.layer && mark.style.raster_tile)
        {
            if !crate::vector::contains_fill(mark, point) {
                continue;
            }
            let Some(id) = mark.style.media_image else {
                continue;
            };
            let Some(([width, height], bytes)) = self.doc.extras.media.pixels(id) else {
                continue;
            };
            let origin = crate::vector::control_bounds(mark).min;
            let [u, v] = if let Some(basis) = mark.style.texture_basis {
                let [a, b, c] = basis.map(crate::vector::pos);
                let (x, y, d) = (b - a, c - a, point - a);
                let det = x.x * y.y - x.y * y.x;
                if det.abs() < 1e-8 {
                    continue;
                }
                [(d.x * y.y - d.y * y.x) / det, (x.x * d.y - x.y * d.x) / det]
            } else {
                [
                    (point.x - origin.x) / width as f32,
                    (point.y - origin.y) / height as f32,
                ]
            };
            let x = (u.clamp(0., 1.) * width as f32) as usize;
            let y = (v.clamp(0., 1.) * height as f32) as usize;
            let offset = (y.min(height - 1) * width + x.min(width - 1)) * 4;
            if bytes[offset + 3] == 0 {
                continue;
            }
            self.color = Color32::from_rgb(bytes[offset], bytes[offset + 1], bytes[offset + 2]);
            self.fill_color = self.color;
            return true;
        }
        false
    }

    pub(crate) fn raster_active(&self) -> bool {
        self.doc
            .layers
            .get(self.layer)
            .is_some_and(|layer| layer.options.raster)
    }

    pub(crate) fn raster_tool(tool: Tool) -> bool {
        matches!(
            tool,
            Tool::Brush
                | Tool::VectorBrush
                | Tool::SketchBrush
                | Tool::FillBrush
                | Tool::ShadeBrush
                | Tool::LightBrush
                | Tool::Airbrush
                | Tool::Eraser
        )
    }

    pub(crate) fn raster_toolbar(&mut self, ui: &mut egui::Ui) {
        let response = crate::design::button(
            ui,
            crate::design::Icon::Brush,
            "",
            "Raster brush",
            Self::raster_tool(self.tool) && self.tool != Tool::Eraser,
            true,
        );
        badge(ui, response.rect, "R");
        let corner = response.rect.right_bottom() - Vec2::splat(2.);
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                corner,
                corner - Vec2::new(5., 0.),
                corner - Vec2::new(0., 5.),
            ],
            crate::themes::muted(),
            egui::Stroke::NONE,
        ));
        let menu_key = response.id.with("raster-presets");
        let mut open = ui
            .ctx()
            .data(|data| data.get_temp::<bool>(menu_key))
            .unwrap_or(false);
        if response.secondary_clicked()
            || response.clicked()
                && response.interact_pointer_pos().is_some_and(|p| {
                    p.x >= response.rect.right() - 10. && p.y >= response.rect.bottom() - 10.
                })
        {
            open = !open;
        } else if response.clicked() {
            self.activate_tool(Tool::Brush);
        }
        egui::Popup::from_response(&response)
            .at_position(Pos2::new(ui.clip_rect().right() + 6., response.rect.top()))
            .align(egui::emath::RectAlign::RIGHT_START)
            .open_bool(&mut open)
            .show(|ui| {
                ui.set_min_width(185.);
                crate::design::category(ui, "Raster brush presets");
                self.raster_presets(ui);
            });
        ui.ctx().data_mut(|data| data.insert_temp(menu_key, open));
    }

    pub(crate) fn raster_settings(&mut self, ui: &mut egui::Ui) {
        let eraser = self.tool == Tool::Eraser;
        ui.heading(tr(if eraser {
            "Raster eraser"
        } else {
            "Raster brush"
        }));
        if !eraser {
            egui::ComboBox::from_id_salt("raster-preset")
                .selected_text(tr(self.studio.prefs.raster_brush.preset.name()))
                .show_ui(ui, |ui| {
                    self.raster_presets(ui);
                });
        }
        let settings = &mut self.studio.prefs.raster_brush;
        let size = if eraser {
            &mut settings.eraser_size
        } else {
            &mut settings.size
        };
        use crate::design::Icon;
        let mut changed = setting_row(ui, Icon::Width, |ui| {
            ui.add(
                egui::Slider::new(size, 1. ..=256.)
                    .text(tr("Diameter"))
                    .suffix(" px"),
            )
            .changed()
        });
        for (icon, label, value, range) in [
            (Icon::Focus, "Hardness", &mut settings.hardness, 0. ..=1.),
            (Icon::Eye, "Brush opacity", &mut settings.opacity, 0. ..=1.),
            (Icon::Airbrush, "Flow", &mut settings.flow, 0.01..=1.),
            (
                Icon::FrameExtend,
                "Dab spacing",
                &mut settings.spacing,
                0.03..=0.75,
            ),
        ] {
            changed |= setting_row(ui, icon, |ui| {
                ui.add(
                    egui::Slider::new(value, range)
                        .text(tr(label))
                        .custom_formatter(|value, _| format!("{:.0}%", value * 100.))
                        .custom_parser(|text| {
                            text.trim_end_matches('%')
                                .trim()
                                .parse::<f64>()
                                .ok()
                                .map(|v| v / 100.)
                        }),
                )
                .changed()
            });
        }
        if !eraser && settings.preset == Preset::Smudge {
            changed |= setting_row(ui, Icon::Blur, |ui| {
                ui.add(
                    egui::Slider::new(&mut settings.smudge, 0. ..=1.).text(tr("Smudge strength")),
                )
                .changed()
            });
        }
        changed |= setting_row(ui, Icon::Pressure, |ui| {
            ui.checkbox(&mut settings.pressure_size, tr("Pressure controls size"))
                .changed()
        });
        changed |= setting_row(ui, Icon::Pressure, |ui| {
            ui.checkbox(
                &mut settings.pressure_opacity,
                tr("Pressure controls opacity"),
            )
            .changed()
        });
        let key = egui::Id::unique("raster-brush-preferences-dirty");
        if changed {
            ui.ctx().data_mut(|data| data.insert_temp(key, true));
        }
        if !ui.input(|input| input.pointer.primary_down())
            && ui
                .ctx()
                .data_mut(|data| data.remove_temp::<bool>(key))
                .unwrap_or(false)
        {
            let _ = self.studio.save();
        }
    }

    pub(crate) fn raster_presets(&mut self, ui: &mut egui::Ui) {
        for preset in Preset::ALL {
            let current = self.studio.prefs.raster_brush.preset == preset;
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(40., 22.), egui::Sense::hover());
                let brush = preset.brush();
                let color = crate::themes::palette().text;
                for y in 0..18 {
                    for x in 0..38 {
                        let center = 10. + (x as f32 * 0.11).sin() * 2.;
                        let dy = (y as f32 - center) / (3. + x as f32 / 8.);
                        let alpha = preset.tip(0., dy, brush.hardness, 9., x, y) * brush.opacity;
                        if alpha > 0. {
                            ui.painter().rect_filled(
                                Rect::from_min_size(
                                    rect.min + Vec2::new(x as f32, y as f32),
                                    Vec2::splat(1.),
                                ),
                                0.,
                                color.gamma_multiply(alpha),
                            );
                        }
                    }
                }
                if crate::design::button(ui, preset.icon(), preset.name(), "", current, true)
                    .clicked()
                {
                    let eraser_size = self.studio.prefs.raster_brush.eraser_size;
                    self.studio.prefs.raster_brush = Brush {
                        eraser_size,
                        ..brush
                    };
                    self.activate_tool(Tool::Brush);
                    let _ = self.studio.save();
                    ui.close();
                }
            });
        }
    }

    pub(crate) fn raster_needs_tick(state: &crate::canvas::Canvas) -> bool {
        state.raster_stroke.as_ref().is_some_and(|stroke| {
            !stroke.pending.is_empty()
                || (stroke.brush.preset == Preset::Airbrush && !stroke.eraser)
        })
    }

    pub(crate) fn finish_raster_stroke(&mut self) {
        if let Some(stroke) = self.canvas.raster_stroke.take() {
            self.commit_raster_stroke(stroke);
        }
    }

    fn commit_raster_stroke(&mut self, stroke: Stroke) {
        if stroke.limit_reached {
            self.status =
                tr("Raster tile limit reached. Save the project or use a shorter stroke.").into();
        }
        if !stroke.started {
            return;
        }
        if let Some(layer) = self.doc.layers.get_mut(stroke.layer)
            && stroke.frame < layer.frames.len()
        {
            let exposure = layer.exposure(stroke.frame);
            let mut drawing = layer.frames[stroke.frame].clone();
            let empty: HashSet<_> = stroke
                .tiles
                .values()
                .filter(|tile| tile.rgba.chunks_exact(4).all(|pixel| pixel[3] == 0))
                .filter_map(|tile| tile.id)
                .collect();
            drawing.retain(|mark| !mark.style.media_image.is_some_and(|id| empty.contains(&id)));
            for frame in exposure {
                layer.frames[frame] = drawing.clone();
            }
        }
        self.raster_cleanup = self.raster_cleanup.saturating_add(stroke.tiles.len());
        if self.raster_cleanup >= 64 {
            self.raster_cleanup = 0;
            let mut project = self.project_document();
            let removed = prune(&mut project);
            self.doc
                .extras
                .media
                .assets
                .retain(|asset| !removed.contains(&asset.id));
            for id in removed {
                self.invalidate_raster_texture(id);
            }
        }
        self.recover();
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn raster_input(
        &mut self,
        state: &mut crate::canvas::Canvas,
        tool: Tool,
        pointer: Option<Pos2>,
        view: View,
        start: bool,
        down: bool,
        subtract: bool,
    ) -> bool {
        let raster = self.raster_active();
        let painting = raster && Self::raster_tool(tool);
        if state.raster_stroke.as_ref().is_some_and(|s| {
            !painting || s.layer != self.layer || s.frame != self.frame || self.playing
        }) {
            self.commit_raster_stroke(state.raster_stroke.take().unwrap());
        }
        if !painting {
            return false;
        }
        let mut stroke = state.raster_stroke.take();
        let point = pointer
            .map(|pos| view.world(pos))
            .filter(|p| p.is_finite() && p.x.abs() < 32768. && p.y.abs() < 32768.);
        let layer = &self.doc.layers[self.layer];
        if start
            && stroke.is_none()
            && !self.playing
            && layer.visible
            && !layer.options.locked
            && crate::folder_groups::layer_active(&self.doc, layer, self.frame)
            && let Some(point) = point
        {
            let mask_brush = layer.options.light_mask && tool == Tool::Brush;
            let eraser = tool == Tool::Eraser || mask_brush && subtract;
            let mut brush = self.studio.prefs.raster_brush;
            if mask_brush && subtract {
                brush.eraser_size = brush.size;
            }
            stroke = Some(Stroke {
                layer: self.layer,
                frame: self.frame,
                last: point,
                distance: 0.,
                pressure: None,
                pending: vec![point],
                wait_for_pressure_until: std::time::Instant::now()
                    + std::time::Duration::from_millis(32),
                brush,
                eraser,
                color: if mask_brush {
                    [255; 4]
                } else {
                    self.fill_color.to_srgba_unmultiplied()
                },
                tiles: BTreeMap::new(),
                started: false,
                bytes: self
                    .doc
                    .extras
                    .media
                    .assets
                    .iter()
                    .map(|asset| asset.data.len())
                    .sum(),
                carried: None,
                next_dab: std::time::Instant::now(),
                limit_reached: false,
            });
            state.clear_selection();
        }
        if let Some(mut stroke) = stroke {
            if !stroke.pending.is_empty() && tool == Tool::Eraser {
                stroke.eraser = true;
            }
            let changed = stroke.paint_samples(&self.doc, point, self.drawing.pen.force(), down);
            if !changed.is_empty() {
                if !stroke.started {
                    self.checkpoint();
                    self.prepare_paint_key(stroke.layer, stroke.frame);
                    stroke.started = true;
                }
                for cell in changed {
                    let tile = stroke.tiles.get_mut(&cell).unwrap();
                    if let Some(id) = tile.id {
                        if let Some(asset) =
                            self.doc.extras.media.assets.iter_mut().find(|a| a.id == id)
                        {
                            asset.data = tile.rgba.clone().into();
                        }
                        self.update_raster_texture(id, &tile.rgba, SIDE as usize);
                    } else {
                        let mut asset = crate::media::bitmap(
                            tile.rgba.clone(),
                            SIDE as u32,
                            SIDE as u32,
                            String::new(),
                        );
                        asset.raster_tile = true;
                        let id = asset.id;
                        self.doc.extras.media.assets.push(asset);
                        let frame = &mut self.doc.layers[stroke.layer].frames[stroke.frame];
                        if let Some((index, _)) = tile.source {
                            frame[index] = tile_mark(cell, id);
                        } else {
                            frame.push(tile_mark(cell, id));
                        }
                        tile.id = Some(id);
                    }
                }
            }
            if down {
                state.raster_stroke = Some(stroke);
            } else {
                self.commit_raster_stroke(stroke);
            }
        }
        true
    }
}

fn setting_row(
    ui: &mut egui::Ui,
    icon: crate::design::Icon,
    content: impl FnOnce(&mut egui::Ui) -> bool,
) -> bool {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(18.), egui::Sense::hover());
        crate::design::draw(ui.painter(), rect, icon, crate::themes::muted());
        content(ui)
    })
    .inner
}
pub(crate) fn badge(ui: &egui::Ui, rect: Rect, text: &str) {
    let center = rect.left_bottom() + Vec2::new(5., -5.);
    let background = crate::themes::palette().panel;
    ui.painter().rect_filled(
        Rect::from_center_size(center, Vec2::new(10., 10.)),
        2.,
        background,
    );
    ui.painter().text(
        center,
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::monospace(9.),
        crate::themes::accent(),
    );
}

pub(crate) fn cursor(painter: &egui::Painter, pos: Pos2, view: View, brush: Brush, erase: bool) {
    let radius = (brush.diameter(erase) * view.scale.abs() * 0.5).max(1.);
    painter.circle_stroke(pos, radius, egui::Stroke::new(3., Color32::BLACK));
    painter.circle_stroke(pos, radius, egui::Stroke::new(1., Color32::WHITE));
}

pub(crate) fn prune(doc: &mut crate::Document) -> HashSet<u64> {
    if !doc.extras.media.assets.iter().any(|a| a.raster_tile) {
        return HashSet::new();
    }
    let mut visited = HashSet::new();
    let mut used = HashSet::new();
    let layers = doc
        .layers
        .iter()
        .chain(
            doc.extras
                .scenes
                .iter()
                .flat_map(|scene| scene.layers.iter()),
        )
        .chain(
            doc.extras
                .symbols
                .iter()
                .flat_map(|symbol| symbol.layers.iter()),
        );
    for layer in layers {
        for frame in &layer.frames {
            if visited.insert(frame.identity()) {
                used.extend(frame.iter().filter_map(|mark| mark.style.media_image));
            }
        }
    }
    let mut removed = HashSet::new();
    doc.extras.media.assets.retain(|asset| {
        if asset.raster_tile && !used.contains(&asset.id) {
            removed.insert(asset.id);
            false
        } else {
            true
        }
    });
    removed
}

pub(crate) fn clipped_mesh(
    mark: &Mark,
    view: View,
    texture: egui::TextureId,
    basis: [[f32; 2]; 3],
) -> Option<egui::Mesh> {
    use lyon_tessellation::{
        BuffersBuilder, FillOptions, FillTessellator, FillVertex, VertexBuffers, math::point,
        path::Path,
    };
    let [origin, right, bottom] = basis.map(crate::vector::pos);
    let (x, y) = (right - origin, bottom - origin);
    let determinant = x.x * y.y - x.y * y.x;
    if determinant.abs() < 1e-8 {
        return None;
    }
    let mut path = Path::builder();
    let outside = crate::vector::flatten(mark, 0.1);
    for ring in std::iter::once(outside).chain(
        mark.holes
            .iter()
            .map(|ring| ring.iter().copied().map(crate::vector::pos).collect()),
    ) {
        let Some(first) = ring.first() else {
            continue;
        };
        path.begin(point(first.x, first.y));
        for p in &ring[1..] {
            path.line_to(point(p.x, p.y));
        }
        path.end(true);
    }
    let mut output: VertexBuffers<egui::epaint::Vertex, u32> = VertexBuffers::new();
    FillTessellator::new()
        .tessellate_path(
            &path.build(),
            &FillOptions::even_odd(),
            &mut BuffersBuilder::new(&mut output, |v: FillVertex<'_>| {
                let p = Pos2::new(v.position().x, v.position().y);
                let d = p - origin;
                let u = (d.x * y.y - d.y * y.x) / determinant;
                let v = (x.x * d.y - x.y * d.x) / determinant;
                egui::epaint::Vertex {
                    pos: view.screen(p),
                    uv: Pos2::new(u.clamp(0., 1.), v.clamp(0., 1.)),
                    color: Color32::from_white_alpha(mark.color[3]),
                }
            }),
        )
        .ok()?;
    Some(egui::Mesh {
        indices: output.indices,
        vertices: output.vertices,
        texture_id: texture,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stroke_clips_nondestructively_and_round_trips_with_undo() {
        let ctx = egui::Context::default();
        let mut editor = Editor::new(&eframe::CreationContext::_new_kittest(ctx.clone()));
        editor.doc = crate::fresh_document();
        editor.recovery =
            std::env::temp_dir().join(format!("raster-test-{}.json", crate::features::id()));
        editor.doc.width = 128;
        editor.doc.height = 64;
        editor.doc.layers[0].frames[0].push(Mark {
            points: vec![[0., 0.], [30., 64.]].into(),
            rectangle: true,
            closed: true,
            stroke: false,
            fill: Some([255; 4]),
            ..Default::default()
        });
        let mut raster = editor.doc.layers[0].clone();
        raster.frames = vec![Default::default(); raster.frames.len()].into();
        raster.init_keys();
        raster.options.raster = true;
        raster.options.clip = true;
        editor.doc.layers.push(raster);
        editor.layer = 1;
        editor.color = Color32::BLUE;
        editor.fill_color = Color32::RED;
        editor.studio.prefs.raster_brush = Brush {
            size: 20.,
            ..Brush::default()
        };
        let view = View {
            center: Pos2::ZERO,
            stage_center: Pos2::ZERO,
            scale: 1.,
            angle: 0.,
        };
        let mut state = std::mem::take(&mut editor.canvas);
        let at = |x| Some(Pos2::new(x, 24.));
        assert!(editor.raster_input(&mut state, Tool::Brush, at(15.), view, true, true, false));
        editor.raster_input(&mut state, Tool::Brush, at(90.), view, false, true, false);
        editor.raster_input(&mut state, Tool::Brush, at(90.), view, false, false, false);
        assert!(
            editor
                .doc
                .extras
                .media
                .assets
                .iter()
                .all(|asset| asset.raster_tile)
        );
        assert!(editor.doc.extras.media.assets.len() >= 2);
        let pixels = |doc: &crate::Document| {
            crate::exporter::raster(doc, 0, 128, 64, &ctx, &mut Default::default())
        };
        let rgb =
            |x: usize, data: &[u8]| <[u8; 3]>::try_from(&data[(24 * 128 + x) * 3..][..3]).unwrap();
        let visible = pixels(&editor.doc);
        assert_eq!(rgb(20, &visible), [255, 0, 0]);
        assert_eq!(rgb(75, &visible), [255; 3]);
        let saved = crate::native_project::encode(&editor.project_document()).unwrap();
        let mut reopened = crate::native_project::decode_if_native(&saved)
            .unwrap()
            .unwrap();
        assert!(reopened.layers[1].options.raster);
        assert_eq!(pixels(&reopened), visible);
        reopened.layers[0].frames[0][0].points[1][0] = 100.;
        assert_eq!(rgb(75, &pixels(&reopened)), [255, 0, 0]);
        editor.history(false);
        assert!(editor.doc.layers[1].frames[0].is_empty());
        assert!(editor.doc.extras.media.assets.is_empty());
        editor.history(true);
        assert_eq!(pixels(&editor.doc), visible);
    }

    #[test]
    fn delayed_and_missing_pen_packets_do_not_leave_full_size_dabs() {
        let doc = crate::fresh_document();
        let start = Pos2::new(24.5, 24.5);
        let mut stroke = Stroke {
            layer: 0,
            frame: 0,
            last: start,
            distance: 0.,
            pressure: None,
            pending: vec![start],
            wait_for_pressure_until: std::time::Instant::now() + std::time::Duration::from_secs(60),
            brush: Brush {
                size: 20.,
                spacing: 0.05,
                pressure_size: true,
                pressure_opacity: true,
                ..Brush::default()
            },
            eraser: false,
            color: [255, 0, 0, 255],
            tiles: BTreeMap::new(),
            started: false,
            bytes: 0,
            carried: None,
            next_dab: std::time::Instant::now(),
            limit_reached: false,
        };
        assert!(
            stroke
                .paint_samples(&doc, Some(Pos2::new(27.5, 24.5)), None, true)
                .is_empty()
        );
        assert!(
            !stroke
                .paint_samples(&doc, Some(Pos2::new(32.5, 24.5)), Some(0.2), true)
                .is_empty()
        );
        assert!(
            !stroke
                .paint_samples(&doc, Some(Pos2::new(38.5, 24.5)), None, true)
                .is_empty()
        );
        assert!(
            stroke
                .paint_samples(&doc, Some(Pos2::new(42.5, 24.5)), None, false)
                .is_empty()
        );
        for x in [24.5, 30.5, 38.5] {
            assert_eq!(stroke.sample(&doc, Pos2::new(x, 24.5))[3], 51);
            assert_eq!(stroke.sample(&doc, Pos2::new(x, 29.5))[3], 0);
        }
        assert_eq!(stroke.pressure, Some(0.2));
    }

    #[test]
    fn raster_mouse_tap_and_inverted_pen_eraser_work_through_canvas() {
        let ctx = egui::Context::default();
        let mut editor = Editor::new(&eframe::CreationContext::_new_kittest(ctx.clone()));
        editor.doc = crate::fresh_document();
        editor.recovery =
            std::env::temp_dir().join(format!("raster-eraser-test-{}.json", crate::features::id()));
        editor.doc.layers[0].options.raster = true;
        editor.tool = Tool::Brush;
        editor.studio.prefs.raster_brush = Brush {
            size: 8.,
            eraser_size: 26.,
            ..Brush::default()
        };
        let point = Pos2::new(500., 350.);
        let tick = |editor: &mut Editor, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000., 700.))),
                    events,
                    ..Default::default()
                },
                |ui| editor.canvas_ui(ui),
            );
            output.textures_delta.clear();
        };
        let event = |pressed| egui::Event::PointerButton {
            pos: point,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        tick(&mut editor, vec![]);
        tick(
            &mut editor,
            vec![egui::Event::PointerMoved(point), event(true)],
        );
        tick(&mut editor, vec![event(false)]);
        assert!(!editor.doc.layers[0].frames[0].is_empty());
        editor.drawing.pen.test_eraser(true);
        editor.drawing.pen.test_pressure(Some(1.));
        tick(&mut editor, vec![event(true)]);
        editor.drawing.pen.test_eraser(false);
        editor.drawing.pen.test_pressure(None);
        tick(&mut editor, vec![event(false)]);
        assert!(editor.doc.layers[0].frames[0].is_empty());
        assert!(editor.tool == Tool::Brush);
        editor.history(false);
        assert!(!editor.doc.layers[0].frames[0].is_empty());
    }

    #[test]
    fn softness_changes_dab_coverage_and_eraser_preserves_rgb() {
        let hard = Preset::Ink.tip(0.75, 0., 0.95, 12., 7, 0);
        let soft = Preset::Soft.tip(0.75, 0., 0.1, 12., 7, 0);
        assert!(hard > soft * 3.);
        let color = blend([0; 4], [160, 80, 20, 255], 0.5, false);
        assert_eq!(color, [160, 80, 20, 128]);
        assert_eq!(blend(color, [255; 4], 0.5, true), [160, 80, 20, 64]);
    }
}
