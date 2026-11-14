use eframe::egui::{self, Color32, Pos2, Rect, Response, Sense, Stroke, Vec2};
pub const ACCENT: Color32 = Color32::from_rgb(91, 158, 255);
pub const MUTED: Color32 = Color32::from_rgb(150, 154, 164);
pub const PANEL: Color32 = Color32::from_rgb(39, 40, 44);
pub const LINE: Color32 = Color32::from_rgb(61, 63, 69);
#[derive(Clone, Copy)]
pub enum Icon {
    Adjust,
    Fx,
    Bulb,
    Mouth,
    CommandHelper,
    Minus,
    Pin,
    Tag,
    Audio,
    LayerAdd,
    LayerRemove,
    FolderRemove,
    KeyAdd,
    AutoKey,
    BlankKeyAdd,
    KeyRemove,
    FrameExtend,
    FrameShorten,
    Reference,
    PingPong,
    Grid,
    Snap,
    Depth,
    Focus,
    Blur,
    Info,
    SendBack,
    StepBack,
    StepFront,
    BringFront,
    Bones,
    Puppet,
    IK,
    WeightPaint,
    AutoWeights,
    Keyframe,
    FolderPlus,
    Rename,
    Home,
    Text,
    Airbrush,
    SketchBrush,
    Pressure,
    Mannequin,
    Head,
    Loop,
    Tween,
    CapRound,
    CapFlat,
    CapSquare,
    CapTriangle,
    New,
    Open,
    Save,
    SaveAs,
    Export,
    Undo,
    Redo,
    Brush,
    VectorBrush,
    FillBrush,
    ShadeBrush,
    LightBrush,
    Ruler,
    Symmetry,
    Perspective,
    Camera,
    Lock,
    Folder,
    Mask,
    Last,
    Symbol,
    Library,
    Mirror,
    Bucket,
    Width,
    Pen,
    Node,
    Rectangle,
    Trash,
    Plus,
    Copy,
    SwapColors,
    ResetColors,
    Cut,
    Paste,
    Stage,
    Minimize,
    Maximize,
    Close,
    Play,
    Pause,
    First,
    Previous,
    Next,
    Onion,
    Eye,
    EyeOff,
    Layers,
    Film,
    Settings,
    DocumentSettings,
    Preferences,
    Select,
    Transform,
    Rotate,
    Fit,
    Lasso,
    PolygonSelect,
    Wand,
    Object,
    Eraser,
    Eyedropper,
    Line,
    Ellipse,
    Polygon,
    Pan,
    Zoom,
}

pub fn theme(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    let mut v = egui::Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.extreme_bg_color = Color32::from_rgb(25, 26, 29);
    v.faint_bg_color = Color32::from_rgb(45, 47, 52);
    v.override_text_color = Some(Color32::from_rgb(218, 220, 226));
    v.selection.bg_fill = Color32::from_rgb(48, 78, 118);
    v.selection.stroke = Stroke::new(1., ACCENT);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = 3.into();
        w.bg_stroke = Stroke::new(1., LINE);
    }
    v.widgets.inactive.bg_fill = Color32::from_rgb(48, 50, 55);
    v.widgets.hovered.bg_fill = Color32::from_rgb(65, 69, 78);
    v.widgets.active.bg_fill = Color32::from_rgb(48, 78, 118);
    ctx.set_visuals_of(egui::Theme::Dark, v);
    ctx.style_mut_of(egui::Theme::Dark, |s| {
        s.spacing.item_spacing = Vec2::new(6., 6.);
        s.spacing.button_padding = Vec2::new(8., 5.);
        s.spacing.slider_width = 135.;
        s.text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(12.));
        s.text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(12.));
        s.text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(15.));
    });
}
/// Original 20-unit line icons, drawn as geometry so they stay sharp at any DPI.
pub fn draw(p: &egui::Painter, r: Rect, icon: Icon, c: Color32) {
    let scale = (r.width().min(r.height()) / 20.).min(1.);
    let origin = r.center() - Vec2::splat(10. * scale);
    let pt = |x: f32, y: f32| origin + Vec2::new(x, y) * scale;
    let s = Stroke::new(1.5, c);
    let line = |a: (f32, f32), b: (f32, f32)| {
        p.line_segment([pt(a.0, a.1), pt(b.0, b.1)], s);
    };
    let path = |coords: &[(f32, f32)]| {
        p.add(egui::Shape::line(
            coords.iter().map(|&(x, y)| pt(x, y)).collect(),
            s,
        ));
    };
    let circle = |x: f32, y: f32, r: f32| {
        p.circle_stroke(pt(x, y), r, s);
    };
    let rect = |x: f32, y: f32, w: f32, h: f32| {
        p.rect_stroke(
            Rect::from_min_size(pt(x, y), Vec2::new(w, h)),
            1.,
            s,
            egui::StrokeKind::Inside,
        );
    };
    match icon {
        Icon::Bulb => {
            path(&[
                (7., 15.),
                (7., 13.),
                (5., 10.),
                (5., 6.),
                (7., 3.),
                (13., 3.),
                (15., 6.),
                (15., 10.),
                (13., 13.),
                (13., 15.),
                (7., 15.),
            ]);
            line((8., 18.), (12., 18.));
            line((1., 8.), (3., 8.));
            line((17., 8.), (19., 8.));
        }
        Icon::Mouth => {
            path(&[
                (2., 10.),
                (7., 6.),
                (10., 8.),
                (13., 6.),
                (18., 10.),
                (14., 14.),
                (6., 14.),
                (2., 10.),
            ]);
            line((2., 10.), (18., 10.));
        }
        Icon::Fx => {
            path(&[(3., 16.), (3., 4.), (11., 4.)]);
            line((3., 9.), (9., 9.));
            line((12., 10.), (18., 16.));
            line((18., 10.), (12., 16.));
        }
        Icon::Adjust => {
            for (y, x) in [(5., 7.), (10., 13.), (15., 9.)] {
                line((2., y), (x - 2., y));
                line((x + 2., y), (18., y));
                circle(x, y, 2.);
            }
        }
        Icon::Minus => line((4., 10.), (16., 10.)),
        Icon::Pin => {
            circle(10., 6., 4.);
            path(&[(7., 9.), (10., 18.), (13., 9.)]);
            circle(10., 6., 1.);
        }
        Icon::Tag => {
            path(&[
                (2., 4.),
                (12., 4.),
                (18., 10.),
                (12., 16.),
                (2., 16.),
                (2., 4.),
            ]);
            circle(6., 10., 1.4);
        }
        Icon::Audio => {
            for (x, h) in [(2., 2.), (6., 5.), (10., 8.), (14., 5.), (18., 2.)] {
                line((x, 10. - h), (x, 10. + h));
            }
        }
        Icon::LayerAdd | Icon::LayerRemove => {
            path(&[(1., 6.), (7., 3.), (13., 6.), (7., 9.), (1., 6.)]);
            path(&[(1., 10.), (7., 13.), (12., 10.)]);
            path(&[(1., 14.), (7., 17.), (10., 15.5)]);
            line((12., 15.), (19., 15.));
            if matches!(icon, Icon::LayerAdd) {
                line((15.5, 11.5), (15.5, 18.5));
            }
        }
        Icon::FolderRemove => {
            path(&[
                (2., 17.),
                (2., 4.),
                (8., 4.),
                (10., 7.),
                (18., 7.),
                (18., 17.),
                (2., 17.),
            ]);
            line((7., 12.), (13., 12.));
        }
        Icon::AutoKey => {
            p.circle_filled(pt(6., 8.), 4.5 * scale, c);
            path(&[(11., 18.), (15., 7.), (19., 18.)]);
            line((12.5, 14.), (17.5, 14.));
        }
        Icon::KeyAdd | Icon::BlankKeyAdd | Icon::KeyRemove => {
            if matches!(icon, Icon::BlankKeyAdd) {
                p.circle_stroke(pt(7., 9.), 4.5 * scale, s);
            } else {
                p.circle_filled(pt(7., 9.), 4.5 * scale, c);
            }
            line((12., 15.), (19., 15.));
            if !matches!(icon, Icon::KeyRemove) {
                line((15.5, 11.5), (15.5, 18.5));
            }
        }
        Icon::FrameExtend | Icon::FrameShorten => {
            rect(1., 4., 10., 12.);
            p.circle_filled(pt(4., 10.), 1.5 * scale, c);
            line((7., 5.), (7., 15.));
            if matches!(icon, Icon::FrameExtend) {
                path(&[(14., 6.), (18., 10.), (14., 14.)]);
            } else {
                path(&[(18., 6.), (14., 10.), (18., 14.)]);
            }
        }
        Icon::Reference => {
            rect(2., 2., 16., 16.);
            circle(6., 6., 1.5);
            path(&[(3., 16.), (8., 10.), (11., 13.), (14., 9.), (18., 15.)]);
        }
        Icon::PingPong => {
            line((2., 6.), (18., 6.));
            path(&[(14., 2.), (18., 6.), (14., 10.)]);
            line((18., 14.), (2., 14.));
            path(&[(6., 10.), (2., 14.), (6., 18.)]);
        }
        Icon::Grid => {
            for i in [4., 10., 16.] {
                line((i, 2.), (i, 18.));
                line((2., i), (18., i));
            }
        }
        Icon::Snap => {
            p.add(egui::Shape::line(
                vec![
                    pt(4., 3.),
                    pt(4., 12.),
                    pt(6., 16.),
                    pt(10., 18.),
                    pt(14., 16.),
                    pt(16., 12.),
                    pt(16., 3.),
                ],
                s,
            ));
            line((3., 7.), (7., 7.));
            line((13., 7.), (17., 7.));
        }

        Icon::Bones | Icon::IK => {
            path(&[(3., 16.), (6., 8.), (16., 3.), (13., 11.), (3., 16.)]);
            circle(3., 16., 2.);
            circle(16., 3., 2.);
        }
        Icon::Puppet => {
            circle(10., 10., 3.);
            path(&[(10., 2.), (10., 7.)]);
            path(&[(10., 13.), (10., 18.)]);
            path(&[(2., 10.), (7., 10.)]);
            path(&[(13., 10.), (18., 10.)]);
        }
        Icon::WeightPaint | Icon::AutoWeights => {
            path(&[
                (3., 17.),
                (7., 8.),
                (14., 2.),
                (18., 6.),
                (10., 14.),
                (3., 17.),
            ]);
        }
        Icon::Keyframe => {
            path(&[(10., 2.), (18., 10.), (10., 18.), (2., 10.), (10., 2.)]);
        }
        Icon::FolderPlus => {
            path(&[
                (2., 17.),
                (2., 4.),
                (8., 4.),
                (10., 7.),
                (18., 7.),
                (18., 17.),
                (2., 17.),
            ]);
            path(&[(10., 9.), (10., 15.)]);
            path(&[(7., 12.), (13., 12.)]);
        }
        Icon::Rename => {
            path(&[
                (3., 16.),
                (5., 11.),
                (14., 2.),
                (18., 6.),
                (9., 15.),
                (3., 16.),
            ]);
            path(&[(3., 19.), (18., 19.)]);
        }
        Icon::Home => {
            path(&[(2., 9.), (10., 2.), (18., 9.)]);
            path(&[
                (4., 8.),
                (4., 18.),
                (8., 18.),
                (8., 12.),
                (12., 12.),
                (12., 18.),
                (16., 18.),
                (16., 8.),
            ]);
        }
        Icon::Text => {
            line((3., 3.), (17., 3.));
            line((10., 3.), (10., 18.));
            line((6., 18.), (14., 18.));
            line((3., 3.), (3., 6.));
            line((17., 3.), (17., 6.));
        }
        Icon::Airbrush => {
            path(&[
                (4., 17.),
                (8., 10.),
                (13., 7.),
                (10., 4.),
                (7., 9.),
                (2., 15.),
            ]);
            for (x, y) in [(14., 3.), (17., 2.), (17., 6.), (19., 4.), (15., 8.)] {
                p.circle_filled(pt(x, y), 0.8, c);
            }
        }
        Icon::SketchBrush => {
            path(&[
                (2., 18.),
                (4., 12.),
                (14., 2.),
                (18., 6.),
                (8., 16.),
                (2., 18.),
            ]);
            line((5., 12.), (9., 16.));
            line((11., 18.), (18., 18.));
        }
        Icon::Pressure => {
            path(&[
                (3., 15.),
                (6., 10.),
                (14., 2.),
                (18., 6.),
                (10., 14.),
                (3., 15.),
            ]);
            line((3., 19.), (17., 19.));
            line((10., 15.), (10., 18.));
        }
        Icon::Mannequin => {
            p.circle_stroke(pt(10., 3.), 2.5, s);
            line((10., 6.), (10., 12.));
            line((4., 8.), (16., 8.));
            line((10., 12.), (5., 19.));
            line((10., 12.), (15., 19.));
            for (x, y) in [(4., 8.), (16., 8.), (10., 12.)] {
                p.circle_filled(pt(x, y), 1.5, c);
            }
        }

        Icon::Loop => {
            path(&[(16., 5.), (12., 2.), (5., 2.), (2., 6.), (2., 12.)]);
            path(&[(4., 15.), (8., 18.), (15., 18.), (18., 14.), (18., 8.)]);
            path(&[(12., 5.), (17., 5.), (17., 0.)]);
            path(&[(8., 15.), (3., 15.), (3., 20.)]);
        }
        Icon::Tween => {
            path(&[(2., 16.), (5., 8.), (11., 6.), (18., 3.)]);
            p.circle_filled(pt(2., 16.), 2.5, c);
            path(&[(13., 2.), (18., 3.), (17., 8.)]);
        }
        Icon::CapRound | Icon::CapFlat | Icon::CapSquare | Icon::CapTriangle => {
            p.line_segment([pt(2., 10.), pt(12., 10.)], Stroke::new(5., c));
            match icon {
                Icon::Home => {
                    path(&[(2., 9.), (10., 2.), (18., 9.)]);
                    path(&[
                        (4., 8.),
                        (4., 18.),
                        (8., 18.),
                        (8., 12.),
                        (12., 12.),
                        (12., 18.),
                        (16., 18.),
                        (16., 8.),
                    ]);
                }
                Icon::Text => {
                    line((3., 3.), (17., 3.));
                    line((10., 3.), (10., 18.));
                    line((6., 18.), (14., 18.));
                    line((3., 3.), (3., 6.));
                    line((17., 3.), (17., 6.));
                }
                Icon::Airbrush => {
                    path(&[
                        (4., 17.),
                        (8., 10.),
                        (13., 7.),
                        (10., 4.),
                        (7., 9.),
                        (2., 15.),
                    ]);
                    for (x, y) in [(14., 3.), (17., 2.), (17., 6.), (19., 4.), (15., 8.)] {
                        p.circle_filled(pt(x, y), 0.8, c);
                    }
                }
                Icon::SketchBrush => {
                    path(&[
                        (2., 18.),
                        (4., 12.),
                        (14., 2.),
                        (18., 6.),
                        (8., 16.),
                        (2., 18.),
                    ]);
                    line((5., 12.), (9., 16.));
                    line((11., 18.), (18., 18.));
                }
                Icon::Pressure => {
                    path(&[
                        (3., 15.),
                        (6., 10.),
                        (14., 2.),
                        (18., 6.),
                        (10., 14.),
                        (3., 15.),
                    ]);
                    line((3., 19.), (17., 19.));
                    line((10., 15.), (10., 18.));
                }
                Icon::Mannequin => {
                    p.circle_stroke(pt(10., 3.), 2.5, s);
                    line((10., 6.), (10., 12.));
                    line((4., 8.), (16., 8.));
                    line((10., 12.), (5., 19.));
                    line((10., 12.), (15., 19.));
                    for (x, y) in [(4., 8.), (16., 8.), (10., 12.)] {
                        p.circle_filled(pt(x, y), 1.5, c);
                    }
                }

                Icon::CapRound => {
                    p.circle_filled(pt(12., 10.), 4., c);
                }
                Icon::CapSquare => {
                    p.rect_filled(Rect::from_min_size(pt(11., 6.), Vec2::splat(8.)), 0., c);
                }
                Icon::CapTriangle => {
                    p.add(egui::Shape::convex_polygon(
                        vec![pt(11., 5.), pt(19., 10.), pt(11., 15.)],
                        c,
                        Stroke::NONE,
                    ));
                }
                _ => {
                    line((13., 5.), (13., 15.));
                }
            }
        }
        Icon::Perspective => {
            line((1., 7.), (19., 7.));
            for x in [1., 6., 14., 19.] {
                line((10., 7.), (x, 19.));
            }
            p.circle_stroke(pt(10., 7.), 2., s);
        }
        Icon::Camera => {
            rect(2., 5., 13., 11.);
            path(&[(15., 8.), (19., 5.), (19., 16.), (15., 13.)]);
        }
        Icon::Lock => {
            rect(4., 9., 12., 10.);
            path(&[(6., 9.), (6., 4.), (10., 1.), (14., 4.), (14., 9.)]);
        }
        Icon::Folder => {
            path(&[
                (1., 5.),
                (1., 17.),
                (19., 17.),
                (19., 7.),
                (10., 7.),
                (8., 3.),
                (1., 3.),
                (1., 5.),
            ]);
        }
        Icon::Mask => {
            rect(1., 3., 13., 13.);
            p.circle_filled(pt(13., 12.), 5., c);
        }
        Icon::Last => {
            path(&[(4., 3.), (14., 10.), (4., 17.), (4., 3.)]);
            line((17., 3.), (17., 17.));
        }
        Icon::Symbol => {
            path(&[(10., 1.), (19., 10.), (10., 19.), (1., 10.), (10., 1.)]);
        }
        Icon::Library => {
            rect(2., 2., 4., 16.);
            rect(8., 2., 4., 16.);
            path(&[(14., 3.), (18., 2.), (20., 17.), (16., 18.), (14., 3.)]);
        }

        Icon::SwapColors => {
            path(&[(2., 7.), (17., 7.), (13., 3.)]);
            path(&[(18., 13.), (3., 13.), (7., 17.)]);
        }
        Icon::ResetColors => {
            p.rect_filled(
                Rect::from_min_size(pt(3., 3.), Vec2::splat(10.)),
                0.,
                Color32::WHITE,
            );
            p.rect_filled(
                Rect::from_min_size(pt(7., 7.), Vec2::splat(10.)),
                0.,
                Color32::BLACK,
            );
            rect(7., 7., 10., 10.);
        }
        Icon::Cut => {
            p.circle_stroke(pt(5., 15.), 3., s);
            p.circle_stroke(pt(15., 15.), 3., s);
            line((7., 13.), (16., 2.));
            line((13., 13.), (4., 2.));
        }
        Icon::Paste => {
            rect(4., 4., 12., 15.);
            p.rect_filled(Rect::from_min_size(pt(7., 2.), Vec2::new(6., 4.)), 1., c);
            line((7., 10.), (13., 10.));
            line((7., 14.), (13., 14.));
        }
        Icon::Stage => {
            rect(2., 3., 16., 14.);
            rect(6., 7., 8., 6.);
        }
        Icon::Minimize => {
            line((4., 14.), (16., 14.));
        }
        Icon::Maximize => {
            rect(4., 4., 12., 12.);
        }
        Icon::Close => {
            line((5., 5.), (15., 15.));
            line((15., 5.), (5., 15.));
        }

        Icon::Ruler => {
            rect(1., 5., 18., 10.);
            for x in [4., 7., 10., 13., 16.] {
                line((x, 5.), (x, if x == 10. { 12. } else { 9. }));
            }
        }
        Icon::Symmetry | Icon::Mirror => {
            line((10., 1.), (10., 19.));
            path(&[(7., 4.), (2., 16.), (7., 16.), (7., 4.)]);
            path(&[(13., 4.), (18., 16.), (13., 16.), (13., 4.)]);
        }
        Icon::FillBrush => {
            rect(3., 8., 14., 10.);
            line((4., 6.), (15., 2.));
        }
        Icon::ShadeBrush => {
            p.circle_filled(pt(10., 10.), 7., c);
            p.circle_filled(pt(13., 7.), 5., crate::themes::panel());
        }
        Icon::LightBrush => {
            p.circle_stroke(pt(10., 10.), 4., s);
            for (a, b) in [
                ((10., 0.), (10., 3.)),
                ((10., 17.), (10., 20.)),
                ((0., 10.), (3., 10.)),
                ((17., 10.), (20., 10.)),
            ] {
                line(a, b);
            }
        }

        Icon::Lasso => {
            path(&[
                (5., 15.),
                (2., 10.),
                (4., 4.),
                (12., 2.),
                (18., 6.),
                (17., 13.),
                (10., 16.),
                (5., 15.),
                (4., 19.),
            ]);
        }
        Icon::PolygonSelect => {
            path(&[(3., 16.), (5., 3.), (17., 6.), (13., 17.), (3., 16.)]);
            rect(2., 15., 2., 2.);
            rect(4., 2., 2., 2.);
            rect(16., 5., 2., 2.);
        }
        Icon::Wand => {
            line((3., 18.), (14., 7.));
            line((10., 8.), (13., 11.));
            line((15., 1.), (15., 4.));
            line((18., 5.), (20., 5.));
            line((9., 2.), (11., 4.));
        }
        Icon::Object => {
            rect(3., 3., 14., 14.);
            path(&[
                (7., 6.),
                (7., 14.),
                (10., 12.),
                (12., 16.),
                (14., 15.),
                (12., 11.),
                (16., 11.),
                (7., 6.),
            ]);
        }
        Icon::Eraser => {
            path(&[
                (2., 12.),
                (11., 3.),
                (18., 10.),
                (10., 18.),
                (8., 18.),
                (2., 12.),
            ]);
            line((6., 8.), (13., 15.));
            line((10., 18.), (19., 18.));
        }
        Icon::Eyedropper => {
            path(&[
                (3., 17.),
                (5., 12.),
                (14., 3.),
                (18., 7.),
                (9., 16.),
                (3., 17.),
            ]);
            line((11., 4.), (17., 10.));
        }
        Icon::Line => {
            line((3., 17.), (17., 3.));
        }
        Icon::Ellipse => {
            p.circle_stroke(pt(10., 10.), 7., s);
        }
        Icon::Polygon => {
            path(&[
                (10., 2.),
                (18., 8.),
                (15., 17.),
                (5., 17.),
                (2., 8.),
                (10., 2.),
            ]);
        }
        Icon::Pan => {
            path(&[
                (5., 11.),
                (5., 6.),
                (7., 6.),
                (7., 2.),
                (9., 2.),
                (9., 7.),
                (10., 7.),
                (10., 2.),
                (12., 2.),
                (12., 7.),
                (13., 7.),
                (13., 4.),
                (15., 4.),
                (15., 9.),
                (17., 7.),
                (19., 9.),
                (15., 17.),
                (9., 19.),
                (3., 13.),
                (3., 10.),
                (5., 11.),
            ]);
        }
        Icon::Zoom => {
            p.circle_stroke(pt(8., 8.), 5., s);
            line((12., 12.), (18., 18.));
            line((5., 8.), (11., 8.));
            line((8., 5.), (8., 11.));
        }
        Icon::New => {
            path(&[
                (5., 2.),
                (12., 2.),
                (16., 6.),
                (16., 18.),
                (5., 18.),
                (5., 2.),
            ]);
            path(&[(12., 2.), (12., 6.), (16., 6.)]);
            line((8., 11.), (13., 11.));
            line((10.5, 8.5), (10.5, 13.5));
        }
        Icon::Open => {
            path(&[(2., 16.), (2., 5.), (8., 5.), (10., 7.), (17., 7.)]);
            path(&[(2., 16.), (5., 9.), (19., 9.), (16., 16.), (2., 16.)]);
        }
        Icon::Save | Icon::SaveAs => {
            path(&[
                (3., 3.),
                (15., 3.),
                (17., 5.),
                (17., 17.),
                (3., 17.),
                (3., 3.),
            ]);
            rect(6., 3., 7., 5.);
            rect(6., 11., 8., 6.);
            if matches!(icon, Icon::SaveAs) {
                line((13., 15.), (19., 15.));
                line((16., 12.), (16., 18.));
            }
        }
        Icon::Export => {
            path(&[(3., 9.), (3., 17.), (17., 17.), (17., 9.)]);
            line((10., 13.), (10., 2.));
            path(&[(6., 6.), (10., 2.), (14., 6.)]);
        }
        Icon::Undo => {
            path(&[(7., 5.), (3., 9.), (7., 13.)]);
            path(&[(3., 9.), (12., 9.), (16., 11.), (16., 16.)]);
        }
        Icon::Redo => {
            path(&[(13., 5.), (17., 9.), (13., 13.)]);
            path(&[(17., 9.), (8., 9.), (4., 11.), (4., 16.)]);
        }
        Icon::Brush => {
            path(&[(7., 12.), (14., 3.), (17., 6.), (9., 14.)]);
            path(&[
                (7., 12.),
                (4., 13.),
                (3., 18.),
                (8., 17.),
                (9., 14.),
                (7., 12.),
            ]);
        }
        Icon::VectorBrush => {
            path(&[(10., 10.), (15., 3.), (18., 6.), (12., 13.)]);
            path(&[(10., 10.), (7., 12.), (6., 16.), (10., 15.), (12., 13.)]);
            p.add(egui::Shape::CubicBezier(
                egui::epaint::CubicBezierShape::from_points_stroke(
                    [pt(1., 18.), pt(10., 20.), pt(1., 8.), pt(7., 13.)],
                    false,
                    Color32::TRANSPARENT,
                    s,
                ),
            ));
            rect(0., 16., 3., 3.);
            rect(5., 11., 3., 3.);
        }
        Icon::Bucket => {
            path(&[(6., 3.), (15., 11.), (8., 18.), (1., 11.), (6., 3.)]);
            line((2., 10.), (13., 10.));
            path(&[(6., 3.), (10., 1.), (14., 5.)]);
            p.circle_filled(pt(17., 17.), 2., c);
            path(&[(16., 13.), (18., 16.)]);
        }
        Icon::Width => {
            path(&[
                (2., 10.),
                (5., 7.),
                (10., 4.),
                (15., 7.),
                (18., 10.),
                (15., 13.),
                (10., 16.),
                (5., 13.),
                (2., 10.),
            ]);
            line((1., 10.), (19., 10.));
            line((10., 1.), (10., 19.));
            path(&[(7., 3.), (10., 1.), (13., 3.)]);
            path(&[(7., 17.), (10., 19.), (13., 17.)]);
        }
        Icon::Pen => {
            path(&[
                (10., 2.),
                (17., 11.),
                (12., 17.),
                (8., 17.),
                (3., 11.),
                (10., 2.),
            ]);
            line((10., 2.), (10., 10.));
            p.circle_stroke(pt(10., 11.), 2., s);
            line((6., 19.), (14., 19.));
        }
        Icon::Node => {
            line((3., 16.), (17., 4.));
            p.circle_stroke(pt(3., 16.), 2., s);
            p.circle_stroke(pt(17., 4.), 2., s);
            p.rect_filled(Rect::from_center_size(pt(10., 10.), Vec2::splat(6.)), 0., c);
        }
        Icon::Rectangle => rect(3., 4., 14., 12.),
        Icon::Trash => {
            line((3., 5.), (17., 5.));
            path(&[(7., 5.), (7., 2.), (13., 2.), (13., 5.)]);
            path(&[(5., 5.), (6., 18.), (14., 18.), (15., 5.)]);
            line((8., 8.), (8., 15.));
            line((12., 8.), (12., 15.));
        }
        Icon::Plus => {
            line((4., 10.), (16., 10.));
            line((10., 4.), (10., 16.));
        }
        Icon::Copy => {
            rect(6., 6., 11., 12.);
            path(&[(13., 4.), (13., 2.), (2., 2.), (2., 14.), (4., 14.)]);
        }
        Icon::Play => {
            p.add(egui::Shape::convex_polygon(
                vec![pt(6., 3.), pt(17., 10.), pt(6., 17.)],
                c,
                Stroke::NONE,
            ));
        }
        Icon::Pause => {
            p.rect_filled(Rect::from_min_size(pt(5., 4.), Vec2::new(3., 12.)), 0., c);
            p.rect_filled(Rect::from_min_size(pt(12., 4.), Vec2::new(3., 12.)), 0., c);
        }
        Icon::Previous | Icon::Next | Icon::First => {
            let flip = matches!(icon, Icon::Next);
            let x = |v: f32| if flip { 20. - v } else { v };
            path(&[(x(13.), 4.), (x(6.), 10.), (x(13.), 16.)]);
            if matches!(icon, Icon::First) {
                line((3., 4.), (3., 16.));
            }
        }
        Icon::Onion => {
            path(&[
                (10., 1.),
                (8., 5.),
                (4., 9.),
                (3., 13.),
                (5., 17.),
                (10., 19.),
                (15., 17.),
                (17., 13.),
                (16., 9.),
                (12., 5.),
                (10., 1.),
            ]);
            path(&[(10., 5.), (7., 10.), (7., 15.), (10., 18.)]);
            path(&[(10., 5.), (13., 10.), (13., 15.), (10., 18.)]);
            line((8., 1.), (10., 4.));
            line((13., 1.), (10., 4.));
        }
        Icon::Eye | Icon::EyeOff => {
            path(&[
                (1., 10.),
                (5., 6.),
                (10., 4.),
                (15., 6.),
                (19., 10.),
                (15., 14.),
                (10., 16.),
                (5., 14.),
                (1., 10.),
            ]);
            p.circle_stroke(pt(10., 10.), 2.5, s);
            if matches!(icon, Icon::EyeOff) {
                line((3., 2.), (17., 18.));
            }
        }
        Icon::SendBack | Icon::StepBack | Icon::StepFront | Icon::BringFront => {
            let front = matches!(icon, Icon::StepFront | Icon::BringFront);
            let y = if front { 5. } else { 15. };
            let base = if front { 8. } else { 12. };
            line((4., 10.), (16., 10.));
            line((10., 10.), (10., y));
            path(&[(7., base), (10., y), (13., base)]);
            if matches!(icon, Icon::SendBack | Icon::BringFront) {
                let edge = if front { 2. } else { 18. };
                line((4., edge), (16., edge));
            }
        }
        Icon::Depth => {
            rect(1., 7., 6., 7.);
            path(&[(7., 9.), (10., 7.), (10., 14.), (7., 12.)]);
            path(&[(12., 4.), (15., 2.), (15., 15.), (12., 17.), (12., 4.)]);
            path(&[(17., 5.), (19., 4.), (19., 17.), (17., 18.)]);
        }
        Icon::Focus => {
            circle(10., 10., 4.);
            line((2., 6.), (2., 2.));
            line((2., 2.), (6., 2.));
            line((14., 2.), (18., 2.));
            line((18., 2.), (18., 6.));
            line((2., 14.), (2., 18.));
            line((2., 18.), (6., 18.));
            line((14., 18.), (18., 18.));
            line((18., 18.), (18., 14.));
        }
        Icon::Blur => {
            for (x, y) in [
                (5., 6.),
                (11., 4.),
                (16., 8.),
                (6., 14.),
                (13., 15.),
                (11., 10.),
            ] {
                circle(x, y, 1.5);
            }
        }
        Icon::CommandHelper => {
            rect(1., 4., 18., 12.);
            line((4., 8.), (8., 8.));
            line((4., 11.), (8., 11.));
            path(&[
                (11., 8.),
                (12., 6.5),
                (15., 6.5),
                (16., 8.),
                (14., 10.),
                (14., 11.),
            ]);
            line((14., 13.), (14.1, 13.));
        }
        Icon::Info => {
            circle(10., 10., 8.);
            path(&[
                (7., 7.),
                (8., 5.),
                (12., 5.),
                (13., 7.),
                (12., 9.),
                (10., 10.),
                (10., 12.),
            ]);
            circle(10., 15., 0.4);
        }
        Icon::Layers => {
            path(&[(2., 7.), (10., 3.), (18., 7.), (10., 11.), (2., 7.)]);
            path(&[(2., 11.), (10., 15.), (18., 11.)]);
            path(&[(2., 15.), (10., 19.), (18., 15.)]);
        }
        Icon::Film => {
            rect(2., 3., 16., 14.);
            line((6., 3.), (6., 17.));
            line((14., 3.), (14., 17.));
            for y in [7., 12.] {
                line((2., y), (6., y));
                line((14., y), (18., y));
            }
        }
        Icon::Select => {
            path(&[
                (4., 2.),
                (4., 17.),
                (8., 13.),
                (11., 19.),
                (14., 17.),
                (11., 12.),
                (17., 12.),
                (4., 2.),
            ]);
        }
        Icon::Transform => {
            rect(4., 4., 12., 12.);
            for (x, y) in [(2., 2.), (14., 2.), (2., 14.), (14., 14.)] {
                p.rect_filled(Rect::from_min_size(pt(x, y), Vec2::splat(4.)), 0., c);
            }
        }
        Icon::Rotate => {
            path(&[
                (16., 7.),
                (13., 3.),
                (7., 3.),
                (3., 7.),
                (3., 13.),
                (7., 17.),
                (13., 17.),
                (17., 13.),
            ]);
            path(&[(16., 2.), (16., 7.), (11., 7.)]);
        }
        Icon::Fit => {
            path(&[(2., 7.), (2., 2.), (7., 2.)]);
            path(&[(13., 2.), (18., 2.), (18., 7.)]);
            path(&[(18., 13.), (18., 18.), (13., 18.)]);
            path(&[(7., 18.), (2., 18.), (2., 13.)]);
            rect(6., 6., 8., 8.);
        }
        Icon::Head => {
            p.circle_stroke(pt(10., 7.), 5., s);
            path(&[(5., 18.), (6., 14.), (14., 14.), (15., 18.)]);
        }
        Icon::DocumentSettings => {
            path(&[
                (5., 2.),
                (13., 2.),
                (17., 6.),
                (17., 18.),
                (5., 18.),
                (5., 2.),
            ]);
            path(&[(13., 2.), (13., 6.), (17., 6.)]);
            line((8., 10.), (14., 10.));
            line((8., 14.), (12., 14.));
        }
        Icon::Preferences => {
            circle(10., 10., 6.);
            circle(10., 10., 2.5);
            for i in 0..8 {
                let angle = i as f32 * std::f32::consts::TAU / 8.;
                let d = Vec2::angled(angle);
                p.line_segment(
                    [pt(10., 10.) + d * 6., pt(10., 10.) + d * 9.],
                    Stroke::new(3., c),
                );
            }
        }
        Icon::Settings => {
            for (y, x) in [(5., 7.), (10., 13.), (15., 8.)] {
                line((2., y), (18., y));
                p.circle_filled(pt(x, y), 2.5, crate::themes::panel());
                p.circle_stroke(pt(x, y), 2.5, s);
            }
        }
    }
}
pub fn button(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    tip: &str,
    selected: bool,
    enabled: bool,
) -> Response {
    let label = crate::i18n::tr(label);
    let tip = crate::i18n::tr(tip);
    let text = ui.painter().layout_no_wrap(
        label.into(),
        egui::FontId::proportional(12.),
        crate::themes::palette().text,
    );
    let compact = label.is_empty() && ui.spacing().interact_size.y <= 24.;
    let width = if compact {
        27.
    } else if label.is_empty() {
        32.
    } else {
        40. + text.size().x
    };
    let (r, response) = ui.allocate_exact_size(
        Vec2::new(
            width,
            if compact {
                ui.spacing().interact_size.y
            } else {
                30.
            },
        ),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let fill = if selected {
        crate::themes::color(Color32::from_rgb(47, 73, 104))
    } else if response.hovered() && enabled {
        crate::themes::color(Color32::from_rgb(61, 64, 72))
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(r, 3., fill);
    if selected {
        ui.painter().rect_stroke(
            r,
            3.,
            Stroke::new(1., crate::themes::accent()),
            egui::StrokeKind::Inside,
        );
    }
    let color = if !enabled {
        crate::themes::color(Color32::from_rgb(87, 90, 99))
    } else if selected {
        crate::themes::accent()
    } else {
        crate::themes::color(Color32::from_rgb(203, 207, 216))
    };
    draw(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(r.left() + if compact { 13.5 } else { 16. }, r.center().y),
            Vec2::splat(if compact { 18. } else { 20. }),
        ),
        icon,
        color,
    );
    if !label.is_empty() {
        ui.painter().text(
            Pos2::new(r.left() + 32., r.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(12.),
            color,
        );
    }
    response.on_hover_text(tip)
}
/// The supplied app artwork, uploaded once and reused by the header.
pub fn logo(p: &egui::Painter, r: Rect) {
    let id = egui::Id::unique((
        "libretoon-app-icon",
        (r.width() * p.ctx().pixels_per_point()).round() as u32,
    ));
    let cached = p
        .ctx()
        .data_mut(|data| data.get_temp::<egui::TextureHandle>(id));
    // Loading a texture reads Context input; never do this while holding data_mut.
    let texture = cached.unwrap_or_else(|| {
        let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/libretoon.png"))
            .expect("header PNG");
        let side = (r.width() * p.ctx().pixels_per_point())
            .round()
            .clamp(16., 128.) as usize;
        let mut rgba = vec![0u8; side * side * 4];
        for y in 0..side {
            for x in 0..side {
                let x0 = x * icon.width as usize / side;
                let x1 = ((x + 1) * icon.width as usize / side).max(x0 + 1);
                let y0 = y * icon.height as usize / side;
                let y1 = ((y + 1) * icon.height as usize / side).max(y0 + 1);
                let mut sums = [0u64; 4];
                let mut count = 0u64;
                for sy in y0..y1 {
                    for sx in x0..x1 {
                        let pixel = &icon.rgba[(sy * icon.width as usize + sx) * 4..][..4];
                        for c in 0..3 {
                            sums[c] += pixel[c] as u64 * pixel[3] as u64;
                        }
                        sums[3] += pixel[3] as u64;
                        count += 1;
                    }
                }
                let offset = (y * side + x) * 4;
                for c in 0..3 {
                    rgba[offset + c] = (sums[c] / sums[3].max(1)) as u8;
                }
                rgba[offset + 3] = (sums[3] / count.max(1)) as u8;
            }
        }
        let image = egui::ColorImage::from_rgba_unmultiplied([side, side], &rgba);
        let texture = p
            .ctx()
            .load_texture("LibreToon", image, egui::TextureOptions::LINEAR);
        p.ctx()
            .data_mut(|data| data.insert_temp(id, texture.clone()));
        texture
    });
    p.image(
        texture.id(),
        r,
        Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
        Color32::WHITE,
    );
}
pub fn window_icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/libretoon-icon.png"))
        .expect("the bundled LibreToon icon must be a valid PNG")
}

#[cfg(test)]
mod icon_tests {
    #[test]
    fn header_icon_loads_and_reuses_texture_across_frames() {
        let ctx = eframe::egui::Context::default();
        for _ in 0..2 {
            let mut output = ctx.run_ui(Default::default(), |ui| {
                eframe::egui::Frame::NONE.show(ui, |ui| {
                    super::logo(
                        ui.painter(),
                        eframe::egui::Rect::from_min_size(
                            eframe::egui::Pos2::ZERO,
                            eframe::egui::vec2(32., 32.),
                        ),
                    );
                });
            });
            output.textures_delta.clear();
        }
    }
}

pub fn category(ui: &mut egui::Ui, title: &str) {
    ui.add_space(12.);
    ui.label(
        egui::RichText::new(crate::i18n::tr(title))
            .size(10.)
            .strong()
            .color(crate::themes::muted()),
    );
    ui.separator();
    ui.add_space(4.);
}
