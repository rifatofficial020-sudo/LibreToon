//! Interoperable XFL subset: vector strokes and frame-by-frame animation.
use crate::document::{Document, Mark};

use xml::attribute::OwnedAttribute;
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
fn attr<'a>(a: &'a [OwnedAttribute], name: &str) -> Option<&'a str> {
    a.iter()
        .find(|a| a.name.local_name == name)
        .map(|a| a.value.as_str())
}
pub(crate) fn number(a: &[OwnedAttribute], name: &str, default: f32) -> Result<f32, String> {
    match attr(a, name) {
        Some(s) => {
            let value = s
                .parse::<f32>()
                .map_err(|_| format!("Valor XFL no válido: {name}"))?;
            if !value.is_finite() {
                return Err(format!("Valor XFL no finito: {name}"));
            }
            Ok(value)
        }
        None => Ok(default),
    }
}
pub fn encode(doc: &Document) -> Result<Vec<u8>, String> {
    doc.validate()?;
    let xml = document_xml(doc);
    if xml.len() > 64 * 1024 * 1024 {
        return Err("Proyecto demasiado grande para el contenedor FLA actual".into());
    }
    crate::native_project::pack(
        doc,
        vec![
            ("mimetype", b"application/vnd.adobe.xfl"),
            ("DOMDocument.xml", xml.as_bytes()),
            ("LIBRETOON/xfl-preview-v2", b"2"),
        ],
    )
}
fn solid(color: [u8; 4]) -> String {
    format!(
        "<SolidColor color=\"#{:02X}{:02X}{:02X}\" alpha=\"{}\"/>",
        color[0],
        color[1],
        color[2],
        color[3] as f32 / 255.
    )
}
fn document_xml(doc: &Document) -> String {
    render_document_xml(doc, false)
}
fn render_document_xml(doc: &Document, legacy: bool) -> String {
    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?><DOMDocument xmlns=\"http://ns.adobe.com/xfl/2008/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" width=\"{}\" height=\"{}\" frameRate=\"{}\" backgroundColor=\"#FFFFFF\" xflVersion=\"2.97\" creatorInfo=\"LibreToon\" currentTimeline=\"1\"><timelines>",
        doc.width, doc.height, doc.fps
    );
    if legacy || doc.extras.scenes.is_empty() {
        timeline_xml(&mut xml, &crate::features::baked(doc), "Escena 1", legacy);
    } else {
        for (index, scene) in doc.extras.scenes.iter().enumerate() {
            let scene_doc = crate::animation::scene_doc(doc, index);
            timeline_xml(
                &mut xml,
                &crate::features::baked(&scene_doc),
                &scene.name,
                false,
            );
        }
    }
    xml += "</timelines></DOMDocument>";
    xml
}
fn timeline_xml(xml: &mut String, doc: &Document, name: &str, legacy: bool) {
    *xml += &format!(
        "<DOMTimeline name=\"{}\" currentFrame=\"0\"><layers>",
        escape(name)
    );
    let folders: std::collections::HashMap<_, _> = doc
        .extras
        .folders
        .iter()
        .map(|folder| (folder.id, folder))
        .collect();
    let mut folder_indices = std::collections::HashMap::new();
    let mut next_index = 0;
    for layer in doc.layers.iter().rev() {
        let mut attributes = String::new();
        if !legacy {
            let mut ancestors = Vec::new();
            let mut parent = layer.options.folder;
            while let Some(folder) = parent.and_then(|id| folders.get(&id)) {
                ancestors.push(*folder);
                parent = folder.parent;
            }
            let mut parent_attribute = String::new();
            for folder in ancestors.into_iter().rev() {
                let index = *folder_indices.entry(folder.id).or_insert_with(|| {
                    *xml += &format!(
                        "<DOMLayer name=\"{}\" layerType=\"folder\" isExpanded=\"{}\"{parent_attribute}><frames/></DOMLayer>",
                        escape(&folder.name), !folder.collapsed,
                    );
                    let index = next_index;
                    next_index += 1;
                    index
                });
                parent_attribute = format!(" parentLayerIndex=\"{index}\"");
            }
            attributes = parent_attribute;
            if layer.options.locked {
                attributes.push_str(" locked=\"true\"");
            }
        }
        next_index += 1;
        *xml += &format!(
            "<DOMLayer name=\"{}\" color=\"#5B9EFF\" visible=\"{}\"{attributes}><frames>",
            escape(&layer.name),
            layer.visible
        );
        for (index, marks) in layer.frames.iter().enumerate() {
            if !layer.key(index) {
                continue;
            }
            let duration = layer.exposure(index).end - index;
            *xml += &format!(
                "<DOMFrame index=\"{index}\" duration=\"{duration}\" keyMode=\"9728\"><elements>"
            );
            let export_marks: Vec<_> = marks.iter().flat_map(crate::width::export_marks).collect();
            for mark in &export_marks {
                let mut rings = vec![crate::vector::flatten(mark, 0.04)];
                rings.extend(
                    mark.holes
                        .iter()
                        .map(|h| h.iter().copied().map(crate::vector::pos).collect()),
                );
                if rings[0].is_empty() {
                    continue;
                }
                *xml += "<DOMShape isFloating=\"true\"><matrix><Matrix/></matrix>";
                if let Some(fill) = mark.fill {
                    *xml += &format!(
                        "<fills><FillStyle index=\"1\">{}</FillStyle></fills>",
                        solid(fill)
                    );
                }
                if mark.stroke {
                    *xml += &format!(
                        "<strokes><StrokeStyle index=\"1\"><SolidStroke weight=\"{}\" caps=\"round\" joints=\"round\" scaleMode=\"none\"><fill>{}</fill></SolidStroke></StrokeStyle></strokes>",
                        mark.width,
                        solid(mark.color)
                    );
                }
                *xml += "<edges>";
                for (i, mut ring) in rings.into_iter().enumerate() {
                    let mut reversed = false;
                    if ring.is_empty() {
                        continue;
                    }
                    if mark.fill.is_some() {
                        if ring.first() != ring.last() {
                            ring.push(ring[0]);
                        }
                        let area = ring
                            .windows(2)
                            .map(|w| w[0].x * w[1].y - w[1].x * w[0].y)
                            .sum::<f32>();
                        if (area > 0.) != (i == 0) {
                            ring.reverse();
                            reversed = true;
                        }
                    }
                    let mut edges = format!("!{} {}", ring[0].x * 20., ring[0].y * 20.);
                    if i == 0 && !mark.nodes.is_empty() {
                        let mut curves = crate::vector::cubics(mark);
                        if reversed {
                            curves.reverse();
                            for curve in &mut curves {
                                curve.0.reverse();
                            }
                        }
                        for curve in curves {
                            quadratic_edges(curve, &mut edges, 0);
                        }
                    } else {
                        for p in &ring[1..] {
                            edges += &format!("|{} {}", p.x * 20., p.y * 20.);
                        }
                    }
                    if ring.len() == 1 {
                        edges += &format!("|{} {}", ring[0].x * 20. + 0.02, ring[0].y * 20.);
                    }
                    *xml += &format!(
                        "<Edge strokeStyle=\"{}\" fillStyle1=\"{}\" edges=\"{edges}\"/>",
                        usize::from(mark.stroke),
                        usize::from(mark.fill.is_some())
                    );
                }
                *xml += "</edges></DOMShape>";
            }
            *xml += "</elements></DOMFrame>";
        }
        *xml += "</frames></DOMLayer>";
    }
    *xml += "</layers></DOMTimeline>";
}
fn unsupported(what: &str) -> String {
    format!(
        "Este FLA contiene {what}, todavía no admitidos por LibreToon. No se ha abierto ni modificado el archivo. Compatibilidad actual: escenario blanco, rellenos sólidos, contornos y animación por fotogramas."
    )
}
fn coordinate(token: &str) -> Result<f32, String> {
    if let Some(hex) = token.strip_prefix('#') {
        let (whole, frac) = hex.split_once('.').unwrap_or((hex, "0"));
        let n = i64::from_str_radix(whole, 16).map_err(|_| "Coordenada XFL no válida")?;
        let signed = if n >= 0x800000 { n - 0x1000000 } else { n };
        let fraction = u32::from_str_radix(frac, 16).map_err(|_| "Fracción XFL no válida")? as f32
            / 16f32.powi(frac.len() as i32);
        Ok(signed as f32 + fraction)
    } else {
        token.parse().map_err(|_| "Coordenada XFL no válida".into())
    }
}
#[cfg(test)]
fn paths(edges: &str) -> Result<Vec<Vec<[f32; 2]>>, String> {
    Ok(editable_paths(edges)?
        .iter()
        .map(|m| {
            crate::vector::flatten(m, 0.04)
                .into_iter()
                .map(crate::vector::xy)
                .collect()
        })
        .collect())
}

// Older exports can differ by a few floating-point ULPs after geometry fixes.
// Compare the full XML structure exactly, allowing only sub-pixel rounding in
// edge coordinates (0.04 twips = 0.002 stage pixels). Never ignore extra content.
fn same_native_preview(expected: &str, actual: &str) -> bool {
    use xml::reader::{EventReader, XmlEvent};
    fn edges_equal(a: &str, b: &str) -> bool {
        fn tokens(s: &str) -> Vec<&str> {
            s.split_inclusive(|c: char| {
                !c.is_ascii_digit() && !matches!(c, '.' | '-' | '+' | 'e' | 'E')
            })
            .collect()
        }
        let a = tokens(a);
        let b = tokens(b);
        a.len() == b.len()
            && a.iter().zip(b).all(|(a, b)| {
                if *a == b {
                    return true;
                }
                let split = |s: &str| {
                    s.trim_end_matches(|c: char| {
                        !c.is_ascii_digit() && !matches!(c, '.' | '-' | '+' | 'e' | 'E')
                    })
                    .len()
                };
                let (an, ad) = a.split_at(split(a));
                let (bn, bd) = b.split_at(split(b));
                ad == bd
                    && an
                        .parse::<f64>()
                        .ok()
                        .zip(bn.parse::<f64>().ok())
                        .is_some_and(|(a, b)| {
                            a.is_finite() && b.is_finite() && (a - b).abs() <= 0.04
                        })
            })
    }
    if expected == actual {
        return true;
    }
    fn events(xml: &str) -> Option<Vec<XmlEvent>> {
        let input = EventReader::from_str(xml)
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        let mut out = Vec::with_capacity(input.len());
        let mut edge_count = 0;
        let mut i = 0;
        while i < input.len() {
            if let XmlEvent::StartElement {
                name, attributes, ..
            } = &input[i]
            {
                if name.local_name == "edges" {
                    edge_count = 0;
                }
                if name.local_name == "Edge" {
                    edge_count += 1;
                    // Polygon boolean operations may discard almost zero-area
                    // interior slivers. Never discard a contour, stroke or curve.
                    if edge_count > 1
                        && attributes.len() == 3
                        && attr(attributes, "strokeStyle") == Some("0")
                        && attr(attributes, "fillStyle1") == Some("1")
                        && attr(attributes, "edges").is_some_and(tiny_linear_ring)
                        && matches!(input.get(i+1), Some(XmlEvent::EndElement { name }) if name.local_name == "Edge")
                    {
                        i += 2;
                        continue;
                    }
                }
            }
            out.push(input[i].clone());
            i += 1;
        }
        Some(out)
    }
    fn tiny_linear_ring(s: &str) -> bool {
        if !s.starts_with('!')
            || s[1..].contains(|c: char| {
                !c.is_ascii_digit() && !matches!(c, '|' | ' ' | '.' | '-' | '+' | 'e' | 'E')
            })
        {
            return false;
        }
        let Some(points) = s[1..]
            .split('|')
            .map(|point| {
                let nums = point
                    .split_whitespace()
                    .map(str::parse::<f64>)
                    .collect::<Result<Vec<_>, _>>()
                    .ok()?;
                (nums.len() == 2 && nums.iter().all(|n| n.is_finite())).then(|| [nums[0], nums[1]])
            })
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        if points.len() < 4 || points.first() != points.last() {
            return false;
        }
        let origin = points[0];
        if points
            .iter()
            .any(|p| (p[0] - origin[0]).abs() > 1. || (p[1] - origin[1]).abs() > 1.)
        {
            return false;
        }
        let area = points
            .windows(2)
            .map(|p| {
                (p[0][0] - origin[0]) * (p[1][1] - origin[1])
                    - (p[1][0] - origin[0]) * (p[0][1] - origin[1])
            })
            .sum::<f64>()
            .abs()
            * 0.5;
        area <= 0.001
    }
    let (Some(a), Some(b)) = (events(expected), events(actual)) else {
        return false;
    };
    let mut a = a.into_iter().map(Ok::<_, ()>);
    let mut b = b.into_iter().map(Ok::<_, ()>);
    loop {
        match (a.next(), b.next()) {
            (None, None) => return true,
            (
                Some(Ok(XmlEvent::StartElement {
                    name: an,
                    attributes: mut aa,
                    namespace: ans,
                })),
                Some(Ok(XmlEvent::StartElement {
                    name: bn,
                    attributes: mut ba,
                    namespace: bns,
                })),
            ) => {
                if an != bn || ans != bns || aa.len() != ba.len() {
                    return false;
                }
                if an.local_name == "Edge" {
                    for (a, b) in aa.iter_mut().zip(ba.iter_mut()) {
                        if a.name.local_name == "edges"
                            && a.name == b.name
                            && edges_equal(&a.value, &b.value)
                        {
                            a.value.clear();
                            b.value.clear();
                        }
                    }
                }
                if aa != ba {
                    return false;
                }
            }
            (Some(Ok(a)), Some(Ok(b))) if a == b => {}
            _ => return false,
        }
    }
}

pub fn decode(bytes: &[u8]) -> Result<Document, String> {
    let xml = crate::fla_zip::document(bytes)?;
    let native = crate::fla_zip::entry(bytes, b"LIBRETOON/document.json");
    let mut stale = false;
    if let Ok(Some(native)) = &native {
        let archive = crate::fla_zip::Archive::new(bytes)?;
        if let Ok(doc) = crate::native_project::restore(&archive, native) {
            if same_native_preview(&document_xml(&doc), &xml)
                || (archive
                    .entry(b"LIBRETOON/xfl-preview-v2")
                    .is_ok_and(|entry| entry.is_none())
                    && same_native_preview(&render_document_xml(&doc, true), &xml))
            {
                return Ok(doc);
            }
        }
        stale = true;
    }
    let mut doc = crate::xfl::import(bytes, &xml)?;
    if stale || native.is_err() {
        doc.extras.import_warnings.push(
            "Metadatos LibreToon desactualizados o dañados: se ha importado el XFL visible".into(),
        );
    }
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::transformed;
    use eframe::egui::{Pos2, Vec2};
    #[test]
    fn visible_xfl_keeps_scenes_and_nested_layer_folders() {
        let mut doc = Document::default();
        let parent = crate::features::id();
        let child = crate::features::id();
        for (id, name, parent) in [(parent, "Cast", None), (child, "Hero", Some(parent))] {
            doc.extras.folders.push(crate::features::Folder {
                id,
                name: name.into(),
                parent,
                collapsed: false,
                transform: Default::default(),
                timeline_slot: None,
                range: None,
                source_shift: 0,
            });
        }
        doc.layers[0].options.folder = Some(child);
        doc.layers[0].options.locked = true;
        for name in ["Opening", "Ending"] {
            let scene = crate::animation::scene_from(&doc, crate::features::id(), name.into());
            doc.extras.scenes.push(scene);
        }
        let xml =
            document_xml(&doc).replacen("keyMode=\"9728\"", "keyMode=\"9728\" name=\"Start\"", 1);
        let bytes = crate::fla_zip::pack(&[("DOMDocument.xml", xml.into_bytes())]);
        let imported = decode(&bytes).unwrap();
        assert_eq!(imported.extras.scenes[0].name, "Opening");
        assert_eq!(imported.extras.scenes[1].name, "Ending");
        assert_eq!(
            imported.extras.folders[1].parent,
            Some(imported.extras.folders[0].id)
        );
        assert_eq!(
            imported.layers[0].options.folder,
            Some(imported.extras.folders[1].id)
        );
        assert!(imported.layers[0].options.locked);
        assert_eq!(imported.extras.markers[0].name, "Start");
        assert!(imported.extras.scenes[1].markers.is_empty());
    }

    #[test]
    fn native_preview_accepts_only_tiny_edge_rounding() {
        let xml = r#"<DOMDocument><Edge edges="!21410.85 11554.219|20 30"/></DOMDocument>"#;
        assert!(same_native_preview(
            xml,
            &xml.replace("11554.219", "11554.22")
        ));
        assert!(!same_native_preview(
            xml,
            &xml.replace("11554.219", "11555")
        ));
        assert!(!same_native_preview(
            xml,
            &xml.replace("</DOMDocument>", "<script/></DOMDocument>")
        ));
        assert!(!same_native_preview(xml, &xml.replace("Edge", "Other")));
        let ring = r#"<Edge strokeStyle="0" fillStyle1="1" edges="!0 0|0.1 0.5|0.1 0.501|0 0"/>"#;
        let base = r#"<DOMDocument><edges><Edge strokeStyle="0" fillStyle1="1" edges="!0 0|20 0|20 20|0 0"/></edges></DOMDocument>"#;
        assert!(same_native_preview(
            base,
            &base.replace("</edges>", &format!("{ring}</edges>"))
        ));
        assert!(!same_native_preview(
            base,
            &base.replace(
                "</edges>",
                &format!("{}</edges>", ring.replace("0.501", "1"))
            )
        ));
        assert!(!same_native_preview(
            base,
            &base.replace(
                "</edges>",
                &format!(
                    "{}</edges>",
                    ring.replace("strokeStyle=\"0\"", "strokeStyle=\"1\"")
                )
            )
        ));
    }

    #[test]
    fn native_fla_roundtrip_geometry_layers_and_frames() {
        let mut d = Document::default();
        d.layers[0].name = "Dibujo & escena".into();
        let m = Mark {
            points: vec![[20., 30.], [140., 180.]].into(),
            width: 6.,
            color: [32, 64, 128, 200],
            rectangle: true,
            ..Mark::default()
        };
        d.layers[0].frames[2].push(transformed(
            &m,
            Pos2::new(20., 30.),
            Vec2::new(2., 1.),
            0.7,
            Vec2::new(-100., 10.),
        ));
        d.layers[0].frames[0].push(m.clone());
        let b = encode(&d).unwrap();
        assert_eq!(&b[..2], b"PK");
        if let Ok(path) = std::env::var("LIBRETOON_FLA_FIXTURE") {
            std::fs::write(path, &b).unwrap();
        }
        let restored = decode(&b).unwrap();
        assert_eq!(restored.layers[0].name, d.layers[0].name);
        assert_eq!(restored.layers[0].frames.len(), 24);
        assert_eq!(
            restored.layers[0].frames[2][0].points,
            d.layers[0].frames[2][0].points
        );
    }
    #[test]
    fn skips_unavailable_symbols_and_reports_them() {
        let x = "<DOMDocument width=\"960\" height=\"540\"><timelines><DOMTimeline><layers><DOMLayer><frames><DOMFrame><elements><DOMSymbolInstance/></elements></DOMFrame></frames></DOMLayer></layers></DOMTimeline></timelines></DOMDocument>";
        let b = crate::fla_zip::pack(&[("DOMDocument.xml", x.as_bytes().to_vec())]);
        let d = decode(&b).unwrap();
        assert!(d.layers[0].frames[0].is_empty());
        assert!(
            d.extras
                .import_warnings
                .iter()
                .any(|s| s.contains("Símbolo"))
        );
    }
    #[test]
    fn quadratic_curve_and_hex_coordinates() {
        let p = paths("!0 0[200 400 400 0").unwrap();
        assert!(p[0].len() >= 5);
        assert!((p[0].iter().map(|p| p[1]).fold(0., f32::max) - 10.).abs() < 0.001);
        assert!((coordinate("#FFFFFF.80").unwrap() + 0.5).abs() < 0.001);
    }
}

pub(crate) fn parse_color(a: &[OwnedAttribute]) -> Result<[u8; 4], String> {
    let hex = attr(a, "color")
        .unwrap_or("#000000")
        .trim_start_matches('#');
    if hex.len() != 6 || !hex.is_ascii() {
        return Err("Color XFL no válido".into());
    }
    let mut color = [0, 0, 0, 255];
    for i in 0..3 {
        color[i] =
            u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).map_err(|_| "Color XFL no válido")?;
    }
    color[3] = (number(a, "alpha", 1.)?.clamp(0., 1.) * 255.).round() as u8;
    Ok(color)
}
pub(crate) fn join_fill_rings(
    mut edges: Vec<Vec<[f32; 2]>>,
    color: [u8; 4],
) -> Result<Vec<Mark>, String> {
    let close =
        |a: [f32; 2], b: [f32; 2]| crate::vector::pos(a).distance(crate::vector::pos(b)) < 0.001;
    edges.retain(|p| p.len() > 1);
    let mut rings = vec![];
    while let Some(mut ring) = edges.pop() {
        while !close(ring[0], *ring.last().unwrap()) {
            let end = *ring.last().unwrap();
            let at = edges
                .iter()
                .position(|e| close(e[0], end) || close(*e.last().unwrap(), end))
                .ok_or("Relleno XFL con bordes abiertos o topología no compatible")?;
            let mut edge = edges.swap_remove(at);
            if !close(edge[0], end) {
                edge.reverse();
            }
            ring.extend(edge.into_iter().skip(1));
        }
        if ring.len() > 3 {
            rings.push(ring);
        }
    }
    let area = |r: &Vec<[f32; 2]>| {
        r.windows(2)
            .map(|w| w[0][0] as f64 * w[1][1] as f64 - w[1][0] as f64 * w[0][1] as f64)
            .sum::<f64>()
            .abs()
    };
    rings.sort_by(|a, b| area(b).total_cmp(&area(a)));
    let contains = |r: &Vec<[f32; 2]>, p: [f32; 2]| {
        crate::vector::inside_ring(
            &r.iter()
                .copied()
                .map(crate::vector::pos)
                .collect::<Vec<_>>(),
            crate::vector::pos(p),
        )
    };
    let mut output: Vec<Mark> = vec![];
    let mut prior: Vec<Vec<[f32; 2]>> = vec![];
    for ring in rings {
        let depth = prior.iter().filter(|r| contains(r, ring[0])).count();
        if depth % 2 == 0 {
            output.push(Mark {
                points: ring.clone().into(),
                fill: Some(color),
                stroke: false,
                closed: true,
                ..Mark::default()
            });
        } else {
            let outer = output
                .iter_mut()
                .rev()
                .find(|m| contains(&m.points, ring[0]))
                .ok_or("Hueco de relleno XFL sin contorno exterior")?;
            outer.holes.push(ring.clone().into());
        }
        prior.push(ring);
    }
    Ok(output)
}

#[cfg(test)]
mod vector_format_tests {
    use super::*;
    use eframe::egui::Pos2;
    fn fixture() -> Document {
        let mut doc = Document::default();
        let curve = crate::vector::Cubic([
            Pos2::new(-50., 20.),
            Pos2::new(20., -80.),
            Pos2::new(100., 120.),
            Pos2::new(200., 20.),
        ]);
        doc.layers[0].frames[0].push(crate::vector::from_cubics(&[curve], &Mark::default()));
        doc.layers[0].frames[0].push(Mark {
            points: vec![
                [10., 10.],
                [110., 10.],
                [110., 110.],
                [10., 110.],
                [10., 10.],
            ]
            .into(),
            holes: vec![vec![[30., 30.], [30., 60.], [60., 60.], [60., 30.], [30., 30.]].into()]
                .into(),
            closed: true,
            fill: Some([220, 80, 30, 150]),
            stroke: true,
            ..Default::default()
        });
        doc
    }
    #[test]
    fn native_fla_preserves_editable_beziers_fills_holes_and_offstage_points() {
        let doc = fixture();
        let restored = decode(&encode(&doc).unwrap()).unwrap();
        assert_eq!(
            restored.layers[0].frames[0][0].nodes,
            doc.layers[0].frames[0][0].nodes
        );
        assert_eq!(
            restored.layers[0].frames[0][1].holes,
            doc.layers[0].frames[0][1].holes
        );
        assert_eq!(
            restored.layers[0].frames[0][1].fill,
            Some([220, 80, 30, 150])
        );
        let svg = restored.svg(0);
        assert!(svg.contains(" C"));
        assert!(svg.contains("fill-rule=\"evenodd\""));
    }
    #[test]
    fn standard_xfl_without_native_metadata_keeps_fill_holes_and_strokes() {
        let doc = fixture();
        let xml = document_xml(&doc);
        let bytes = crate::fla_zip::pack(&[("DOMDocument.xml", xml.into_bytes())]);
        let restored = decode(&bytes).unwrap();
        assert!(
            restored.layers[0].frames[0]
                .iter()
                .any(|m| m.fill == Some([220, 80, 30, 150]) && m.holes.len() == 1)
        );
        assert!(
            restored.layers[0].frames[0]
                .iter()
                .any(|m| m.stroke && (!m.points.is_empty() || !m.nodes.is_empty()))
        );
    }
    #[test]
    fn stale_native_metadata_does_not_hide_external_xml_edits_or_unsupported_content() {
        let doc = fixture();
        let native = serde_json::to_vec(&doc).unwrap();
        let changed = document_xml(&doc).replace("frameRate=\"12\"", "frameRate=\"24\"");
        let bytes = crate::fla_zip::pack(&[
            ("DOMDocument.xml", changed.into_bytes()),
            ("LIBRETOON/document.json", native.clone()),
        ]);
        assert_eq!(decode(&bytes).unwrap().fps, 24);
        let unsupported =
            document_xml(&doc).replace("</elements>", "<DOMSymbolInstance/></elements>");
        let bytes = crate::fla_zip::pack(&[
            ("DOMDocument.xml", unsupported.into_bytes()),
            ("LIBRETOON/document.json", native),
        ]);
        let d = decode(&bytes).unwrap();
        assert!(
            d.extras
                .import_warnings
                .iter()
                .any(|s| s.contains("desactualizados"))
        );
    }
}
// XFL stores quadratic edges. Subdivide cubics until degree reduction is within
// tolerance; native metadata retains the original cubic anchors exactly.
fn quadratic_edges(curve: crate::vector::Cubic, out: &mut String, depth: u32) {
    let [a, b, c, d] = curve.0;
    let chord = d - a;
    let distance = |p: eframe::egui::Pos2| {
        let t = ((p - a).dot(chord) / chord.length_sq().max(1e-12)).clamp(0., 1.);
        p.distance(a + chord * t)
    };
    if distance(b).max(distance(c)) < 0.001 {
        *out += &format!("|{} {}", d.x * 20., d.y * 20.);
        return;
    }
    let q = eframe::egui::Pos2::ZERO
        + (b.to_vec2() * 3. + c.to_vec2() * 3. - a.to_vec2() - d.to_vec2()) / 4.;
    let error = b
        .distance(a + (q - a) * (2. / 3.))
        .max(c.distance(d + (q - d) * (2. / 3.)));
    if error <= 0.04 || depth >= 18 {
        *out += &format!("[{} {} {} {}", q.x * 20., q.y * 20., d.x * 20., d.y * 20.);
    } else {
        let (left, right) = curve.split(0.5);
        quadratic_edges(left, out, depth + 1);
        quadratic_edges(right, out, depth + 1);
    }
}
pub(crate) fn editable_paths(edges: &str) -> Result<Vec<Mark>, String> {
    use crate::vector::{Cubic, pos, xy};
    let mut separated = String::new();
    for c in edges.chars() {
        if "!|[]".contains(c) {
            separated.push(' ');
            separated.push(c);
            separated.push(' ');
        } else {
            separated.push(c);
        }
    }
    let tokens: Vec<_> = separated.split_whitespace().collect();
    let mut at = 0;
    let mut output = vec![];
    let mut curves = vec![];
    let mut current = None;
    let finish =
        |curves: &mut Vec<Cubic>, current: Option<eframe::egui::Pos2>, output: &mut Vec<Mark>| {
            if !curves.is_empty() {
                output.push(crate::vector::from_cubics(curves, &Mark::default()));
                curves.clear();
            } else if let Some(p) = current {
                output.push(Mark {
                    points: vec![xy(p)].into(),
                    ..Mark::default()
                });
            }
        };
    while at < tokens.len() {
        let command = tokens[at];
        at += 1;
        if command.starts_with('S') && command[1..].chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let count = match command {
            "!" | "|" => 2,
            "[" | "]" => 4,
            _ => return Err(unsupported("comandos de trazado no compatibles")),
        };
        if at + count > tokens.len() {
            return Err("Trazado XFL truncado".into());
        }
        let values = tokens[at..at + count]
            .iter()
            .map(|t| coordinate(t).map(|v| v / 20.))
            .collect::<Result<Vec<_>, _>>()?;
        at += count;
        if values.iter().any(|v| !v.is_finite() || v.abs() > 1e7) {
            return Err("Coordenada XFL fuera de rango".into());
        }
        let target = pos([values[count - 2], values[count - 1]]);
        if command == "!" {
            finish(&mut curves, current, &mut output);
            current = Some(target);
            continue;
        }
        let start = current.ok_or("Trazado XFL sin origen")?;
        let curve = if command == "|" {
            Cubic([start, start, target, target])
        } else {
            let q = pos([values[0], values[1]]);
            Cubic([
                start,
                start + (q - start) * (2. / 3.),
                target + (q - target) * (2. / 3.),
                target,
            ])
        };
        curves.push(curve);
        current = Some(target);
    }
    finish(&mut curves, current, &mut output);
    Ok(output)
}

#[cfg(test)]
mod styled_format_tests {
    use super::*;
    #[test]
    fn custom_resolution_fps_and_width_stops_survive_native_and_svg_exports() {
        let mut doc = Document::default();
        doc.width = 4096;
        doc.height = 2160;
        doc.fps = 120;
        doc.layers[0].frames[0].push(Mark {
            points: vec![[100., 100.], [600., 100.]].into(),
            width: 24.,
            width_profile: crate::width::Preset::Tapered.stops(),
            ..Default::default()
        });
        let restored = decode(&encode(&doc).unwrap()).unwrap();
        assert_eq!(
            (restored.width, restored.height, restored.fps),
            (4096, 2160, 120)
        );
        assert_eq!(
            restored.layers[0].frames[0][0].width_profile,
            doc.layers[0].frames[0][0].width_profile
        );
        let svg = restored.svg(0);
        assert!(svg.contains("viewBox=\"0 0 4096 2160\""));
        assert!(svg.contains("stroke=\"none\""));
        let xml = document_xml(&doc);
        let standard = crate::fla_zip::pack(&[("DOMDocument.xml", xml.into_bytes())]);
        let external = decode(&standard).unwrap();
        assert_eq!(external.width, 4096);
        assert!(
            external.layers[0].frames[0]
                .iter()
                .any(|m| m.fill.is_some())
        );
    }
    #[test]
    fn invalid_width_profiles_are_rejected_before_rendering() {
        let mut doc = Document::default();
        let mut mark = Mark::default();
        mark.width_profile = vec![
            crate::width::WidthStop { t: 0.5, width: 1. },
            crate::width::WidthStop { t: 0.5, width: 2. },
        ]
        .into();
        doc.layers[0].frames[0].push(mark);
        assert!(doc.validate().is_err());
    }
}

#[cfg(test)]
mod legacy_fixture_tests {
    use super::*;
    #[test]
    #[ignore = "manual external legacy FLA fixture"]
    fn external_legacy_native_fla_preserves_layers_and_exposures() {
        let path = std::path::PathBuf::from(
            std::env::var("LIBRETOON_LEGACY_FLA").expect("Set LIBRETOON_LEGACY_FLA"),
        );
        let bytes = std::fs::read(&path).unwrap();
        let native = crate::fla_zip::entry(&bytes, b"LIBRETOON/document.json")
            .unwrap()
            .unwrap();
        let source: Document = serde_json::from_slice(&native).unwrap();
        assert!(source.validate().is_ok());
        let imported = decode(&bytes).unwrap();
        assert_eq!(imported.layers.len(), source.layers.len());
        let ctx = eframe::egui::Context::default();
        let mut editor = crate::Editor::new(&eframe::CreationContext::_new_kittest(ctx.clone()));
        let mut flattened = crate::Document::default();
        flattened.fps = 17;
        editor.add_document(flattened, Some(path));
        let old = editor.hub.active.unwrap();
        editor.hub.tabs[old].record.dirty = true;
        editor.open_saved_document_copy();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while editor.hub.active == Some(old) {
            editor.poll_open(&ctx);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(editor.doc.layers.len(), 3);
        assert!(editor.path.is_none()); // Save As protects the original.
        editor.activate_document(old);
        assert_eq!(editor.doc.layers.len(), 1);
        assert_eq!(editor.doc.fps, 17);
        assert!(editor.hub.tabs[old].record.dirty);

        for (a, b) in source.layers.iter().zip(imported.layers.iter()) {
            assert_eq!(a.name, b.name);
            assert_eq!(a.options.keys, b.options.keys);
            assert_eq!(a.frames.len(), b.frames.len());
        }
    }
}
