use crate::{
    Document,
    document::{Layer, Mark},
    features::{self, Instance, Playback, Symbol, SymbolClock},
};
fn shape(color: &str) -> String {
    format!(
        r##"<DOMShape><fills><FillStyle index="1"><SolidColor color="{color}"/></FillStyle></fills><edges><Edge fillStyle1="1" edges="!0 0|200 0|200 200|0 200|0 0"/></edges></DOMShape>"##
    )
}
fn frame(index: usize, duration: usize, attrs: &str, elements: &str) -> String {
    format!(
        r#"<DOMFrame index="{index}" duration="{duration}" {attrs}><elements>{elements}</elements></DOMFrame>"#
    )
}
fn timeline(name: &str, frames: &str) -> String {
    format!(
        r#"<DOMTimeline name="{name}"><layers><DOMLayer name="Art"><frames>{frames}</frames></DOMLayer></layers></DOMTimeline>"#
    )
}
fn archive(scenes: &str, library: &[(&str, &str, String)], extra: &str) -> Vec<u8> {
    let includes = library
        .iter()
        .map(|(path, _, _)| format!(r#"<Include href="{path}"/>"#))
        .collect::<String>();
    let xml = format!(
        r##"<DOMDocument xmlns="http://ns.adobe.com/xfl/2008/" width="640" height="480" frameRate="24" backgroundColor="#FFFFFF"><symbols>{includes}</symbols><timelines>{scenes}</timelines>{extra}</DOMDocument>"##
    );
    let mut entries = vec![("DOMDocument.xml".to_owned(), xml.into_bytes())];
    for (path, name, tl) in library {
        entries.push((
            format!("LIBRARY/{path}"),
            format!(r#"<DOMSymbolItem name="{name}"><timeline>{tl}</timeline></DOMSymbolItem>"#)
                .into_bytes(),
        ));
    }
    crate::fla_zip::pack(
        &entries
            .iter()
            .map(|(name, data)| (name.as_str(), data.clone()))
            .collect::<Vec<_>>(),
    )
}
fn inst(name: &str, attrs: &str, inside: &str) -> String {
    format!(r#"<DOMSymbolInstance libraryItemName="{name}" {attrs}>{inside}</DOMSymbolInstance>"#)
}
fn decode(bytes: Vec<u8>) -> Document {
    let d = crate::fla::decode(&bytes).unwrap();
    assert!(d.validate().is_ok());
    d
}
#[test]
fn animate_scenes_exposures_empty_keys_and_scripts_import_together() {
    let red = shape("#FF0000");
    let blue = shape("#0000FF");
    let a = timeline("Scene A", &(frame(0, 3, "", &red) + &frame(3, 1, "", "")));
    let b = timeline("Scene B", &frame(0, 2, "soundName=\"unused\"", &blue));
    let d = decode(archive(
        &(a + &b),
        &[],
        "<ActionScript><script><![CDATA[stop(); throw new Error('never execute')]]></script></ActionScript><media><DOMSoundItem name=\"unused\"/></media>",
    ));
    assert_eq!(d.extras.scenes.len(), 2);
    assert_eq!(crate::animation::movie_count(&d), 6);
    assert!(crate::animation::movie_svg(&d, 0).contains("rgb(255,0,0)"));
    assert!(!crate::animation::movie_svg(&d, 3).contains("rgb(255,0,0)"));
    assert!(crate::animation::movie_svg(&d, 4).contains("rgb(0,0,255)"));
    assert!(
        d.extras
            .import_warnings
            .iter()
            .any(|s| s.contains("ActionScript"))
    );
    assert_eq!(
        d.layers[0].frames[0].identity(),
        d.layers[0].frames[2].identity()
    );
}
#[test]
fn animate_nested_groups_symbol_matrices_and_button_up_state() {
    let child = timeline(
        "Button",
        &(frame(0, 1, "", &shape("#00FF00")) + &frame(1, 3, "", &shape("#FF0000"))),
    );
    let instance = inst(
        "Folder/Button",
        "symbolType=\"button\"",
        "<matrix><Matrix tx=\"100\"/></matrix><ActionScript><script>on(release){}</script></ActionScript>",
    );
    let group = format!(
        "<DOMGroup><matrix><Matrix ty=\"50\"/></matrix><members>{instance}</members></DOMGroup>"
    );
    let d = decode(archive(
        &timeline("Main", &frame(0, 4, "", &group)),
        &[("Folder/Button.xml", "Folder/Button", child)],
        "",
    ));
    assert!(crate::animation::movie_svg(&d, 3).contains("rgb(0,255,0)"));
    let marks = features::resolve_export(&d, 3, false);
    let bounds = crate::vector::control_bounds(&marks[0].2);
    assert_eq!((bounds.left(), bounds.top()), (100., 50.));
    assert!(
        d.extras
            .import_warnings
            .iter()
            .any(|s| s.contains("Botones"))
    );
}
#[test]
fn animate_classic_motion_tween_and_alpha_are_interpolated() {
    let symbol = timeline("Art", &frame(0, 1, "", &shape("#FF0000")));
    let a = inst(
        "Art",
        "symbolType=\"graphic\"",
        "<matrix><Matrix/></matrix><color><Color alphaMultiplier=\"1\"/></color>",
    );
    let b = inst(
        "Art",
        "symbolType=\"graphic\"",
        "<matrix><Matrix tx=\"100\"/></matrix><color><Color alphaMultiplier=\"0\"/></color>",
    );
    let d = decode(archive(
        &timeline(
            "Scene",
            &(frame(0, 4, "tweenType=\"motion\"", &a) + &frame(4, 1, "", &b)),
        ),
        &[("Art.xml", "Art", symbol)],
        "",
    ));
    assert_eq!(d.layers[0].options.tweens.len(), 1);
    let marks = features::resolve_export(&d, 2, false);
    assert!((crate::vector::control_bounds(&marks[0].2).left() - 50.).abs() < 0.01);
    assert!((marks[0].2.fill.unwrap()[3] as i32 - 128).abs() <= 1);
    assert_eq!(
        d.layers[0].frames[0][0]
            .style
            .instance
            .as_ref()
            .unwrap()
            .born,
        d.layers[0].frames[4][0]
            .style
            .instance
            .as_ref()
            .unwrap()
            .born
    );
}
#[test]
fn animate_movie_clip_keeps_own_clock_under_one_frame_parent() {
    let child = timeline(
        "Clip",
        &(frame(0, 1, "", &shape("#FF0000")) + &frame(1, 1, "", &shape("#0000FF"))),
    );
    let parent = timeline(
        "Parent",
        &frame(0, 1, "", &inst("Clip", "symbolType=\"movie clip\"", "")),
    );
    let d = decode(archive(
        &timeline(
            "Main",
            &frame(0, 2, "", &inst("Parent", "symbolType=\"graphic\"", "")),
        ),
        &[
            ("Clip.xml", "Clip", child),
            ("Parent.xml", "Parent", parent),
        ],
        "",
    ));
    let marks = features::resolve_at(&d, 0, false, 1);
    assert!(
        marks
            .iter()
            .any(|(_, _, m)| m.fill == Some([0, 0, 255, 255]))
    );
}
#[test]
fn missing_media_and_unsupported_effects_leave_other_vectors_intact() {
    let elements = format!(
        "{}<DOMBitmapInstance libraryItemName=\"missing.png\"/>{}<DOMComponentInstance/>",
        shape("#CCAA11"),
        inst("missing", "", "<filters/>")
    );
    let d = decode(archive(
        &timeline("Scene", &frame(0, 2, "", &elements)),
        &[],
        "",
    ));
    assert_eq!(d.layers[0].frames[0].len(), 1);
    assert!(
        d.extras
            .import_warnings
            .iter()
            .any(|s| s.contains("DOMBitmapInstance"))
    );
    assert!(d.extras.imported);
}
#[test]
fn gradient_and_colored_background_survive_vector_import() {
    let mut s = shape("#FF0000");
    s=s.replace("<SolidColor color=\"#FF0000\"/>","<LinearGradient><matrix><Matrix a=\"0.006103515625\" d=\"0.006103515625\" tx=\"5\"/></matrix><GradientEntry ratio=\"0\" color=\"#FF0000\"/><GradientEntry ratio=\"1\" color=\"#0000FF\"/></LinearGradient>");
    let bytes = archive(&timeline("Scene", &frame(0, 1, "", &s)), &[], "");
    let root = crate::fla_zip::document(&bytes)
        .unwrap()
        .replace("backgroundColor=\"#FFFFFF\"", "backgroundColor=\"#223344\"");
    let d = decode(crate::fla_zip::pack(&[(
        "DOMDocument.xml",
        root.into_bytes(),
    )]));
    assert_eq!(d.layers.len(), 2);
    assert_eq!(d.layers[0].frames[0][0].fill, Some([34, 51, 68, 255]));
    let g = d.layers[1].frames[0][0].style.gradient.as_ref().unwrap();
    assert!((g.start[0]).abs() < 0.01);
    assert!((g.end[0] - 10.).abs() < 0.01);
    assert!(d.svg(0).contains("linearGradient"));
}
#[test]
fn malformed_element_is_skipped_and_fractional_fps_are_reported() {
    let xml = crate::fla_zip::document(&archive(
        &timeline(
            "Scene",
            &frame(
                0,
                1,
                "",
                &(shape("#00FF00") + "<DOMShape><matrix><Matrix a=\"NaN\"/></matrix></DOMShape>"),
            ),
        ),
        &[],
        "",
    ))
    .unwrap()
    .replace("frameRate=\"24\"", "frameRate=\"23.976\"");
    let d = decode(crate::fla_zip::pack(&[(
        "DOMDocument.xml",
        xml.into_bytes(),
    )]));
    assert_eq!(d.fps, 24);
    assert_eq!(d.layers[0].frames[0].len(), 1);
    assert!(
        d.extras
            .import_warnings
            .iter()
            .any(|s| s.contains("NaN") || s.contains("DOMShape"))
    );
}
#[test]
fn corrupt_native_metadata_falls_back_to_visible_xfl() {
    let bytes = archive(
        &timeline("Scene", &frame(0, 1, "", &shape("#112233"))),
        &[],
        "",
    );
    let xml = crate::fla_zip::document(&bytes).unwrap();
    let d = decode(crate::fla_zip::pack(&[
        ("DOMDocument.xml", xml.into_bytes()),
        ("LIBRETOON/document.json", b"broken".to_vec()),
    ]));
    assert_eq!(d.layers[0].frames[0][0].fill, Some([17, 34, 51, 255]));
    assert!(
        d.extras
            .import_warnings
            .iter()
            .any(|s| s.contains("Metadatos"))
    );
}
#[test]
fn recursive_animate_symbols_are_pruned_without_discarding_other_art() {
    let a = timeline(
        "A",
        &frame(0, 1000, "", &(inst("B", "", "") + &shape("#00FF00"))),
    );
    let b = timeline("B", &frame(0, 1000, "", &inst("A", "", "")));
    let d = decode(archive(
        &timeline("Main", &frame(0, 2, "", &inst("A", "", ""))),
        &[("A.xml", "A", a), ("B.xml", "B", b)],
        "",
    ));
    assert!(
        d.extras
            .import_warnings
            .iter()
            .any(|s| s.contains("recursivas"))
    );
    assert!(d.svg(0).contains("rgb(0,255,0)"));
}
#[test]
fn unimplemented_motion_keeps_keys_and_exposures_with_a_report() {
    let d = decode(archive(
        &timeline(
            "Main",
            &(frame(0, 4, "tweenType=\"shape\"", &shape("#FF0000"))
                + &frame(4, 1, "", &shape("#0000FF"))),
        ),
        &[],
        "",
    ));
    assert!(d.layers[0].options.tweens.is_empty());
    assert_eq!(d.layers[0].frames.len(), 5);
    assert!(
        d.extras
            .import_warnings
            .iter()
            .any(|s| s.contains("Interpolación shape"))
    );
}
fn raw_symbol(id: u64, child: Option<u64>) -> Symbol {
    let mark = child.map(|id| {
        let mut m = Mark::default();
        m.style.instance = Some(Instance {
            symbol: id,
            matrix: crate::rig::IDENTITY,
            mode: Playback::Loop,
            first: 0,
            born: 0,
            clock: SymbolClock::Scene,
        });
        m
    });
    Symbol {
        id,
        name: format!("S{id}"),
        rig: Default::default(),
        folders: Default::default(),
        perspective: Default::default(),
        library_folder: None,
        mouth_poses: Default::default(),
        layers: vec![Layer {
            name: "Art".into(),
            visible: true,
            options: Default::default(),
            frames: vec![mark.into_iter().collect()].into(),
        }]
        .into(),
    }
}
#[test]
fn audit_symbol_depth_is_independent_of_library_order() {
    let mut d = Document::default();
    for id in (1..=9).rev() {
        d.extras
            .symbols
            .push(raw_symbol(id, (id < 9).then_some(id + 1)));
    }
    assert!(d.validate().is_err());
    d.extras.symbols.remove(0);
    for s in &mut d.extras.symbols {
        for l in &mut s.layers {
            for f in &mut l.frames {
                f.retain(|m| m.style.instance.as_ref().is_none_or(|i| i.symbol != 9));
            }
        }
    }
    assert!(d.validate().is_ok());
}
#[test]
fn audit_exponential_symbol_expansion_is_rejected_before_render() {
    let mut d = Document::default();
    for id in 1..=7 {
        let mut s = raw_symbol(id, if id < 7 { Some(id + 1) } else { None });
        if id < 7 {
            let m = s.layers[0].frames[0][0].clone();
            s.layers[0].frames[0] = vec![m; 30].into();
        } else {
            s.layers[0].frames[0] = vec![Mark::default()].into();
        }
        d.extras.symbols.push(s);
    }
    assert!(d.validate().unwrap_err().contains("Expanded"));
}
#[test]
fn audit_empty_or_out_of_range_frame_lookup_is_safe() {
    let mut l = Document::default().layers[0].clone();
    assert!(crate::animation::marks(&l, usize::MAX).is_empty());
    l.frames.clear();
    assert!(crate::animation::marks(&l, 10).is_empty());
}
#[test]
fn audit_clipboard_rejects_same_id_different_symbol_definition_and_refreshes_tracks() {
    let mut e = crate::Editor::new(&eframe::CreationContext::_new_kittest(
        eframe::egui::Context::default(),
    ));
    e.recovery = std::env::temp_dir().join(format!(
        "compat-edit-{}-{:?}.json",
        std::process::id(),
        std::thread::current().id()
    ));
    e.doc = Document::default();
    e.doc.extras.symbols.push(raw_symbol(1, None));
    let mut m = Mark::default();
    m.style.track = 42;
    m.style.instance = Some(Instance {
        symbol: 1,
        matrix: crate::rig::IDENTITY,
        mode: Playback::Loop,
        first: 0,
        born: 0,
        clock: SymbolClock::Scene,
    });
    e.doc.layers[0].frames[0].push(m);
    e.canvas.selected = vec![(0, 0)];
    e.copy_selection(false);
    let mut other = raw_symbol(1, None);
    other.name = "Different".into();
    other.layers[0].frames[0].push(Mark::default());
    e.doc.extras.symbols[0] = other;
    e.paste_selection();
    assert_eq!(e.doc.layers[0].frames[0].len(), 1);
    e.doc.extras.symbols = e.studio.clipboard_symbols.clone();
    e.paste_selection();
    assert_eq!(e.doc.layers[0].frames[0].len(), 2);
    assert_ne!(e.doc.layers[0].frames[0][1].style.track, 42);
}
#[test]
fn audit_archive_index_checks_crc_duplicates_and_local_header_consistency() {
    let b = crate::fla_zip::pack(&[
        ("DOMDocument.xml", b"<DOMDocument/>".to_vec()),
        ("LIBRARY/a.xml", b"abc".to_vec()),
    ]);
    let index = crate::fla_zip::Archive::new(&b).unwrap();
    assert_eq!(index.entry(b"LIBRARY/a.xml").unwrap().unwrap(), b"abc");
    let mut bad = b.clone();
    bad[8] = 8;
    assert!(crate::fla_zip::entry(&bad, b"DOMDocument.xml").is_err());
    let duplicates = crate::fla_zip::pack(&[("same", vec![]), ("same", vec![])]);
    assert!(crate::fla_zip::Archive::new(&duplicates).is_err());
}
#[test]
fn malformed_xml_and_legacy_binary_fla_fail_clearly() {
    let bad = crate::fla_zip::pack(&[("DOMDocument.xml", b"<DOMDocument><".to_vec())]);
    assert!(crate::fla::decode(&bad).is_err());
    assert!(
        crate::fla::decode(b"\xd0\xcf\x11\xe0old FLA")
            .err()
            .unwrap()
            .contains("CS5")
    );
}
#[test]
fn compressed_animate_container_is_supported() {
    use std::io::Write;
    let xml = crate::fla_zip::document(&archive(
        &timeline("Scene", &frame(0, 2, "", &shape("#010203"))),
        &[],
        "",
    ))
    .unwrap();
    let mut encoder =
        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(xml.as_bytes()).unwrap();
    let compressed = encoder.finish().unwrap();
    let mut zip = crate::fla_zip::pack(&[("DOMDocument.xml", compressed)]);
    let center = zip
        .windows(4)
        .position(|w| w == [0x50, 0x4b, 1, 2])
        .unwrap();
    zip[8..10].copy_from_slice(&8u16.to_le_bytes());
    zip[center + 10..center + 12].copy_from_slice(&8u16.to_le_bytes());
    let crc = crc32fast::hash(xml.as_bytes()).to_le_bytes();
    zip[14..18].copy_from_slice(&crc);
    zip[center + 16..center + 20].copy_from_slice(&crc);
    let len = (xml.len() as u32).to_le_bytes();
    zip[22..26].copy_from_slice(&len);
    zip[center + 24..center + 28].copy_from_slice(&len);
    let d = decode(zip);
    assert_eq!(d.layers[0].frames[0][0].fill, Some([1, 2, 3, 255]));
}
#[test]
#[ignore = "manual audit benchmark; reports CPU validation only"]
fn benchmark_audit_shared_validation() {
    let mut d = Document::default();
    let m = Mark {
        points: vec![[0., 0.], [100., 0.], [100., 100.], [0., 100.]].into(),
        closed: true,
        fill: Some([100, 150, 200, 255]),
        ..Default::default()
    };
    let frame: crate::shared::SharedVec<_> = vec![m; 200].into();
    d.layers[0].frames = vec![frame; 10000].into();
    let start = std::time::Instant::now();
    for _ in 0..10 {
        std::hint::black_box(d.validate().unwrap());
    }
    eprintln!(
        "audit validation: {:?}/document, 200 marks held for 10000 frames, 10 iterations",
        start.elapsed() / 10
    );
}
#[test]
fn utf16_xfl_with_bom_is_supported() {
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-16\"?>{}",
        crate::fla_zip::document(&archive(
            &timeline("Escena ñ", &frame(0, 1, "", &shape("#123456"))),
            &[],
            ""
        ))
        .unwrap()
    );
    let mut data = vec![0xff, 0xfe];
    for word in xml.encode_utf16() {
        data.extend(word.to_le_bytes());
    }
    let d = decode(crate::fla_zip::pack(&[("DOMDocument.xml", data)]));
    assert_eq!(d.layers[0].frames[0][0].fill, Some([18, 52, 86, 255]));
}
#[test]
fn pasted_rigged_symbol_keeps_pose_with_independent_track_and_binding() {
    let mut e = crate::Editor::new(&eframe::CreationContext::_new_kittest(
        eframe::egui::Context::default(),
    ));
    e.recovery = std::env::temp_dir().join(format!("compat-rig-paste-{}.json", std::process::id()));
    e.doc = Document::default();
    let mut s = raw_symbol(1, None);
    s.layers[0].frames[0].push(Mark {
        points: vec![[0., 0.], [100., 100.]].into(),
        rectangle: true,
        closed: true,
        fill: Some([100, 150, 200, 255]),
        stroke: false,
        ..Default::default()
    });
    e.doc.extras.symbols.push(s);
    let mut m = Mark {
        points: vec![[0., 0.], [100., 100.]].into(),
        rectangle: true,
        closed: true,
        stroke: false,
        fill: Some([0, 0, 0, 0]),
        ..Default::default()
    };
    m.style.track = 42;
    m.style.instance = Some(Instance {
        symbol: 1,
        matrix: crate::rig::IDENTITY,
        mode: Playback::Loop,
        first: 0,
        born: 0,
        clock: SymbolClock::Scene,
    });
    e.doc.layers[0].frames[0].push(m);
    e.add_bone(
        eframe::egui::Pos2::new(0., 50.),
        eframe::egui::Pos2::new(100., 50.),
        None,
    );
    let bone = e.rigging.bone.unwrap();
    e.doc.extras.rig.set_bone_key(
        bone,
        crate::rig::BoneKey {
            angle: 0.5,
            ..Default::default()
        },
    );
    e.canvas.selected = vec![(0, 0)];
    e.copy_selection(false);
    e.paste_selection();
    assert_eq!(e.doc.extras.rig.bindings.len(), 2);
    let resolved = features::resolve_export(&e.doc, 0, false);
    assert_eq!(resolved.len(), 2);
    let a = crate::vector::control_bounds(&resolved[0].2);
    let b = crate::vector::control_bounds(&resolved[1].2);
    assert!(a.min.distance(b.min) < 0.001);
    assert!(e.doc.validate().is_ok());
}
#[test]
fn legacy_native_json_with_fla_extension_remains_readable() {
    let path = std::env::temp_dir().join(format!("lt-legacy-json-{}.fla", std::process::id()));
    let mut doc = Document::default();
    doc.fps = 37;
    std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
    assert_eq!(crate::documents::read_project(&path).unwrap().fps, 37);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn nested_movie_clip_starts_when_its_parent_enters_the_scene() {
    let child = timeline(
        "Clip",
        &(frame(0, 1, "", &shape("#FF0000")) + &frame(1, 1, "", &shape("#0000FF"))),
    );
    let parent = timeline(
        "Parent",
        &frame(0, 1, "", &inst("Clip", "symbolType=\"movie clip\"", "")),
    );
    let d = decode(archive(
        &timeline(
            "Main",
            &frame(19, 2, "", &inst("Parent", "symbolType=\"graphic\"", "")),
        ),
        &[
            ("Clip.xml", "Clip", child),
            ("Parent.xml", "Parent", parent),
        ],
        "",
    ));
    assert!(crate::animation::movie_svg(&d, 19).contains("rgb(255,0,0)"));
    assert!(crate::animation::movie_svg(&d, 20).contains("rgb(0,0,255)"));
}
#[test]
fn absent_symbol_type_uses_animate_default_movie_clip_clock() {
    let child = timeline(
        "Clip",
        &(frame(0, 1, "", &shape("#FF0000")) + &frame(1, 1, "", &shape("#0000FF"))),
    );
    let parent = timeline("Parent", &frame(0, 1, "", &inst("Clip", "", "")));
    let d = decode(archive(
        &timeline(
            "Main",
            &frame(0, 2, "", &inst("Parent", "symbolType=\"graphic\"", "")),
        ),
        &[
            ("Clip.xml", "Clip", child),
            ("Parent.xml", "Parent", parent),
        ],
        "",
    ));
    assert!(crate::animation::movie_svg(&d, 1).contains("rgb(0,0,255)"));
}

#[test]
#[ignore = "manual external FLA fixture; set LIBRETOON_IMPORT_FIXTURES"]
fn external_student_fla_import() {
    let paths = std::env::var("LIBRETOON_IMPORT_FIXTURES").expect("semicolon separated FLA paths");
    let mut failures = Vec::new();
    for path in paths.split(';') {
        let bytes = std::fs::read(path).unwrap();
        match crate::documents::read_project(std::path::Path::new(path)) {
            Ok(d) => {
                println!(
                    "{}: {}x{} fps={} scenes={} layers={} symbols={} frames={} warnings={:?}",
                    path,
                    d.width,
                    d.height,
                    d.fps,
                    d.extras.scenes.len(),
                    d.layers.len(),
                    d.extras.symbols.len(),
                    d.layers[0].frames.len(),
                    d.extras.import_warnings
                );
                assert!(d.validate().is_ok());
                let count = (0..d.layers[0].frames.len())
                    .step_by(10)
                    .map(|f| crate::features::resolve(&d, f, false).len())
                    .sum::<usize>();
                println!(
                    "sampled visible marks: {count}; embedded media: {}",
                    d.extras.media.assets.len()
                );
                assert!(count > 0, "import must preserve visible artwork");
                let ctx = eframe::egui::Context::default();
                let mut cache = crate::paint::RenderCache::default();
                let sample_frames = [
                    0,
                    d.layers[0].frames.len() / 2,
                    d.layers[0].frames.len() - 1,
                ];
                let rendered: Vec<_> = sample_frames
                    .iter()
                    .map(|&f| {
                        let pixels = crate::exporter::raster(&d, f, 320, 180, &ctx, &mut cache);
                        (
                            pixels
                                .chunks_exact(3)
                                .filter(|p| *p != [255, 255, 255])
                                .count(),
                            crc32fast::hash(&pixels),
                        )
                    })
                    .collect();
                println!("rendered frame samples (nonwhite pixels, CRC): {rendered:?}");
                assert!(rendered.iter().any(|(count, _)| *count > 0));
                assert!(
                    rendered.windows(2).any(|w| w[0].1 != w[1].1),
                    "animation samples must change"
                );
                let saved =
                    crate::native_project::encode(&d).expect("import should save as a native copy");
                let restored = crate::native_project::decode_if_native(&saved)
                    .expect("native copy should reopen")
                    .unwrap();
                assert_eq!(restored.layers.len(), d.layers.len());
                assert_eq!(restored.layers[0].frames.len(), d.layers[0].frames.len());
                assert_eq!(restored.extras.symbols.len(), d.extras.symbols.len());
                assert_eq!(
                    std::fs::read(path).unwrap(),
                    bytes,
                    "source file must remain untouched"
                );
            }
            Err(e) => {
                println!("{path}: ERROR {e}");
                failures.push(e);
            }
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn animate_directory_size_quirk_recovers_art_and_reports_it_without_relaxing_integrity() {
    let original = archive(
        &timeline("Main", &frame(0, 3, "", &shape("#123456"))),
        &[],
        "",
    );
    let xml = crate::fla_zip::document(&original).unwrap();
    let mut bytes = crate::fla_zip::pack(&[
        ("DOMDocument.xml", xml.into_bytes()),
        ("mimetype", b"application/x-fla".to_vec()),
    ]);
    let end = bytes.len() - 22;
    let size = u32::from_le_bytes(bytes[end + 12..end + 16].try_into().unwrap());
    bytes[end + 12..end + 16].copy_from_slice(&(size + 54).to_le_bytes());
    let d = crate::fla::decode(&bytes).unwrap();
    assert_eq!(d.layers[0].frames.len(), 3);
    assert_eq!(d.layers[0].frames[0][0].fill, Some([0x12, 0x34, 0x56, 255]));
    assert!(
        d.extras
            .import_warnings
            .iter()
            .any(|w| w.contains("Directorio ZIP"))
    );
    let mut wrong = bytes.clone();
    wrong[end + 12..end + 16].copy_from_slice(&(size + 53).to_le_bytes());
    assert!(crate::fla::decode(&wrong).is_err());
    let mut crc = bytes.clone();
    crc[30 + "DOMDocument.xml".len() + 20] ^= 1;
    assert!(crate::fla::decode(&crc).is_err());
    let mut truncated = bytes.clone();
    truncated.drain(end - 1..end);
    assert!(crate::fla::decode(&truncated).is_err());
    let mut without_marker = original.clone();
    let end = without_marker.len() - 22;
    let size = u32::from_le_bytes(without_marker[end + 12..end + 16].try_into().unwrap());
    without_marker[end + 12..end + 16].copy_from_slice(&(size + 54).to_le_bytes());
    assert!(crate::fla::decode(&without_marker).is_err());
}

#[test]
fn animate_import_keeps_wav_on_its_scene_and_masked_art_and_guides() {
    let mask = shape("#FFFFFF");
    let art = shape("#FF0000").replace("200", "400");
    let guide = shape("#0000FF");
    let timeline = format!(
        r#"<DOMTimeline name="Shot"><layers>
        <DOMLayer name="Mask" layerType="mask"><frames>{}</frames></DOMLayer>
        <DOMLayer name="Art" layerType="masked" parentLayerIndex="0"><frames>{}</frames></DOMLayer>
        <DOMLayer name="Guide" layerType="guide"><frames>{}</frames></DOMLayer>
        </layers></DOMTimeline>"#,
        frame(0, 3, "", &mask),
        frame(0, 3, r#"soundName="voice.wav" soundSync="stream""#, &art),
        frame(0, 3, "", &guide)
    );
    let xml = format!(
        r#"<DOMDocument width="64" height="64" frameRate="24">
        <media><DOMSoundItem name="voice.wav" href="voice.wav"/></media>
        <timelines>{timeline}</timelines></DOMDocument>"#
    );
    let wav = crate::media::test_wav();
    let imported = decode(crate::fla_zip::pack(&[
        ("DOMDocument.xml", xml.into_bytes()),
        ("LIBRARY/voice.wav", wav),
    ]));
    assert_eq!(imported.extras.media.assets.len(), 1);
    assert_eq!(imported.extras.media.tracks.len(), 1);
    assert!(
        imported
            .layers
            .iter()
            .any(|layer| layer.frames[0].iter().any(|mark| mark.style.guide))
    );
    let marks = features::resolve_export(&imported, 0, false);
    assert!(!marks.is_empty());
    assert!(
        marks
            .iter()
            .all(|(_, _, mark)| mark.fill == Some([255, 0, 0, 255])
                && crate::vector::control_bounds(mark).max.x <= 10.01)
    );
    let reopened = crate::fla::decode(&crate::fla::encode(&imported).unwrap()).unwrap();
    assert_eq!(reopened.extras.media.tracks.len(), 1);
    assert_eq!(
        features::resolve_export(&reopened, 0, false).len(),
        marks.len()
    );
}
