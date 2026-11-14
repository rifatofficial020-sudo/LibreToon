use crate::{
    Document, Editor,
    animation::Curve,
    canvas::Tool,
    document::{Layer, Mark},
    features::{Instance, Playback, Symbol, SymbolClock},
    rig::*,
    vector,
};
use eframe::egui::{self, Pos2, Rect, Vec2};

#[test]
fn character_turn_channels_compose_with_bone_keys_and_round_trip() {
    let mut rig = Rig::default();
    rig.bones.push(bone(1, None, [0., 0.], [10., 0.]));
    rig.bones.push(bone(2, Some(1), [10., 0.], [20., 0.]));
    rig.set_bone_key(
        2,
        BoneKey {
            angle: 0.2,
            ..Default::default()
        },
    );
    let pose = |body, head| {
        std::sync::Arc::new(SavedPose {
            name: "View".into(),
            bones: [(1, body), (2, head)]
                .map(|(id, angle)| {
                    (
                        id,
                        BoneKey {
                            angle,
                            ..Default::default()
                        },
                    )
                })
                .into(),
        })
    };
    rig.character_turns.body.root = Some(1);
    rig.character_turns.head.root = Some(2);
    rig.character_turns.body.views[2] = Some(pose(0., 0.));
    rig.character_turns.body.views[3] = Some(pose(0.4, 1.5));
    rig.character_turns.head.views[2] = Some(pose(0., 0.));
    rig.character_turns.head.views[3] = Some(pose(0., -0.3));
    rig.character_turns.body.set(10, 1.);
    rig.character_turns.head.set(5, 1.);
    let saved = serde_json::to_vec(&rig).unwrap();
    let loaded: Rig = serde_json::from_slice(&saved).unwrap();
    loaded.validate().unwrap();
    let poses = loaded.poses(5);
    assert!((poses[&1].angle - 0.2).abs() < 1e-5);
    assert!((poses[&2].angle - 0.1).abs() < 1e-5);
    assert!((bone_value(&loaded.bones[1], 5).angle - 0.2).abs() < 1e-5);
}
fn editor() -> Editor {
    let mut e = Editor::new(&eframe::CreationContext::_new_kittest(
        egui::Context::default(),
    ));
    e.doc = crate::fresh_document();
    e.recovery = std::env::temp_dir().join(format!(
        "rig-{}-{:?}.json",
        std::process::id(),
        std::thread::current().id()
    ));
    e
}
fn art(x: f32) -> Mark {
    Mark {
        points: vec![[x, 0.], [x + 100., 100.]].into(),
        rectangle: true,
        closed: true,
        stroke: false,
        fill: Some([130, 90, 210, 255]),
        ..Default::default()
    }
}
fn symbol(id: u64, marks: Vec<Mark>) -> Symbol {
    Symbol {
        id,
        name: format!("Symbol {id}"),
        layers: vec![Layer {
            name: "Art".into(),
            visible: true,
            frames: vec![marks.into()].into(),
            options: Default::default(),
        }]
        .into(),
        folders: Default::default(),
        perspective: Default::default(),
        rig: Default::default(),
        library_folder: None,
        mouth_poses: Default::default(),
    }
}
fn instance(id: u64, track: u64) -> Mark {
    let mut m = art(0.);
    m.style.track = track;
    m.style.instance = Some(Instance {
        symbol: id,
        matrix: IDENTITY,
        clock: SymbolClock::Scene,
        mode: Playback::Loop,
        first: 0,
        born: 0,
    });
    m
}
fn bone(id: u64, parent: Option<u64>, start: [f32; 2], end: [f32; 2]) -> Bone {
    Bone {
        id,
        name: format!("Bone {id}"),
        parent,
        start,
        end,
        radius: 20.,
        ik: false,
        targets: Default::default(),
        keys: vec![BoneKey::default()].into(),
    }
}
fn near(a: Pos2, b: Pos2) {
    assert!(a.distance(b) < 0.06, "{a:?} != {b:?}");
}
fn setup() -> Editor {
    let mut e = editor();
    e.doc.extras.symbols.push(symbol(1, vec![art(0.)]));
    for f in &mut e.doc.layers[0].frames {
        *f = vec![instance(1, 10), instance(1, 11)].into();
    }
    e
}
#[test]
fn bone_crossing_multiple_symbols_binds_both_and_shares_weights() {
    let mut e = setup();
    e.add_bone(Pos2::new(0., 50.), Pos2::new(100., 50.), None);
    assert_eq!(e.doc.extras.rig.bones[0].targets.len(), 2);
    let parent = e.doc.extras.rig.bones[0].id;
    e.add_bone(Pos2::new(100., 50.), Pos2::new(100., 100.), Some(parent));
    let b = &e.doc.extras.rig.bindings[0];
    let influences = b.sample(Pos2::new(75., 50.));
    assert_eq!(influences.len(), 2);
    assert!((influences.iter().map(|v| v.weight).sum::<f32>() - 1.).abs() < 0.001);
    assert!(e.doc.validate().is_ok());
}
#[test]
fn fk_inherits_rotation_and_ik_reaches_goal() {
    let mut r = Rig::default();
    r.bones = vec![
        bone(1, None, [0., 0.], [100., 0.]),
        bone(2, Some(1), [100., 0.], [200., 0.]),
    ]
    .into();
    r.set_bone_key(
        1,
        BoneKey {
            angle: std::f32::consts::FRAC_PI_2,
            ..Default::default()
        },
    );
    near(r.poses(0)[&2].end, Pos2::new(0., 200.));
    r.solve_ik(2, Pos2::new(100., 100.), 10);
    near(r.poses(10)[&2].end, Pos2::new(100., 100.));
    assert!(r.bones.iter().all(|b| b.keys.iter().any(|k| k.frame == 10)));
    assert!(!r.can_parent(1, Some(2)));
    assert!(r.validate().is_ok());
}
#[test]
fn weights_paint_remove_smooth_and_undo_keep_sources_shared() {
    let mut e = setup();
    e.add_bone(Pos2::new(0., 50.), Pos2::new(100., 50.), None);
    let id = e.rigging.bone.unwrap();
    let before = e.doc.clone();
    let center = Pos2::new(50., 50.);
    e.rigging.radius = 45.;
    e.rigging.strength = 1.;
    e.checkpoint();
    e.paint_rig_weights(center, true, false);
    let removed = e.doc.extras.rig.bindings[0].weight(center, id);
    assert!(removed < 0.05);
    e.paint_rig_weights(center, false, true);
    let smoothed = e.doc.extras.rig.bindings[0].weight(center, id);
    assert!(smoothed > removed);
    e.paint_rig_weights(center, false, false);
    assert!(e.doc.extras.rig.bindings[0].weight(center, id) > 0.99);
    assert_eq!(before.extras.rig.bindings[0].weight(center, id), 1.);
    assert_eq!(
        before.extras.symbols[0].layers[0].frames[0].identity(),
        e.doc.extras.symbols[0].layers[0].frames[0].identity()
    );
    e.history(false);
    assert_eq!(e.doc.extras.rig.bindings[0].weight(center, id), 1.);
}
#[test]
fn skin_weights_blend_continuously_and_leave_unweighted_points_in_place() {
    let mut r = Rig::default();
    r.bones = vec![
        bone(1, None, [0., 0.], [100., 0.]),
        bone(2, None, [0., 0.], [100., 0.]),
    ]
    .into();
    for b in &mut r.bones {
        b.targets = vec![10].into();
    }
    r.set_bone_key(
        1,
        BoneKey {
            translation: [20., 0.],
            ..Default::default()
        },
    );
    r.set_bone_key(
        2,
        BoneKey {
            translation: [0., 40.],
            ..Default::default()
        },
    );
    let mut b = Binding::new(
        10,
        1,
        IDENTITY,
        Rect::from_min_size(Pos2::ZERO, Vec2::splat(100.)),
    );
    for c in &mut b.cells {
        c.set(1, 0.5);
        c.set(2, 0.5);
    }
    near(
        skin_point(&b, &r.poses(0), Pos2::new(50., 50.)),
        Pos2::new(60., 70.),
    );
    b.cells = vec![Cell::default(); CELLS].into();
    near(
        skin_point(&b, &r.poses(0), Pos2::new(50., 50.)),
        Pos2::new(50., 50.),
    );
}
#[test]
fn puppet_pins_interpolate_exactly_without_mutating_vector_source() {
    let mut e = setup();
    for p in [
        Pos2::new(0., 0.),
        Pos2::new(100., 0.),
        Pos2::new(0., 100.),
        Pos2::new(100., 100.),
    ] {
        e.add_puppet_pin(0, 0, p);
    }
    let original = e.doc.extras.symbols[0].layers[0].frames[0].clone();
    let pin = e.doc.extras.rig.bindings[0].pins[3].id;
    e.doc.extras.rig.set_pin_key(
        10,
        pin,
        PinKey {
            frame: 19,
            position: [140., 130.],
            curve: Default::default(),
        },
    );
    let poses = e.doc.extras.rig.poses(19);
    let deform = Deformer::new(&e.doc.extras.rig.bindings[0], &poses, 19);
    near(deform.point(Pos2::new(100., 100.)), Pos2::new(140., 130.));
    near(deform.point(Pos2::ZERO), Pos2::ZERO);
    let mid = Deformer::new(&e.doc.extras.rig.bindings[0], &poses, 9);
    assert!(mid.point(Pos2::new(100., 100.)).x > 115.);
    let output = crate::features::resolve_export(&e.doc, 19, false);
    assert!(output.iter().any(|(_, _, m)| !m.nodes.is_empty()));
    assert_eq!(
        original.identity(),
        e.doc.extras.symbols[0].layers[0].frames[0].identity()
    );
    let svg = crate::animation::movie_svg(&e.doc, 19);
    assert!(!svg.contains("<image"));
    assert!(svg.contains("<path"));
}
#[test]
fn native_fla_roundtrip_keeps_rig_and_bakes_visible_projection() {
    let mut e = setup();
    e.add_bone(Pos2::new(0., 50.), Pos2::new(100., 50.), None);
    let id = e.rigging.bone.unwrap();
    e.doc.extras.rig.set_bone_key(
        id,
        BoneKey {
            frame: 19,
            angle: 0.5,
            ..Default::default()
        },
    );
    let bytes = crate::fla::encode(&e.doc).unwrap();
    let copy = crate::fla::decode(&bytes).unwrap();
    assert_eq!(copy.extras.rig.bones.len(), 1);
    assert_eq!(copy.extras.rig.bindings.len(), 2);
    assert_eq!(
        crate::animation::movie_svg(&copy, 19),
        crate::animation::movie_svg(&e.doc, 19)
    );
    let baked = crate::features::baked(&copy);
    assert!(baked.extras.rig.is_empty());
    assert!(
        baked
            .layers
            .iter()
            .flat_map(|l| l.frames.iter().flatten())
            .all(|m| m.style.instance.is_none())
    );
}
#[test]
fn symbols_begin_with_one_frame_and_extend_held_art_for_rigging() {
    let mut e = editor();
    e.doc.layers[0].frames[0].push(art(0.));
    e.canvas.selected = vec![(0, 0)];
    e.convert_symbol();
    let id = e.doc.extras.symbols[0].id;
    assert_eq!(e.doc.extras.symbols[0].layers[0].frames.len(), 1);
    e.enter_symbol(id);
    e.set_duration(1.);
    assert_eq!(e.doc.layers[0].frames.len(), e.doc.fps as usize);
    assert!(!e.doc.layers[0].key(10));
    assert_eq!(
        e.doc.layers[0].frames[0].identity(),
        e.doc.layers[0].frames[e.doc.fps as usize - 1].identity()
    );
    e.exit_symbol();
    assert_eq!(
        e.doc.extras.symbols[0].layers[0].frames.len(),
        e.doc.fps as usize
    );
}
#[test]
fn nested_independent_symbol_uses_root_clock_instead_of_parent_loop() {
    let mut d = Document::default();
    let mut inner = symbol(1, vec![art(0.)]);
    inner.layers[0].frames.push(vec![art(100.)].into());
    let mut child = instance(1, 0);
    child.style.instance.as_mut().unwrap().clock = SymbolClock::Independent;
    d.extras.symbols = vec![inner, symbol(2, vec![child])].into();
    d.layers[0].frames[0] = vec![instance(2, 0)].into();
    let first = crate::features::resolve_at(&d, 0, false, 0);
    let second = crate::features::resolve_at(&d, 0, false, 1);
    assert_eq!(vector::control_bounds(&first[0].2).left(), 0.);
    assert_eq!(vector::control_bounds(&second[0].2).left(), 100.);
    let nested = &mut d.extras.symbols[1].layers[0].frames[0][0];
    nested.style.instance.as_mut().unwrap().clock = SymbolClock::Scene;
    assert_eq!(
        vector::control_bounds(&crate::features::resolve_at(&d, 0, false, 1)[0].2).left(),
        0.
    );
}
#[test]
fn rig_survives_nested_editing_scene_switch_and_tab_switch() {
    let mut e = setup();
    e.enter_symbol(1);
    e.add_bone(Pos2::new(0., 0.), Pos2::new(100., 0.), None);
    e.exit_symbol();
    assert_eq!(e.doc.extras.symbols[0].rig.bones.len(), 1);
    e.add_bone(Pos2::new(0., 50.), Pos2::new(100., 50.), None);
    let scene = crate::animation::scene_from(&e.doc, 50, "Scene 1".into());
    let mut empty = crate::fresh_document();
    empty.layers[0].frames[0].clear();
    e.doc.extras.scenes = vec![
        scene,
        crate::animation::scene_from(&empty, 51, "Scene 2".into()),
    ]
    .into();
    e.switch_scene(1);
    assert!(e.doc.extras.rig.is_empty());
    e.switch_scene(0);
    assert_eq!(e.doc.extras.rig.bones.len(), 1);
    assert!(e.doc.validate().is_ok());
}
#[test]
fn library_moves_reorders_colors_and_rejects_folder_cycles() {
    use crate::library::{Asset, Folder};
    let mut e = setup();
    e.doc.extras.library.folders = vec![
        Folder {
            id: 30,
            name: "Character".into(),
            parent: None,
            color: [200, 50, 80, 255],
            collapsed: false,
        },
        Folder {
            id: 31,
            name: "Props".into(),
            parent: None,
            color: [50, 150, 220, 255],
            collapsed: false,
        },
    ]
    .into();
    e.move_library_asset(Asset::Symbol(1), Asset::Folder(30));
    assert_eq!(e.doc.extras.symbols[0].library_folder, Some(30));
    e.move_library_asset(Asset::Folder(31), Asset::Folder(30));
    e.move_library_asset(Asset::Folder(30), Asset::Folder(31));
    assert_eq!(e.doc.extras.library.folders[0].parent, None);
    e.reorder_library_folder(31, 30, true);
    assert_eq!(e.doc.extras.library.folders[0].id, 31);
    assert_eq!(e.doc.extras.library.folders[0].parent, None);
    e.move_library_asset(Asset::Symbol(1), Asset::Root);
    assert_eq!(e.doc.extras.symbols[0].library_folder, None);
    let restored: Document = serde_json::from_slice(&serde_json::to_vec(&e.doc).unwrap()).unwrap();
    assert_eq!(
        restored.extras.library.folders[0].color,
        [50, 150, 220, 255]
    );
    assert!(restored.validate().is_ok());
}
#[test]
fn pace_handles_change_bone_rotation_and_scale_timing_without_endpoint_drift() {
    let mut b = bone(1, None, [0., 0.], [100., 0.]);
    b.keys.push(BoneKey {
        frame: 19,
        angle: 1.,
        scale: 2.,
        ..Default::default()
    });
    let normal = bone_value(&b, 5);
    b.keys[0].curve.pace = [0., 0.];
    let slow = bone_value(&b, 5);
    assert!(slow.angle < normal.angle);
    assert!(slow.scale < normal.scale);
    assert_eq!(bone_value(&b, 0).angle, 0.);
    assert_eq!(bone_value(&b, 19).angle, 1.);
    let mut c = Curve::default();
    c.pace = [1., 1.];
    assert!((0..100).all(|i| c.time(i as f32 / 100.) <= c.time((i + 1) as f32 / 100.)));
}
#[test]
fn malformed_hierarchy_weights_and_pin_data_are_rejected() {
    let mut r = Rig::default();
    r.bones = vec![
        bone(1, Some(2), [0., 0.], [1., 0.]),
        bone(2, Some(1), [1., 0.], [2., 0.]),
    ]
    .into();
    assert!(r.validate().is_err());
    r.bones.clear();
    let mut b = Binding::new(
        10,
        1,
        IDENTITY,
        Rect::from_min_size(Pos2::ZERO, Vec2::splat(100.)),
    );
    b.cells[0].influences[0] = Influence {
        bone: 100,
        weight: f32::NAN,
    };
    r.bindings.push(b);
    assert!(r.validate().is_err());
}
#[test]
fn rig_tools_and_library_render_in_headless_ui() {
    let mut e = setup();
    e.hub.home = false;
    e.add_bone(Pos2::new(0., 50.), Pos2::new(100., 50.), None);
    e.add_puppet_pin(0, 0, Pos2::new(50., 50.));
    let ctx = egui::Context::default();
    crate::design::theme(&ctx);
    for tool in [Tool::Bones, Tool::Puppet] {
        e.tool = tool;
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1500., 1000.))),
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    e.properties_ui(ui);
                    e.asset_library_ui(ui);
                });
            },
        );
        assert!(!out.shapes.is_empty());
        out.textures_delta.clear();
    }
}

#[test]
fn grouping_an_animated_rig_keeps_the_current_pose_in_nested_symbol() {
    let mut e = setup();
    e.add_bone(Pos2::new(0., 50.), Pos2::new(100., 50.), None);
    let id = e.rigging.bone.unwrap();
    e.doc.extras.rig.set_bone_key(
        id,
        BoneKey {
            frame: 19,
            angle: 0.5,
            ..Default::default()
        },
    );
    e.frame = 19;
    let before = crate::animation::movie_svg(&e.doc, 19);
    e.canvas.selected = vec![(0, 0), (0, 1)];
    e.convert_symbol();
    let nested = e.doc.extras.symbols.last().unwrap();
    assert_eq!(nested.rig.bindings.len(), 2);
    assert_eq!(nested.rig.bones[0].keys[0].frame, 0);
    assert!((nested.rig.bones[0].keys[0].angle - 0.5).abs() < 0.001);
    assert_eq!(before, crate::animation::movie_svg(&e.doc, 19));
    assert!(e.doc.validate().is_ok());
}
#[test]
fn transformed_bound_instance_keeps_pins_and_weight_paint_in_world_space() {
    let mut e = setup();
    e.add_bone(Pos2::new(0., 50.), Pos2::new(100., 50.), None);
    let id = e.rigging.bone.unwrap();
    let transform = [2., 0., 0., 2., 300., 200.];
    e.doc.layers[0].frames[0][0] =
        crate::features::affine(&e.doc.layers[0].frames[0][0], transform);
    e.add_puppet_pin(0, 0, Pos2::new(400., 300.));
    let b = &e.doc.extras.rig.bindings[0];
    near(vector::pos(b.pins[0].rest), Pos2::new(50., 50.));
    e.rigging.radius = 45.;
    e.rigging.strength = 1.;
    e.paint_rig_weights(Pos2::new(400., 300.), true, false);
    assert!(e.doc.extras.rig.bindings[0].weight(Pos2::new(50., 50.), id) < 0.01);
    assert!(e.doc.extras.rig.bindings[1].weight(Pos2::new(50., 50.), id) > 0.99);
}
#[test]
fn adaptive_vector_deformation_tracks_continuous_field_without_rasterization() {
    let source = art(0.);
    let field = |p: Pos2| p + Vec2::new(0., (p.x / 30.).sin() * 25.);
    let deformed = deform_path(&source, &field);
    assert!(deformed.nodes.len() > 4);
    let flat = vector::flatten(&deformed, 0.002);
    for edge in vector::cubics(&source) {
        for step in 0..100 {
            let expected = field(edge.at(step as f32 / 99.));
            let d = flat
                .windows(2)
                .map(|p| segment_distance(expected, p[0], p[1]))
                .fold(f32::INFINITY, f32::min);
            assert!(d < 0.04, "error {d}");
        }
    }
}
#[test]
fn weight_paint_respects_locked_and_hidden_instances() {
    let mut e = setup();
    e.add_bone(Pos2::new(0., 50.), Pos2::new(100., 50.), None);
    let id = e.rigging.bone.unwrap();
    e.rigging.strength = 1.;
    e.doc.layers[0].options.locked = true;
    e.paint_rig_weights(Pos2::new(50., 50.), true, false);
    assert_eq!(
        e.doc.extras.rig.bindings[0].weight(Pos2::new(50., 50.), id),
        1.
    );
    e.doc.layers[0].options.locked = false;
    e.doc.layers[0].visible = false;
    e.paint_rig_weights(Pos2::new(50., 50.), true, false);
    assert_eq!(
        e.doc.extras.rig.bindings[0].weight(Pos2::new(50., 50.), id),
        1.
    );
}
#[test]
fn pointer_gestures_create_pose_and_move_puppet_pins() {
    let mut e = setup();
    let ctx = egui::Context::default();
    let view = crate::canvas::View {
        center: Pos2::ZERO,
        stage_center: Pos2::ZERO,
        angle: 0.,
        scale: 1.,
    };
    let tick = |e: &mut Editor, events: Vec<egui::Event>| {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(500.))),
                events,
                ..Default::default()
            },
            |ui| {
                e.rig_input(ui, ui.max_rect(), view, &[]);
            },
        );
        out.textures_delta.clear();
    };
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    };
    let drag = |e: &mut Editor, a: Pos2, b: Pos2| {
        tick(e, vec![egui::Event::PointerMoved(a), button(a, true)]);
        tick(e, vec![egui::Event::PointerMoved(b)]);
        tick(e, vec![button(b, false)]);
    };
    e.tool = Tool::Bones;
    tick(&mut e, vec![]);
    drag(&mut e, Pos2::new(10., 50.), Pos2::new(90., 50.));
    assert_eq!(e.doc.extras.rig.bones.len(), 1);
    assert_eq!(e.doc.extras.rig.bones[0].targets.len(), 2);
    e.rigging.create = false;
    drag(&mut e, Pos2::new(90., 50.), Pos2::new(10., 130.));
    assert!((e.doc.extras.rig.bones[0].keys[0].angle - std::f32::consts::FRAC_PI_2).abs() < 0.01);
    let id = e.rigging.bone.unwrap();
    e.doc.extras.rig.set_bone_key(id, BoneKey::default());
    e.tool = Tool::Puppet;
    let pos = Pos2::new(50., 50.);
    tick(
        &mut e,
        vec![egui::Event::PointerMoved(pos), button(pos, true)],
    );
    tick(&mut e, vec![button(pos, false)]);
    assert_eq!(
        e.doc
            .extras
            .rig
            .bindings
            .iter()
            .map(|b| b.pins.len())
            .sum::<usize>(),
        1
    );
    drag(&mut e, pos, Pos2::new(80., 70.));
    let b = e
        .doc
        .extras
        .rig
        .bindings
        .iter()
        .find(|b| !b.pins.is_empty())
        .unwrap();
    near(pin_value(&b.pins[0], 0), Pos2::new(80., 70.));
    assert!(e.doc.validate().is_ok());
    e.tool = Tool::Bones;
    let parent = e.doc.extras.rig.bones[0].id;
    let tip = e.doc.extras.rig.poses(0)[&parent].end;
    drag(
        &mut e,
        tip + Vec2::new(12., -12.),
        tip + Vec2::new(80., 50.),
    );
    assert_eq!(e.doc.extras.rig.bones.len(), 2);
    assert_eq!(e.doc.extras.rig.bones[1].parent, Some(parent));
    near(
        e.doc.extras.rig.poses(0)[&e.doc.extras.rig.bones[1].id].start,
        tip,
    );
}
#[test]
fn library_thumbnail_can_select_and_drag_a_symbol() {
    let mut e = setup();
    e.doc.extras.library.folders.push(crate::library::Folder {
        id: 300,
        name: "Assets".into(),
        parent: None,
        color: [80, 190, 180, 255],
        collapsed: false,
    });
    let ctx = egui::Context::default();
    let tick = |e: &mut Editor, events: Vec<egui::Event>| {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(500., 600.))),
                events,
                ..Default::default()
            },
            |ui| e.asset_library_ui(ui),
        );
        out.textures_delta.clear();
        out
    };
    let out = tick(&mut e, vec![]);
    let label = out
        .shapes
        .iter()
        .find_map(|s| {
            if let egui::Shape::Text(t) = &s.shape {
                (t.galley.job.text == "Symbol 1").then_some(t.pos)
            } else {
                None
            }
        })
        .expect("symbol row label");
    let p = Pos2::new(24., label.y + 8.);
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    };
    tick(&mut e, vec![egui::Event::PointerMoved(p), button(p, true)]);
    tick(&mut e, vec![button(p, false)]);
    assert!(e.assets.selected == Some(crate::library::Asset::Symbol(1)));
    let folder = out
        .shapes
        .iter()
        .find_map(|s| {
            if let egui::Shape::Text(t) = &s.shape {
                (t.galley.job.text == "Assets").then_some(t.pos)
            } else {
                None
            }
        })
        .unwrap();
    let q = Pos2::new(180., folder.y + 8.);
    tick(&mut e, vec![egui::Event::PointerMoved(p), button(p, true)]);
    tick(
        &mut e,
        vec![egui::Event::PointerMoved(p + Vec2::new(20., 0.))],
    );
    tick(&mut e, vec![egui::Event::PointerMoved(q)]);
    tick(&mut e, vec![button(q, false)]);
    assert_eq!(e.doc.extras.symbols[0].library_folder, Some(300));
    let out = tick(&mut e, vec![]);
    let label = out
        .shapes
        .iter()
        .find_map(|s| {
            if let egui::Shape::Text(t) = &s.shape {
                (t.galley.job.text == "Symbol 1").then_some(t.pos)
            } else {
                None
            }
        })
        .unwrap();
    let p = Pos2::new(65., label.y + 8.);
    let outside_folder = Pos2::new(300., 400.);
    tick(&mut e, vec![egui::Event::PointerMoved(p), button(p, true)]);
    let preview = tick(&mut e, vec![egui::Event::PointerMoved(outside_folder)]);
    assert!(
        preview
            .shapes
            .iter()
            .any(|s| if let egui::Shape::Text(t) = &s.shape {
                t.galley.job.text.contains("→")
            } else {
                false
            })
    );
    tick(&mut e, vec![button(outside_folder, false)]);
    assert_eq!(e.doc.extras.symbols[0].library_folder, None);
}

#[test]
fn detaching_discards_weights_without_reweighting_other_bones_and_ignores_stale_data() {
    let mut e = setup();
    e.add_bone(Pos2::new(0., 50.), Pos2::new(100., 50.), None);
    let first = e.rigging.bone.unwrap();
    e.add_bone(Pos2::new(0., 60.), Pos2::new(100., 60.), None);
    let second = e.rigging.bone.unwrap();
    let source = e.doc.extras.symbols[0].layers[0].frames[0].identity();
    let old = e.doc.clone();
    let previous = e.doc.extras.rig.bindings[0].cells[0].weight(second);
    e.doc.extras.rig.detach(first, 10);
    assert!(!e.doc.extras.rig.bones[0].targets.contains(&10));
    for cell in &e.doc.extras.rig.bindings[0].cells {
        assert_eq!(cell.weight(first), 0.);
    }
    assert_eq!(
        e.doc.extras.rig.bindings[0].cells[0].weight(second),
        previous
    );
    assert!(old.extras.rig.bindings[0].cells[0].weight(first) > 0.);
    e.rigging.bone = Some(first);
    e.paint_rig_weights(Pos2::new(50., 50.), false, false);
    assert_eq!(e.doc.extras.rig.bindings[0].cells[0].weight(first), 0.);
    e.doc.extras.rig.detach(second, 10);
    // Simulate old saved weights that disagree with the attachment list.
    for cell in &mut e.doc.extras.rig.bindings[0].cells {
        cell.set(first, 1.);
    }
    e.doc.extras.rig.set_bone_key(
        first,
        BoneKey {
            translation: [100., 50.],
            ..Default::default()
        },
    );
    let poses = e.doc.extras.rig.poses(0);
    let d = Deformer::new(&e.doc.extras.rig.bindings[0], &poses, 0);
    near(d.point(Pos2::new(50., 50.)), Pos2::new(50., 50.));
    assert!(!d.active);
    assert_eq!(
        e.doc.extras.symbols[0].layers[0].frames[0].identity(),
        source
    );
}

#[test]
fn timeline_cell_actions_edit_the_clicked_layer_even_with_bones_selected_and_undo() {
    let mut e = setup();
    e.tool = Tool::Bones;
    let before = e.doc.layers[0].frames[8].identity();
    e.timeline_cell_action(0, 8, 1);
    assert!(e.doc.layers[0].key(8));
    assert!(e.doc.layers[0].frames[8].is_empty());
    assert!(e.doc.layers[0].frames[7].len() > 0);
    e.history(false);
    assert_eq!(e.doc.layers[0].frames[8].identity(), before);
    e.timeline_cell_action(0, 8, 0);
    assert!(e.doc.layers[0].key(8));
    e.timeline_cell_action(0, 8, 5);
    assert!(crate::animation::span(&e.doc.layers[0], 9).is_some());
    e.timeline_cell_action(0, 9, 6);
    assert!(crate::animation::span(&e.doc.layers[0], 9).is_none());
    e.doc.layers[0].options.locked = true;
    let locked = e.doc.layers[0].frames[10].identity();
    e.timeline_cell_action(0, 10, 1);
    assert_eq!(e.doc.layers[0].frames[10].identity(), locked);
}

#[test]
fn weight_paint_changes_only_the_selected_bone() {
    let mut c = Cell::default();
    c.set(1, 0.5);
    c.set(2, 0.5);
    c.set(1, 0.1);
    assert_eq!(c.weight(2), 0.5);
    assert_eq!(c.weight(1), 0.1);
    c.set(1, 1.);
    assert_eq!(c.weight(2), 0.5);
    assert_eq!(c.weight(1), 0.5);
}

#[test]
fn library_symbol_drop_inserts_at_the_canvas_pointer() {
    let mut e = editor();
    e.doc.extras.symbols.push(symbol(1, vec![art(0.)]));
    let ctx = egui::Context::default();
    let run = |e: &mut Editor, events| {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900., 600.))),
                events,
                ..Default::default()
            },
            |ui| {
                egui::Panel::right("test-library")
                    .exact_size(270.)
                    .show(ui, |ui| e.asset_library_ui(ui));
                egui::CentralPanel::default().show(ui, |ui| e.canvas_ui(ui));
            },
        );
        out.textures_delta.clear();
        out
    };
    let out = run(&mut e, vec![]);
    let source = out
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape {
                (text.galley.job.text == "Symbol 1").then(|| text.pos + Vec2::new(15., 7.))
            } else {
                None
            }
        })
        .expect("symbol row");
    let destination = Pos2::new(240., 270.);
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    };
    run(
        &mut e,
        vec![egui::Event::PointerMoved(source), button(source, true)],
    );
    run(&mut e, vec![egui::Event::PointerMoved(destination)]);
    assert!(e.assets.drag == Some(crate::library::Asset::Symbol(1)));
    run(&mut e, vec![button(destination, false)]);
    let mark = e.doc.layers[0].frames[0].last().expect("inserted symbol");
    let instance = mark.style.instance.as_ref().expect("symbol instance");
    assert_eq!(instance.symbol, 1);
    assert_ne!(&instance.matrix[4..], &[0., 0.]);
    assert!(e.assets.drag.is_none());
}

#[test]
fn folder_split_relocation_preserves_rigged_poses_and_source_key_editing() {
    let mut e = setup();
    e.doc.extras.folders.push(crate::features::Folder {
        id: 555,
        name: "Rig folder".into(),
        parent: None,
        collapsed: false,
        transform: Default::default(),
        timeline_slot: None,
        range: None,
        source_shift: 0,
    });
    e.doc.layers[0].options.folder = Some(555);
    e.add_bone(Pos2::new(0., 50.), Pos2::new(100., 50.), None);
    let bone = e.rigging.bone.unwrap();
    e.doc.extras.rig.set_bone_key(
        bone,
        BoneKey {
            frame: 20,
            angle: 0.9,
            translation: [30., 10.],
            ..Default::default()
        },
    );
    let original = e.doc.clone();
    let right = e.split_folder_group(555, 12).unwrap();
    e.move_folder_group(right, 30).unwrap();
    for f in 30..42 {
        let before = crate::features::resolve_export(&original, f - 18, false);
        let after = crate::features::resolve_export(&e.doc, f, false);
        assert_eq!(before.len(), after.len());
        for (a, b) in before.iter().zip(&after) {
            let a = vector::control_bounds(&a.2);
            let b = vector::control_bounds(&b.2);
            near(a.min, b.min);
            near(a.max, b.max);
        }
    }
    e.frame = 35;
    e.tool = Tool::Bones;
    e.rig_key_action(0);
    assert!(e.doc.extras.rig.bones[0].keys.iter().any(|k| k.frame == 17));
    assert!(e.doc.validate().is_ok());
}
