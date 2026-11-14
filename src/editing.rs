//! Geometry for area selections, erasing and drawing-mode fill composition.
use crate::{
    document::{Mark, Node},
    vector::{self, Cubic},
};
use eframe::egui::{Pos2, Rect, Vec2};
use geo::{BooleanOps, BoundingRect, Buffer, Coord, LineString, MultiPolygon, Polygon};

fn ring(points: &[Pos2]) -> LineString<f64> {
    let mut coords: Vec<_> = points
        .iter()
        .map(|p| Coord {
            x: p.x as f64,
            y: p.y as f64,
        })
        .collect();
    if coords.first() != coords.last() {
        if let Some(p) = coords.first().copied() {
            coords.push(p);
        }
    }
    LineString(coords)
}
pub fn area(points: &[Pos2]) -> Polygon<f64> {
    Polygon::new(ring(points), vec![])
}
fn cross(a: Vec2, b: Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}
fn intersection(a: Pos2, b: Pos2, c: Pos2, d: Pos2) -> Option<(f32, f32)> {
    let ab = b - a;
    let cd = d - c;
    let det = cross(ab, cd);
    if det.abs() < 1e-9 {
        return None;
    }
    let t = cross(c - a, cd) / det;
    let u = cross(c - a, ab) / det;
    ((0. ..=1.).contains(&t) && (0. ..=1.).contains(&u)).then_some((t, u))
}
fn curve_bounds(c: Cubic) -> Rect {
    let mut r = Rect::NOTHING;
    for p in c.0 {
        r.extend_with(p);
    }
    r
}
fn samples(c: Cubic, a: f32, b: f32, depth: u32, out: &mut Vec<(f32, Pos2)>) {
    let part = c.sub(a, b);
    let chord = part.0[3] - part.0[0];
    let distance = |p: Pos2| {
        let t = ((p - part.0[0]).dot(chord) / chord.length_sq().max(1e-12)).clamp(0., 1.);
        p.distance(part.0[0] + chord * t)
    };
    if depth >= 16 || distance(part.0[1]).max(distance(part.0[2])) < 0.025 {
        out.push((b, c.at(b)));
    } else {
        let m = (a + b) * 0.5;
        samples(c, a, m, depth + 1, out);
        samples(c, m, b, depth + 1, out);
    }
}
fn sampled(c: Cubic) -> Vec<(f32, Pos2)> {
    let mut v = vec![(0., c.0[0])];
    samples(c, 0., 1., 0, &mut v);
    v
}
fn derivative(c: Cubic, t: f32) -> Vec2 {
    let [a, b, d, e] = c.0;
    (b - a) * (3. * (1. - t).powi(2)) + (d - b) * (6. * (1. - t) * t) + (e - d) * (3. * t * t)
}
fn curve_hits(a: Cubic, b: Cubic) -> Vec<(f32, f32)> {
    if !curve_bounds(a).expand(0.001).intersects(curve_bounds(b)) {
        return vec![];
    }
    let sa = sampled(a);
    let sb = sampled(b);
    let mut hits = vec![];
    for aw in sa.windows(2) {
        for bw in sb.windows(2) {
            if let Some((x, y)) = intersection(aw[0].1, aw[1].1, bw[0].1, bw[1].1) {
                let mut t = aw[0].0 + (aw[1].0 - aw[0].0) * x;
                let mut u = bw[0].0 + (bw[1].0 - bw[0].0) * y;
                for _ in 0..8 {
                    let delta = b.at(u) - a.at(t);
                    let da = derivative(a, t);
                    let db = derivative(b, u);
                    let det = cross(da, db);
                    if det.abs() < 1e-9 {
                        if da.length_sq() > 1e-12 {
                            t = (t + delta.dot(da) / da.length_sq()).clamp(0., 1.);
                        }
                        if db.length_sq() > 1e-12 {
                            u = (u - delta.dot(db) / db.length_sq()).clamp(0., 1.);
                        }
                        continue;
                    }
                    t = (t + cross(delta, db) / det).clamp(0., 1.);
                    u = (u + cross(delta, da) / det).clamp(0., 1.);
                }
                if a.at(t).distance(b.at(u)) < 0.05 {
                    hits.push((t, u));
                }
            }
        }
    }
    hits
}
fn line(a: Pos2, b: Pos2) -> Cubic {
    Cubic([a, a.lerp(b, 1. / 3.), a.lerp(b, 2. / 3.), b])
}
fn parts(
    mark: &Mark,
    cuts: &[Vec<f32>],
    classify: impl Fn(Pos2) -> bool,
    break_every: bool,
) -> (Vec<Mark>, Vec<Mark>) {
    let curves = vector::cubics(mark);
    let mut groups: Vec<(bool, Vec<Cubic>)> = vec![];
    for (i, curve) in curves.iter().enumerate() {
        let mut ts = cuts[i].clone();
        ts.extend([0., 1.]);
        ts.sort_by(f32::total_cmp);
        ts.dedup_by(|a, b| (*a - *b).abs() < 1e-5);
        for w in ts.windows(2) {
            if w[1] - w[0] < 1e-5 {
                continue;
            }
            let inside = classify(curve.at((w[0] + w[1]) * 0.5));
            let segment = curve.sub(w[0], w[1]);
            let boundary_cut = cuts[i].iter().any(|t| *t < 1e-5)
                || (i > 0 && cuts[i - 1].iter().any(|t| *t > 1. - 1e-5));
            if !break_every || (w[0] < 1e-5 && !boundary_cut) {
                if let Some((kind, list)) = groups.last_mut() {
                    if *kind == inside {
                        list.push(segment);
                        continue;
                    }
                }
            }
            groups.push((inside, vec![segment]));
        }
    }
    let mut outside = vec![];
    let mut inside = vec![];
    for (selected, curves) in groups {
        let mut m = vector::from_cubics(&curves, mark);
        m.fill = None;
        crate::width::remap(mark, &mut m);
        if selected {
            inside.push(m);
        } else {
            outside.push(m);
        }
    }
    if curves.is_empty() {
        if let Some(p) = mark.points.first() {
            if classify(vector::pos(*p)) {
                inside.push(mark.clone())
            } else {
                outside.push(mark.clone())
            }
        }
    }
    (outside, inside)
}
/// Cut area with polygonal fill booleans and retain exact cubic stroke sections.
pub fn split_area(mark: &Mark, cutter: &Polygon<f64>) -> (Vec<Mark>, Vec<Mark>) {
    let Some(b) = cutter.bounding_rect() else {
        return (vec![mark.clone()], vec![]);
    };
    let bounds = Rect::from_min_max(
        Pos2::new(b.min().x as f32, b.min().y as f32),
        Pos2::new(b.max().x as f32, b.max().y as f32),
    );
    if !vector::control_bounds(mark).intersects(bounds) {
        return (vec![mark.clone()], vec![]);
    }
    if mark.style.instance.is_some() || mark.style.text.is_some() {
        return (vec![], vec![mark.clone()]);
    }
    let mut outside = vec![];
    let mut inside = vec![];
    if let Some(color) = mark.fill {
        let poly = vector::polygon(mark);
        let selected = poly.intersection(cutter);
        if selected.0.is_empty() && !mark.stroke {
            return (vec![mark.clone()], vec![]);
        }
        let rest = poly.difference(cutter);
        if rest.0.is_empty() && !mark.stroke {
            return (vec![], vec![mark.clone()]);
        }
        outside.extend(crate::features::styled(rest, color, &mark.style));
        inside.extend(crate::features::styled(selected, color, &mark.style));
    }
    if mark.stroke {
        let curves = vector::cubics(mark);
        let mut cuts = vec![vec![]; curves.len()];
        for (i, c) in curves.iter().enumerate() {
            for ring in std::iter::once(cutter.exterior()).chain(cutter.interiors().iter()) {
                for edge in ring.0.windows(2) {
                    let a = Pos2::new(edge[0].x as f32, edge[0].y as f32);
                    let b = Pos2::new(edge[1].x as f32, edge[1].y as f32);
                    cuts[i].extend(curve_hits(*c, line(a, b)).into_iter().map(|h| h.0));
                }
            }
        }
        let contains = |p: Pos2| {
            use geo::Contains;
            cutter.contains(&geo::Point::new(p.x as f64, p.y as f64))
        };
        let (a, b) = parts(mark, &cuts, contains, false);
        outside.extend(a);
        inside.extend(b);
    }
    (outside, inside)
}
/// Split a clicked centerline at intersections, preserving handles and widths.
pub fn crossing_parts(mark: &Mark, others: &[Mark]) -> Vec<Mark> {
    if !mark.stroke {
        return vec![mark.clone()];
    }
    let curves = vector::cubics(mark);
    let mut cuts = vec![vec![]; curves.len()];
    let bounds = vector::control_bounds(mark);
    for other in others {
        if !bounds.intersects(vector::control_bounds(other)) {
            continue;
        }
        for c in vector::cubics(other) {
            for (i, a) in curves.iter().enumerate() {
                cuts[i].extend(curve_hits(*a, c).into_iter().map(|p| p.0));
            }
        }
    }
    for i in 0..curves.len() {
        for j in i + 1..curves.len() {
            for (t, u) in curve_hits(curves[i], curves[j]) {
                if t > 1e-5 && t < 1. - 1e-5 {
                    cuts[i].push(t);
                }
                if u > 1e-5 && u < 1. - 1e-5 {
                    cuts[j].push(u);
                }
            }
        }
    }
    if cuts.iter().all(Vec::is_empty) {
        return vec![mark.clone()];
    }
    parts(mark, &cuts, |_| false, true).0
}
/// New paint replaces covered fills; matching colors become a single region.
fn compatible_paint_style(
    new: &crate::features::MarkStyle,
    old: &crate::features::MarkStyle,
) -> bool {
    if new == old {
        return true;
    }
    let mut old = old.clone();
    old.palette_fill = old.palette_fill.or(new.palette_fill);
    old.palette_stroke = old.palette_stroke.or(new.palette_stroke);
    new == &old
}

pub fn compose(existing: &[Mark], incoming: Vec<Mark>) -> Vec<Mark> {
    let mut result = existing.to_vec();
    for new in incoming {
        if new.style.text.is_some() {
            result.push(new);
            continue;
        }
        let Some(color) = new.fill else {
            result.push(new);
            continue;
        };
        let mut region = MultiPolygon(vec![vector::polygon(&new)]);
        let mut next = vec![];
        let mut touched = false;
        let mut first_outline = None;
        let paint_bounds = vector::control_bounds(&new);
        for old in &result {
            if old.style.instance.is_some() || old.style.text.is_some() {
                next.push(old.clone());
                continue;
            }
            let Some(old_color) = old.fill else {
                if !new.stroke
                    && old.stroke
                    && vector::control_bounds(old)
                        .expand(old.width)
                        .intersects(paint_bounds)
                {
                    first_outline.get_or_insert(next.len());
                }
                next.push(old.clone());
                continue;
            };
            if !vector::control_bounds(old).intersects(vector::control_bounds(&new)) {
                next.push(old.clone());
                continue;
            }
            let poly = MultiPolygon(vec![vector::polygon(old)]);
            if poly.intersection(&region).0.is_empty() {
                next.push(old.clone());
                continue;
            }
            touched = true;
            if !new.style.primitive
                && color == old_color
                && compatible_paint_style(&new.style, &old.style)
                && new.bucket == old.bucket
            {
                region = region.union(&poly);
            } else {
                next.extend(
                    crate::features::styled(poly.difference(&region), old_color, &old.style)
                        .into_iter()
                        .map(|mut part| {
                            part.bucket = old.bucket;
                            part
                        }),
                );
            }
            if old.stroke {
                if !new.stroke {
                    first_outline.get_or_insert(next.len());
                }
                let mut contour = old.clone();
                contour.fill = None;
                next.push(contour);
            }
        }
        let mut paint = Vec::new();
        if !touched || new.style.primitive {
            paint.push(new);
        } else {
            paint.extend(
                crate::features::styled(region, color, &new.style)
                    .into_iter()
                    .map(|mut part| {
                        part.bucket = new.bucket;
                        part
                    }),
            );
            if new.stroke {
                let mut contour = new;
                contour.fill = None;
                paint.push(contour);
            }
        }
        let position = first_outline.unwrap_or(next.len());
        next.splice(position..position, paint);
        result = next;
    }
    result
}
pub fn eraser_area(a: Pos2, b: Pos2, radius: f32) -> MultiPolygon<f64> {
    if a.distance(b) < 1e-6 {
        return geo::Point::new(a.x as f64, a.y as f64).buffer(radius as f64);
    }
    LineString::from(vec![(a.x as f64, a.y as f64), (b.x as f64, b.y as f64)]).buffer(radius as f64)
}
pub fn shape(
    tool: crate::canvas::Tool,
    a: Pos2,
    b: Pos2,
    sides: usize,
    star: bool,
    ratio: f32,
    style: &Mark,
) -> Mark {
    use crate::canvas::Tool;
    let mut m = style.clone();
    m.points.clear();
    m.nodes.clear();
    m.rectangle = false;
    if tool == Tool::Line {
        m.nodes = vec![Node::corner(vector::xy(a)), Node::corner(vector::xy(b))].into();
        m.fill = None;
        m.closed = false;
        return m;
    }
    let r = Rect::from_two_pos(a, b);
    let center = r.center();
    m.closed = true;
    if tool == Tool::Ellipse {
        let k = 0.5522848;
        let rx = r.width() * 0.5;
        let ry = r.height() * 0.5;
        m.nodes = vec![
            Node {
                position: vector::xy(center + Vec2::new(rx, 0.)),
                in_handle: vector::xy(center + Vec2::new(rx, -k * ry)),
                out_handle: vector::xy(center + Vec2::new(rx, k * ry)),
            },
            Node {
                position: vector::xy(center + Vec2::new(0., ry)),
                in_handle: vector::xy(center + Vec2::new(k * rx, ry)),
                out_handle: vector::xy(center + Vec2::new(-k * rx, ry)),
            },
            Node {
                position: vector::xy(center + Vec2::new(-rx, 0.)),
                in_handle: vector::xy(center + Vec2::new(-rx, k * ry)),
                out_handle: vector::xy(center + Vec2::new(-rx, -k * ry)),
            },
            Node {
                position: vector::xy(center + Vec2::new(0., -ry)),
                in_handle: vector::xy(center + Vec2::new(-k * rx, -ry)),
                out_handle: vector::xy(center + Vec2::new(k * rx, -ry)),
            },
        ]
        .into();
    } else {
        let n = sides.clamp(3, 64) * if star { 2 } else { 1 };
        m.nodes = (0..n)
            .map(|i| {
                let angle =
                    -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::TAU / n as f32;
                let scale = if star && i % 2 == 1 { ratio } else { 1. };
                Node::corner(vector::xy(
                    center
                        + Vec2::new(
                            angle.cos() * r.width() * 0.5,
                            angle.sin() * r.height() * 0.5,
                        ) * scale,
                ))
            })
            .collect();
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::Area;
    fn rect(x: f32, color: [u8; 4]) -> Mark {
        Mark {
            points: vec![[x, 0.], [x + 100., 100.]].into(),
            rectangle: true,
            closed: true,
            fill: Some(color),
            stroke: false,
            ..Default::default()
        }
    }
    #[test]
    fn fusion_replaces_covered_paint_and_unifies_matching_colors() {
        let red = [255, 0, 0, 255];
        let blue = [0, 0, 255, 255];
        let marks = compose(&[rect(0., red)], vec![rect(50., red)]);
        assert_eq!(marks.len(), 1);
        assert!((vector::polygon(&marks[0]).unsigned_area() - 15000.).abs() < 0.1);
        let marks = compose(&[rect(0., red)], vec![rect(50., blue)]);
        assert!(
            (marks
                .iter()
                .filter(|m| m.fill == Some(red))
                .map(|m| vector::polygon(m).unsigned_area())
                .sum::<f64>()
                - 5000.)
                .abs()
                < 0.1
        );
    }
    #[test]
    fn hand_painted_fill_keeps_crossed_line_art_above_it() {
        let outline = Mark {
            points: vec![[20., -10.], [20., 110.]].into(),
            color: [0, 0, 0, 255],
            ..Default::default()
        };
        let fill = rect(0., [40, 100, 200, 255]);
        let marks = compose(&[outline.clone()], vec![fill.clone()]);
        assert!(marks[0] == fill);
        assert!(marks[1] == outline);
    }
    #[test]
    fn overlapping_primitive_keeps_its_fill_and_closed_stroke_in_one_mark() {
        let mut shape = rect(50., [30, 50, 70, 255]);
        shape.stroke = true;
        shape.style.primitive = true;
        let marks = compose(&[rect(0., [30, 50, 70, 255])], vec![shape.clone()]);
        let last = marks.last().unwrap();
        assert_eq!(last.points, shape.points);
        assert_eq!(last.fill, shape.fill);
        assert!(last.stroke && last.style.primitive);
    }
    #[test]
    fn polygon_selection_preserves_area_and_exact_curve_endpoints() {
        let m = rect(0., [1, 2, 3, 255]);
        let cutter = area(&[Pos2::new(0., 0.), Pos2::new(100., 0.), Pos2::new(0., 100.)]);
        let (a, b) = split_area(&m, &cutter);
        assert!(
            (a.iter()
                .chain(&b)
                .map(|m| vector::polygon(m).unsigned_area())
                .sum::<f64>()
                - 10000.)
                .abs()
                < 0.1
        );
        assert!(
            (b.iter()
                .map(|m| vector::polygon(m).unsigned_area())
                .sum::<f64>()
                - 5000.)
                .abs()
                < 0.1
        );
    }
    #[test]
    fn crossing_splits_only_clicked_axis_and_retains_profile() {
        let a = Mark {
            points: vec![[0., 50.], [100., 50.]].into(),
            width_profile: crate::width::Preset::Tapered.stops(),
            ..Default::default()
        };
        let b = Mark {
            points: vec![[50., 0.], [50., 100.]].into(),
            ..Default::default()
        };
        let parts = crossing_parts(&a, &[b]);
        assert_eq!(parts.len(), 2);
        assert!(
            vector::cubics(&parts[0])
                .last()
                .unwrap()
                .at(1.)
                .distance(Pos2::new(50., 50.))
                < 0.01
        );
        assert!(!parts[0].nodes.is_empty());
        assert!(!parts[0].width_profile.is_empty());
    }
}
