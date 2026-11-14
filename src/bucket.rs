//! Vector flood fill: the bounded faces of the active frame are extracted from
//! actual ink. Gap bridges are temporary and never change the user's contours.
use crate::{document::Mark, vector, width};
use eframe::egui::{Pos2, Rect};
use geo::{
    Area, BooleanOps, BoundingRect, Buffer, Contains, Coord, LineString, MultiPolygon, Polygon,
    algorithm::buffer::{BufferStyle, LineCap, LineJoin},
};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Gap {
    #[default]
    None,
    Small,
    Medium,
    Large,
}
impl Gap {
    pub const ALL: [Self; 4] = [Self::None, Self::Small, Self::Medium, Self::Large];
    pub fn label(self) -> &'static str {
        crate::i18n::tr(match self {
            Self::None => "No cerrar huecos",
            Self::Small => "Cerrar huecos pequeños",
            Self::Medium => "Cerrar huecos medianos",
            Self::Large => "Cerrar huecos grandes",
        })
    }
    pub fn distance(self) -> f32 {
        match self {
            Self::None => 0.,
            Self::Small => 6.,
            Self::Medium => 16.,
            Self::Large => 36.,
        }
    }
}
fn line(points: impl IntoIterator<Item = Pos2>) -> LineString<f64> {
    LineString::new(
        points
            .into_iter()
            .map(|p| Coord {
                x: p.x as f64,
                y: p.y as f64,
            })
            .collect(),
    )
}
pub fn fill(marks: &[Mark], click: Pos2, gap: Gap, color: [u8; 4]) -> Result<Vec<Mark>, String> {
    let exact = fill_at_level(marks, click, Gap::None, color);
    if gap == Gap::None {
        return exact;
    }
    if let Ok(original) = exact {
        // A bounded region can still leak into a tiny neighbouring tip.
        // Keep only local closures that trim a small part of the original face.
        if let Ok(closed) = fill_at_level(marks, click, gap, color) {
            let polygons =
                |marks: &[Mark]| MultiPolygon(marks.iter().map(vector::polygon).collect());
            let before = polygons(&original);
            let after = polygons(&closed);
            let area = before.unsigned_area();
            let overlap = before.intersection(&after).unsigned_area();
            if overlap >= area * 0.95 && after.unsigned_area() - overlap <= area * 0.005 {
                return Ok(closed);
            }
        }
        return Ok(original);
    }
    let mut result = exact;
    for level in Gap::ALL
        .into_iter()
        .skip(1)
        .take_while(|level| level.distance() <= gap.distance())
    {
        result = fill_at_level(marks, click, level, color);
        if result.is_ok() {
            break;
        }
    }
    result
}

fn fill_at_level(
    marks: &[Mark],
    click: Pos2,
    gap: Gap,
    color: [u8; 4],
) -> Result<Vec<Mark>, String> {
    let mut paths: Vec<(Vec<Pos2>, f32, bool)> = vec![];
    let mut obstacles: Vec<MultiPolygon<f64>> = vec![];
    let mut silhouettes = Vec::new();
    let mut bounds = Rect::from_min_max(click, click);
    for mark in marks.iter().filter(|m| {
        !m.bucket
            && !m.style.guide
            && m.style.media_image.is_none()
            && m.style.instance.is_none()
            && (m.stroke || m.fill.is_some_and(|color| color[3] > 0))
    }) {
        let points = vector::flatten(mark, 0.04);
        if points.len() < 2 {
            continue;
        }
        for p in &points {
            bounds.extend_with(*p);
        }
        let thickness = if mark.stroke {
            mark.width.max(0.01)
        } else {
            0.5
        };
        if mark.stroke
            && (!mark.width_profile.is_empty()
                || mark.style.start_cap != crate::features::Cap::Round
                || mark.style.end_cap != crate::features::Cap::Round)
        {
            obstacles.push(width::outline(mark));
        } else if mark.stroke {
            obstacles.push(line(points.iter().copied()).buffer(thickness as f64 / 2.));
        } else if mark.fill.is_some() {
            // Filled brush silhouettes already describe the real ink boundary.
            // Buffering their edge creates an artificial white gutter inside it.
            let polygon = vector::polygon(mark);
            silhouettes.push(polygon.clone());
            obstacles.push(MultiPolygon(vec![polygon]));
        }
        paths.push((
            points,
            thickness,
            !mark.stroke || mark.closed || mark.rectangle,
        ));
        for hole in &mark.holes {
            let points: Vec<_> = hole.iter().copied().map(vector::pos).collect();
            if points.len() > 2 {
                if mark.stroke {
                    obstacles.push(line(points.iter().copied()).buffer(thickness as f64 / 2.));
                }
                paths.push((points, thickness, true));
            }
        }
    }
    if obstacles.is_empty() {
        return Err("No se encontró una zona cerrada en la capa activa".into());
    }
    let real_ink = geo::algorithm::bool_ops::unary_union(&obstacles);
    let mut bridges = Vec::new();
    if gap != Gap::None {
        let threshold = gap.distance();
        for (owner, (path, thickness, closed)) in paths.iter().enumerate() {
            if *closed || path.first() == path.last() {
                continue;
            }
            let (lengths, total) = width::lengths(path);
            for (endpoint, at_end) in [(path[0], false), (*path.last().unwrap(), true)] {
                let adjacent = if at_end {
                    path[path.len() - 2]
                } else {
                    path[1]
                };
                let outward = (endpoint - adjacent).normalized();
                let mut nearest: Option<(Pos2, f32)> = None;
                let mut joined = false;
                'neighbors: for (other, (other_path, _, _)) in paths.iter().enumerate() {
                    for (segment, pair) in other_path.windows(2).enumerate() {
                        if other == owner {
                            let distance_along = if at_end {
                                total - lengths[(segment + 1).min(lengths.len() - 1)]
                            } else {
                                lengths[segment]
                            };
                            if distance_along < threshold * 2. {
                                continue;
                            }
                        }
                        let d = pair[1] - pair[0];
                        let t =
                            ((endpoint - pair[0]).dot(d) / d.length_sq().max(1e-12)).clamp(0., 1.);
                        let target = pair[0] + d * t;
                        let distance = endpoint.distance(target);
                        if other != owner && distance <= 0.08 {
                            joined = true;
                            break 'neighbors;
                        }
                        if distance > 0.001
                            && distance <= threshold
                            && (target - endpoint).dot(outward) > distance * 0.12
                            && nearest.as_ref().is_none_or(|(_, best)| distance < *best)
                        {
                            nearest = Some((target, distance));
                        }
                    }
                }
                if !joined && let Some((target, _)) = nearest {
                    bridges.push(
                        line([endpoint, target]).buffer_with_style(
                            BufferStyle::new(f64::from(*thickness).min(1.) / 2.)
                                .line_cap(LineCap::Square),
                        ),
                    );
                }
            }
        }
    }
    let pad = bounds.width().max(bounds.height()).max(256.) * 0.15 + gap.distance() + 16.;
    let guard = bounds.expand(pad);
    let outer = Polygon::new(
        line([
            guard.left_top(),
            guard.right_top(),
            guard.right_bottom(),
            guard.left_bottom(),
            guard.left_top(),
        ]),
        vec![],
    );
    let ink = if bridges.is_empty() {
        real_ink.clone()
    } else {
        real_ink.union(&geo::algorithm::bool_ops::unary_union(&bridges))
    };
    let region = match bounded_face(&outer, &ink, guard, click) {
        Ok(region) => region,
        Err(error) if gap != Gap::None && !silhouettes.is_empty() => {
            // Filled brush paths have no centerline endpoints. Close their silhouettes
            // separately so a filled mark cannot round the corners of every stroke.
            let radius = f64::from(gap.distance()) / 2.;
            let silhouettes = geo::algorithm::bool_ops::unary_union(&silhouettes);
            let closed = silhouettes
                .buffer_with_style(BufferStyle::new(radius).line_join(LineJoin::Miter(2.)))
                .buffer_with_style(BufferStyle::new(-radius).line_join(LineJoin::Miter(2.)));
            let closed_ink = ink.union(&closed);
            bounded_face(&outer, &closed_ink, guard, click).map_err(|_| error)?
        }
        Err(error) => return Err(error),
    };
    // Overlap real ink to cover antialiasing seams. Virtual bridges stay flat.
    let face = MultiPolygon(vec![region]);
    let underpaint = face.buffer(0.35).intersection(&real_ink);
    let mut shapes = vector::polygons_to_marks(face.union(&underpaint), color);
    for shape in &mut shapes {
        shape.bucket = true;
    }
    Ok(shapes)
}

pub(crate) fn covered_bucket_fills(marks: &[Mark], replacement: &[Mark]) -> Vec<usize> {
    let faces: Vec<_> = replacement.iter().map(vector::polygon).collect();
    let replacement = geo::algorithm::bool_ops::unary_union(&faces);
    marks
        .iter()
        .enumerate()
        .filter_map(|(index, mark)| {
            if !mark.bucket || mark.fill.is_none() {
                return None;
            }
            let old = vector::polygon(mark);
            let area = old.unsigned_area();
            (area > 0. && replacement.intersection(&old).unsigned_area() >= area * 0.8)
                .then_some(index)
        })
        .collect()
}

fn bounded_face(
    outer: &Polygon<f64>,
    ink: &MultiPolygon<f64>,
    guard: Rect,
    click: Pos2,
) -> Result<Polygon<f64>, String> {
    let free = outer.difference(ink);
    let point = geo::Point::new(click.x as f64, click.y as f64);
    let region = free
        .0
        .into_iter()
        .find(|p| p.contains(&point))
        .ok_or("Haz clic dentro de una zona, fuera del contorno")?;
    let rect = region.bounding_rect().ok_or("Zona de relleno vacía")?;
    if (rect.min().x - guard.left() as f64).abs() < 0.1
        || (rect.max().x - guard.right() as f64).abs() < 0.1
        || (rect.min().y - guard.top() as f64).abs() < 0.1
        || (rect.max().y - guard.bottom() as f64).abs() < 0.1
    {
        return Err("La zona está abierta. Prueba otro nivel de cierre de huecos".into());
    }
    Ok(region)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn contour(gap: f32) -> Mark {
        Mark {
            points: vec![[0., 0.], [100., 0.], [100., 100.], [0., 100.], [0., gap]].into(),
            width: 1.,
            ..Default::default()
        }
    }
    #[test]
    fn fills_closed_regions_and_rejects_unbounded_space() {
        let mut m = contour(0.);
        m.closed = true;
        let shapes = fill(
            &[m.clone()],
            Pos2::new(50., 50.),
            Gap::None,
            [20, 80, 200, 255],
        )
        .unwrap();
        assert_eq!(shapes.len(), 1);
        assert!(shapes[0].bucket);
        assert!(vector::contains_fill(&shapes[0], Pos2::new(50., 50.)));
        assert!(fill(&[m], Pos2::new(200., 200.), Gap::Large, [0, 0, 0, 255]).is_err());
    }
    #[test]
    fn brush_fill_meets_ink_without_a_gutter_or_spilling_outside() {
        let outline = Mark {
            points: vec![[8., 8.], [56., 8.], [56., 56.], [8., 56.], [8., 8.]].into(),
            holes: vec![vec![[12., 12.], [12., 52.], [52., 52.], [52., 12.], [12., 12.]].into()]
                .into(),
            fill: Some([0, 0, 0, 255]),
            stroke: false,
            closed: true,
            ..Default::default()
        };
        let original = serde_json::to_vec(&outline).unwrap();
        let filled = fill(
            &[outline.clone()],
            Pos2::new(32., 32.),
            Gap::None,
            [89, 72, 220, 255],
        )
        .unwrap();
        assert!(
            filled
                .iter()
                .any(|m| vector::contains_fill(m, Pos2::new(12.05, 32.)))
        );
        assert!(
            filled
                .iter()
                .any(|m| vector::contains_fill(m, Pos2::new(11.8, 32.)))
        );
        assert!(
            !filled
                .iter()
                .any(|m| vector::contains_fill(m, Pos2::new(7.9, 32.)))
        );
        assert_eq!(original, serde_json::to_vec(&outline).unwrap());
        let mut doc = crate::Document::default();
        doc.width = 64;
        doc.height = 64;
        doc.layers[0].frames[0] = filled.into();
        doc.layers[0].frames[0].push(outline);
        for scale in [1, 11] {
            let size = 64 * scale;
            let pixels = crate::exporter::raster(
                &doc,
                0,
                size,
                size,
                &eframe::egui::Context::default(),
                &mut Default::default(),
            );
            for x in 10 * scale..15 * scale {
                let at = ((32 * scale * size + x) * 3) as usize;
                assert!(
                    pixels[at] <= 90 && pixels[at + 1] <= 73 && pixels[at + 2] <= 221,
                    "White seam at scale {scale}, x {x}: {:?}",
                    &pixels[at..at + 3]
                );
            }
        }
    }

    #[test]
    fn unrelated_fill_does_not_round_a_narrow_channel_with_a_line_gap() {
        let wall = Mark {
            points: vec![[0., 0.], [80., 0.], [80., 8.], [0., 8.], [0., 5.]].into(),
            width: 1.,
            ..Default::default()
        };
        let decoration = Mark {
            points: vec![[120., 0.], [140., 15.]].into(),
            rectangle: true,
            fill: Some([40, 40, 40, 255]),
            stroke: false,
            ..Default::default()
        };
        let shapes = fill(&[wall, decoration], Pos2::new(20., 4.), Gap::Large, [20; 4]).unwrap();
        for point in [Pos2::new(1., 4.), Pos2::new(40., 1.), Pos2::new(79., 7.)] {
            assert!(
                shapes.iter().any(|mark| vector::contains_fill(mark, point)),
                "{point:?}"
            );
        }
        assert!(
            !shapes
                .iter()
                .any(|mark| vector::contains_fill(mark, Pos2::new(-1., 4.)))
        );
    }
    #[test]
    fn large_gap_does_not_create_a_face_from_open_parallel_strokes() {
        let make = |x| Mark {
            points: vec![[x, 0.], [x, 140.]].into(),
            width: 3.,
            ..Default::default()
        };
        let marks = [make(0.), make(24.)];
        // Both ends line up across the channel; neither stroke points into a gap.
        assert!(fill(&marks, Pos2::new(12., 70.), Gap::Large, [0, 0, 0, 255]).is_err());
    }

    #[test]
    fn closing_gaps_keeps_paint_out_of_a_small_tip_connected_to_a_bounded_face() {
        let marks = [
            Mark {
                closed: true,
                ..contour(0.)
            },
            Mark {
                points: vec![[10., 0.], [10., 8.], [2., 10.]].into(),
                width: 1.,
                ..Default::default()
            },
        ];
        let click = Pos2::new(50., 50.);
        let tip = Pos2::new(5., 4.);
        assert!(
            fill(&marks, click, Gap::None, [255; 4])
                .unwrap()
                .iter()
                .any(|m| vector::contains_fill(m, tip))
        );
        assert!(
            !fill(&marks, click, Gap::Medium, [255; 4])
                .unwrap()
                .iter()
                .any(|m| vector::contains_fill(m, tip))
        );
    }
    #[test]
    fn fill_overlap_covers_ink_without_painting_over_a_virtual_bridge() {
        let shapes = fill(&[contour(8.)], Pos2::new(50., 50.), Gap::Medium, [255; 4]).unwrap();
        assert!(
            shapes
                .iter()
                .any(|mark| vector::contains_fill(mark, Pos2::new(0.75, 4.)))
        );
        assert!(
            !shapes
                .iter()
                .any(|mark| vector::contains_fill(mark, Pos2::new(0.25, 4.)))
        );
    }
    #[test]
    fn gap_levels_close_only_gaps_within_their_threshold() {
        let m = contour(8.);
        assert!(fill(&[m.clone()], Pos2::new(50., 50.), Gap::None, [0, 0, 0, 255]).is_err());
        assert!(
            fill(
                &[m.clone()],
                Pos2::new(50., 50.),
                Gap::Small,
                [0, 0, 0, 255]
            )
            .is_err()
        );
        assert!(
            fill(
                &[m.clone()],
                Pos2::new(50., 50.),
                Gap::Medium,
                [0, 0, 0, 255]
            )
            .is_ok()
        );
        assert_eq!(m.points[4], [0., 8.]);
    }
}

#[cfg(test)]
mod painted_gap_regressions {
    use super::*;
    #[test]
    fn gaps_between_filled_brush_silhouettes_close_without_rewriting_contours() {
        let axis = Mark {
            points: vec![[0., 8.], [0., 100.], [100., 100.], [100., 0.], [0., 0.]].into(),
            width: 4.,
            ..Default::default()
        };
        let ink = vector::brush_fill(&axis, [0, 0, 0, 255]);
        let original = serde_json::to_vec(&ink).unwrap();
        assert!(fill(&ink, Pos2::new(50., 50.), Gap::None, [200, 0, 0, 255]).is_err());
        let result = fill(&ink, Pos2::new(50., 50.), Gap::Medium, [200, 0, 0, 255]).unwrap();
        assert!(
            result
                .iter()
                .any(|m| vector::contains_fill(m, Pos2::new(50., 50.)))
        );
        assert_eq!(original, serde_json::to_vec(&ink).unwrap());
        assert!(fill(&ink, Pos2::new(200., 200.), Gap::Large, [200, 0, 0, 255]).is_err());
    }
}
