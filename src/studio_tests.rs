use crate::{Editor, Mark, canvas::Tool};
use eframe::egui::{self, Pos2, Rect, Vec2};

fn editor() -> Editor {
    let ctx = egui::Context::default();
    let mut e = Editor::new(&eframe::CreationContext::_new_kittest(ctx));
    e.recovery = std::env::temp_dir().join(format!(
        "studio-test-{}-{:?}.json",
        std::process::id(),
        std::thread::current().id()
    ));
    e.doc = crate::fresh_document();
    e
}
fn ink() -> Mark {
    Mark {
        points: vec![[0., 0.], [60., 60.]].into(),
        rectangle: true,
        closed: true,
        fill: Some([10, 80, 200, 255]),
        stroke: false,
        ..Default::default()
    }
}

#[test]
fn inspecting_transform_does_not_modify_thin_strokes_on_later_frames() {
    let mut editor = editor();
    let ctx = egui::Context::default();
    editor.frame = 2;
    let reference = Mark {
        points: vec![[10., 100.], [10.001, 100.], [10.001, 200.]].into(),
        ..Mark::default()
    };
    let stroke = Mark {
        points: vec![[100., 100.], [500., 100.], [500., 100.]].into(),
        ..Mark::default()
    };
    for track in [0, 1] {
        let mut reference = reference.clone();
        let mut stroke = stroke.clone();
        reference.style.track = track;
        stroke.style.track = track;
        editor.doc.layers[0].frames[0] = vec![reference].into();
        editor.doc.layers[0].frames[2] = vec![stroke.clone()].into();
        editor.canvas.selected = vec![(0, 0)];
        ctx.run_ui(egui::RawInput::default(), |ui| {
            editor.motion_transform_ui(ui)
        })
        .textures_delta
        .clear();
        assert!(editor.doc.layers[0].frames[2][0] == stroke);
        assert!(editor.undo.is_empty());
    }
}

#[test]
fn switch_object_folder_uses_the_keyed_child_and_survives_save() {
    use crate::features::{ObjectFolder, ObjectFolderMode};
    let mut editor = editor();
    let layer = &mut editor.doc.layers[0];
    let mut first = ink();
    first.style.object_folder = Some([1, 10]);
    let mut second = ink();
    second.style.object_folder = Some([1, 11]);
    second.fill = Some([200, 0, 0, 255]);
    layer.frames[0] = vec![first, second].into();
    layer.frames[1] = layer.frames[0].clone();
    layer.options.object_folders.push(ObjectFolder {
        id: 1,
        name: "Mouths".into(),
        mode: ObjectFolderMode::Switch,
        choices: vec![(0, 10), (1, 11)].into(),
    });
    let restored: crate::Document =
        serde_json::from_slice(&serde_json::to_vec(&editor.doc).unwrap()).unwrap();
    for (frame, object) in [(0, 0), (1, 1)] {
        let marks = crate::features::resolve(&restored, frame, false);
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].1, object);
    }
}

#[test]
fn cut_and_paste_vector_and_symbol_to_another_layer() {
    let mut editor = editor();
    editor.doc.layers[0].frames[0].push(ink());
    editor.canvas.selected = vec![(0, 0)];
    editor.convert_symbol();
    editor.doc.layers[0].frames[0].push(ink());
    let mut target = editor.doc.layers[0].clone();
    for cel in &mut target.frames {
        cel.clear();
    }
    editor.doc.layers.push(target);
    editor.canvas.selected = vec![(0, 0), (0, 1)];
    editor.copy_selection(false);
    editor.layer = 1;
    editor.paste_selection();
    assert_eq!(editor.doc.layers[0].frames[0].len(), 2);
    assert_eq!(editor.doc.layers[1].frames[0].len(), 2);
    assert!(editor.doc.layers[1].frames[0][0].style.instance.is_some());
    editor.canvas.selected = vec![(0, 0), (0, 1)];
    editor.copy_selection(true);
    editor.layer = 1;
    editor.paste_selection();
    assert!(editor.doc.layers[0].frames[0].is_empty());
    assert_eq!(editor.doc.layers[1].frames[0].len(), 4);
}

#[test]
fn document_switch_preserves_art_history_view_and_symbol_context() {
    let mut e = editor();
    e.add_document(crate::fresh_document(), None);
    e.checkpoint();
    e.doc.layers[0].frames[0].push(ink());
    e.canvas.pan = Vec2::new(90., 40.);
    e.canvas.zoom = 2.;
    let id = e.doc.layers.identity();
    let points = e.doc.layers[0].frames[0][0].points.identity();
    e.add_document(crate::fresh_document(), None);
    e.checkpoint();
    e.doc.layers[0].frames[0].push(Mark {
        color: [255, 0, 0, 255],
        ..ink()
    });
    e.activate_document(0);
    assert_eq!(e.doc.layers.identity(), id);
    assert_eq!(e.doc.layers[0].frames[0][0].points.identity(), points);
    assert_eq!(e.canvas.pan, Vec2::new(90., 40.));
    assert_eq!(e.canvas.zoom, 2.);
    e.history(false);
    assert!(e.doc.layers[0].frames[0].is_empty());
    e.activate_document(1);
    assert_eq!(e.doc.layers[0].frames[0][0].color, [255, 0, 0, 255]);
    e.canvas.selected = vec![(0, 0)];
    e.convert_symbol();
    let symbol = e.doc.extras.symbols[0].id;
    e.enter_symbol(symbol);
    e.activate_document(0);
    e.activate_document(1);
    assert_eq!(e.feature.symbol_stack.len(), 1);
    assert_eq!(e.project_document().extras.symbols[0].id, symbol);
}

#[test]
fn startup_always_uses_home_and_many_documents_are_scrollable() {
    let mut e = editor();
    assert!(e.hub.home);
    for _ in 0..30 {
        e.add_document(crate::fresh_document(), None);
    }
    let ctx = egui::Context::default();
    let mut width = 0.;
    for _ in 0..2 {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1100., 740.))),
                ..Default::default()
            },
            |ui| {
                e.document_tabs(ui);
                width = ui.clip_rect().width();
            },
        );
        out.textures_delta.clear();
    }
    assert_eq!(e.hub.tabs.len(), 30);
    assert!(width <= 1100.);
    assert!(e.hub.tab_extent.0 > e.hub.tab_extent.1 * 3.);
    e.activate_document(0);
    assert!(!e.hub.home);
    e.hub.home = true;
    assert_eq!(e.hub.tabs.len(), 30);
}

#[test]
fn recovered_documents_are_lazy_and_home_stays_selected() {
    let mut e = editor();
    e.add_document(crate::fresh_document(), None);
    e.checkpoint();
    e.doc.layers[0].frames[0].push(ink());
    e.recover();
    e.add_document(crate::fresh_document(), None);
    e.checkpoint();
    e.doc.layers[0].name = "Segundo documento".into();
    e.recover();
    e.persist_documents();
    drop(e);
    let ctx = egui::Context::default();
    let mut restored = Editor::new(&eframe::CreationContext::_new_kittest(ctx.clone()));
    assert!(restored.hub.home);
    assert!(restored.hub.active.is_none());
    assert_eq!(restored.hub.tabs.len(), 2);
    assert!(restored.doc.layers[0].frames[0].is_empty());
    let activate = |editor: &mut Editor, index| {
        editor.activate_document(index);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while editor.hub.active != Some(index) {
            assert!(std::time::Instant::now() < deadline, "{}", editor.status);
            editor.poll_open(&ctx);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    };
    activate(&mut restored, 0);
    assert_eq!(restored.doc.layers[0].frames[0].len(), 1);
    activate(&mut restored, 1);
    assert_eq!(restored.doc.layers[0].name, "Segundo documento");
}

#[test]
fn horizontal_wheel_reaches_documents_beyond_the_visible_tabs() {
    let mut e = editor();
    for _ in 0..30 {
        e.add_document(crate::fresh_document(), None);
    }
    e.activate_document(0);
    let ctx = egui::Context::default();
    for i in 0..8 {
        let events = if i >= 3 {
            vec![
                egui::Event::PointerMoved(Pos2::new(500., 20.)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Unknown,
                    delta: Vec2::new(-300., 0.),
                    modifiers: Default::default(),
                },
            ]
        } else {
            vec![]
        };
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1100., 740.))),
                events,
                ..Default::default()
            },
            |ui| e.document_tabs(ui),
        );
        out.textures_delta.clear();
    }
    assert!(e.hub.tab_offset > 100., "offset {}", e.hub.tab_offset);
    assert_eq!(e.hub.tabs.len(), 30);
}

#[test]
fn rounded_rectangle_keeps_real_editable_curves_and_roundtrips() {
    let mut d = crate::fresh_document();
    let m = crate::drawing::rounded_rectangle(Pos2::ZERO, Pos2::new(120., 80.), 15., &ink());
    assert_eq!(m.nodes.len(), 8);
    assert!(!m.rectangle);
    assert!(!crate::vector::contains_fill(&m, Pos2::new(1., 1.)));
    assert!(crate::vector::contains_fill(&m, Pos2::new(60., 40.)));
    d.layers[0].frames[0].push(m.clone());
    let copy = crate::fla::decode(&crate::fla::encode(&d).unwrap()).unwrap();
    assert_eq!(copy.layers[0].frames[0][0].nodes, m.nodes);
    assert!(copy.svg(0).contains(" C"));
}

#[test]
fn sketch_and_mannequin_are_saved_but_never_exported_even_in_symbols() {
    let mut e = editor();
    let mut mark = ink();
    mark.style.guide = true;
    e.doc.layers[0].frames[0].push(mark);
    assert_eq!(crate::features::resolve(&e.doc, 0, false).len(), 1);
    assert!(crate::animation::resolve_movie(&e.doc, 0, true).is_empty());
    e.add_mannequin(Pos2::new(300., 250.));
    e.add_mannequin(Pos2::ZERO);
    assert_eq!(
        e.doc
            .layers
            .iter()
            .filter(|l| l.options.mannequin.is_some())
            .count(),
        1
    );
    let copy = crate::fla::decode(&crate::fla::encode(&e.project_document()).unwrap()).unwrap();
    assert!(copy.layers[0].frames[0][0].style.guide);
    assert!(copy.layers[1].options.mannequin.is_some());
    assert!(!copy.svg(0).contains("rgb(10,80,200)"));
    let baked = crate::features::baked(&copy);
    assert!(baked.layers[0].frames[0].is_empty());
    e.layer = 0;
    e.canvas.selected = vec![(0, 0)];
    e.convert_symbol();
    assert!(crate::animation::resolve_movie(&e.doc, 0, false).is_empty());
}

#[test]
fn airbrush_soft_edge_and_graphite_grain_are_deterministic() {
    let center = Mark {
        points: vec![[10., 20.], [80., 20.]].into(),
        width: 20.,
        color: [50, 100, 200, 255],
        ..Default::default()
    };
    let spray = crate::drawing::airbrush(&center, 0.5, 0.2, false);
    assert!(spray.len() > 4);
    let g = spray[0].style.gradient.as_ref().unwrap();
    assert!(g.radial);
    assert_eq!(g.stops.last().unwrap().color[3], 0);
    assert!(g.stops[0].color[3] > 0);
    let a = crate::drawing::airbrush(&center, 0.5, 0.2, true);
    let b = crate::drawing::airbrush(&center, 0.5, 0.2, true);
    assert_eq!(
        serde_json::to_value(&a).unwrap(),
        serde_json::to_value(&b).unwrap()
    );
    assert!(a.iter().all(|m| m.style.guide));
}

#[test]
fn mannequin_pose_keeps_lengths_when_joint_rotates_and_all_presets_validate() {
    let mut m = crate::mannequin::Mannequin::default();
    let (a, _) = m.pose();
    m.angles[5] += 0.9;
    let (b, _) = m.pose();
    assert!((a[5].distance(a[4]) - b[5].distance(b[4])).abs() < 0.001);
    assert!((a[6].distance(a[5]) - b[6].distance(b[5])).abs() < 0.001);
    assert!(a[6].distance(b[6]) > 10.);
    assert_eq!(a[8], b[8]);
    for heads in [3, 4, 6, 7, 8] {
        m.heads = heads;
        assert!(m.valid());
        assert!(m.pose().0.iter().all(|p| p.is_finite()));
    }
}

#[test]
fn editable_text_preserves_content_spacing_gradient_and_transform_without_font_on_reload() {
    let fonts = crate::typography::system_fonts();
    let (_, font) = fonts.first().expect("Test machine needs a TTF/OTF font");
    let mut s = crate::typography::TextSpec {
        content: "BOO\nTexto".into(),
        font: font.to_string_lossy().into_owned(),
        outline: true,
        letter_spacing: 3.,
        word_spacing: 7.,
        ..Default::default()
    };
    s.gradient = Some(Default::default());
    let m = crate::typography::geometry(&s).unwrap();
    let bounds = crate::vector::control_bounds(&m);
    assert!(bounds.width() > 50. && bounds.height() > 50.);
    let shifted = crate::vector::transform(&m, |p| p + Vec2::new(70., 50.));
    assert!((shifted.style.text.as_ref().unwrap().matrix[4] - s.matrix[4] - 70.).abs() < 0.01);
    let mut doc = crate::fresh_document();
    doc.layers[0].frames[0].push(shifted);
    let copy = crate::fla::decode(&crate::fla::encode(&doc).unwrap()).unwrap();
    let text = copy.layers[0].frames[0][0].style.text.as_ref().unwrap();
    assert_eq!(text.content, s.content);
    assert_eq!(text.word_spacing, 7.);
    assert!(copy.svg(0).contains("linearGradient"));
    let mut portable = copy.clone();
    portable.layers[0].frames[0][0]
        .style
        .text
        .as_mut()
        .unwrap()
        .font = "missing-font.ttf".into();
    assert!(portable.validate().is_ok());
    assert!(portable.svg(0).contains("<path"));
    let output = crate::features::resolve_export(&portable, 0, false);
    assert_eq!(output.len(), 1);
}

#[test]
fn locked_reference_layer_cannot_receive_text_or_paste() {
    let mut e = editor();
    e.add_mannequin(Pos2::ZERO);
    let before = serde_json::to_value(&e.doc).unwrap();
    e.studio.clipboard = vec![ink()];
    e.paste_selection();
    e.commit_text();
    assert_eq!(serde_json::to_value(&e.doc).unwrap(), before);
}

#[test]
fn tool_properties_and_reference_canvas_render_without_panics() {
    let mut e = editor();
    let ctx = egui::Context::default();
    for tool in [
        Tool::Text,
        Tool::Rectangle,
        Tool::Airbrush,
        Tool::SketchBrush,
        Tool::Mannequin,
    ] {
        e.tool = tool;
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1400., 900.))),
                ..Default::default()
            },
            |ui| {
                egui::Panel::right("property-test")
                    .exact_size(305.)
                    .show(ui, |ui| e.properties_ui(ui));
                egui::CentralPanel::default().show(ui, |ui| e.canvas_ui(ui));
            },
        );
        assert!(!out.shapes.is_empty());
        out.textures_delta.clear();
    }
}

#[test]
fn real_pressure_changes_the_saved_width_profile_and_mouse_stays_constant() {
    let mut d = crate::drawing::Drawing::default();
    d.pen.test_pressure(None);
    d.start(Pos2::ZERO, Tool::Brush);
    d.sample(Pos2::new(40., 0.));
    assert!(d.profile(&[]).is_empty());
    d.pen.test_pressure(Some(0.2));
    d.start(Pos2::ZERO, Tool::Brush);
    d.pen.test_pressure(Some(0.8));
    d.sample(Pos2::new(40., 0.));
    // Releasing the pen must not erase pressure collected during the stroke.
    d.pen.test_pressure(None);
    let profile = d.profile(&[]);
    assert!((profile[0].width - 0.2).abs() < 0.001);
    assert!((profile[127].width - 0.8).abs() < 0.001);
    d.pressure = false;
    d.start(Pos2::ZERO, Tool::Brush);
    assert!(d.profile(&[]).is_empty());
    let mark = Mark {
        points: vec![[0., 0.], [40., 0.]].into(),
        width_profile: profile,
        width: 20.,
        ..Default::default()
    };
    let area = crate::width::outline(&mark);
    assert!(!area.0.is_empty());
    let mut doc = crate::fresh_document();
    doc.layers[0].frames[0].push(mark);
    let restored = crate::fla::decode(&crate::fla::encode(&doc).unwrap()).unwrap();
    assert!((restored.layers[0].frames[0][0].width_profile[127].width - 0.8).abs() < 0.001);
}

#[test]
#[ignore = "manual UI review; writes PPM images to LIBRETOON_PREVIEWS"]
fn render_studio_review_images() {
    use std::{collections::HashMap, io::Write};
    let folder = std::env::var_os("LIBRETOON_PREVIEWS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("libretoon-previews"));
    std::fs::create_dir_all(&folder).unwrap();
    let mut e = editor();
    let ctx = egui::Context::default();
    crate::design::theme(&ctx);
    crate::i18n::set_language(true);
    e.add_document(crate::fresh_document(), None);
    e.doc.layers[0].frames[0].push(crate::drawing::rounded_rectangle(
        Pos2::new(80., 80.),
        Pos2::new(380., 260.),
        35.,
        &ink(),
    ));
    let fonts = crate::typography::system_fonts();
    if let Some((_, font)) = fonts
        .iter()
        .find(|(n, _)| n.to_lowercase().contains("sans"))
        .or(fonts.first())
    {
        e.drawing.text.spec.font = font.to_string_lossy().into_owned();
        e.drawing.text.spec.content = "Hello, LibreToon".into();
        e.drawing.text.spec.matrix[4] = 95.;
        e.drawing.text.spec.matrix[5] = 320.;
        e.drawing.text.spec.size = 38.;
        e.commit_text();
    }
    e.layer = 0;
    e.canvas.selected = vec![(0, 0)];
    e.convert_symbol();
    e.add_bone(Pos2::new(90., 170.), Pos2::new(230., 170.), None);
    let parent = e.rigging.bone;
    e.add_bone(Pos2::new(230., 170.), Pos2::new(370., 170.), parent);
    e.add_puppet_pin(0, 1, Pos2::new(120., 130.));
    e.add_puppet_pin(0, 1, Pos2::new(320., 220.));
    e.doc.extras.library.folders.push(crate::library::Folder {
        id: 900,
        name: "Personaje".into(),
        parent: None,
        color: [80, 190, 180, 255],
        collapsed: false,
    });
    e.doc.extras.symbols[0].library_folder = Some(900);
    e.add_mannequin(Pos2::new(1300., 580.));
    e.tool = Tool::Mannequin;
    let mut textures: HashMap<egui::TextureId, egui::ColorImage> = HashMap::new();
    for (name, home, tool) in [
        ("home", true, Tool::Select),
        ("mannequin", false, Tool::Mannequin),
        ("mannequin-3d", false, Tool::Mannequin),
        ("text-box", false, Tool::Text),
        ("new-project", true, Tool::Select),
        ("text", false, Tool::Text),
        ("sketch-brush", false, Tool::SketchBrush),
        ("bones", false, Tool::Bones),
        ("weights", false, Tool::Bones),
        ("puppet", false, Tool::Puppet),
        ("library", false, Tool::Select),
        ("paper", false, Tool::Brush),
        ("midnight", true, Tool::Select),
        ("forest", false, Tool::Brush),
        ("plum", false, Tool::Puppet),
        ("graphite", false, Tool::Bones),
        ("japanese", false, Tool::Text),
        ("chinese", false, Tool::Bones),
        ("korean", false, Tool::Airbrush),
        ("preferences-paper", false, Tool::Brush),
    ] {
        use crate::{i18n::Language, themes::Theme};
        let (theme, language) = match name {
            "paper" | "preferences-paper" => (Theme::Paper, Language::English),
            "midnight" => (Theme::Midnight, Language::English),
            "forest" => (Theme::Forest, Language::PortuguesePortugal),
            "plum" => (Theme::Plum, Language::French),
            "graphite" => (Theme::Graphite, Language::German),
            "japanese" => (Theme::Midnight, Language::Japanese),
            "chinese" => (Theme::Paper, Language::ChineseSimplified),
            "korean" => (Theme::Forest, Language::Korean),
            _ => (Theme::Studio, Language::Spanish),
        };
        e.studio.prefs.theme = theme;
        e.studio.prefs.language = Some(language);
        crate::themes::ensure(&ctx, theme);
        crate::i18n::set_locale(language);
        crate::i18n::ensure_fonts(&ctx, language);
        e.document_settings = None;
        if let Some(l) = e.mannequin_layer() {
            let m = e.doc.layers[l].options.mannequin.as_mut().unwrap();
            m.rotation = if name == "mannequin-3d" {
                [0.3, 0.85, 0.1]
            } else {
                [0.; 3]
            };
            m.depth_angles[5] = if name == "mannequin-3d" { 0.8 } else { 0. };
        }
        if name == "new-project" {
            e.new_document();
        }
        if name == "text-box" {
            if let Some((index, spec)) = e.doc.layers[0].frames[0]
                .iter()
                .enumerate()
                .find_map(|(i, m)| m.style.text.as_ref().map(|s| (i, (**s).clone())))
            {
                e.drawing.text.spec = spec;
                e.drawing.text.spec.content = "Editable text box\nLive properties".into();
                e.drawing.text.spec.box_size = Some([650., 240.]);
                e.drawing.text.spec.matrix = [1., 0., 0., 1., 450., 500.];
                e.drawing.text.editing = Some((0, index, 0));
                e.layer = 0;
                e.drawing.text.transaction = false;
                e.drawing.text.canvas_edit = true;
                e.commit_text();
            }
        }
        e.studio.preferences_open = name == "preferences-paper";
        e.drawing.reference_joint = if name == "mannequin-3d" { 4 } else { 0 };
        e.rigging.paint = name == "weights";
        e.rigging.create = name == "bones";
        e.feature.library = name == "library";
        e.hub.home = home;
        e.tool = tool;
        e.canvas.clear_selection();
        let frames = if matches!(name, "preferences-paper" | "new-project") {
            12
        } else {
            3
        };
        for frame in 0..frames {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1366., 768.))),
                    ..Default::default()
                },
                |ui| {
                    e.workspace(ui);
                    if !e.hub.home {
                        egui::CentralPanel::default()
                            .frame(
                                egui::Frame::new()
                                    .fill(crate::themes::color(egui::Color32::from_gray(29)))
                                    .inner_margin(0.),
                            )
                            .show(ui, |ui| e.canvas_ui(ui));
                    }
                },
            );
            for (id, deltas) in &output.textures_delta.set {
                for delta in deltas {
                    let egui::ImageData::Color(image) = &delta.image;
                    if let Some([x, y]) = delta.pos {
                        if let Some(old) = textures.get_mut(id) {
                            for row in 0..image.size[1] {
                                let start = (y + row) * old.size[0] + x;
                                old.pixels[start..start + image.size[0]].copy_from_slice(
                                    &image.pixels[row * image.size[0]..(row + 1) * image.size[0]],
                                );
                            }
                        }
                    } else {
                        textures.insert(*id, (**image).clone());
                    }
                }
            }
            output.textures_delta.clear();
            if frame + 1 != frames {
                continue;
            }
            let mut rgb = vec![24u8; 1366 * 768 * 3];
            for primitive in ctx.tessellate(output.shapes, output.pixels_per_point) {
                let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive else {
                    continue;
                };
                let texture = textures.get(&mesh.texture_id);
                for indices in mesh.indices.chunks_exact(3) {
                    let v = [
                        mesh.vertices[indices[0] as usize],
                        mesh.vertices[indices[1] as usize],
                        mesh.vertices[indices[2] as usize],
                    ];
                    let [a, b, c] = v.map(|v| v.pos);
                    let cross = |u: Vec2, v: Vec2| u.x * v.y - u.y * v.x;
                    let det = cross(b - a, c - a);
                    if det.abs() < 0.00001 {
                        continue;
                    }
                    let bounds = Rect::from_min_max(a.min(b).min(c), a.max(b).max(c))
                        .intersect(primitive.clip_rect);
                    for y in (bounds.top().floor().max(0.) as usize)
                        ..(bounds.bottom().ceil().max(0.) as usize).min(768)
                    {
                        for x in (bounds.left().floor().max(0.) as usize)
                            ..(bounds.right().ceil().max(0.) as usize).min(1366)
                        {
                            let p = Pos2::new(x as f32 + 0.5, y as f32 + 0.5);
                            let w = [
                                cross(b - p, c - p) / det,
                                cross(c - p, a - p) / det,
                                cross(a - p, b - p) / det,
                            ];
                            if w.iter().any(|v| *v < 0.) {
                                continue;
                            }
                            let color: [f32; 4] = std::array::from_fn(|ch| {
                                (0..3)
                                    .map(|i| v[i].color.to_array()[ch] as f32 * w[i] / 255.)
                                    .sum()
                            });
                            let uv = Pos2::ZERO
                                + v[0].uv.to_vec2() * w[0]
                                + v[1].uv.to_vec2() * w[1]
                                + v[2].uv.to_vec2() * w[2];
                            let sample = texture
                                .map(|t| {
                                    let x = ((uv.x * t.size[0] as f32) as usize).min(t.size[0] - 1);
                                    let y = ((uv.y * t.size[1] as f32) as usize).min(t.size[1] - 1);
                                    t.pixels[y * t.size[0] + x].to_array()
                                })
                                .unwrap_or([255; 4]);
                            let alpha = color[3] * sample[3] as f32 / 255.;
                            let at = (y * 1366 + x) * 3;
                            for ch in 0..3 {
                                rgb[at + ch] = (color[ch] * sample[ch] as f32
                                    + rgb[at + ch] as f32 * (1. - alpha))
                                    .clamp(0., 255.)
                                    as u8;
                            }
                        }
                    }
                }
            }
            let mut file = std::fs::File::create(folder.join(format!("{name}.ppm"))).unwrap();
            write!(file, "P6\n1366 768\n255\n").unwrap();
            file.write_all(&rgb).unwrap();
        }
    }
}

#[test]
fn preferences_render_from_home() {
    fn contains_text(shape: &egui::Shape, text: &str) -> bool {
        match shape {
            egui::Shape::Text(shape) => shape.galley.job.text.contains(text),
            egui::Shape::Vec(shapes) => shapes.iter().any(|shape| contains_text(shape, text)),
            _ => false,
        }
    }
    let mut e = editor();
    e.hub.home = true;
    e.studio.preferences_open = true;
    crate::i18n::set_locale(crate::i18n::Language::English);
    let ctx = egui::Context::default();
    let mut found = false;
    for _ in 0..3 {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1400., 900.))),
                ..Default::default()
            },
            |ui| e.workspace(ui),
        );
        found |= out
            .shapes
            .iter()
            .any(|shape| contains_text(&shape.shape, "Language"));
        out.textures_delta.clear();
    }
    assert!(
        found,
        "Home must render the language controls in Preferences"
    );
    assert!(e.hub.home);
}

#[test]
fn laptop_toolbar_keeps_color_controls_visible_and_preserves_category_order() {
    let mut e = editor();
    e.hub.home = false;
    crate::i18n::set_locale(crate::i18n::Language::English);
    let ctx = egui::Context::default();
    for height in [720., 768.] {
        let mut texts = vec![];
        for _ in 0..3 {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1366., height))),
                    ..Default::default()
                },
                |ui| e.workspace(ui),
            );
            texts = out
                .shapes
                .iter()
                .filter_map(|s| {
                    if let egui::Shape::Text(t) = &s.shape {
                        (t.pos.x < 76.).then_some((t.galley.job.text.clone(), t.pos.y, s.clip_rect))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            out.textures_delta.clear();
        }
        let categories = [
            "SELECTION",
            "EDIT",
            "PAINT",
            "VECTOR",
            "SHAPES",
            "TEXT",
            "CANVAS",
            "RIGGING",
            "COLOR",
        ];
        let mut previous = 0.;
        for category in categories {
            let (_, y, clip) = texts
                .iter()
                .find(|(text, _, _)| text == category)
                .unwrap_or_else(|| panic!("Missing {category}: {texts:?}"));
            assert!(*y >= previous);
            assert!(
                *y < clip.bottom(),
                "{category} is clipped at height {height}"
            );
            previous = *y;
        }
        assert!(
            previous + 85. < height,
            "Color controls must fit at height {height}"
        );
    }
}

#[test]
fn timeline_and_properties_resize_with_pointer_drags() {
    let mut e = editor();
    e.hub.home = false;
    let ctx = egui::Context::default();
    let tick = |e: &mut Editor, events: Vec<egui::Event>| {
        let mut panels = [Rect::NOTHING; 2];
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1366., 768.))),
                events,
                ..Default::default()
            },
            |ui| {
                e.workspace(ui);
                for (i, name) in ["timeline", "properties"].into_iter().enumerate() {
                    panels[i] =
                        egui::containers::panel::PanelState::load(&ctx, ui.scope_id().with(name))
                            .unwrap()
                            .outer_rect;
                }
            },
        );
        out.textures_delta.clear();
        panels
    };
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    };
    tick(&mut e, vec![]);
    let panels = tick(&mut e, vec![]);
    let a = Pos2::new(panels[0].center().x, panels[0].top());
    let b = a - Vec2::new(0., 70.);
    tick(&mut e, vec![egui::Event::PointerMoved(a)]);
    tick(&mut e, vec![button(a, true)]);
    tick(&mut e, vec![egui::Event::PointerMoved(b)]);
    tick(&mut e, vec![button(b, false)]);
    let resized = tick(&mut e, vec![]);
    assert!(resized[0].height() > panels[0].height() + 40.);
    let a = Pos2::new(resized[1].left(), resized[1].center().y);
    let b = a - Vec2::new(80., 0.);
    tick(&mut e, vec![egui::Event::PointerMoved(a)]);
    tick(&mut e, vec![button(a, true)]);
    tick(&mut e, vec![egui::Event::PointerMoved(b)]);
    tick(&mut e, vec![button(b, false)]);
    let final_panels = tick(&mut e, vec![]);
    assert!(final_panels[1].width() > resized[1].width() + 40.);
}
