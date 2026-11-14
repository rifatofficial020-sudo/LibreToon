//! World-space tessellation cache. Drafts deliberately bypass it so retaining a
//! cached snapshot cannot force a full COW copy on every new brush sample.
use crate::{canvas::View, document::Mark, vector};
use eframe::egui::{self, Color32, Pos2, Stroke};
use lyon_tessellation::{
    BuffersBuilder, FillOptions, FillTessellator, FillVertex, VertexBuffers, math::point,
    path::Path,
};
use std::{collections::HashMap, sync::Arc};

#[derive(Hash, PartialEq, Eq)]
struct Key {
    points: usize,
    nodes: usize,
    holes: usize,
    profile: usize,
    caps: (crate::features::Cap, crate::features::Cap),
    width: u32,
    stroke: bool,
    rectangle: bool,
    closed: bool,
    fill: bool,
    detail: i32,
    composited: bool,
}
struct Prepared {
    _owner: Mark,
    rings: Vec<Vec<Pos2>>,
    fill: VertexBuffers<[f32; 2], u32>,
    valid: bool,
    stroke_mesh: VertexBuffers<[f32; 2], u32>,
    stroke_rings: Vec<Vec<Pos2>>,
    stroke_holes: Vec<bool>,
}
impl Prepared {
    fn new(mark: &Mark, tolerance: f32, composited: bool) -> Self {
        let mut rings = vec![vector::flatten(mark, tolerance)];
        rings.extend(
            mark.holes
                .iter()
                .map(|r| r.iter().copied().map(vector::pos).collect()),
        );
        let mut fill = VertexBuffers::new();
        let mut valid = true;
        if mark.fill.is_some() && rings[0].len() > 2 {
            let mut path = Path::builder();
            for r in &rings {
                if let Some(first) = r.first() {
                    path.begin(point(first.x, first.y));
                    for p in &r[1..] {
                        path.line_to(point(p.x, p.y));
                    }
                    path.end(true);
                }
            }
            valid = FillTessellator::new()
                .tessellate_path(
                    &path.build(),
                    &FillOptions::even_odd(),
                    &mut BuffersBuilder::new(&mut fill, |v: FillVertex<'_>| {
                        let p = v.position();
                        [p.x, p.y]
                    }),
                )
                .is_ok();
        }
        let mut stroke_mesh = VertexBuffers::new();
        let mut stroke_rings = vec![];
        let mut stroke_holes = vec![];
        if mark.stroke
            && (composited
                || !mark.width_profile.is_empty()
                || mark.style.start_cap != crate::features::Cap::Round
                || mark.style.end_cap != crate::features::Cap::Round)
        {
            let outlines = if composited
                && mark.width_profile.is_empty()
                && mark.style.start_cap == crate::features::Cap::Round
                && mark.style.end_cap == crate::features::Cap::Round
            {
                use geo::Buffer;
                geo::MultiPolygon(
                    rings
                        .iter()
                        .flat_map(|ring| {
                            if let [point] = ring.as_slice() {
                                geo::Point::new(f64::from(point.x), f64::from(point.y))
                                    .buffer(f64::from(mark.width) * 0.5)
                                    .0
                            } else {
                                geo::LineString::from(
                                    ring.iter()
                                        .map(|p| (f64::from(p.x), f64::from(p.y)))
                                        .collect::<Vec<_>>(),
                                )
                                .buffer(f64::from(mark.width) * 0.5)
                                .0
                            }
                        })
                        .collect(),
                )
            } else {
                crate::width::outline(mark)
            };
            for polygon in &outlines.0 {
                stroke_holes.push(false);
                stroke_holes.extend(polygon.interiors().iter().map(|_| true));
                stroke_rings.push(
                    polygon
                        .exterior()
                        .0
                        .iter()
                        .map(|p| Pos2::new(p.x as f32, p.y as f32))
                        .collect(),
                );
                stroke_rings.extend(polygon.interiors().iter().map(|r| {
                    r.0.iter()
                        .map(|p| Pos2::new(p.x as f32, p.y as f32))
                        .collect::<Vec<_>>()
                }));
            }
            let mut path = Path::builder();
            for ring in &stroke_rings {
                if let Some(first) = ring.first() {
                    path.begin(point(first.x, first.y));
                    for p in &ring[1..] {
                        path.line_to(point(p.x, p.y));
                    }
                    path.end(true);
                }
            }
            if !stroke_rings.is_empty() {
                valid &= FillTessellator::new()
                    .tessellate_path(
                        &path.build(),
                        &FillOptions::even_odd(),
                        &mut BuffersBuilder::new(&mut stroke_mesh, |v: FillVertex<'_>| {
                            let p = v.position();
                            [p.x, p.y]
                        }),
                    )
                    .is_ok();
            }
        }
        Self {
            stroke_mesh,
            stroke_rings,
            stroke_holes,
            _owner: mark.clone(),
            rings,
            fill,
            valid,
        }
    }
    fn bytes(&self) -> usize {
        self.rings.iter().map(|r| r.len() * 8).sum::<usize>()
            + self.fill.vertices.len() * 8
            + self.fill.indices.len() * 4
            + self.stroke_rings.iter().map(|r| r.len() * 8).sum::<usize>()
            + self.stroke_mesh.vertices.len() * 8
            + self.stroke_mesh.indices.len() * 4
            + self.stroke_holes.len()
    }
}
#[derive(Default)]
pub struct RenderCache {
    pub(crate) lit_planes: crate::exporter::LitPlanes,
    pub(crate) post_fx: crate::post::EffectCache,
    entries: HashMap<Key, (Arc<Prepared>, u64)>,
    epoch: u64,
    bytes: usize,
    #[cfg(test)]
    builds: usize,
}
impl RenderCache {
    pub fn begin_frame(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.entries
            .retain(|_, (_, last)| last.saturating_add(1) >= self.epoch);
        self.bytes = self.entries.values().map(|(p, _)| p.bytes()).sum();
    }
    fn get(&mut self, mark: &Mark, scale: f32, composited: bool) -> Arc<Prepared> {
        // Keep the outline geometry stable as an animated camera crosses zoom levels.
        let detail = if composited {
            4
        } else {
            scale.max(0.001).log2().ceil() as i32
        };
        let key = Key {
            points: mark.points.identity(),
            nodes: mark.nodes.identity(),
            holes: mark.holes.identity(),
            profile: mark.width_profile.identity(),
            caps: (mark.style.start_cap, mark.style.end_cap),
            width: if !composited
                && mark.width_profile.is_empty()
                && mark.style.start_cap == crate::features::Cap::Round
                && mark.style.end_cap == crate::features::Cap::Round
            {
                0
            } else {
                mark.width.to_bits()
            },
            stroke: mark.stroke,
            rectangle: mark.rectangle,
            closed: mark.closed,
            fill: mark.fill.is_some(),
            detail,
            composited,
        };
        if let Some((p, last)) = self.entries.get_mut(&key) {
            *last = self.epoch;
            return p.clone();
        }
        let prepared = Arc::new(Prepared::new(
            mark,
            (0.25 / 2f32.powi(detail)).clamp(0.003, 0.5),
            composited,
        ));
        #[cfg(test)]
        {
            self.builds += 1;
        }
        let bytes = prepared.bytes();
        let budget = 32 * 1024 * 1024;
        if self.bytes + bytes > budget {
            self.entries.clear();
            self.bytes = 0;
        }
        if bytes <= budget {
            self.entries.insert(key, (prepared.clone(), self.epoch));
            self.bytes += bytes;
        }
        prepared
    }
}
fn mesh(data: &VertexBuffers<[f32; 2], u32>, view: View, color: Color32) -> egui::Mesh {
    egui::Mesh {
        indices: data.indices.clone(),
        vertices: data
            .vertices
            .iter()
            .map(|p| egui::epaint::Vertex {
                pos: view.screen(vector::pos(*p)),
                uv: egui::epaint::WHITE_UV,
                color,
            })
            .collect(),
        ..Default::default()
    }
}
pub fn draw(
    p: &egui::Painter,
    mark: &Mark,
    view: View,
    ghost: bool,
    selected: bool,
    cache: Option<&mut RenderCache>,
) -> bool {
    let thick_primitive =
        mark.stroke && mark.width > 20. && (mark.rectangle || mark.closed && mark.nodes.is_empty());
    draw_prepared(p, mark, view, ghost, selected, cache, thick_primitive)
}

pub(crate) fn draw_composited(p: &egui::Painter, mark: &Mark, view: View, cache: &mut RenderCache) {
    draw_prepared(p, mark, view, false, false, Some(cache), true);
}

fn draw_prepared(
    p: &egui::Painter,
    mark: &Mark,
    view: View,
    ghost: bool,
    selected: bool,
    cache: Option<&mut RenderCache>,
    composited: bool,
) -> bool {
    let prepared = match cache {
        Some(cache) => cache.get(mark, view.scale, composited),
        None => Arc::new(Prepared::new(
            mark,
            (0.2 / view.scale).clamp(0.003, 0.5),
            composited,
        )),
    };
    let color = |rgba: [u8; 4]| {
        if ghost {
            Color32::from_rgba_unmultiplied(110, 145, 220, 65)
        } else {
            Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3])
        }
    };
    if let Some(fill) = mark.fill {
        let mut fill = color(fill);
        if selected {
            fill = fill.lerp_to_gamma(Color32::WHITE, 0.18);
        }
        if prepared.valid {
            if let Some(g) = mark.style.gradient.as_ref().filter(|_| !ghost) {
                p.add(egui::Shape::mesh(gradient_mesh(
                    &prepared.fill,
                    view,
                    g,
                    selected,
                )));
            } else {
                p.add(egui::Shape::mesh(mesh(&prepared.fill, view, fill)));
            }
        }
        let feather = p.ctx().pixels_per_point().max(0.1).recip();
        // Bucket regions include underpaint clipped to the existing ink. An outward
        // screen-space AA fringe could still poke through thin outlines.
        for (index, ring) in prepared.rings.iter().enumerate().filter(|_| !mark.bucket) {
            let mut edge = boundary_fringe(
                ring,
                index > 0,
                view,
                if prepared.valid { fill } else { Color32::RED },
                feather,
            );
            if !ghost {
                if let Some(g) = &mark.style.gradient {
                    for vertex in &mut edge.vertices {
                        let alpha = vertex.color.a() as f32 / fill.a().max(1) as f32;
                        let c = g.color(view.world(vertex.pos));
                        vertex.color = Color32::from_rgba_unmultiplied(
                            c[0],
                            c[1],
                            c[2],
                            (c[3] as f32 * alpha.clamp(0., 1.)).round() as u8,
                        );
                    }
                }
            }
            if !edge.indices.is_empty() {
                p.add(egui::Shape::mesh(edge));
            }
        }
    }
    if mark.stroke && !prepared.stroke_mesh.indices.is_empty() {
        let ink = color(mark.color);
        p.add(egui::Shape::mesh(mesh(&prepared.stroke_mesh, view, ink)));
        let feather = p.ctx().pixels_per_point().max(0.1).recip();
        for (ring, hole) in prepared.stroke_rings.iter().zip(&prepared.stroke_holes) {
            let edge = boundary_fringe(ring, *hole, view, ink, feather);
            if !edge.indices.is_empty() {
                p.add(egui::Shape::mesh(edge));
            }
        }
    }
    for ring in &prepared.rings {
        let points: Vec<_> = ring.iter().map(|p| view.screen(*p)).collect();
        if mark.stroke
            && prepared.stroke_mesh.indices.is_empty()
            && mark.width_profile.is_empty()
            && mark.style.start_cap == crate::features::Cap::Round
            && mark.style.end_cap == crate::features::Cap::Round
        {
            let ink = color(mark.color);
            let width = mark.width * view.scale;
            if points.len() == 1 {
                p.circle_filled(points[0], width / 2., ink);
            } else if points.len() > 1 {
                p.add(egui::Shape::line(points.clone(), Stroke::new(width, ink)));
                if !mark.closed && !mark.rectangle && points.first() != points.last() {
                    p.circle_filled(points[0], width / 2., ink);
                    p.circle_filled(*points.last().unwrap(), width / 2., ink);
                }
            }
        }
        if selected {
            let mut carry = 0.;
            let mut count = 0;
            for pair in points.windows(2) {
                let length = pair[0].distance(pair[1]);
                if length < 0.001 {
                    continue;
                }
                let direction = (pair[1] - pair[0]) / length;
                let mut at = carry;
                while at < length && count < 4096 {
                    p.circle_filled(
                        pair[0] + direction * at,
                        0.8,
                        Color32::from_rgb(110, 190, 255),
                    );
                    at += 4.;
                    count += 1;
                }
                carry = at - length;
            }
        }
    }
    prepared.valid
}

pub(crate) fn selection_pattern(
    painter: &egui::Painter,
    mark: &Mark,
    view: View,
    cache: &mut RenderCache,
) {
    let prepared = cache.get(mark, view.scale, true);
    let rings = if mark.fill.is_some() {
        &prepared.rings
    } else {
        &prepared.stroke_rings
    };
    let rings: Vec<Vec<Pos2>> = rings
        .iter()
        .map(|ring| ring.iter().map(|&p| view.screen(p)).collect())
        .collect();
    let clip = painter.clip_rect();
    let bounds = rings
        .iter()
        .flatten()
        .fold(egui::Rect::NOTHING, |mut bounds, &point| {
            bounds.extend_with(point);
            bounds
        })
        .intersect(clip);
    if !bounds.is_positive() || !bounds.is_finite() {
        return;
    }
    let step = ((bounds.area() / 12000.).sqrt().ceil() as i32).max(4);
    let mut crossings = Vec::new();
    let mut row = (bounds.top().ceil() as i32).div_euclid(step) * step;
    while row < bounds.bottom().ceil() as i32 {
        let y = row as f32;
        crossings.clear();
        for ring in &rings {
            for (&a, &b) in ring.iter().zip(ring.iter().cycle().skip(1)) {
                if (a.y > y) != (b.y > y) {
                    crossings.push(a.x + (b.x - a.x) * ((y - a.y) / (b.y - a.y)));
                }
            }
        }
        crossings.sort_unstable_by(f32::total_cmp);
        for interval in crossings.chunks_exact(2) {
            let left = interval[0].max(bounds.left());
            let right = interval[1].min(bounds.right());
            let mut column = (left / step as f32).ceil() as i32 * step;
            while (column as f32) < right {
                let point = Pos2::new(column as f32, y);
                painter.circle_filled(point, 1.05, Color32::from_black_alpha(195));
                painter.circle_filled(point, 0.62, Color32::from_white_alpha(225));
                column += step;
            }
        }
        row += step;
    }
    if mark.stroke && mark.fill.is_none() {
        let mut carry = 0.;
        for pair in prepared.rings[0].windows(2) {
            let (a, b) = (view.screen(pair[0]), view.screen(pair[1]));
            let delta = b - a;
            let length = delta.length();
            if length <= 0.001 {
                continue;
            }
            if egui::Rect::from_two_pos(a, b).intersects(bounds) {
                let clipped = (bounds.center() - a).dot(delta / length) - bounds.size().length();
                let mut at = carry + ((clipped - carry) / 4.).ceil().max(0.) * 4.;
                let end = ((bounds.center() - a).dot(delta / length) + bounds.size().length())
                    .min(length);
                while at < end {
                    let point = a + delta * (at / length);
                    painter.circle_filled(point, 1.05, Color32::from_black_alpha(195));
                    painter.circle_filled(point, 0.62, Color32::from_white_alpha(225));
                    at += 4.;
                }
            }
            carry = (carry - length).rem_euclid(4.);
        }
    }
}

fn gradient_mesh(
    data: &VertexBuffers<[f32; 2], u32>,
    view: View,
    g: &crate::features::Gradient,
    selected: bool,
) -> egui::Mesh {
    let mut mesh = egui::Mesh::default();
    let mut triangle = |points: [Pos2; 3]| {
        let first = mesh.vertices.len() as u32;
        for p in points {
            let c = g.color(p);
            let mut color = Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
            if selected {
                color = color.lerp_to_gamma(Color32::WHITE, 0.18);
            }
            mesh.vertices.push(egui::epaint::Vertex {
                pos: view.screen(p),
                uv: egui::epaint::WHITE_UV,
                color,
            });
        }
        mesh.indices.extend([first, first + 1, first + 2]);
    };
    for indices in data.indices.chunks_exact(3) {
        let points = [
            vector::pos(data.vertices[indices[0] as usize]),
            vector::pos(data.vertices[indices[1] as usize]),
            vector::pos(data.vertices[indices[2] as usize]),
        ];
        if g.radial {
            let n = ((points[0]
                .distance(points[1])
                .max(points[1].distance(points[2]))
                .max(points[2].distance(points[0]))
                * view.scale
                / 12.)
                .ceil() as usize)
                .clamp(1, 32);
            let at = |i: usize, j: usize| {
                points[0]
                    + (points[1] - points[0]) * (i as f32 / n as f32)
                    + (points[2] - points[0]) * (j as f32 / n as f32)
            };
            for i in 0..n {
                for j in 0..n - i {
                    triangle([at(i, j), at(i + 1, j), at(i, j + 1)]);
                    if i + j + 1 < n {
                        triangle([at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)]);
                    }
                }
            }
        } else {
            let origin = vector::pos(g.start);
            let direction = vector::pos(g.end) - origin;
            let project =
                |p: Pos2| (p - origin).dot(direction) / direction.length_sq().max(0.00001);
            let mut cuts = vec![-f32::MAX];
            cuts.extend(g.stops.iter().map(|s| s.t));
            cuts.push(f32::MAX);
            for band in cuts.windows(2) {
                let mut polygon = points.to_vec();
                for (lower, limit) in [(true, band[0]), (false, band[1])] {
                    if polygon.is_empty() {
                        break;
                    }
                    let mut clipped = vec![];
                    for i in 0..polygon.len() {
                        let a = polygon[(i + polygon.len() - 1) % polygon.len()];
                        let b = polygon[i];
                        let da = project(a) - limit;
                        let db = project(b) - limit;
                        let inside = |d: f32| if lower { d >= 0. } else { d <= 0. };
                        if inside(da) != inside(db) {
                            clipped.push(a + (b - a) * (da / (da - db)));
                        }
                        if inside(db) {
                            clipped.push(b);
                        }
                    }
                    polygon = clipped;
                }
                for i in 1..polygon.len().saturating_sub(1) {
                    triangle([polygon[0], polygon[i], polygon[i + 1]]);
                }
            }
        }
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concave_fills_and_holes_tessellate_and_cache_survives_color_change() {
        let mut mark = Mark {
            points: vec![[0., 0.], [100., 0.], [100., 100.], [50., 60.], [0., 100.]].into(),
            fill: Some([0, 0, 0, 255]),
            stroke: false,
            closed: true,
            ..Default::default()
        };
        mark.holes = vec![vec![[10., 10.], [20., 10.], [20., 20.], [10., 20.]].into()].into();
        let mut cache = RenderCache::default();
        cache.begin_frame();
        let first = cache.get(&mark, 1., false);
        assert!(first.valid);
        assert!(!first.fill.indices.is_empty());
        mark.fill = Some([255, 0, 0, 255]);
        let second = cache.get(&mark, 1., false);
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(cache.builds, 1);
        mark.points[0] = [-10., 0.];
        let third = cache.get(&mark, 1., false);
        assert!(!Arc::ptr_eq(&first, &third));
        assert_eq!(first.rings[0][0], Pos2::ZERO);
    }
}

#[cfg(test)]
mod tip_regressions {
    use super::*;
    #[test]
    fn tapered_render_vertices_stay_within_the_actual_ink_boundary() {
        let ctx = egui::Context::default();
        for profile in [
            crate::width::Preset::Tapered.stops(),
            crate::width::Preset::Start.stops(),
            crate::width::Preset::End.stops(),
            vec![
                crate::width::WidthStop {
                    t: 0.,
                    width: 0.000001,
                },
                crate::width::WidthStop {
                    t: 0.35,
                    width: 1.5,
                },
                crate::width::WidthStop { t: 1., width: 0. },
            ]
            .into(),
        ] {
            for scale in [0.2, 1., 4., 12.] {
                let mark = Mark {
                    points: vec![[0., 0.], [300., 0.]].into(),
                    width: 20.,
                    width_profile: profile.clone(),
                    ..Default::default()
                };
                let view = View {
                    center: Pos2::new(200., 300.),
                    stage_center: Pos2::ZERO,
                    scale,
                    angle: 0.37,
                };
                let prepared = Prepared::new(&mark, 0.04, false);
                assert!(prepared.valid);
                let mut bounds = egui::Rect::NOTHING;
                for ring in &prepared.stroke_rings {
                    for point in ring {
                        bounds.extend_with(view.screen(*point));
                    }
                }
                let bounds = bounds.expand(2.);
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            Pos2::ZERO,
                            egui::Vec2::new(5000., 2000.),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        assert!(draw(ui.painter(), &mark, view, false, false, None));
                    },
                );
                let shapes = std::mem::take(&mut output.shapes);
                output.textures_delta.clear();
                let primitives = ctx.tessellate(shapes, output.pixels_per_point);
                let mut count = 0;
                for primitive in primitives {
                    if let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive {
                        for vertex in mesh.vertices {
                            count += 1;
                            assert!(
                                vertex.pos.is_finite() && bounds.contains(vertex.pos),
                                "spike at {:?}, ink bounds {:?}, scale={scale}",
                                vertex.pos,
                                bounds
                            );
                        }
                    }
                }
                assert!(count > 0);
            }
        }
    }
}

/// Antialias a filled boundary without stroking it. A conventional closed-line
/// stroke creates unbounded miters at a zero-width cusp (the tapered-tip spikes).
/// These feather vertices can never extend more than 1.5 physical pixels.
fn boundary_fringe(
    ring: &[Pos2],
    hole: bool,
    view: View,
    color: Color32,
    feather: f32,
) -> egui::Mesh {
    let mut points: Vec<Pos2> = Vec::with_capacity(ring.len());
    for point in ring {
        let point = view.screen(*point);
        if points
            .last()
            .is_none_or(|last| last.distance_sq(point) > 1e-8)
        {
            points.push(point);
        }
    }
    if points.len() > 1 && points[0].distance_sq(*points.last().unwrap()) <= 1e-8 {
        points.pop();
    }
    if points.len() < 3 || color.a() == 0 {
        return egui::Mesh::default();
    }
    let n = points.len();
    let area = (0..n)
        .map(|i| {
            let a = points[i];
            let b = points[(i + 1) % n];
            a.x as f64 * b.y as f64 - b.x as f64 * a.y as f64
        })
        .sum::<f64>();
    if area.abs() < 1e-9 {
        return egui::Mesh::default();
    }
    let side = if (area > 0.) ^ hole { 1. } else { -1. };
    let normals: Vec<_> = (0..n)
        .map(|i| {
            let delta = points[(i + 1) % n] - points[i];
            eframe::egui::Vec2::new(delta.y, -delta.x).normalized() * side
        })
        .collect();
    let mut mesh = egui::Mesh::default();
    mesh.vertices.reserve(n * 2);
    mesh.indices.reserve(n * 6);
    for i in 0..n {
        let before = normals[(i + n - 1) % n];
        let after = normals[i];
        let sum = before + after;
        let mut normal = sum.normalized();
        if normal == eframe::egui::Vec2::ZERO {
            normal = after;
        }
        // Bevel-limit very sharp joins rather than letting a miter shoot out
        // along the tangent. The inner vertex stays exactly on the ink edge.
        let distance = (feather / normal.dot(after).abs().max(1e-6)).min(feather * 1.5);
        for (pos, color) in [
            (points[i], color),
            (points[i] + normal * distance, Color32::TRANSPARENT),
        ] {
            mesh.vertices.push(egui::epaint::Vertex {
                pos,
                uv: egui::epaint::WHITE_UV,
                color,
            });
        }
    }
    for i in 0..n {
        let a = (i * 2) as u32;
        let b = (((i + 1) % n) * 2) as u32;
        mesh.indices
            .extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
    }
    mesh
}

#[cfg(test)]
mod fringe_regressions {
    use super::*;
    #[test]
    fn cusps_stay_bounded_with_holes_reversed_winding_and_dpi_scaling() {
        let view = View {
            center: Pos2::ZERO,
            stage_center: Pos2::ZERO,
            scale: 1.,
            angle: 0.,
        };
        let ring = vec![
            Pos2::new(0., 0.),
            Pos2::new(100., 0.),
            Pos2::new(100., 100.),
            Pos2::new(0., 100.),
            Pos2::new(0., 0.),
        ];
        for reversed in [false, true] {
            for hole in [false, true] {
                for dpi in [0.75, 1., 2., 3.] {
                    let mut points = ring.clone();
                    if reversed {
                        points.reverse();
                    }
                    let mesh = boundary_fringe(&points, hole, view, Color32::WHITE, 1. / dpi);
                    for pair in mesh.vertices.chunks_exact(2) {
                        assert!(pair[0].pos.distance(pair[1].pos) * dpi <= 1.5001);
                    }
                    let corner = mesh
                        .vertices
                        .chunks_exact(2)
                        .find(|p| p[0].pos == Pos2::ZERO)
                        .unwrap()[1]
                        .pos;
                    if hole {
                        assert!(corner.x > 0. && corner.y > 0.);
                    } else {
                        assert!(corner.x < 0. && corner.y < 0.);
                    }
                }
            }
        }
        let cusp = [
            Pos2::new(0., 0.),
            Pos2::new(500., 0.001),
            Pos2::new(600., 0.),
            Pos2::new(500., -0.001),
            Pos2::new(0., 0.),
        ];
        let mesh = boundary_fringe(&cusp, false, view, Color32::WHITE, 1.);
        for pair in mesh.vertices.chunks_exact(2) {
            assert!(pair[0].pos.distance(pair[1].pos) <= 1.5001);
        }
    }
    #[test]
    fn curved_width_edits_and_exported_fill_silhouettes_have_no_render_spikes() {
        let ctx = egui::Context::default();
        let mut style = Mark {
            width: 24.,
            ..Default::default()
        };
        style.width_profile = vec![
            crate::width::WidthStop { t: 0., width: 0. },
            crate::width::WidthStop { t: 0.3, width: 1. },
            crate::width::WidthStop { t: 0.5, width: 0. },
            crate::width::WidthStop { t: 0.7, width: 1.5 },
            crate::width::WidthStop { t: 1., width: 0. },
        ]
        .into();
        let curved = crate::vector::from_cubics(
            &[
                crate::vector::Cubic([
                    Pos2::new(0., 0.),
                    Pos2::new(70., -80.),
                    Pos2::new(90., 100.),
                    Pos2::new(150., 0.),
                ]),
                crate::vector::Cubic([
                    Pos2::new(150., 0.),
                    Pos2::new(200., -80.),
                    Pos2::new(270., 80.),
                    Pos2::new(300., 0.),
                ]),
            ],
            &style,
        );
        let mut cases = vec![curved.clone()];
        cases.extend(crate::width::export_marks(&curved));
        for mark in cases {
            for scale in [0.25, 1., 8.] {
                let view = View {
                    center: Pos2::new(200., 500.),
                    stage_center: Pos2::ZERO,
                    scale,
                    angle: -0.2,
                };
                let prepared = Prepared::new(&mark, 0.04, false);
                assert!(prepared.valid);
                let rings = if mark.stroke {
                    &prepared.stroke_rings
                } else {
                    &prepared.rings
                };
                let mut bounds = egui::Rect::NOTHING;
                for ring in rings {
                    for point in ring {
                        bounds.extend_with(view.screen(*point));
                    }
                }
                let bounds = bounds.expand(2.);
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            Pos2::ZERO,
                            egui::Vec2::new(4000., 2000.),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        assert!(draw(ui.painter(), &mark, view, false, false, None));
                    },
                );
                let shapes = std::mem::take(&mut output.shapes);
                output.textures_delta.clear();
                for primitive in ctx.tessellate(shapes, output.pixels_per_point) {
                    if let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive {
                        for vertex in mesh.vertices {
                            assert!(
                                vertex.pos.is_finite() && bounds.contains(vertex.pos),
                                "spike {:?} outside {:?}",
                                vertex.pos,
                                bounds
                            );
                        }
                    }
                }
            }
        }
    }
    #[test]
    #[ignore = "manual release benchmark"]
    fn benchmark_tip_antialiasing() {
        let ctx = egui::Context::default();
        let mut init = ctx.run_ui(Default::default(), |_| {});
        init.textures_delta.clear();
        let raw: Vec<_> = (0..2000)
            .map(|i| {
                let x = i as f32 * 0.35;
                [x, (x * 0.02).sin() * 60.]
            })
            .collect();
        let mut mark = crate::vector::from_cubics(
            &crate::vector::fit(&raw, 1.5),
            &Mark {
                width: 20.,
                ..Default::default()
            },
        );
        mark.width_profile = crate::width::Preset::Tapered.stops();
        let prepared = Prepared::new(&mark, 0.1, false);
        let view = View {
            center: Pos2::new(200., 200.),
            stage_center: Pos2::ZERO,
            scale: 1.,
            angle: 0.,
        };
        let clip_rect = egui::Rect::EVERYTHING;
        let start = std::time::Instant::now();
        for _ in 0..200 {
            let shapes = prepared
                .stroke_rings
                .iter()
                .map(|ring| egui::epaint::ClippedShape {
                    transform_after_tessellation: egui::emath::TSTransform::IDENTITY,
                    clip_rect,
                    shape: egui::Shape::closed_line(
                        ring.iter().map(|p| view.screen(*p)).collect(),
                        Stroke::new(0.6, Color32::WHITE),
                    ),
                })
                .collect();
            std::hint::black_box(ctx.tessellate(shapes, 1.));
        }
        let previous = start.elapsed() / 200;
        let start = std::time::Instant::now();
        for _ in 0..200 {
            let shapes = prepared
                .stroke_rings
                .iter()
                .zip(&prepared.stroke_holes)
                .map(|(ring, hole)| egui::epaint::ClippedShape {
                    transform_after_tessellation: egui::emath::TSTransform::IDENTITY,
                    clip_rect,
                    shape: egui::Shape::mesh(boundary_fringe(
                        ring,
                        *hole,
                        view,
                        Color32::WHITE,
                        1.,
                    )),
                })
                .collect();
            std::hint::black_box(ctx.tessellate(shapes, 1.));
        }
        eprintln!(
            "tip edge, 200 runs: previous={previous:?}/frame, bounded={:?}/frame",
            start.elapsed() / 200
        );
    }
}

/// Grayscale weight visualization on a subdivided vector fill. No raster mask.
pub fn weight_overlay(
    p: &egui::Painter,
    mark: &Mark,
    view: View,
    map: impl Fn(Pos2) -> (Pos2, f32),
) {
    let prepared = Prepared::new(mark, (0.2 / view.scale).clamp(0.003, 0.5), false);
    let mut mesh = egui::Mesh::default();
    for triangle in prepared.fill.indices.chunks_exact(3) {
        let a = vector::pos(prepared.fill.vertices[triangle[0] as usize]);
        let b = vector::pos(prepared.fill.vertices[triangle[1] as usize]);
        let c = vector::pos(prepared.fill.vertices[triangle[2] as usize]);
        let n = (a.distance(b).max(b.distance(c)).max(c.distance(a)) / 16.)
            .ceil()
            .clamp(1., 32.) as usize;
        for row in 0..n {
            for col in 0..n - row {
                let at = |i: usize, j: usize| {
                    a + (b - a) * (i as f32 / n as f32) + (c - a) * (j as f32 / n as f32)
                };
                let mut add = |points: [Pos2; 3]| {
                    let base = mesh.vertices.len() as u32;
                    for q in points {
                        let (q, w) = map(q);
                        mesh.colored_vertex(
                            view.screen(q),
                            Color32::from_gray((w.clamp(0., 1.) * 255.) as u8),
                        );
                    }
                    mesh.add_triangle(base, base + 1, base + 2);
                };
                add([at(row, col), at(row + 1, col), at(row, col + 1)]);
                if row + col + 1 < n {
                    add([at(row + 1, col), at(row + 1, col + 1), at(row, col + 1)]);
                }
            }
        }
    }
    p.add(mesh);
}

#[cfg(test)]
mod bucket_edge_regressions {
    use super::*;
    #[test]
    fn bucket_render_vertices_do_not_expand_outside_the_fill_face() {
        let ctx = egui::Context::default();
        let contour = Mark {
            points: vec![[0., 0.], [200., 200.]].into(),
            rectangle: true,
            closed: true,
            width: 0.5,
            ..Default::default()
        };
        let mark = crate::bucket::fill(
            &[contour],
            Pos2::new(100., 100.),
            crate::bucket::Gap::None,
            [200, 20, 20, 255],
        )
        .unwrap()
        .remove(0);
        let world_bounds = vector::control_bounds(&mark).expand(0.001);
        for scale in [0.2, 1., 8.] {
            let view = View {
                center: Pos2::new(100., 100.),
                stage_center: Pos2::ZERO,
                scale,
                angle: 0.4,
            };
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                assert!(draw(ui.painter(), &mark, view, false, false, None));
            });
            let shapes = std::mem::take(&mut output.shapes);
            output.textures_delta.clear();
            let primitives = ctx.tessellate(shapes, output.pixels_per_point);
            let mut count = 0;
            for primitive in primitives {
                if let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive {
                    for vertex in mesh.vertices {
                        count += 1;
                        assert!(world_bounds.contains(view.world(vertex.pos)));
                    }
                }
            }
            assert!(count > 0);
        }
    }
}
