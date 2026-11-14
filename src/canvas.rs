use crate::{Editor, design::Icon, document::Mark};
use eframe::egui::{self, Color32, CursorIcon, PointerButton, Pos2, Rect, Sense, Stroke, Vec2};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    Bones,
    Puppet,
    Text,
    Airbrush,
    SketchBrush,
    Mannequin,
    #[default]
    Brush,
    VectorBrush,
    FillBrush,
    ShadeBrush,
    LightBrush,
    Ruler,
    Symmetry,
    Perspective,
    Camera,
    Mirror,
    Pen,
    Node,
    Bucket,
    Width,
    Rectangle,
    Select,
    Transform,
    RotateView,
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
    Trim,
}
impl Tool {
    pub fn name(self) -> &'static str {
        crate::i18n::tr(match self {
            Self::Trim => "Vector trim",
            Self::Bones => "Huesos",
            Self::Puppet => "Puppet",
            Self::Text => "Texto",
            Self::Airbrush => "Pincel difuminador",
            Self::SketchBrush => "Pincel borrador",
            Self::Mannequin => "Maniquí",
            Self::Brush => "Pincel Estándar",
            Self::VectorBrush => "Pincel Vectorial",
            Self::FillBrush => "Pincel de relleno",
            Self::ShadeBrush => "Pincel de sombreado",
            Self::LightBrush => "Pincel de iluminación",
            Self::Ruler => "Regla",
            Self::Symmetry => "Simetría",
            Self::Perspective => "Perspectiva",
            Self::Camera => "Cámara",
            Self::Mirror => "Espejo",
            Self::Pen => "Pluma",
            Self::Node => "Editar nodos",
            Self::Bucket => "Bote de pintura",
            Self::Width => "Anchura del contorno",
            Self::Rectangle => "Rectángulo",
            Self::Select => "Selección",
            Self::Transform => "Transformación libre",
            Self::RotateView => "Rotar vista",
            Self::Lasso => "Lazo",
            Self::PolygonSelect => "Selección poligonal",
            Self::Wand => "Varita mágica",
            Self::Object => "Selección de objeto",
            Self::Eraser => "Goma de borrar",
            Self::Eyedropper => "Cuentagotas",
            Self::Line => "Línea",
            Self::Ellipse => "Círculo / elipse",
            Self::Polygon => "Polígono / estrella",
            Self::Pan => "Mano",
            Self::Zoom => "Lupa",
        })
    }
    pub fn icon(self) -> Icon {
        match self {
            Self::Trim => Icon::Cut,
            Self::Bones => Icon::Bones,
            Self::Puppet => Icon::Puppet,
            Self::Text => Icon::Text,
            Self::Airbrush => Icon::Airbrush,
            Self::SketchBrush => Icon::SketchBrush,
            Self::Mannequin => Icon::Mannequin,
            Self::Brush => Icon::Brush,
            Self::VectorBrush => Icon::VectorBrush,
            Self::FillBrush => Icon::FillBrush,
            Self::ShadeBrush => Icon::ShadeBrush,
            Self::LightBrush => Icon::LightBrush,
            Self::Ruler => Icon::Ruler,
            Self::Symmetry => Icon::Symmetry,
            Self::Perspective => Icon::Perspective,
            Self::Camera => Icon::Camera,
            Self::Mirror => Icon::Mirror,
            Self::Pen => Icon::Pen,
            Self::Node => Icon::Node,
            Self::Bucket => Icon::Bucket,
            Self::Width => Icon::Width,
            Self::Rectangle => Icon::Rectangle,
            Self::Select => Icon::Select,
            Self::Transform => Icon::Transform,
            Self::RotateView => Icon::Rotate,
            Self::Lasso => Icon::Lasso,
            Self::PolygonSelect => Icon::PolygonSelect,
            Self::Wand => Icon::Wand,
            Self::Object => Icon::Object,
            Self::Eraser => Icon::Eraser,
            Self::Eyedropper => Icon::Eyedropper,
            Self::Line => Icon::Line,
            Self::Ellipse => Icon::Ellipse,
            Self::Polygon => Icon::Polygon,
            Self::Pan => Icon::Pan,
            Self::Zoom => Icon::Zoom,
        }
    }
}
#[derive(Clone, Copy)]
pub struct View {
    pub center: Pos2,
    pub scale: f32,
    pub angle: f32,
    pub stage_center: Pos2,
}
pub fn rotate(v: Vec2, a: f32) -> Vec2 {
    let (s, c) = a.sin_cos();
    Vec2::new(c * v.x - s * v.y, s * v.x + c * v.y)
}
impl View {
    pub fn screen(self, p: Pos2) -> Pos2 {
        self.center + rotate(p - self.stage_center, self.angle) * self.scale
    }
    pub fn world(self, p: Pos2) -> Pos2 {
        self.stage_center + rotate((p - self.center) / self.scale, -self.angle)
    }
}
#[derive(Clone, Copy, Debug)]
enum Operation {
    Move,
    Scale(usize),
    Rotate,
    Pivot,
}
struct TransformDrag {
    operation: Operation,
    start: Pos2,
    pivot: Pos2,
    bounds: SelectionBox,
    originals: Vec<((usize, usize), Mark)>,
    changed: bool,
    last_input: Option<(Pos2, bool, bool)>,
    toggle_on_click: Option<(usize, usize)>,
}
enum Drag {
    Area {
        points: Vec<Pos2>,
        add: bool,
    },
    Erase {
        last: Pos2,
        changed: bool,
    },
    Pan(Pos2),
    ViewRotate {
        start: f32,
        angle: f32,
    },
    Draw {
        smoother: crate::brush::StrokeSmoother,
        tool: Tool,
    },
    Marquee {
        start: Pos2,
        end: Pos2,
        add: bool,
    },
    Transform(TransformDrag),
    PenNode {
        index: usize,
        start: Pos2,
        original: crate::document::Node,
        convert: bool,
    },
    Node(NodeDrag),
    Width(WidthDrag),
}
pub struct Canvas {
    pub(crate) raster_stroke: Option<crate::raster::Stroke>,
    pub zoom: f32,
    pub actual_scale: f32,
    pub pan: Vec2,
    pub angle: f32,
    pub selected: Vec<(usize, usize)>,
    pub(crate) partial: std::collections::HashMap<(usize, usize), (Vec<Mark>, Vec<Mark>)>,
    pub pivot: Option<Pos2>,
    orientation: f32,
    frame: usize,
    drag: Option<Drag>,
    pen: Option<PenDraft>,
    pen_redo: Vec<PenDraft>,
    pub(crate) node_selection: Option<(usize, usize, usize)>,
    cache: crate::paint::RenderCache,
    stage_size: Vec2,
    polygon_selection: Vec<Pos2>,
    polygon_add: bool,
}
impl Default for Canvas {
    fn default() -> Self {
        Self {
            raster_stroke: None,
            zoom: 1.,
            actual_scale: 1.,
            pan: Vec2::ZERO,
            angle: 0.,
            selected: vec![],
            partial: Default::default(),
            pivot: None,
            orientation: 0.,
            frame: 0,
            drag: None,
            pen: None,
            pen_redo: vec![],
            node_selection: None,
            cache: Default::default(),
            stage_size: Vec2::new(960., 540.),
            polygon_selection: vec![],
            polygon_add: false,
        }
    }
}
impl Canvas {
    fn view(&self, area: Rect) -> View {
        View {
            center: area.center() + self.pan,
            scale: fit_scale(area, self.stage_size) * self.zoom,
            angle: self.angle,
            stage_center: Pos2::ZERO + self.stage_size * 0.5,
        }
    }
    pub fn clear_selection(&mut self) {
        self.polygon_selection.clear();
        self.selected.clear();
        self.partial.clear();
        self.node_selection = None;
        self.pivot = None;
        self.orientation = 0.;
        self.drag = None;
    }
    pub fn fit(&mut self) {
        self.zoom = 1.;
        self.pan = Vec2::ZERO;
        self.angle = 0.;
    }
    fn transform_pivot(&self, editor: &Editor, bounds: SelectionBox) -> Pos2 {
        self.pivot
            .or_else(|| {
                let &[(layer, index)] = self.selected.as_slice() else {
                    return None;
                };
                let marks = crate::animation::marks(&editor.doc.layers[layer], editor.frame);
                let mark = marks.get(index)?;
                let instance = mark.style.instance.as_ref()?;
                let local = crate::vector::pos(mark.style.symbol_pivot?);
                Some(editor.depth_point(
                    crate::rig::point(instance.matrix, local),
                    layer,
                    editor.layer,
                ))
            })
            .unwrap_or_else(|| bounds.center())
    }
    fn bounds(&self, e: &Editor) -> Option<SelectionBox> {
        let mut r = Rect::NOTHING;
        let mut evaluated = std::collections::HashMap::new();
        for &(l, m) in &self.selected {
            let marks = evaluated
                .entry(l)
                .or_insert_with(|| crate::animation::marks(&e.doc.layers[l], e.frame));
            if let Some((_, inside)) = self.partial.get(&(l, m)) {
                for point in inside.iter().flat_map(vertices) {
                    let point = e.depth_point(point, l, e.layer);
                    r.extend_with(Pos2::ZERO + rotate(point.to_vec2(), -self.orientation));
                }
                continue;
            }
            let mark = &marks[m];
            let points = mark
                .style
                .instance
                .as_ref()
                .and_then(|instance| {
                    let bounds = e.animation_instance_bounds(mark)?;
                    Some(
                        [
                            bounds.left_top(),
                            bounds.right_top(),
                            bounds.right_bottom(),
                            bounds.left_bottom(),
                        ]
                        .map(|point| crate::rig::point(instance.matrix, point))
                        .to_vec(),
                    )
                })
                .unwrap_or_else(|| vertices(mark));
            for p in points {
                let p = e.depth_point(p, l, e.layer);
                r.extend_with(Pos2::ZERO + rotate(p.to_vec2(), -self.orientation));
            }
        }
        if r.is_finite() {
            if r.width() < 1. {
                r = r.expand2(Vec2::new(0.5, 0.));
            }
            if r.height() < 1. {
                r = r.expand2(Vec2::new(0., 0.5));
            }
            Some(SelectionBox {
                rect: r,
                angle: self.orientation,
            })
        } else {
            None
        }
    }
}
const SELECTION_BLUE: Color32 = Color32::from_rgb(104, 193, 255);

fn paint_selection_outline(painter: &egui::Painter, corners: [Pos2; 4], dashed: bool) {
    if corners.iter().any(|point| !point.is_finite()) {
        return;
    }
    let color = Stroke::new(
        1.5,
        if dashed {
            SELECTION_BLUE
        } else {
            crate::themes::accent()
        },
    );
    let clip = painter.clip_rect().expand(2.);
    for (a, b) in corners.into_iter().zip(corners.into_iter().cycle().skip(1)) {
        if !dashed {
            painter.line_segment([a, b], color);
            continue;
        }
        // Limit dash generation to the visible portion of large or zoomed objects.
        let delta = b - a;
        let (mut start, mut end) = (0.0_f32, 1.0_f32);
        for axis in 0..2 {
            if delta[axis] == 0. {
                if a[axis] < clip.min[axis] || a[axis] > clip.max[axis] {
                    start = 1.;
                    end = 0.;
                }
            } else {
                let p = (clip.min[axis] - a[axis]) / delta[axis];
                let q = (clip.max[axis] - a[axis]) / delta[axis];
                start = start.max(p.min(q));
                end = end.min(p.max(q));
            }
        }
        if start < end {
            painter.extend(egui::Shape::dashed_line(
                &[a + delta * start, a + delta * end],
                color,
                5.,
                3.,
            ));
        }
    }
}

pub fn vertices(m: &Mark) -> Vec<Pos2> {
    let mut points = crate::vector::flatten(m, 0.1);
    if m.style.text.is_some() {
        for ring in &m.holes {
            points.extend(ring.iter().copied().map(crate::vector::pos));
        }
    }
    points
}
fn handles(r: Rect) -> [Pos2; 8] {
    [
        r.left_top(),
        r.center_top(),
        r.right_top(),
        r.right_center(),
        r.right_bottom(),
        r.center_bottom(),
        r.left_bottom(),
        r.left_center(),
    ]
}
fn distance_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0. {
        ((p - a).dot(ab) / ab.length_sq()).clamp(0., 1.)
    } else {
        0.
    };
    p.distance(a + ab * t)
}
pub(crate) fn stroke_hit(m: &Mark, p: Pos2, tolerance: f32) -> bool {
    if !m.stroke
        || !crate::vector::control_bounds(m)
            .expand(tolerance + m.width * if m.width_profile.is_empty() { 0.5 } else { 2. })
            .contains(p)
    {
        return false;
    }
    let points = vertices(m);
    let (lengths, total) = crate::width::lengths(&points);
    points.first().is_some_and(|q| {
        q.distance(p) <= tolerance + m.width * crate::width::sample(&m.width_profile, 0.) * 0.5
    }) || points.windows(2).enumerate().any(|(i, w)| {
        let d = w[1] - w[0];
        let t = ((p - w[0]).dot(d) / d.length_sq().max(1e-12)).clamp(0., 1.);
        let fraction = (lengths[i] + d.length() * t) / total.max(1e-6);
        p.distance(w[0] + d * t)
            <= tolerance + m.width * crate::width::sample(&m.width_profile, fraction) * 0.5
    }) || m.holes.iter().any(|r| {
        r.windows(2).any(|w| {
            distance_to_segment(p, crate::vector::pos(w[0]), crate::vector::pos(w[1]))
                <= tolerance + m.width * 0.5
        })
    })
}
pub(crate) fn axis_distance(m: &Mark, p: Pos2) -> f32 {
    vertices(m)
        .windows(2)
        .map(|w| distance_to_segment(p, w[0], w[1]))
        .fold(f32::INFINITY, f32::min)
}
fn hit(m: &Mark, p: Pos2, tolerance: f32) -> bool {
    stroke_hit(m, p, tolerance) || fill_hit(m, p, tolerance)
}
/// Filled brush ink needs the same screen-space picking margin as a stroke.
/// Test the real contour, including counters, rather than its bounding box.
fn fill_hit(mark: &Mark, p: Pos2, tolerance: f32) -> bool {
    if mark.fill.is_none()
        || !crate::vector::control_bounds(mark)
            .expand(tolerance)
            .contains(p)
    {
        return false;
    }
    if crate::vector::contains_fill(mark, p) {
        return true;
    }
    let outer = crate::vector::flatten(mark, 0.15);
    let near = |ring: &[Pos2]| {
        ring.len() >= 2
            && ring
                .iter()
                .zip(ring.iter().cycle().skip(1))
                .take(ring.len())
                .any(|(&a, &b)| distance_to_segment(p, a, b) <= tolerance)
    };
    near(&outer)
        || mark.holes.iter().any(|ring| {
            near(
                &ring
                    .iter()
                    .copied()
                    .map(crate::vector::pos)
                    .collect::<Vec<_>>(),
            )
        })
}
pub fn transformed(m: &Mark, pivot: Pos2, scale: Vec2, angle: f32, translation: Vec2) -> Mark {
    crate::vector::transform(m, |p| {
        let v = p - pivot;
        let p = pivot + rotate(Vec2::new(v.x * scale.x, v.y * scale.y), angle) + translation;
        Pos2::new(p.x.clamp(-1e7, 1e7), p.y.clamp(-1e7, 1e7))
    })
}
fn safe_ratio(a: f32, b: f32) -> f32 {
    if b.abs() < 0.0001 {
        1.
    } else {
        let s = a / b;
        if s.abs() < 0.01 {
            if s < 0. { -0.01 } else { 0.01 }
        } else {
            s.clamp(-1000., 1000.)
        }
    }
}
pub(crate) fn paint_mark(
    p: &egui::Painter,
    m: &Mark,
    v: View,
    ghost: bool,
    selected: bool,
    cache: Option<&mut crate::paint::RenderCache>,
) -> bool {
    crate::paint::draw(p, m, v, ghost, selected, cache)
}
impl Editor {
    pub(crate) fn selection_hit(&self, mark: &Mark, point: Pos2, tolerance: f32) -> bool {
        if let Some(instance) = &mark.style.instance
            && let Some(bounds) = self.animation_instance_bounds(mark)
            && let Some(inverse) = crate::rig::inverse(instance.matrix)
        {
            let local = crate::rig::point(inverse, point);
            return bounds.contains(local);
        }
        hit(mark, point, tolerance)
    }

    fn canvas_edit_view(&self, stage: View) -> View {
        let camera = self.camera_view(stage);
        if self.tool == Tool::Camera {
            camera
        } else {
            self.camera_depth_view(camera, self.layer)
        }
    }

    fn zoom_canvas_at(&self, state: &mut Canvas, area: Rect, screen: Pos2, zoom: f32) -> View {
        let anchor = self.canvas_edit_view(state.view(area)).world(screen);
        state.zoom = zoom;
        let mut view = self.canvas_edit_view(state.view(area));
        let correction = screen - view.screen(anchor);
        state.pan += correction;
        view.center += correction;
        view
    }

    pub fn canvas_ui(&mut self, ui: &mut egui::Ui) {
        self.bucket_range_ui(ui.ctx());
        let tool = if self.drawing.pen.eraser() {
            Tool::Eraser
        } else {
            self.tool
        };
        let tool = if self.doc.layers[self.layer].options.animation_track
            && !matches!(
                tool,
                Tool::Select
                    | Tool::Object
                    | Tool::Transform
                    | Tool::Pan
                    | Tool::Zoom
                    | Tool::Eyedropper
                    | Tool::Trim
            ) {
            Tool::Object
        } else {
            tool
        };
        let picker = self.studio.color_picker;
        let mut state = std::mem::take(&mut self.canvas);
        state.stage_size = Vec2::new(self.doc.width as f32, self.doc.height as f32);
        if state.pen.as_ref().is_some_and(|p| {
            tool != Tool::Pen || p.frame != self.frame || p.layer != self.layer || self.playing
        }) {
            let pen = state.pen.take().unwrap();
            state.drag = None;
            state.pen_redo.clear();
            self.commit_pen(pen);
        }
        if tool != Tool::Pen || state.frame != self.frame || self.playing {
            state.pen_redo.clear();
        }
        if tool != Tool::PolygonSelect {
            state.polygon_selection.clear();
        }
        state.cache.begin_frame();
        if state.frame != self.frame || self.playing {
            self.draft = None;
            state.clear_selection();
            state.frame = self.frame;
        }
        state.selected.retain(|&(l, m)| {
            self.doc.layers.get(l).is_some_and(|l| {
                l.visible
                    && !l.options.locked
                    && l.frames[self.frame]
                        .get(m)
                        .is_some_and(|mark| !mark.style.raster_tile)
            })
        });
        state.partial.retain(|id, _| state.selected.contains(id));
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let ruler_area = response.rect;
        let area = if self.studio.rulers {
            Rect::from_min_max(ruler_area.min + Vec2::splat(22.), ruler_area.max)
        } else {
            ruler_area
        };
        let ruler_painter = painter.clone();
        let painter = painter.with_clip_rect(area);
        if response.contains_pointer()
            && state.drag.is_none()
            && let Some((screen, scroll)) = ui.input(|i| {
                let screen = i.pointer.hover_pos()?;
                let scroll = i.smooth_scroll_delta().y;
                (area.contains(screen) && scroll != 0. && !i.pointer.primary_down())
                    .then_some((screen, scroll))
            })
        {
            let zoom = (state.zoom * (scroll * 0.0025).exp()).clamp(0.08, 24.);
            self.zoom_canvas_at(&mut state, area, screen, zoom);
        }
        let stage_view = state.view(area);
        let mut view = self.canvas_edit_view(stage_view);
        let ctx = ui.ctx().clone();
        if egui::DragAndDrop::has_payload_of_type::<crate::media::MediaDrag>(&ctx)
            && let Some(id) = response.dnd_release_payload::<crate::media::MediaDrag>()
            && let Some(pos) = ctx.input(|i| i.pointer.interact_pos())
            && area.contains(pos)
        {
            self.place_media_at(id.0, self.frame, None, Some(view.world(pos)));
        }
        if ctx.input(|i| i.pointer.primary_released()) {
            let drag = self.assets.drag.take();
            if !self.playing
                && let Some(crate::library::Asset::Symbol(id)) = drag
                && let Some(pos) = ctx.input(|i| i.pointer.interact_pos())
                && area.contains(pos)
                && ui.rect_contains_pointer(area)
            {
                self.canvas = state;
                self.insert_symbol_at(id, Some(view.world(pos)));
                state = std::mem::take(&mut self.canvas);
            }
        }
        let press_position = ctx.input(|i| {
            i.events.iter().rev().find_map(|event| match event {
                egui::Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed: true,
                    ..
                } => Some(*pos),
                _ => None,
            })
        });
        let picker_click = picker.is_some()
            && press_position.is_some_and(|pos| {
                area.contains(pos) && ctx.layer_id_at(pos) == Some(ui.layer_id())
            });
        let trace_input = !picker_click && self.shift_trace_input(ui, area, view);
        let light_input = trace_input
            || (!picker_click && self.light_gizmo(ui, self.camera_view(stage_view), true));
        let trim_input = !picker_click
            && !light_input
            && !self.raster_active()
            && self.vector_trim_input(ui, area, self.camera_view(stage_view));
        let folder_input = !picker_click
            && !light_input
            && !trim_input
            && self.folder_canvas_input(ui, area, self.camera_view(stage_view));
        let video_input = !picker_click
            && !light_input
            && !folder_input
            && self.video_transform_input(ui, area, self.camera_view(stage_view));
        if video_input {
            state.clear_selection();
        }
        let wants_keyboard = ctx.egui_wants_keyboard_input()
            || self.animation.scenes_open
            || self.animation.picker.is_some()
            || self.studio.preferences_open
            || self.document_settings.is_some()
            || self.view_options.draft.is_some()
            || self.studio.export.is_some();
        let (
            pointer,
            pressed,
            middle_pressed,
            primary_down,
            middle_down,
            released,
            space,
            shift,
            escape,
        ) = ctx.input(|i| {
            (
                i.pointer.interact_pos(),
                !picker_click && !video_input && i.pointer.button_pressed(PointerButton::Primary),
                i.pointer.button_pressed(PointerButton::Middle),
                i.pointer.button_down(PointerButton::Primary),
                i.pointer.button_down(PointerButton::Middle),
                i.pointer.button_released(PointerButton::Primary),
                i.key_down(egui::Key::Space) && !wants_keyboard,
                i.modifiers.shift,
                i.key_pressed(egui::Key::Escape),
            )
        });
        let over = response.contains_pointer();
        if pressed
            && over
            && state.drag.is_none()
            && crate::depth::active(self)
            && state.selected.len() <= 1
            && matches!(
                tool,
                Tool::Select | Tool::Object | Tool::Transform | Tool::Node
            )
        {
            if let Some(screen) = pointer {
                let base = self.camera_view(stage_view);
                let target = self
                    .doc
                    .layers
                    .iter()
                    .enumerate()
                    .rev()
                    .filter(|(_, l)| l.visible && !l.options.locked)
                    .find_map(|(l, layer)| {
                        let local = self.camera_depth_view(base, l).world(screen);
                        crate::animation::marks(layer, self.frame)
                            .iter()
                            .any(|m| self.selection_hit(m, local, 4. / view.scale))
                            .then_some(l)
                    });
                if let Some(l) = target {
                    self.layer = l;
                    view = self.camera_depth_view(base, l);
                }
            }
        }
        if let Some(fill) = picker.filter(|_| picker_click) {
            let _ = self.sample_at(
                view.world(press_position.unwrap()),
                3. / view.scale,
                Some(fill),
            );
            self.studio.color_picker = None;
        }
        let character_bones = self.character_bone_input(ui, &painter, area, view);
        let visual_handled =
            !picker_click && self.visual_canvas_input(ui, area, view, &state.selected);
        let mut guide_handled = if picker_click
            || (self.adjustment_active()
                && !space
                && !middle_down
                && !matches!(
                    tool,
                    Tool::Pan
                        | Tool::Zoom
                        | Tool::RotateView
                        | Tool::Select
                        | Tool::Object
                        | Tool::Transform
                ))
            || character_bones
            || light_input
            || trim_input
            || folder_input
            || self.folder_groups.dialog.is_some()
            || self.document_settings.is_some()
            || self.view_options.draft.is_some()
        {
            true
        } else if self.drawing.pen.eraser() {
            false
        } else {
            visual_handled
                | self.rulers_ui(ui, &ruler_painter, ruler_area, view)
                | self.advanced_canvas_input(ui, area, view)
                | self.mannequin_input(ui, area, view)
                | self.rig_input(ui, area, view, &state.selected)
        };
        let raster_edit = self.raster_active() && Self::raster_tool(tool);
        let raster_blocked = self.raster_active()
            && !matches!(
                tool,
                Tool::Select
                    | Tool::Object
                    | Tool::Transform
                    | Tool::Pan
                    | Tool::Zoom
                    | Tool::RotateView
                    | Tool::Ruler
                    | Tool::Eyedropper
            );
        let raster_input = self.raster_input(
            &mut state,
            tool,
            pointer,
            view,
            pressed
                && over
                && pointer.is_some_and(|p| area.contains(p))
                && !guide_handled
                && !space,
            primary_down && !space && !escape,
            ctx.input(|i| i.modifiers.ctrl || i.modifiers.command),
        );
        if Self::raster_needs_tick(&state) {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        if !space && !middle_down && (raster_input || raster_blocked) {
            guide_handled = true;
        }
        if over
            && ctx.input(|i| i.pointer.secondary_pressed())
            && state.drag.is_none()
            && matches!(
                tool,
                Tool::Select | Tool::Object | Tool::Transform | Tool::Node
            )
        {
            if let Some(world) = pointer.map(|p| view.world(p)) {
                let hit = self
                    .doc
                    .layers
                    .iter()
                    .enumerate()
                    .rev()
                    .filter(|(_, l)| l.visible && !l.options.locked)
                    .find_map(|(l, layer)| {
                        crate::animation::marks(layer, self.frame)
                            .iter()
                            .enumerate()
                            .rev()
                            .find(|(_, mark)| self.depth_hit(l, mark, world, 4. / view.scale))
                            .map(|(m, _)| (l, m))
                    });
                if let Some(id) = hit.filter(|id| !state.selected.contains(id)) {
                    state.clear_selection();
                    state.selected.push(id);
                    self.layer = id.0;
                }
            }
        }
        let mut convert = false;
        let mut lip_symbol = false;
        let mut arrange = None;
        let mut clipboard = None;
        if tool != Tool::Mannequin {
            response.context_menu(|ui| {
                let selected = !state.selected.is_empty() && !self.playing;
                for (icon, label, enabled) in [
                    (Icon::Copy, "Copiar", selected),
                    (Icon::Cut, "Cortar", selected),
                    (
                        Icon::Paste,
                        "Pegar",
                        !self.playing && !self.doc.layers[self.layer].options.locked,
                    ),
                ] {
                    if crate::design::button(ui, icon, label, label, false, enabled).clicked() {
                        clipboard = Some(icon);
                        ui.close();
                    }
                }
                ui.separator();
                arrange = crate::ordering::menu(ui, !state.selected.is_empty() && !self.playing);
                ui.separator();
                if crate::design::button(
                    ui,
                    Icon::Symbol,
                    "Convertir a símbolo · F8",
                    "Convertir a símbolo",
                    false,
                    !state.selected.is_empty(),
                )
                .clicked()
                {
                    convert = true;
                    ui.close();
                }
                if crate::design::button(
                    ui,
                    Icon::Mouth,
                    "Convert to Lip Sync symbol",
                    "",
                    false,
                    !state.selected.is_empty() && !self.playing,
                )
                .clicked()
                {
                    lip_symbol = true;
                    ui.close();
                }
            });
        }
        if let Some(action) = clipboard {
            self.canvas = state;
            match action {
                Icon::Copy => self.copy_canvas_to_clipboard(&ctx, false),
                Icon::Cut => self.copy_canvas_to_clipboard(&ctx, true),
                _ => self.paste_canvas_clipboard(),
            }
            state = std::mem::take(&mut self.canvas);
            ctx.request_repaint();
        }
        if let Some(action) = arrange {
            self.arrange_selection(&mut state, action);
        }
        if convert || lip_symbol {
            self.canvas = state;
            self.convert_symbol_kind(lip_symbol);
            state = std::mem::take(&mut self.canvas);
            ctx.request_repaint();
        }
        if !picker_click
            && !video_input
            && !light_input
            && over
            && ctx.input(|i| i.pointer.button_double_clicked(PointerButton::Primary))
            && matches!(tool, Tool::Select | Tool::Object | Tool::Transform)
        {
            if let Some(screen) = pointer {
                let world = view.world(screen);
                let hit = self
                    .doc
                    .layers
                    .iter()
                    .enumerate()
                    .rev()
                    .filter(|(_, l)| l.visible)
                    .find_map(|(l, layer)| {
                        let marks = crate::animation::marks(layer, self.frame);
                        marks
                            .iter()
                            .enumerate()
                            .rev()
                            .find(|(_, mark)| self.depth_hit(l, mark, world, 3. / view.scale))
                            .map(|(m, mark)| (l, m, mark.style.instance.as_ref().map(|i| i.symbol)))
                    });
                if let Some((l, _, Some(id))) = hit
                    && !self.doc.layers[l].options.locked
                {
                    self.canvas = state;
                    self.enter_symbol(id);
                    return;
                }
                if let Some((l, m, None)) = hit
                    && matches!(tool, Tool::Select | Tool::Object)
                    && !self.doc.layers[l].options.locked
                    && stroke_hit(
                        &self.doc.layers[l].frames[self.frame][m],
                        self.depth_point(world, self.layer, l),
                        3. / view.scale,
                    )
                {
                    self.select_connected_strokes(&mut state, (l, m));
                    state.drag = None;
                    state.pivot = None;
                    self.layer = l;
                    ctx.request_repaint();
                }
                let stage_bounds = Rect::from_min_size(Pos2::ZERO, state.stage_size);
                if hit.is_none()
                    && !self.feature.symbol_stack.is_empty()
                    && stage_bounds.contains(stage_view.world(screen))
                {
                    self.canvas = state;
                    self.exit_symbol();
                    return;
                }
            }
        }
        let alt = ctx.input(|i| i.modifiers.alt);
        let command = ctx.input(|i| i.modifiers.command);
        let mut handled_delete = false;
        if !wants_keyboard && ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            if let Some(pen) = state.pen.take() {
                state.drag = None;
                state.pen_redo.clear();
                self.commit_pen(pen);
            }
        }
        if matches!(tool, Tool::Node | Tool::Pen)
            && !wants_keyboard
            && ctx.input(|i| i.key_pressed(egui::Key::Delete))
        {
            if let Some((l, m, index)) = state.node_selection.take() {
                let mut mark = editable_nodes(&self.doc.layers[l].frames[self.frame][m]);
                self.checkpoint();
                handled_delete = true;
                state.drag = None;
                if mark.nodes.len() <= 2 {
                    self.doc.layers[l].frames[self.frame].remove(m);
                    state.clear_selection();
                } else {
                    mark.nodes.remove(index);
                    self.doc.layers[l].frames[self.frame][m] = mark;
                }
                self.recover();
            }
        }
        if escape {
            self.cancel_guide_drag();
            let canceled = state.drag.take();
            if let Some(Drag::Width(d)) = &canceled {
                self.doc.layers[d.id.0].frames[self.frame][d.id.1] = d.original.clone();
            }
            if let Some(Drag::Node(d)) = &canceled {
                self.doc.layers[d.id.0].frames[self.frame][d.id.1] = d.original.clone();
            }
            if let Some(Drag::Transform(d)) = canceled {
                for ((l, m), original) in d.originals {
                    self.doc.layers[l].frames[self.frame][m] = original;
                }
                state.pivot = Some(d.pivot);
                state.orientation = d.bounds.angle;
            } else {
                state.clear_selection();
            }
            self.draft = None;
            state.pen = None;
            state.pen_redo.clear();
        }
        if !ctx.egui_wants_keyboard_input() && state.drag.is_none() {
            if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::A))
                && matches!(tool, Tool::Select | Tool::Transform | Tool::Object)
            {
                state.selected.clear();
                state.partial.clear();
                for (l, layer) in self
                    .doc
                    .layers
                    .iter()
                    .enumerate()
                    .filter(|(_, l)| l.visible && !l.options.locked)
                {
                    for (m, mark) in layer.frames[self.frame].iter().enumerate() {
                        if !mark.style.raster_tile {
                            state.selected.push((l, m));
                        }
                    }
                }
                state.pivot = state.bounds(self).map(|b| b.center());
            }
            if !handled_delete
                && ctx.input(|i| i.key_pressed(egui::Key::Delete))
                && !state.selected.is_empty()
            {
                if state
                    .selected
                    .iter()
                    .any(|(l, _)| !self.doc.layers[*l].options.animation_track)
                {
                    self.checkpoint();
                }
                self.materialize_fragments(&mut state);
                let mut ids = state.selected.clone();
                ids.sort_unstable();
                for (l, m) in ids.into_iter().rev() {
                    if !self.queue_animation_clip_delete(l) {
                        self.doc.layers[l].frames[self.frame].remove(m);
                    }
                }
                state.clear_selection();
                self.recover();
            }
            if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::D))
                && !state.selected.is_empty()
            {
                if state
                    .selected
                    .iter()
                    .any(|(l, _)| !self.doc.layers[*l].options.animation_track)
                {
                    self.checkpoint();
                }
                let mut selected = vec![];
                for &(l, m) in &state.selected {
                    if self.queue_animation_clip_duplicate(l) {
                        continue;
                    }
                    let source = state.partial.get(&(l, m)).map_or_else(
                        || std::slice::from_ref(&self.doc.layers[l].frames[self.frame][m]),
                        |(_, inside)| inside.as_slice(),
                    );
                    let copies: Vec<_> = source
                        .iter()
                        .map(|mark| {
                            transformed(mark, Pos2::ZERO, Vec2::splat(1.), 0., Vec2::splat(15.))
                        })
                        .collect();
                    let index = self.doc.layers[l].frames[self.frame].len();
                    selected.extend((index..index + copies.len()).map(|m| (l, m)));
                    self.doc.layers[l].frames[self.frame].extend(copies);
                }
                state.partial.clear();
                state.selected = selected;
                state.pivot = state.pivot.map(|p| p + Vec2::splat(15.));
                self.recover();
            }
        }
        if !guide_handled
            && (!matches!(
                tool,
                Tool::Ruler
                    | Tool::Symmetry
                    | Tool::Mirror
                    | Tool::Perspective
                    | Tool::Camera
                    | Tool::Mannequin
                    | Tool::Bones
                    | Tool::Puppet
            ) || space
                || middle_pressed)
            && (press_position
                .or(pointer)
                .is_some_and(|p| area.contains(p) && ctx.layer_id_at(p) == Some(ui.layer_id())))
            && state.drag.is_none()
            && (pressed || middle_pressed)
        {
            let screen = press_position.or(pointer).unwrap();
            let world = view.world(screen);
            if middle_pressed || (pressed && (space || tool == Tool::Pan)) {
                state.drag = Some(Drag::Pan(screen));
            } else if tool == Tool::Zoom {
                let zoom =
                    (state.zoom * if self.zoom_out ^ alt { 0.8 } else { 1.25 }).clamp(0.05, 32.);
                view = self.zoom_canvas_at(&mut state, area, screen, zoom);
            } else if !self.playing
                && tool == Tool::Text
                && !self.doc.layers[self.layer].options.locked
                && self.doc.layers[self.layer].options.mannequin.is_none()
            {
                self.text_at(world);
                if self.drawing.text.editing.is_none() {
                    self.drawing.text.box_drag = Some(world);
                }
                state.selected = self.canvas.selected.clone();
            } else if !self.playing && tool == Tool::Eyedropper {
                if !self.raster_active() || !self.pick_raster_color(world) {
                    self.sample_apply_at(&state.selected, world, 3. / view.scale);
                }
            } else if !self.playing && tool == Tool::Wand {
                self.wand_select(&mut state, world, shift);
            } else if !self.playing && tool == Tool::Lasso {
                state.drag = Some(Drag::Area {
                    points: vec![world],
                    add: shift,
                });
            } else if !self.playing && tool == Tool::PolygonSelect {
                if state.polygon_selection.is_empty() {
                    state.polygon_add = shift;
                }
                if state.polygon_selection.len() >= 3
                    && view.screen(state.polygon_selection[0]).distance(screen) < 8.
                {
                    let points = std::mem::take(&mut state.polygon_selection);
                    let add = state.polygon_add;
                    self.cut_area_selection(&mut state, &points, add);
                } else {
                    state.polygon_selection.push(world);
                }
            } else if !self.playing
                && tool == Tool::Eraser
                && self.doc.layers[self.layer].visible
                && !self.doc.layers[self.layer].options.locked
                && self.doc.layers[self.layer].options.mannequin.is_none()
            {
                state.clear_selection();
                state.drag = Some(Drag::Erase {
                    last: world,
                    changed: false,
                });
            } else if !self.playing
                && tool == Tool::Select
                && selected_node_hit(self, &state, world, view)
            {
                self.start_node_drag(&mut state, world, view, alt);
            } else if tool == Tool::RotateView {
                let delta = screen - view.center;
                state.drag = Some(Drag::ViewRotate {
                    start: delta.y.atan2(delta.x),
                    angle: state.angle,
                });
            } else if !self.playing
                && tool == Tool::Pen
                && self.doc.layers[self.layer].visible
                && !self.doc.layers[self.layer].options.locked
                && self.doc.layers[self.layer].options.mannequin.is_none()
            {
                if self.feature.pen_edit_only || (alt && state.pen.is_none()) {
                    self.start_node_drag(&mut state, world, view, alt);
                } else if state.pen.is_some()
                    || !self.grab_selected_node(&mut state, world, view, alt)
                {
                    self.begin_pen(&mut state, world, view, alt);
                }
            } else if !self.playing
                && tool == Tool::Bucket
                && self.doc.layers[self.layer].visible
                && !self.doc.layers[self.layer].options.locked
                && self.doc.layers[self.layer].options.mannequin.is_none()
            {
                if self.feature.bucket_range_armed || self.feature.bucket_next {
                    self.start_bucket_range(&ctx, world, shift);
                } else {
                    self.bucket_at(&mut state, world, shift);
                }
            } else if !self.playing && tool == Tool::Width {
                self.start_width_drag(&mut state, world, view);
            } else if !self.playing && tool == Tool::Node {
                self.start_node_drag(&mut state, world, view, alt);
            } else if !self.playing
                && matches!(
                    tool,
                    Tool::Brush
                        | Tool::Airbrush
                        | Tool::SketchBrush
                        | Tool::FillBrush
                        | Tool::ShadeBrush
                        | Tool::LightBrush
                        | Tool::VectorBrush
                        | Tool::Rectangle
                        | Tool::Line
                        | Tool::Ellipse
                        | Tool::Polygon
                )
                && self.doc.layers[self.layer].visible
                && !self.doc.layers[self.layer].options.locked
                && self.doc.layers[self.layer].options.mannequin.is_none()
                && match tool {
                    Tool::Brush
                    | Tool::Airbrush
                    | Tool::SketchBrush
                    | Tool::FillBrush
                    | Tool::ShadeBrush
                    | Tool::LightBrush => true,
                    Tool::VectorBrush | Tool::Line => self.stroke_enabled,
                    _ => self.fill_enabled || self.stroke_enabled,
                }
            {
                let world = if matches!(
                    tool,
                    Tool::Rectangle | Tool::Ellipse | Tool::Polygon | Tool::Line
                ) {
                    self.prepare_snapping(&[]);
                    self.snap_point(world, view, alt)
                } else {
                    world
                };
                self.drawing.start(world, tool);
                self.draft = Some(Mark {
                    points: vec![[world.x, world.y]].into(),
                    width: self.width,
                    width_profile: if matches!(
                        tool,
                        Tool::VectorBrush | Tool::Line | Tool::Ellipse | Tool::Polygon
                    ) {
                        self.width_profile.clone()
                    } else {
                        Default::default()
                    },
                    color: if matches!(
                        tool,
                        Tool::Brush
                            | Tool::Airbrush
                            | Tool::SketchBrush
                            | Tool::FillBrush
                            | Tool::ShadeBrush
                            | Tool::LightBrush
                            | Tool::VectorBrush
                    ) {
                        self.fill_color.to_srgba_unmultiplied()
                    } else {
                        self.color.to_array()
                    },
                    rectangle: tool == Tool::Rectangle,
                    closed: tool == Tool::Rectangle,
                    stroke: if matches!(tool, Tool::Rectangle | Tool::Ellipse | Tool::Polygon) {
                        self.stroke_enabled
                    } else {
                        true
                    },
                    fill: if matches!(tool, Tool::Rectangle | Tool::Ellipse | Tool::Polygon)
                        && self.fill_enabled
                    {
                        Some(self.fill_color.to_array())
                    } else {
                        None
                    },
                    ..Mark::default()
                });
                state.clear_selection();
                state.drag = Some(Drag::Draw {
                    smoother: crate::brush::StrokeSmoother::new([world.x, world.y], self.smoothing),
                    tool: tool,
                });
            } else if !self.playing && matches!(tool, Tool::Select | Tool::Transform | Tool::Object)
            {
                let force_marquee = command && tool == Tool::Select;
                let mut op = None;
                if let Some(bounds) = state.bounds(self).filter(|_| !force_marquee) {
                    let pivot = state.transform_pivot(self, bounds);
                    state.pivot = Some(pivot);
                    if tool == Tool::Transform {
                        if screen.distance(view.screen(pivot)) < 10. {
                            op = Some(Operation::Pivot);
                        }
                        let top = view.screen(bounds.center_top());
                        let rotation = top + rotate(Vec2::new(0., -30.), view.angle + bounds.angle);
                        if op.is_none() && screen.distance(rotation) < 10. {
                            op = Some(Operation::Rotate);
                        }
                        if op.is_none() {
                            for (h, p) in bounds.handles().into_iter().enumerate() {
                                if screen.distance(view.screen(p)) < 8. {
                                    op = Some(Operation::Scale(h));
                                    break;
                                }
                            }
                        }
                    }
                    if op.is_none()
                        && !shift
                        && bounds.contains(world)
                        && (tool == Tool::Transform
                            || state.selected.iter().any(|&(l, m)| {
                                self.depth_hit(
                                    l,
                                    &crate::animation::marks(&self.doc.layers[l], self.frame)[m],
                                    world,
                                    4. / view.scale,
                                )
                            }))
                    {
                        op = Some(Operation::Move);
                    }
                }
                let mut toggle_on_click = None;
                if op.is_none() {
                    let mut found = None;
                    'layers: for (l, layer) in self
                        .doc
                        .layers
                        .iter()
                        .enumerate()
                        .rev()
                        .filter(|(_, l)| l.visible && !l.options.locked)
                    {
                        for (m, mark) in crate::animation::marks(layer, self.frame)
                            .iter()
                            .enumerate()
                            .rev()
                        {
                            if !force_marquee && self.depth_hit(l, mark, world, 6. / view.scale) {
                                found = Some((l, m));
                                break 'layers;
                            }
                        }
                    }
                    if let Some(mut id) = found {
                        if tool == Tool::Object {
                            if shift && state.selected.contains(&id) {
                                toggle_on_click = Some(id);
                                op = Some(Operation::Move);
                            } else {
                                self.select_connected_object(&mut state, id, shift);
                                self.layer = id.0;
                                if !shift {
                                    op = Some(Operation::Move);
                                }
                            }
                            state.pivot = None;
                        } else {
                            let mut segment = None;
                            if tool == Tool::Select
                                && !state.selected.contains(&id)
                                && self.doc.layers[id.0].frames[self.frame][id.1]
                                    .style
                                    .instance
                                    .is_none()
                                && self.doc.layers[id.0].frames[self.frame][id.1]
                                    .style
                                    .media_image
                                    .is_none()
                            {
                                let wants_stroke = stroke_hit(
                                    &self.doc.layers[id.0].frames[self.frame][id.1],
                                    world,
                                    6. / view.scale,
                                );
                                id = self.separate_component(&mut state, id, wants_stroke);
                                if wants_stroke
                                    && !self.doc.layers[id.0].frames[self.frame][id.1]
                                        .style
                                        .primitive
                                {
                                    segment = self.preview_stroke_segment(
                                        id,
                                        self.depth_point(world, self.layer, id.0),
                                    );
                                } else if !wants_stroke {
                                    id = self.split_fill_face(&mut state, id, world);
                                }
                            }
                            if shift && state.selected.contains(&id) {
                                toggle_on_click = Some(id);
                                op = Some(Operation::Move);
                            } else {
                                if !shift && !state.selected.contains(&id) {
                                    state.selected.clear();
                                    state.partial.clear();
                                }
                                if !state.selected.contains(&id) {
                                    state.selected.push(id);
                                }
                                if let Some(parts) = segment {
                                    state.partial.insert(id, parts);
                                }
                                self.layer = id.0;
                                state.pivot = None;
                                op = Some(Operation::Move);
                            }
                        }
                    } else {
                        if !shift {
                            state.clear_selection();
                        }
                        state.drag = Some(Drag::Marquee {
                            start: world,
                            end: world,
                            add: shift,
                        });
                    }
                }
                if let (Some(operation), Some(bounds)) = (op, state.bounds(self)) {
                    self.prepare_snapping(&state.selected);
                    let pivot = state.transform_pivot(self, bounds);
                    let originals = state
                        .selected
                        .iter()
                        .map(|&(l, m)| {
                            (
                                (l, m),
                                crate::animation::marks(&self.doc.layers[l], self.frame)[m].clone(),
                            )
                        })
                        .collect();
                    state.drag = Some(Drag::Transform(TransformDrag {
                        operation,
                        start: world,
                        pivot,
                        bounds,
                        originals,
                        changed: false,
                        last_input: None,
                        toggle_on_click,
                    }));
                }
            }
        }
        if let Some(screen) = pointer {
            let world = view.world(screen);
            match state.drag.as_mut() {
                Some(Drag::Erase { last, changed }) => {
                    let samples = ctx.input(|i| {
                        i.events
                            .iter()
                            .filter_map(|e| match e {
                                egui::Event::PointerMoved(p) => Some(view.world(*p)),
                                egui::Event::PointerButton {
                                    pos,
                                    button: PointerButton::Primary,
                                    pressed: false,
                                    ..
                                } => Some(view.world(*pos)),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                    });
                    for point in samples.into_iter().chain(std::iter::once(world)) {
                        let before = (!*changed).then(|| self.doc.clone());
                        let did_change = self.erase_segment(*last, point);
                        if did_change && !*changed {
                            self.hub.dirty();
                            self.undo.push(before.unwrap());
                            if self.undo.len() > 60 {
                                self.undo.remove(0);
                            }
                            self.redo.clear();
                        }
                        *changed |= did_change;
                        *last = point;
                    }
                    if released || !primary_down {
                        if *changed {
                            self.recover();
                        }
                        state.drag = None;
                    }
                }
                Some(Drag::Area { points, add }) => {
                    let samples = ctx.input(|i| {
                        i.events
                            .iter()
                            .filter_map(|e| {
                                if let egui::Event::PointerMoved(p) = e {
                                    Some(view.world(*p))
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<_>>()
                    });
                    for p in samples {
                        if points
                            .last()
                            .is_none_or(|q| q.distance(p) * view.scale > 2.)
                        {
                            points.push(p);
                        }
                    }
                    if released || !primary_down {
                        points.push(world);
                        let points = points.clone();
                        let add = *add;
                        self.cut_area_selection(&mut state, &points, add);
                        state.drag = None;
                    }
                }
                Some(Drag::Pan(last)) => {
                    if primary_down || middle_down || released {
                        state.pan += screen - *last;
                        *last = screen;
                        view.center = area.center() + state.pan;
                        if released {
                            state.drag = None;
                        }
                    } else {
                        state.drag = None;
                    }
                }
                Some(Drag::ViewRotate { start, angle }) => {
                    if primary_down {
                        let d = screen - view.center;
                        state.angle = *angle + d.y.atan2(d.x) - *start;
                        if shift {
                            state.angle = (state.angle / (std::f32::consts::PI / 12.)).round()
                                * (std::f32::consts::PI / 12.);
                        }
                        view.angle = state.angle;
                    } else {
                        state.drag = None;
                    }
                }
                Some(Drag::PenNode {
                    index,
                    start,
                    original,
                    convert,
                }) => {
                    if let Some(pen) = &mut state.pen {
                        if *convert && (primary_down || released) {
                            let delta = world - *start;
                            if delta.length() > 2. / view.scale {
                                let handle = crate::vector::pos(original.out_handle) + delta;
                                pen.mark.nodes[*index].out_handle = crate::vector::xy(handle);
                                if original.in_handle == original.position
                                    && original.out_handle == original.position
                                {
                                    pen.mark.nodes[*index].in_handle = crate::vector::xy(
                                        crate::vector::pos(original.position) - delta,
                                    );
                                }
                            } else if released {
                                pen.mark.nodes[*index] =
                                    crate::document::Node::corner(original.position);
                            }
                        } else if primary_down
                            && (!pen.closing || world.distance(*start) > 1. / view.scale)
                        {
                            let node = &mut pen.mark.nodes[*index];
                            let anchor = crate::vector::pos(node.position);
                            let mut delta = world - anchor;
                            if shift {
                                let angle = (delta.y.atan2(delta.x) / (std::f32::consts::PI / 4.))
                                    .round()
                                    * (std::f32::consts::PI / 4.);
                                delta = Vec2::angled(angle) * delta.length();
                            }
                            node.out_handle = crate::vector::xy(anchor + delta);
                            node.in_handle = if alt {
                                node.position
                            } else {
                                crate::vector::xy(anchor - delta)
                            };
                        }
                    }
                    if released || !primary_down {
                        state.drag = None;
                        if state.pen.as_ref().is_some_and(|p| p.closing) {
                            let pen = state.pen.take().unwrap();
                            state.pen_redo.clear();
                            self.commit_pen(pen);
                        }
                    }
                }
                Some(Drag::Width(d)) => {
                    if d.changed || world.distance(d.start) > 1.5 / view.scale {
                        if !d.changed {
                            self.checkpoint();
                            d.changed = true;
                        }
                        let mut mark = d.original.clone();
                        let half = (world - d.anchor).dot(d.normal).abs();
                        self.profile_point = Some(crate::width::edit_stop(
                            &mut mark.width_profile,
                            d.t,
                            (half * 2. / mark.width).clamp(0., 4.),
                        ));
                        self.width_profile = mark.width_profile.clone();
                        self.width_preset = crate::width::Preset::Custom;
                        self.doc.layers[d.id.0].frames[self.frame][d.id.1] = mark;
                    }
                    if released || !primary_down {
                        if d.changed {
                            self.recover();
                        }
                        state.drag = None;
                    }
                }
                Some(Drag::Node(d)) => {
                    let world = ctx
                        .input(|i| node_contact_position(&i.events, primary_down))
                        .map(|p| self.depth_point(view.world(p), self.layer, d.id.0));
                    if let Some(world) = world.filter(|p| p.is_finite()) {
                        if (d.handle == 3 && released)
                            || world.distance(d.start)
                                > if d.handle == 3 {
                                    2. / view.scale
                                } else {
                                    0.001
                                }
                            || d.changed
                        {
                            if !d.changed {
                                self.checkpoint();
                                d.changed = true;
                            }
                            let delta = world - d.start;
                            let mark =
                                if d.handle == 3 && delta.length() <= 2. / view.scale && !d.moved {
                                    let mut mark = editable_nodes(&d.original);
                                    let node = &mut mark.nodes[d.index];
                                    *node = crate::document::Node::corner(node.position);
                                    mark
                                } else {
                                    d.moved = true;
                                    move_node(
                                        &d.original,
                                        d.index,
                                        d.handle,
                                        delta,
                                        alt || d.independent,
                                    )
                                };
                            self.doc.layers[d.id.0].frames[self.frame][d.id.1] = mark;
                        }
                    }
                    if released || !primary_down {
                        if d.changed {
                            self.recover();
                        }
                        state.drag = None;
                    }
                }
                Some(Drag::Draw { smoother, tool }) => {
                    let shape_world = if matches!(
                        *tool,
                        Tool::Rectangle | Tool::Line | Tool::Ellipse | Tool::Polygon
                    ) {
                        self.snap_point(world, view, alt)
                    } else {
                        world
                    };
                    if let Some(m) = &mut self.draft {
                        if matches!(
                            *tool,
                            Tool::Line | Tool::Ellipse | Tool::Polygon | Tool::Rectangle
                        ) {
                            let a = smoother.last();
                            let mut b = shape_world;
                            if shift {
                                let delta = b - a;
                                if *tool == Tool::Line {
                                    let angle = (delta.angle() / std::f32::consts::FRAC_PI_4)
                                        .round()
                                        * std::f32::consts::FRAC_PI_4;
                                    b = a + Vec2::angled(angle) * delta.length();
                                } else {
                                    let size = delta.x.abs().max(delta.y.abs());
                                    b = a + Vec2::new(
                                        delta.x.signum() * size,
                                        delta.y.signum() * size,
                                    );
                                }
                            }
                            *m = if *tool == Tool::Rectangle {
                                crate::drawing::rounded_rectangle(a, b, self.drawing.radius, m)
                            } else {
                                crate::editing::shape(
                                    *tool,
                                    a,
                                    b,
                                    self.polygon_sides,
                                    self.polygon_star,
                                    self.star_ratio,
                                    m,
                                )
                            };
                        } else {
                            let samples = ctx.input(|i| {
                                let first = if pressed {
                                    i.events
                                        .iter()
                                        .rposition(|e| {
                                            matches!(
                                                e,
                                                egui::Event::PointerButton {
                                                    button: PointerButton::Primary,
                                                    pressed: true,
                                                    ..
                                                }
                                            )
                                        })
                                        .map_or(0, |n| n + 1)
                                } else {
                                    0
                                };
                                let mut samples = Vec::new();
                                for event in &i.events[first..] {
                                    match event {
                                        egui::Event::PointerMoved(p) if p.is_finite() => {
                                            samples.push(*p)
                                        }
                                        egui::Event::PointerButton {
                                            pos,
                                            button: PointerButton::Primary,
                                            pressed: false,
                                            ..
                                        } => {
                                            if pos.is_finite() {
                                                samples.push(*pos);
                                            }
                                            break;
                                        }
                                        egui::Event::PointerGone
                                        | egui::Event::Touch {
                                            phase: egui::TouchPhase::End | egui::TouchPhase::Cancel,
                                            ..
                                        } => break,
                                        _ => {}
                                    }
                                }
                                samples
                            });
                            for screen in samples {
                                let p = crate::vector::xy(view.world(screen));
                                if smoother.last().distance(crate::vector::pos(p)) > 0.08 {
                                    smoother.push(p, &mut m.points);
                                    self.drawing.sample(crate::vector::pos(p));
                                }
                            }
                        }
                    }
                    if released || !primary_down {
                        if let Some(mut m) = self.draft.take() {
                            m.width_profile = self.drawing.profile(&m.width_profile);
                            let mut parts = vec![];
                            if matches!(
                                *tool,
                                Tool::Rectangle | Tool::Line | Tool::Ellipse | Tool::Polygon
                            ) {
                                m.style.primitive = *tool != Tool::Line;
                                parts.push(m);
                            } else {
                                let curves =
                                    crate::vector::fit(&m.points, 0.2 + self.smoothing * 3.8);
                                if !curves.is_empty() {
                                    m = crate::vector::from_cubics(&curves, &m);
                                }
                                if matches!(*tool, Tool::Airbrush | Tool::SketchBrush) {
                                    parts = crate::drawing::airbrush(
                                        &m,
                                        self.drawing.opacity,
                                        self.drawing.hardness,
                                        *tool == Tool::SketchBrush,
                                    );
                                } else if matches!(
                                    *tool,
                                    Tool::Brush
                                        | Tool::Airbrush
                                        | Tool::SketchBrush
                                        | Tool::FillBrush
                                        | Tool::ShadeBrush
                                        | Tool::LightBrush
                                ) {
                                    parts = crate::vector::brush_fill(&m, m.color);
                                } else {
                                    parts.push(m);
                                }
                            }
                            parts.retain(|m| {
                                !matches!(
                                    *tool,
                                    Tool::Rectangle | Tool::Line | Tool::Ellipse | Tool::Polygon
                                ) || crate::vector::control_bounds(m).size().length() > 0.01
                            });
                            if !parts.is_empty() {
                                self.checkpoint();
                                self.prepare_paint_key(self.layer, self.frame);
                                let parts = self.symmetry_marks(parts);
                                if matches!(*tool, Tool::ShadeBrush | Tool::LightBrush) {
                                    self.tone_paint(parts, *tool == Tool::LightBrush);
                                } else if *tool == Tool::FillBrush {
                                    self.fill_brush(parts);
                                } else {
                                    let parts = self.prepare_style(parts);
                                    if matches!(*tool, Tool::Airbrush | Tool::SketchBrush) {
                                        self.materialize_motion(&[(self.layer, 0)]);
                                        self.doc.layers[self.layer].frames[self.frame]
                                            .extend(parts);
                                    } else {
                                        self.commit_paint(self.layer, self.frame, parts);
                                    }
                                }
                                self.recover();
                            }
                        }
                        state.drag = None;
                    } else {
                        ctx.request_repaint();
                    }
                }
                Some(Drag::Marquee { start, end, add }) => {
                    *end = world;
                    if released || !primary_down {
                        let rect = Rect::from_two_pos(*start, *end);
                        let additive = *add;
                        if tool == Tool::Object {
                            if !additive {
                                state.clear_selection();
                            }
                            let seeds: Vec<_> = self
                                .doc
                                .layers
                                .iter()
                                .enumerate()
                                .filter(|(_, l)| l.visible && !l.options.locked)
                                .flat_map(|(l, layer)| {
                                    layer.frames[self.frame].iter().enumerate().filter_map(
                                        move |(m, mark)| {
                                            crate::vector::control_bounds(mark)
                                                .intersects(rect)
                                                .then_some((l, m))
                                        },
                                    )
                                })
                                .collect();
                            for seed in seeds {
                                self.select_connected_object(&mut state, seed, true);
                            }
                        } else {
                            self.cut_selection(&mut state, rect, additive);
                        }
                        state.drag = None;
                    }
                }
                Some(Drag::Transform(d)) => {
                    if d.last_input != Some((world, shift, alt))
                        && (d.changed || world.distance(d.start) * view.scale > 2.)
                    {
                        d.last_input = Some((world, shift, alt));
                        if !d.changed && !matches!(d.operation, Operation::Pivot) {
                            self.checkpoint();
                            if !state.partial.is_empty() {
                                let mut selection = Canvas {
                                    selected: std::mem::take(&mut state.selected),
                                    partial: std::mem::take(&mut state.partial),
                                    ..Default::default()
                                };
                                self.materialize_fragments(&mut selection);
                                state.selected = selection.selected;
                                d.originals = state
                                    .selected
                                    .iter()
                                    .map(|&(l, m)| {
                                        ((l, m), self.doc.layers[l].frames[self.frame][m].clone())
                                    })
                                    .collect();
                            }
                            self.autokey_motion(&state.selected);
                        }
                        d.changed = true;
                        match d.operation {
                            Operation::Pivot => {
                                state.pivot = Some(world);
                            }
                            operation => {
                                let mut scale = Vec2::splat(1.);
                                let mut angle = 0.;
                                let mut translation = Vec2::ZERO;
                                match operation {
                                    Operation::Move => {
                                        translation = world - d.start;
                                        let axis = shift.then(|| {
                                            usize::from(translation.y.abs() > translation.x.abs())
                                        });
                                        if let Some(axis) = axis {
                                            translation[1 - axis] = 0.;
                                        }
                                        translation = self.snap_translation_axes(
                                            Rect::from_points(&d.bounds.handles()),
                                            translation,
                                            view,
                                            alt,
                                            axis,
                                        );
                                        state.pivot = Some(d.pivot + translation);
                                    }
                                    Operation::Rotate => {
                                        let a = d.start - d.pivot;
                                        let b = world - d.pivot;
                                        angle = b.y.atan2(b.x) - a.y.atan2(a.x);
                                        if shift {
                                            angle = (angle / (std::f32::consts::PI / 12.)).round()
                                                * (std::f32::consts::PI / 12.);
                                        }
                                    }
                                    Operation::Scale(h) => {
                                        let handle = d.bounds.handles()[h];
                                        let initial = rotate(handle - d.pivot, -d.bounds.angle);
                                        let target =
                                            initial + rotate(world - d.start, -d.bounds.angle);
                                        if h % 2 == 0 || h == 3 || h == 7 {
                                            scale.x = safe_ratio(target.x, initial.x);
                                        }
                                        if h % 2 == 0 || h == 1 || h == 5 {
                                            scale.y = safe_ratio(target.y, initial.y);
                                        }
                                        if shift {
                                            let factor =
                                                if (scale.x - 1.).abs() > (scale.y - 1.).abs() {
                                                    scale.x
                                                } else {
                                                    scale.y
                                                };
                                            scale = Vec2::splat(factor);
                                        }
                                    }
                                    _ => {}
                                }
                                state.orientation = d.bounds.angle + angle;
                                for ((l, m), original) in &d.originals {
                                    let track =
                                        self.doc.layers[*l].frames[self.frame][*m].style.track;
                                    let projected = self.depth_mark(original, *l, self.layer);
                                    let mut moved = transformed_in_basis(
                                        &projected,
                                        d.pivot,
                                        scale,
                                        angle,
                                        translation,
                                        d.bounds.angle,
                                    );
                                    moved = self.depth_mark(&moved, self.layer, *l);
                                    moved.style.track = track;
                                    self.doc.layers[*l].frames[self.frame][*m] = moved;
                                }
                            }
                        }
                    }
                    if released || !primary_down {
                        if d.changed
                            && matches!(d.operation, Operation::Pivot)
                            && let [((layer, index), original)] = d.originals.as_slice()
                            && let Some(instance) = &original.style.instance
                            && let Some(inverse) = crate::rig::inverse(instance.matrix)
                            && let Some(pivot) = state.pivot
                        {
                            let pivot = self.depth_point(pivot, self.layer, *layer);
                            let local = crate::rig::point(inverse, pivot);
                            if local.is_finite() {
                                self.checkpoint();
                                self.doc.layers[*layer].frames[self.frame][*index]
                                    .style
                                    .symbol_pivot = Some(crate::vector::xy(local));
                            }
                        }
                        if d.changed {
                            self.recover();
                        } else if let Some(id) = d.toggle_on_click {
                            if tool == Tool::Object {
                                let mut connected = Canvas::default();
                                self.select_connected_object(&mut connected, id, false);
                                state
                                    .selected
                                    .retain(|item| !connected.selected.contains(item));
                            } else {
                                state.selected.retain(|item| *item != id);
                            }
                            state
                                .partial
                                .retain(|item, _| state.selected.contains(item));
                            state.pivot = state.bounds(self).map(|bounds| bounds.center());
                        }
                        state.drag = None;
                    }
                }
                None => {}
            }
        }
        if tool == Tool::PolygonSelect
            && (response.double_clicked()
                || (!wants_keyboard && ctx.input(|i| i.key_pressed(egui::Key::Enter))))
        {
            let points = std::mem::take(&mut state.polygon_selection);
            let add = state.polygon_add;
            self.cut_area_selection(&mut state, &points, add);
        }
        if escape {
            state.polygon_selection.clear();
        }
        if tool == Tool::Pen && response.double_clicked() {
            if let Some(mut pen) = state.pen.take() {
                let n = pen.mark.nodes.len();
                if n > 1
                    && view
                        .screen(crate::vector::pos(pen.mark.nodes[n - 1].position))
                        .distance(view.screen(crate::vector::pos(pen.mark.nodes[n - 2].position)))
                        < 4.
                {
                    pen.mark.nodes.pop();
                }
                state.drag = None;
                state.pen_redo.clear();
                self.commit_pen(pen);
            }
        }
        if over {
            ctx.set_cursor_icon(if picker.is_some() {
                CursorIcon::Crosshair
            } else if matches!(state.drag, Some(Drag::Pan(_))) {
                CursorIcon::Grabbing
            } else if space || tool == Tool::Pan {
                CursorIcon::Grab
            } else if matches!(
                tool,
                Tool::Rectangle | Tool::Ellipse | Tool::Polygon | Tool::Line
            ) {
                CursorIcon::None
            } else if tool == Tool::Pen {
                CursorIcon::Crosshair
            } else if matches!(tool, Tool::Select | Tool::Transform | Tool::Object) {
                CursorIcon::Default
            } else if matches!(
                tool,
                Tool::Brush
                    | Tool::Airbrush
                    | Tool::SketchBrush
                    | Tool::FillBrush
                    | Tool::ShadeBrush
                    | Tool::LightBrush
                    | Tool::VectorBrush
                    | Tool::Eraser
            ) {
                CursorIcon::None
            } else {
                CursorIcon::Default
            });
        }
        let stage_view = state.view(area);
        let corners = [
            Pos2::ZERO,
            Pos2::new(state.stage_size.x, 0.),
            Pos2::new(state.stage_size.x, state.stage_size.y),
            Pos2::new(0., state.stage_size.y),
        ];
        let screen: Vec<Pos2> = corners.into_iter().map(|p| stage_view.screen(p)).collect();
        painter.add(egui::Shape::convex_polygon(
            screen.clone(),
            Color32::WHITE,
            Stroke::new(1., Color32::from_gray(90)),
        ));
        self.paint_grid(&painter, area, view);
        if self.onion && !self.playing && !self.animation.frame_workflow.tracing() {
            for f in self.onion_frames() {
                if f == self.frame {
                    continue;
                }
                for (layer, _, mut mark) in crate::features::resolve(&self.doc, f, false) {
                    if mark.style.raster_tile {
                        mark.color[3] = (f32::from(mark.color[3]) * 0.3).round() as u8;
                        let layer_view =
                            self.camera_depth_view(self.camera_view(stage_view), layer);
                        self.paint_media_image(&painter, layer_view, &mark);
                    } else if mark.style.media_image.is_none() {
                        paint_mark(&painter, &mark, view, true, false, Some(&mut state.cache));
                    }
                }
            }
        }
        let blur_rendered = self.camera_blur_preview(&painter, stage_view);
        let media_painter = if blur_rendered {
            painter.with_clip_rect(Rect::NOTHING)
        } else {
            painter.clone()
        };
        let mut marks = if blur_rendered {
            vec![]
        } else {
            crate::features::resolve_at(
                &self.doc,
                self.frame,
                false,
                if self.playing {
                    self.animation.clock
                } else {
                    self.frame
                },
            )
        }
        .into_iter()
        .peekable();
        for l in 0..self.doc.layers.len() {
            self.paint_media_video(&media_painter, self.camera_view(stage_view), self.frame, l);
            let layer_view = self.camera_depth_view(self.camera_view(stage_view), l);
            if crate::depth::active(self)
                && crate::depth::camera(&self.doc, self.frame).is_some_and(|c| {
                    !crate::depth::visible(
                        &self.doc.layers[l].options.depth,
                        &crate::animation::camera_optics(c, self.frame),
                    )
                })
            {
                while marks.peek().is_some_and(|(layer, _, _)| *layer == l) {
                    marks.next();
                }
                continue;
            }
            if crate::folder_groups::layer_active(&self.doc, &self.doc.layers[l], self.frame) {
                self.paint_mannequin_layer(&painter, layer_view, l);
            }
            while marks.peek().is_some_and(|(layer, _, _)| *layer == l) {
                let (_, _, mark) = marks.next().unwrap();
                if self.paint_media_image(&painter, layer_view, &mark) {
                    continue;
                }
                paint_mark(
                    &painter,
                    &mark,
                    layer_view,
                    false,
                    false,
                    Some(&mut state.cache),
                );
            }
        }
        self.paint_media_video(
            &media_painter,
            self.camera_view(stage_view),
            self.frame,
            self.doc.layers.len(),
        );
        self.paint_video_selection(&painter, self.camera_view(stage_view));
        self.paint_mannequin_gizmo(&painter, view);
        self.text_canvas_ui(ui, &painter, view);
        if tool == Tool::Text {
            state.selected = self.canvas.selected.clone();
        }
        if let Some(mark) = &self.draft {
            let mut mark = mark.clone();
            mark.width_profile = self.drawing.profile(&mark.width_profile);
            let previews = self.symmetry_marks(vec![mark]);
            if matches!(tool, Tool::FillBrush | Tool::ShadeBrush | Tool::LightBrush) {
                let fills: Vec<_> = self.doc.layers[self.layer].frames[self.frame]
                    .iter()
                    .filter(|m| {
                        m.fill.is_some()
                            && m.style.instance.is_none()
                            && match tool {
                                Tool::ShadeBrush => m.style.shade < self.feature.toon_levels,
                                Tool::LightBrush => m.style.light < self.feature.toon_levels,
                                _ => true,
                            }
                    })
                    .cloned()
                    .map(|mut m| {
                        m.stroke = false;
                        m
                    })
                    .collect();
                let mask = crate::features::ink(&fills);
                for preview in previews {
                    let color = match tool {
                        Tool::ShadeBrush => [0, 0, 0, (self.studio.strength * 180.) as u8],
                        Tool::LightBrush => [255, 255, 255, (self.studio.strength * 180.) as u8],
                        _ => preview.color,
                    };
                    for ink in crate::vector::brush_fill(&preview, color) {
                        for part in crate::features::clip_to(&ink, &mask) {
                            paint_mark(&painter, &part, view, false, false, None);
                        }
                    }
                }
            } else if matches!(tool, Tool::Airbrush | Tool::SketchBrush) {
                for preview in previews {
                    for m in crate::drawing::airbrush(
                        &preview,
                        self.drawing.opacity,
                        self.drawing.hardness,
                        tool == Tool::SketchBrush,
                    ) {
                        paint_mark(&painter, &m, view, false, false, None);
                    }
                }
            } else {
                for preview in previews {
                    paint_mark(&painter, &preview, view, false, false, None);
                }
            }
        }
        self.advanced_overlays(&painter, area, view);
        self.visual_canvas_overlays(&painter, view, &state.selected);
        self.rig_overlays(&painter, view);
        self.paint_guides(&painter, area, view);
        if tool == Tool::Width {
            for &(l, m) in &state.selected {
                paint_width_handles(&painter, &self.doc.layers[l].frames[self.frame][m], view);
            }
        }
        if matches!(tool, Tool::Node | Tool::Select) || tool == Tool::Pen && state.pen.is_none() {
            for &(l, m) in &state.selected {
                let mark = &self.doc.layers[l].frames[self.frame][m];
                if tool == Tool::Select && (!mark.stroke || mark.nodes.is_empty()) {
                    continue;
                }
                paint_nodes(
                    &painter,
                    &self.doc.layers[l].frames[self.frame][m],
                    view,
                    state
                        .node_selection
                        .filter(|(a, b, _)| *a == l && *b == m)
                        .map(|(_, _, n)| n),
                );
            }
        }
        if let Some(pen) = &state.pen {
            let mut preview = pen.mark.clone();
            if !preview.closed {
                preview.fill = None;
            }
            if !preview.stroke {
                preview.stroke = true;
                preview.color = crate::themes::accent().to_array();
            }
            paint_mark(&painter, &preview, view, false, false, None);
            paint_nodes(&painter, &pen.mark, view, None);
            if state.drag.is_none() && over {
                if let (Some(last), Some(pointer)) = (pen.mark.nodes.last(), pointer) {
                    let end = view.world(pointer);
                    let curve = crate::vector::Cubic([
                        crate::vector::pos(last.position),
                        crate::vector::pos(last.out_handle),
                        end,
                        end,
                    ]);
                    let line = crate::vector::from_cubics(
                        &[curve],
                        &Mark {
                            color: crate::themes::accent().to_array(),
                            width: 1. / view.scale,
                            ..Mark::default()
                        },
                    );
                    paint_mark(&painter, &line, view, false, false, None);
                }
            }
        }
        if tool != Tool::Transform {
            let mut evaluated = std::collections::HashMap::new();
            for &(layer, mark_index) in &state.selected {
                let marks = evaluated.entry(layer).or_insert_with(|| {
                    crate::animation::marks(&self.doc.layers[layer], self.frame)
                });
                let Some(mark) = marks.get(mark_index) else {
                    continue;
                };
                let local_view = self.camera_depth_view(self.camera_view(stage_view), layer);
                if mark.style.instance.is_none() && mark.style.media_image.is_none() {
                    if let Some((_, inside)) = state.partial.get(&(layer, mark_index)) {
                        for part in inside {
                            crate::paint::selection_pattern(
                                &painter,
                                part,
                                local_view,
                                &mut state.cache,
                            );
                        }
                    } else {
                        crate::paint::selection_pattern(
                            &painter,
                            mark,
                            local_view,
                            &mut state.cache,
                        );
                    }
                    continue;
                }
                let mut bounds = crate::vector::control_bounds(mark);
                let mut matrix = crate::rig::IDENTITY;
                if let Some(instance) = &mark.style.instance
                    && let Some(symbol_bounds) = self.animation_instance_bounds(mark)
                {
                    bounds = symbol_bounds;
                    matrix = instance.matrix;
                } else if mark.stroke {
                    bounds = bounds.expand(mark.width * 0.5);
                }
                if !bounds.is_finite() {
                    continue;
                }
                let padding = (2. / local_view.scale.max(0.001)).max(0.);
                let bounds = bounds.expand(padding);
                let corners = [
                    bounds.left_top(),
                    bounds.right_top(),
                    bounds.right_bottom(),
                    bounds.left_bottom(),
                ]
                .map(|p| local_view.screen(crate::rig::point(matrix, p)));
                paint_selection_outline(&painter, corners, mark.style.instance.is_some());
            }
        }
        if tool == Tool::Transform
            && let Some(bounds) = state.bounds(self)
        {
            let corners = [
                bounds.left_top(),
                bounds.right_top(),
                bounds.right_bottom(),
                bounds.left_bottom(),
            ]
            .map(|p| view.screen(p));
            let dashed = match state.selected.as_slice() {
                [(l, m)] => self.doc.layers[*l].frames[self.frame]
                    .get(*m)
                    .is_some_and(|mark| mark.style.instance.is_some()),
                _ => false,
            };
            paint_selection_outline(&painter, corners, dashed);
            for p in bounds.handles() {
                painter.rect_filled(
                    Rect::from_center_size(view.screen(p), Vec2::splat(7.)),
                    0.,
                    Color32::from_rgb(32, 36, 43),
                );
                painter.rect_stroke(
                    Rect::from_center_size(view.screen(p), Vec2::splat(7.)),
                    0.,
                    Stroke::new(1., crate::themes::accent()),
                    egui::StrokeKind::Inside,
                );
            }
            let top = view.screen(bounds.center_top());
            let rot = top + rotate(Vec2::new(0., -30.), view.angle + bounds.angle);
            painter.line_segment([top, rot], Stroke::new(1., crate::themes::accent()));
            painter.circle_filled(rot, 5., Color32::from_rgb(32, 36, 43));
            painter.circle_stroke(rot, 5., Stroke::new(1.5, crate::themes::accent()));
            let pivot = view.screen(state.transform_pivot(self, bounds));
            painter.circle_filled(pivot, 5., Color32::from_gray(28));
            painter.circle_stroke(pivot, 5., Stroke::new(1.5, crate::themes::accent()));
            painter.line_segment(
                [pivot - Vec2::new(8., 0.), pivot + Vec2::new(8., 0.)],
                Stroke::new(1., crate::themes::accent()),
            );
            painter.line_segment(
                [pivot - Vec2::new(0., 8.), pivot + Vec2::new(0., 8.)],
                Stroke::new(1., crate::themes::accent()),
            );
        }
        if let Some(Drag::Marquee { start, end, .. }) = &state.drag {
            let r = Rect::from_two_pos(*start, *end);
            let mut pts: Vec<_> = [
                r.left_top(),
                r.right_top(),
                r.right_bottom(),
                r.left_bottom(),
                r.left_top(),
            ]
            .into_iter()
            .map(|p| view.screen(p))
            .collect();
            painter.add(egui::Shape::line(
                std::mem::take(&mut pts),
                Stroke::new(1., crate::themes::accent()),
            ));
        }
        state.actual_scale = view.scale;
        if let Some(Drag::Area { points, .. }) = &state.drag {
            let mut points: Vec<_> = points.iter().map(|p| view.screen(*p)).collect();
            if let Some(first) = points.first().copied() {
                points.push(first);
            }
            painter.add(egui::Shape::line(
                points,
                Stroke::new(1., crate::themes::accent()),
            ));
        }
        if !state.polygon_selection.is_empty() {
            let mut points: Vec<_> = state
                .polygon_selection
                .iter()
                .map(|p| view.screen(*p))
                .collect();
            if let Some(p) = pointer {
                points.push(p);
            }
            if let Some(first) = points.first().copied() {
                points.push(first);
            }
            painter.add(egui::Shape::line(
                points,
                Stroke::new(1., crate::themes::accent()),
            ));
        }
        self.paint_frame_workflow(&painter, view);
        self.light_gizmo(ui, self.camera_view(stage_view), false);
        self.paint_folder_gizmo(&painter, self.camera_view(stage_view));
        self.paint_vector_trim(&painter, self.camera_view(stage_view));
        if raster_edit
            && over
            && !space
            && let Some(pos) = pointer
        {
            crate::raster::cursor(
                &painter,
                pos,
                view,
                self.studio.prefs.raster_brush,
                tool == Tool::Eraser,
            );
        }
        if !raster_edit
            && over
            && !space
            && matches!(
                tool,
                Tool::Brush
                    | Tool::Airbrush
                    | Tool::SketchBrush
                    | Tool::FillBrush
                    | Tool::ShadeBrush
                    | Tool::LightBrush
                    | Tool::VectorBrush
                    | Tool::Eraser
            )
        {
            if let Some(pointer) = pointer {
                let width = if tool == Tool::Eraser {
                    self.eraser_width
                } else {
                    self.width
                };
                let radius = (width * view.scale * 0.5).max(1.);
                painter.circle_stroke(pointer, radius, Stroke::new(3., Color32::BLACK));
                painter.circle_stroke(pointer, radius, Stroke::new(1., Color32::WHITE));
            }
        }
        if video_input
            || matches!(
                state.drag,
                Some(Drag::Transform(_))
                    | Some(Drag::Draw {
                        tool: Tool::Rectangle | Tool::Ellipse | Tool::Polygon | Tool::Line,
                        ..
                    })
            )
        {
            self.paint_snap_feedback(&painter, area, view);
        }
        self.stage_mask(&painter, area, stage_view);
        self.paint_preview_progress(&painter, area);
        if over
            && !space
            && !matches!(state.drag, Some(Drag::Pan(_)))
            && matches!(
                tool,
                Tool::Rectangle | Tool::Ellipse | Tool::Polygon | Tool::Line
            )
        {
            if let Some(pos) = pointer {
                let cursor = ui.painter().with_clip_rect(area);
                let on_stage = Rect::from_min_size(Pos2::ZERO, state.stage_size)
                    .contains(stage_view.world(pos));
                let (color, halo) = if on_stage {
                    (Color32::BLACK, Color32::WHITE)
                } else {
                    (Color32::WHITE, Color32::BLACK)
                };
                for arm in [Vec2::new(7., 0.), Vec2::new(0., 7.)] {
                    cursor.line_segment([pos - arm, pos + arm], Stroke::new(3., halo));
                    cursor.line_segment([pos - arm, pos + arm], Stroke::new(1., color));
                }
                crate::design::draw(
                    &cursor,
                    Rect::from_min_size(pos + Vec2::new(13., 13.), Vec2::splat(15.)),
                    tool.icon(),
                    crate::themes::accent(),
                );
            }
            response.clone().on_hover_text(crate::i18n::tr("Arrastra para crear la forma. Mayús mantiene la proporción o el ángulo; Alt omite los ajustes."));
        }
        self.paint_rulers(&ruler_painter, ruler_area, view);
        self.command_helper_ui(ui, area);
        self.sync_selected_style(&state.selected);
        self.canvas = state;
        self.animation_canvas_drop(ui, area, view);
        self.finish_animation_clip_ui(&ctx);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn mark() -> Mark {
        Mark {
            points: vec![[10., 20.], [50., 60.]].into(),
            color: [0, 0, 0, 255],
            width: 4.,
            rectangle: false,
            ..Mark::default()
        }
    }
    #[test]
    fn view_inverse_with_rotation_pan_zoom() {
        let v = View {
            center: Pos2::new(143., -22.),
            scale: 2.7,
            angle: 1.3,
            stage_center: Pos2::new(480., 270.),
        };
        let p = Pos2::new(-32., 750.);
        assert!(v.world(v.screen(p)).distance(p) < 0.001);
    }
    #[test]
    fn pivot_stays_fixed_under_scale_and_rotation() {
        let mut m = mark();
        m.points[0] = [10., 20.];
        let out = transformed(&m, Pos2::new(10., 20.), Vec2::new(2., 3.), 1.2, Vec2::ZERO);
        assert_eq!(out.points[0], [10., 20.]);
    }
    #[test]
    fn rotated_rectangle_preserves_four_edges() {
        let mut m = mark();
        m.rectangle = true;
        let out = transformed(
            &m,
            Pos2::ZERO,
            Vec2::splat(1.),
            std::f32::consts::FRAC_PI_2,
            Vec2::ZERO,
        );
        assert!(!out.rectangle);
        assert_eq!(out.points.len(), 5);
        assert!((out.points[1][0] + 20.).abs() < 0.001);
        assert!((out.points[1][1] - 50.).abs() < 0.001);
    }
    #[test]
    fn symbol_outline_dashes_are_visible_and_bounded_at_high_zoom() {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let clip = Rect::from_min_max(Pos2::ZERO, Pos2::new(96., 64.));
            paint_selection_outline(
                &ui.painter().with_clip_rect(clip),
                [
                    Pos2::new(-1_000_000., 10.),
                    Pos2::new(1_000_000., 10.),
                    Pos2::new(1_000_000., 30.),
                    Pos2::new(-1_000_000., 30.),
                ],
                true,
            );
        });
        let count = output.shapes.iter().filter(|shape| {
            matches!(&shape.shape, egui::Shape::LineSegment { stroke, .. } if stroke.color == SELECTION_BLUE)
        }).count();
        assert!((15..=32).contains(&count), "unexpected dash count: {count}");
        output.textures_delta.clear();
    }

    #[test]
    fn stroke_hit_tests_geometry_not_bounds() {
        assert!(hit(&mark(), Pos2::new(30., 40.), 2.));
        assert!(!hit(&mark(), Pos2::new(12., 58.), 2.));
    }
}

#[cfg(test)]
mod gesture_tests {
    use super::*;
    fn event(pos: Pos2, button: PointerButton, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }
    fn tick(ctx: &egui::Context, e: &mut Editor, events: Vec<egui::Event>) {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000., 700.))),
                events,
                ..Default::default()
            },
            |ui| {
                e.canvas_ui(ui);
            },
        );
        output.textures_delta.clear();
    }
    #[test]
    fn middle_and_space_drag_pan_without_drawing() {
        for middle in [true, false] {
            let ctx = egui::Context::default();
            let cc = eframe::CreationContext::_new_kittest(ctx.clone());
            let mut e = Editor::new(&cc);
            e.doc = crate::Document::default();
            let p = Pos2::new(500., 350.);
            let end = p + Vec2::new(70., 45.);
            let button = if middle {
                PointerButton::Middle
            } else {
                PointerButton::Primary
            };
            tick(&ctx, &mut e, vec![]);
            let mut events = vec![egui::Event::PointerMoved(p)];
            if !middle {
                events.push(egui::Event::Key {
                    key: egui::Key::Space,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            events.push(event(p, button, true));
            tick(&ctx, &mut e, events);
            tick(&ctx, &mut e, vec![egui::Event::PointerMoved(end)]);
            tick(&ctx, &mut e, vec![event(end, button, false)]);
            assert!(
                (e.canvas.pan - Vec2::new(70., 45.)).length() < 0.01,
                "middle={middle}, pan={:?}",
                e.canvas.pan
            );
            assert!(e.doc.layers[0].frames[0].is_empty());
            assert!(e.draft.is_none());
        }
    }
    #[test]
    fn transformed_offstage_art_survives_save_and_export() {
        let mut doc = crate::Document::default();
        let mark = Mark {
            points: vec![[0., 0.], [40., 30.]].into(),
            color: [20, 30, 40, 255],
            width: 3.,
            rectangle: true,
            ..Mark::default()
        };
        doc.layers[0].frames[0].push(transformed(
            &mark,
            Pos2::ZERO,
            Vec2::new(2., 1.),
            0.5,
            Vec2::new(-200., 800.),
        ));
        let restored: crate::Document =
            serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert!(restored.validate().is_ok());
        assert!(restored.svg(0).contains("<path"));
        assert_eq!(restored.layers[0].frames[0][0].points.len(), 5);
    }
}

fn fit_scale(area: Rect, size: Vec2) -> f32 {
    ((area.width() - 60.) / size.x)
        .min((area.height() - 60.) / size.y)
        .max(0.01)
}

#[cfg(test)]
mod feature_gesture_tests {
    use super::*;
    #[test]
    fn fill_brush_gesture_never_adds_ink_outside_the_fill_and_locked_layers_ignore_it() {
        for locked in [false, true] {
            let ctx = egui::Context::default();
            let cc = eframe::CreationContext::_new_kittest(ctx.clone());
            let mut e = Editor::new(&cc);
            e.doc = crate::Document::default();
            e.recovery = std::env::temp_dir().join(format!(
                "libretoon-fill-gesture-{}-{locked}.json",
                std::process::id()
            ));
            e.doc.layers[0].frames[0].push(Mark {
                points: vec![[0., 0.], [100., 100.]].into(),
                rectangle: true,
                closed: true,
                fill: Some([30, 80, 120, 255]),
                stroke: false,
                ..Default::default()
            });
            e.doc.layers[0].options.locked = locked;
            e.activate_tool(Tool::FillBrush);
            e.fill_color = Color32::from_rgb(230, 30, 60);
            e.width = 20.;
            e.smoothing = 0.;
            let tick = |e: &mut Editor, events: Vec<egui::Event>| {
                let mut out = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000., 700.))),
                        events,
                        ..Default::default()
                    },
                    |ui| e.canvas_ui(ui),
                );
                out.textures_delta.clear();
            };
            tick(&mut e, vec![]);
            let area = Rect::from_min_max(Pos2::new(8., 8.), Pos2::new(992., 692.));
            let view = e.canvas.view(area);
            let a = view.screen(Pos2::new(50., 50.));
            let b = view.screen(Pos2::new(180., 50.));
            let button = |pos, pressed| egui::Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            };
            let before = serde_json::to_value(&e.doc).unwrap();
            tick(&mut e, vec![egui::Event::PointerMoved(a), button(a, true)]);
            tick(&mut e, vec![egui::Event::PointerMoved(b)]);
            tick(&mut e, vec![button(b, false)]);
            assert!(
                e.doc.layers[0].frames[0]
                    .iter()
                    .all(|m| crate::vector::control_bounds(m).right() <= 100.001)
            );
            if locked {
                assert_eq!(before, serde_json::to_value(&e.doc).unwrap());
            } else {
                assert!(
                    e.doc.layers[0].frames[0]
                        .iter()
                        .any(|m| m.fill == Some([230, 30, 60, 255]))
                );
            }
        }
    }
}
#[derive(Clone, Copy)]
struct SelectionBox {
    rect: Rect,
    angle: f32,
}
impl SelectionBox {
    fn world(self, p: Pos2) -> Pos2 {
        Pos2::ZERO + rotate(p.to_vec2(), self.angle)
    }
    fn center(self) -> Pos2 {
        self.world(self.rect.center())
    }
    fn center_top(self) -> Pos2 {
        self.world(self.rect.center_top())
    }
    fn left_top(self) -> Pos2 {
        self.world(self.rect.left_top())
    }
    fn right_top(self) -> Pos2 {
        self.world(self.rect.right_top())
    }
    fn left_bottom(self) -> Pos2 {
        self.world(self.rect.left_bottom())
    }
    fn right_bottom(self) -> Pos2 {
        self.world(self.rect.right_bottom())
    }
    fn handles(self) -> [Pos2; 8] {
        handles(self.rect).map(|p| self.world(p))
    }
    fn contains(self, p: Pos2) -> bool {
        self.rect
            .contains(Pos2::ZERO + rotate(p.to_vec2(), -self.angle))
    }
}
pub(crate) fn transformed_in_basis(
    m: &Mark,
    pivot: Pos2,
    scale: Vec2,
    angle: f32,
    translation: Vec2,
    basis: f32,
) -> Mark {
    crate::vector::transform(m, |p| {
        let v = rotate(p - pivot, -basis);
        let p =
            pivot + rotate(Vec2::new(v.x * scale.x, v.y * scale.y), basis + angle) + translation;
        Pos2::new(p.x.clamp(-1e7, 1e7), p.y.clamp(-1e7, 1e7))
    })
}

#[cfg(test)]
mod rotation_regressions {
    use super::*;
    #[test]
    fn view_rotation_preserves_scale_and_distances() {
        let area = Rect::from_min_size(Pos2::ZERO, Vec2::new(1000., 700.));
        let mut c = Canvas::default();
        c.zoom = 2.;
        c.pan = Vec2::new(12., -40.);
        let initial = c.view(area);
        let a = Pos2::new(20., 80.);
        let b = Pos2::new(700., 400.);
        for degrees in (0..360).step_by(7) {
            c.angle = (degrees as f32).to_radians();
            let v = c.view(area);
            assert_eq!(v.scale, initial.scale);
            assert!(
                (v.screen(a).distance(v.screen(b)) - initial.screen(a).distance(initial.screen(b)))
                    .abs()
                    < 0.001
            );
        }
    }
    #[test]
    fn fit_resets_position_zoom_and_rotation() {
        let mut c = Canvas::default();
        c.zoom = 9.;
        c.pan = Vec2::new(-230., 960.);
        c.angle = 2.3;
        c.fit();
        assert_eq!(c.zoom, 1.);
        assert_eq!(c.pan, Vec2::ZERO);
        assert_eq!(c.angle, 0.);
        let area = Rect::from_min_size(Pos2::new(40., 30.), Vec2::new(1000., 700.));
        assert_eq!(c.view(area).screen(Pos2::new(480., 270.)), area.center());
    }
    #[test]
    fn rotation_and_oriented_frame_are_rigid() {
        let mark = Mark {
            points: vec![
                [20., 30.],
                [180., 30.],
                [180., 100.],
                [20., 100.],
                [20., 30.],
            ]
            .into(),
            color: [0, 0, 0, 255],
            width: 6.,
            rectangle: false,
            ..Mark::default()
        };
        let pivot = Pos2::new(-80., 190.);
        for degrees in (0..360).step_by(11) {
            let angle = (degrees as f32).to_radians();
            let result =
                transformed_in_basis(&mark, pivot, Vec2::splat(1.), angle, Vec2::ZERO, 0.4);
            for (i, a) in result.points.iter().enumerate() {
                let a = Pos2::new(a[0], a[1]);
                let origin = Pos2::new(mark.points[i][0], mark.points[i][1]);
                assert!((a.distance(pivot) - origin.distance(pivot)).abs() < 0.001);
            }
            assert_eq!(result.width, mark.width);
            let mut rect = Rect::NOTHING;
            for p in &result.points {
                rect.extend_with(Pos2::ZERO + rotate(Vec2::new(p[0], p[1]), -angle));
            }
            assert!((rect.width() - 160.).abs() < 0.001);
            assert!((rect.height() - 70.).abs() < 0.001);
        }
    }
}

#[derive(Clone)]
struct PenDraft {
    mark: Mark,
    layer: usize,
    frame: usize,
    closing: bool,
}
struct NodeDrag {
    id: (usize, usize),
    index: usize,
    handle: u8,
    start: Pos2,
    original: Mark,
    changed: bool,
    moved: bool,
    independent: bool,
}

impl Canvas {
    pub fn cancel_drafts(&mut self) {
        self.pen_redo.clear();
        self.pen = None;
        self.drag = None;
        self.node_selection = None;
    }
    pub(crate) fn can_undo_pen(&self) -> bool {
        self.pen.as_ref().is_some_and(|p| !p.mark.nodes.is_empty())
    }
    pub(crate) fn can_redo_pen(&self) -> bool {
        !self.pen_redo.is_empty()
    }
    pub fn undo_pen(&mut self) -> bool {
        if let Some(mut pen) = self.pen.take() {
            self.pen_redo.push(pen.clone());
            if self.pen_redo.len() > 60 {
                self.pen_redo.remove(0);
            }
            pen.mark.nodes.pop();
            if !pen.mark.nodes.is_empty() {
                self.pen = Some(pen);
            }
            self.drag = None;
            true
        } else {
            false
        }
    }
    pub(crate) fn redo_pen(&mut self) -> bool {
        if let Some(pen) = self.pen_redo.pop() {
            self.pen = Some(pen);
            self.drag = None;
            true
        } else {
            false
        }
    }
}
impl Editor {
    pub fn finish_pending_pen(&mut self) {
        self.finish_raster_stroke();
        self.canvas.pen_redo.clear();
        if let Some(pen) = self.canvas.pen.take() {
            self.commit_pen(pen);
        }
    }
    fn commit_pen(&mut self, mut pen: PenDraft) {
        if pen.mark.nodes.len() < 2
            || pen.layer >= self.doc.layers.len()
            || pen.frame >= self.doc.layers[pen.layer].frames.len()
        {
            return;
        }
        if !pen.mark.closed {
            if !pen.mark.stroke && pen.mark.fill.is_some() && pen.mark.nodes.len() >= 2 {
                pen.mark.closed = true;
            } else {
                pen.mark.fill = None;
            }
        }
        if !pen.mark.stroke && pen.mark.fill.is_none() {
            return;
        }
        self.checkpoint();
        let marks = self.symmetry_marks(vec![pen.mark]);
        let marks = self.prepare_style(marks);
        self.commit_paint(pen.layer, pen.frame, marks);
        self.recover();
        self.status = "Trazado Bézier creado".into();
    }
    fn begin_pen(&mut self, state: &mut Canvas, world: Pos2, view: View, alt: bool) {
        state.pen_redo.clear();
        if state.pen.is_none() {
            if !self.stroke_enabled && !self.fill_enabled {
                self.status = "Activa relleno o contorno para dibujar".into();
                return;
            }
            state.clear_selection();
            state.pen = Some(PenDraft {
                mark: Mark {
                    color: self.color.to_array(),
                    width: self.width,
                    width_profile: self.width_profile.clone(),
                    stroke: self.stroke_enabled,
                    fill: self.fill_enabled.then(|| self.fill_color.to_array()),
                    ..Mark::default()
                },
                layer: self.layer,
                frame: self.frame,
                closing: false,
            });
        }
        let pen = state.pen.as_mut().unwrap();
        if alt
            && let Some(index) = pen.mark.nodes.iter().position(|node| {
                view.screen(crate::vector::pos(node.position))
                    .distance(view.screen(world))
                    < 9.
            })
        {
            state.drag = Some(Drag::PenNode {
                index,
                start: world,
                original: pen.mark.nodes[index].clone(),
                convert: true,
            });
            return;
        }
        if pen.mark.nodes.len() >= 2
            && view
                .screen(crate::vector::pos(pen.mark.nodes[0].position))
                .distance(view.screen(world))
                < 9.
        {
            pen.mark.closed = true;
            pen.closing = true;
            state.drag = Some(Drag::PenNode {
                index: 0,
                start: world,
                original: pen.mark.nodes[0].clone(),
                convert: false,
            });
        } else {
            let index = pen.mark.nodes.len();
            pen.mark
                .nodes
                .push(crate::document::Node::corner(crate::vector::xy(world)));
            state.drag = Some(Drag::PenNode {
                index,
                start: world,
                original: pen.mark.nodes[index].clone(),
                convert: false,
            });
        }
        self.status="Pluma: clic crea esquina · arrastrar crea curva · primer nodo cierra · Enter termina · Esc cancela".into();
    }
    fn grab_selected_node(&self, state: &mut Canvas, world: Pos2, view: View, alt: bool) -> bool {
        let mut found = None;
        let mut best = 8.;
        for &(l, m) in &state.selected {
            let mark = editable_nodes(&self.doc.layers[l].frames[self.frame][m]);
            for (index, node) in mark.nodes.iter().enumerate() {
                for (handle, p) in [
                    (1, node.in_handle),
                    (2, node.out_handle),
                    (0, node.position),
                ] {
                    if handle != 0 && p == node.position {
                        continue;
                    }
                    let distance = view
                        .screen(self.depth_point(crate::vector::pos(p), l, self.layer))
                        .distance(view.screen(world));
                    if distance < best || (distance == best && handle == 0) {
                        best = distance;
                        found = Some(((l, m), index, handle));
                    }
                }
            }
        }
        if let Some((id, index, handle)) = found {
            state.node_selection = Some((id.0, id.1, index));
            state.drag = Some(Drag::Node(NodeDrag {
                id,
                index,
                handle: if alt && handle == 0 { 3 } else { handle },
                start: self.depth_point(world, self.layer, id.0),
                original: self.doc.layers[id.0].frames[self.frame][id.1].clone(),
                changed: false,
                moved: false,
                independent: alt,
            }));
            return true;
        }
        false
    }
    fn start_node_drag(&mut self, state: &mut Canvas, world: Pos2, view: View, alt: bool) {
        if self.grab_selected_node(state, world, view, alt) {
            return;
        }
        'layers: for (l, layer) in self
            .doc
            .layers
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, l)| l.visible && !l.options.locked)
        {
            for (m, mark) in crate::animation::marks(layer, self.frame)
                .iter()
                .enumerate()
                .rev()
            {
                if self.depth_hit(l, mark, world, 6. / view.scale) {
                    state.clear_selection();
                    state.selected.push((l, m));
                    self.layer = l;
                    let editable = editable_nodes(mark);
                    if let Some(index) = editable.nodes.iter().position(|node| {
                        view.screen(self.depth_point(
                            crate::vector::pos(node.position),
                            l,
                            self.layer,
                        ))
                        .distance(view.screen(world))
                            < 8.
                    }) {
                        state.node_selection = Some((l, m, index));
                        state.drag = Some(Drag::Node(NodeDrag {
                            id: (l, m),
                            index,
                            handle: if alt { 3 } else { 0 },
                            start: self.depth_point(world, self.layer, l),
                            original: mark.clone(),
                            changed: false,
                            moved: false,
                            independent: alt,
                        }));
                    }
                    break 'layers;
                }
            }
        }
    }
    fn cut_selection(&mut self, state: &mut Canvas, rect: Rect, add: bool) {
        if rect.width() < 0.001 || rect.height() < 0.001 {
            return;
        }
        self.select_fragments(state, add, |mark| crate::vector::split_rect(mark, rect));
        state.orientation = 0.;
        state.pivot = state.bounds(self).map(|b| b.center());
    }
    pub(crate) fn materialize_fragments(&mut self, state: &mut Canvas) {
        if state.partial.is_empty() {
            return;
        }
        self.materialize_motion(&state.selected);
        let mut partial = std::mem::take(&mut state.partial);
        let mut selected = std::mem::take(&mut state.selected);
        selected.sort_unstable();
        let mut shift = 0;
        let mut layer = None;
        for (l, m) in selected {
            if layer != Some(l) {
                shift = 0;
                layer = Some(l);
            }
            let index = m + shift;
            if let Some((outside, inside)) = partial.remove(&(l, m)) {
                let start = index + outside.len();
                let end = start + inside.len();
                self.doc.layers[l].frames[self.frame]
                    .splice(index..index + 1, outside.into_iter().chain(inside));
                state.selected.extend((start..end).map(|m| (l, m)));
                shift += end - index - 1;
            } else {
                state.selected.push((l, index));
            }
        }
    }
    pub(crate) fn separate_component(
        &mut self,
        state: &mut Canvas,
        id: (usize, usize),
        stroke: bool,
    ) -> (usize, usize) {
        let mark = &self.doc.layers[id.0].frames[self.frame][id.1];
        if mark.fill.is_none() || !mark.stroke {
            return id;
        }
        let mut fill = mark.clone();
        fill.stroke = false;
        let mut line = mark.clone();
        line.fill = None;
        self.checkpoint();
        self.doc.layers[id.0].frames[self.frame][id.1] = fill;
        self.doc.layers[id.0].frames[self.frame].insert(id.1 + 1, line);
        for (l, m) in &mut state.selected {
            if *l == id.0 && *m >= id.1 {
                *m += 1;
            }
        }
        self.recover();
        (id.0, id.1 + usize::from(stroke))
    }
    pub fn apply_drawing_style(&mut self) {
        if self.canvas.selected.is_empty() || (!self.fill_enabled && !self.stroke_enabled) {
            return;
        }
        if !self.stroke_enabled
            && self.canvas.selected.iter().any(|&(l, m)| {
                let mark = &self.doc.layers[l].frames[self.frame][m];
                !mark.closed && !mark.rectangle
            })
        {
            self.status = "Cierra primero el trazado para aplicarle solo relleno".into();
            return;
        }
        let ids = self.canvas.selected.clone();
        self.checkpoint();
        for &(l, m) in &ids {
            let mark = &mut self.doc.layers[l].frames[self.frame][m];
            mark.color = self.color.to_array();
            mark.style.palette_stroke = None;
            mark.style.palette_fill = None;
            mark.width = self.width;
            mark.width_profile = self.width_profile.clone();
            mark.stroke = self.stroke_enabled;
            mark.fill = if self.fill_enabled && (mark.closed || mark.rectangle) {
                Some(self.fill_color.to_array())
            } else {
                None
            };
        }
        self.canvas.selected = ids;
        self.recover();
    }
}
// Stop at the first release/cancel: tablet hover may jump immediately afterwards.
fn node_contact_position(events: &[egui::Event], down: bool) -> Option<Pos2> {
    let mut last = None;
    for event in events {
        match event {
            egui::Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed: false,
                ..
            } => {
                return pos.is_finite().then_some(*pos);
            }
            egui::Event::PointerGone => return last,
            egui::Event::Touch {
                phase: egui::TouchPhase::End | egui::TouchPhase::Cancel,
                ..
            } => return last,
            egui::Event::PointerMoved(p) if p.is_finite() => last = Some(*p),
            _ => {}
        }
    }
    if down { last } else { None }
}
fn move_node(original: &Mark, index: usize, handle: u8, delta: Vec2, independent: bool) -> Mark {
    let mut mark = editable_nodes(original);
    let Some(node) = mark.nodes.get_mut(index) else {
        return mark;
    };
    let anchor = crate::vector::pos(node.position);
    if handle == 0 {
        node.position = crate::vector::xy(anchor + delta);
        node.in_handle = crate::vector::xy(crate::vector::pos(node.in_handle) + delta);
        node.out_handle = crate::vector::xy(crate::vector::pos(node.out_handle) + delta);
    } else if handle == 3 && node.in_handle == node.position && node.out_handle == node.position {
        node.in_handle = crate::vector::xy(anchor - delta);
        node.out_handle = crate::vector::xy(anchor + delta);
    } else {
        let (moved, opposite) = if handle == 1 {
            (&mut node.in_handle, &mut node.out_handle)
        } else {
            (&mut node.out_handle, &mut node.in_handle)
        };
        let a = crate::vector::pos(*moved) - anchor;
        let b = crate::vector::pos(*opposite) - anchor;
        let value = crate::vector::pos(*moved) + delta;
        *moved = crate::vector::xy(value);
        // Smooth nodes rotate the opposite tangent without stretching its segment.
        // Corners and collapsed handles remain independent (Alt always breaks linkage).
        if !independent
            && a.length_sq() > 1e-8
            && b.length_sq() > 1e-8
            && a.normalized().dot(b.normalized()) < -0.999
        {
            let direction = value - anchor;
            if direction.length_sq() > 1e-8 {
                *opposite = crate::vector::xy(anchor - direction.normalized() * b.length());
            }
        }
    }
    mark
}
fn editable_nodes(mark: &Mark) -> Mark {
    if !mark.nodes.is_empty() {
        return mark.clone();
    }
    let mut editable = crate::vector::from_cubics(&crate::vector::cubics(mark), mark);
    editable.holes = mark.holes.clone();
    editable
}
fn paint_nodes(p: &egui::Painter, mark: &Mark, view: View, active: Option<usize>) {
    for (index, node) in editable_nodes(mark).nodes.iter().enumerate() {
        let anchor = view.screen(crate::vector::pos(node.position));
        for h in [node.in_handle, node.out_handle] {
            let handle = view.screen(crate::vector::pos(h));
            if handle.distance(anchor) > 0.5 {
                p.line_segment([anchor, handle], Stroke::new(1., crate::themes::accent()));
                p.circle_filled(handle, 3.4, Color32::from_rgb(34, 36, 44));
                p.circle_stroke(handle, 3.4, Stroke::new(1., crate::themes::accent()));
            }
        }
        let rect = Rect::from_center_size(
            anchor,
            Vec2::splat(if active == Some(index) { 8. } else { 6. }),
        );
        p.rect_filled(
            rect,
            0.,
            if active == Some(index) {
                crate::themes::accent()
            } else {
                Color32::from_rgb(35, 38, 44)
            },
        );
        p.rect_stroke(
            rect,
            0.,
            Stroke::new(1., crate::themes::accent()),
            egui::StrokeKind::Inside,
        );
    }
}

#[cfg(test)]
mod vector_gesture_tests {
    use super::*;
    fn button(p: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: p,
            button: PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }
    fn key(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }
    fn tick(ctx: &egui::Context, e: &mut Editor, events: Vec<egui::Event>) {
        tick_with_modifiers(ctx, e, events, egui::Modifiers::NONE);
    }
    fn tick_with_modifiers(
        ctx: &egui::Context,
        e: &mut Editor,
        mut events: Vec<egui::Event>,
        modifiers: egui::Modifiers,
    ) {
        events.insert(0, egui::Event::ModifiersChanged(modifiers));
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000., 700.))),
                events,
                ..Default::default()
            },
            |ui| e.canvas_ui(ui),
        );
        out.textures_delta.clear();
    }
    fn run(test: impl FnOnce(&egui::Context, &mut Editor)) {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut e = Editor::new(&cc);
        e.doc = crate::Document::default();
        let recovery = std::env::temp_dir().join(format!(
            "libretoon-vector-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ));
        e.recovery = recovery.clone();
        tick(&ctx, &mut e, vec![]);
        test(&ctx, &mut e);
        drop(e);
        let _ = std::fs::remove_file(recovery);
    }
    #[test]
    fn brush_can_start_and_finish_outside_the_white_stage() {
        run(|ctx, e| {
            e.tool = Tool::Brush;
            let a = Pos2::new(300., 20.);
            let b = Pos2::new(600., 30.);
            tick(ctx, e, vec![egui::Event::PointerMoved(a), button(a, true)]);
            tick(ctx, e, vec![egui::Event::PointerMoved(b), button(b, false)]);
            assert!(!e.doc.layers[0].frames[0].is_empty());
            assert!(
                e.doc.layers[0].frames[0]
                    .iter()
                    .all(|m| crate::vector::control_bounds(m).max.y < 0.)
            );
            assert!(e.doc.validate().is_ok());
        });
    }
    #[test]
    fn vector_brush_records_two_document_pixels_at_different_zooms() {
        use geo::BoundingRect;
        run(|ctx, e| {
            e.tool = Tool::VectorBrush;
            e.width = 2.;
            let area = Rect::from_min_size(Pos2::ZERO, Vec2::new(1000., 700.));
            let draw = |editor: &mut Editor, zoom, pressure| {
                editor.canvas.zoom = zoom;
                editor.drawing.pen.test_pressure(Some(pressure));
                let view = editor.canvas.view(area);
                let a = view.screen(Pos2::new(420., 270.));
                let b = view.screen(Pos2::new(540., 270.));
                tick(
                    ctx,
                    editor,
                    vec![egui::Event::PointerMoved(a), button(a, true)],
                );
                tick(ctx, editor, vec![egui::Event::PointerMoved(b)]);
                tick(
                    ctx,
                    editor,
                    vec![
                        button(b, false),
                        egui::Event::PointerMoved(b + Vec2::Y * 20.),
                    ],
                );
            };
            draw(e, 0.5, 0.2);
            draw(e, 2., 0.7);
            assert_eq!(e.doc.layers[0].frames[0].len(), 2);
            for mark in &e.doc.layers[0].frames[0] {
                assert_eq!(mark.width, 2.);
                assert!(mark.width_profile.is_empty());
                let outline = crate::width::outline(mark).bounding_rect().unwrap();
                assert!((outline.height() - 2.).abs() < 0.001, "{outline:?}");
            }
            e.drawing.vector_pressure = true;
            draw(e, 2., 0.5);
            let mark = &e.doc.layers[0].frames[0][2];
            let outline = crate::width::outline(mark).bounding_rect().unwrap();
            assert!((outline.height() - 1.).abs() < 0.001, "{outline:?}");
        });
    }

    #[test]
    fn all_pointer_samples_in_one_frame_survive_vector_brush_completion() {
        run(|ctx, e| {
            e.tool = Tool::VectorBrush;
            e.color = Color32::BLUE;
            e.fill_color = Color32::RED;
            let a = Pos2::new(200., 200.);
            let middle = Pos2::new(350., 380.);
            let b = Pos2::new(500., 200.);
            tick(
                ctx,
                e,
                vec![
                    egui::Event::PointerMoved(a),
                    button(a, true),
                    egui::Event::PointerMoved(Pos2::new(270., 300.)),
                    egui::Event::PointerMoved(middle),
                    egui::Event::PointerMoved(Pos2::new(430., 300.)),
                    egui::Event::PointerMoved(b),
                    button(b, false),
                ],
            );
            assert_eq!(e.doc.layers[0].frames[0].len(), 1);
            let mark = &e.doc.layers[0].frames[0][0];
            assert!(!mark.nodes.is_empty());
            assert!(mark.stroke && mark.fill.is_none());
            assert_eq!(mark.color, Color32::RED.to_array());
            let bounds = crate::vector::control_bounds(mark);
            assert!(bounds.height() > 130., "{bounds:?}");
        });
    }
    #[test]
    fn pen_alt_click_converts_existing_curve_without_adding_an_anchor() {
        run(|ctx, e| {
            e.tool = Tool::Pen;
            let anchor = Pos2::new(200., 200.);
            let handle = Pos2::new(235., 170.);
            tick(
                ctx,
                e,
                vec![egui::Event::PointerMoved(anchor), button(anchor, true)],
            );
            tick(ctx, e, vec![egui::Event::PointerMoved(handle)]);
            tick(ctx, e, vec![button(handle, false)]);
            let alt = egui::Modifiers::ALT;
            tick_with_modifiers(
                ctx,
                e,
                vec![egui::Event::PointerMoved(anchor), button(anchor, true)],
                alt,
            );
            tick_with_modifiers(ctx, e, vec![button(anchor, false)], alt);
            let pen = e.canvas.pen.as_ref().unwrap();
            assert_eq!(pen.mark.nodes.len(), 1);
            let node = &pen.mark.nodes[0];
            assert_eq!(node.position, node.in_handle);
            assert_eq!(node.position, node.out_handle);
            let handle = Pos2::new(230., 215.);
            tick_with_modifiers(ctx, e, vec![button(anchor, true)], alt);
            tick_with_modifiers(ctx, e, vec![egui::Event::PointerMoved(handle)], alt);
            tick_with_modifiers(ctx, e, vec![button(handle, false)], alt);
            let node = &e.canvas.pen.as_ref().unwrap().mark.nodes[0];
            assert_ne!(node.position, node.in_handle);
            assert_ne!(node.position, node.out_handle);
        });
    }

    #[test]
    fn pen_click_drag_creates_real_handles_and_enter_finishes_open_path() {
        run(|ctx, e| {
            e.tool = Tool::Pen;
            for (a, b) in [
                (Pos2::new(200., 200.), Pos2::new(240., 160.)),
                (Pos2::new(400., 250.), Pos2::new(440., 290.)),
            ] {
                tick(ctx, e, vec![egui::Event::PointerMoved(a), button(a, true)]);
                tick(ctx, e, vec![egui::Event::PointerMoved(b)]);
                tick(ctx, e, vec![button(b, false)]);
            }
            assert!(e.canvas.pen.is_some());
            tick(ctx, e, vec![key(egui::Key::Enter)]);
            let mark = &e.doc.layers[0].frames[0][0];
            assert_eq!(mark.nodes.len(), 2);
            assert!(!mark.closed);
            assert_ne!(mark.nodes[0].position, mark.nodes[0].out_handle);
            assert!(mark.fill.is_none());
        });
    }
    #[test]
    fn clicking_first_pen_anchor_closes_a_filled_shape() {
        run(|ctx, e| {
            e.tool = Tool::Pen;
            for a in [
                Pos2::new(200., 200.),
                Pos2::new(400., 200.),
                Pos2::new(350., 400.),
                Pos2::new(200., 200.),
            ] {
                tick(ctx, e, vec![egui::Event::PointerMoved(a), button(a, true)]);
                tick(ctx, e, vec![button(a, false)]);
            }
            assert!(e.canvas.pen.is_none());
            let mark = &e.doc.layers[0].frames[0][0];
            assert_eq!(mark.nodes.len(), 3);
            assert!(mark.closed && mark.fill.is_some());
        });
    }
    #[test]
    fn rectangle_modes_keep_fill_and_stroke_independent() {
        for (fill, stroke) in [(true, false), (false, true), (true, true)] {
            run(|ctx, e| {
                e.tool = Tool::Rectangle;
                e.fill_enabled = fill;
                e.stroke_enabled = stroke;
                let a = Pos2::new(150., 150.);
                let b = Pos2::new(400., 400.);
                tick(ctx, e, vec![egui::Event::PointerMoved(a), button(a, true)]);
                tick(ctx, e, vec![egui::Event::PointerMoved(b), button(b, false)]);
                let mark = &e.doc.layers[0].frames[0][0];
                assert_eq!(mark.fill.is_some(), fill);
                assert_eq!(mark.stroke, stroke);
            });
        }
    }
    #[test]
    fn selection_marquee_separates_fill_instead_of_requiring_whole_object() {
        run(|ctx, e| {
            e.tool = Tool::Select;
            e.doc.layers[0].frames[0].push(Mark {
                points: vec![[200., 100.], [500., 400.]].into(),
                rectangle: true,
                closed: true,
                fill: Some([80, 100, 200, 255]),
                stroke: false,
                ..Default::default()
            });
            let view = e
                .canvas
                .view(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000., 700.)));
            let a = view.screen(Pos2::new(150., 50.));
            let b = view.screen(Pos2::new(350., 250.));
            tick(ctx, e, vec![egui::Event::PointerMoved(a), button(a, true)]);
            tick(ctx, e, vec![egui::Event::PointerMoved(b), button(b, false)]);
            assert_eq!(e.doc.layers[0].frames[0].len(), 1);
            assert!(!e.canvas.selected.is_empty());
            let (l, m) = e.canvas.selected[0];
            let selected = &e.canvas.partial[&(l, m)].1[0];
            assert!(crate::vector::control_bounds(selected).max.x < 400.);
            let inside = view.screen(Pos2::new(275., 175.));
            let moved = view.screen(Pos2::new(325., 175.));
            tick(
                ctx,
                e,
                vec![egui::Event::PointerMoved(inside), button(inside, true)],
            );
            assert_eq!(e.doc.layers[0].frames[0].len(), 1);
            tick(
                ctx,
                e,
                vec![egui::Event::PointerMoved(moved), button(moved, false)],
            );
            assert!(e.doc.layers[0].frames[0].len() >= 2);
            assert!(e.canvas.partial.is_empty());
        });
    }
}

struct WidthDrag {
    id: (usize, usize),
    t: f32,
    anchor: Pos2,
    normal: Vec2,
    start: Pos2,
    original: Mark,
    changed: bool,
}
impl Editor {
    fn start_width_drag(&mut self, state: &mut Canvas, world: Pos2, view: View) {
        let mut hit = None;
        for (l, layer) in self
            .doc
            .layers
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, l)| l.visible && !l.options.locked)
        {
            for (m, mark) in layer.frames[self.frame]
                .iter()
                .enumerate()
                .rev()
                .filter(|(_, m)| m.stroke)
            {
                if state.selected.contains(&(l, m)) {
                    for stop in &mark.width_profile {
                        if let Some((anchor, normal)) = crate::width::at_length(mark, stop.t) {
                            let radius = mark.width * stop.width * 0.5;
                            if [-1., 1.].iter().any(|side| {
                                view.screen(anchor + normal * radius * *side)
                                    .distance(view.screen(world))
                                    < 9.
                            }) {
                                hit = Some(((l, m), stop.t, anchor, normal));
                                break;
                            }
                        }
                    }
                }
                if hit.is_none() {
                    if let Some(p) = crate::width::nearest(mark, world) {
                        if p.distance
                            <= mark.width * crate::width::sample(&mark.width_profile, p.t) * 0.5
                                + 9. / view.scale
                        {
                            hit = Some(((l, m), p.t, p.point, p.normal));
                        }
                    }
                }
                if hit.is_some() {
                    break;
                }
            }
            if hit.is_some() {
                break;
            }
        }
        if let Some((id, t, anchor, normal)) = hit {
            let original = self.doc.layers[id.0].frames[self.frame][id.1].clone();
            state.clear_selection();
            state.selected.push(id);
            self.layer = id.0;
            self.width = original.width;
            self.width_profile = original.width_profile.clone();
            self.width_preset = crate::width::Preset::Custom;
            state.drag = Some(Drag::Width(WidthDrag {
                id,
                t,
                anchor,
                normal,
                start: world,
                original,
                changed: false,
            }));
        }
    }
    fn bucket_at(&mut self, state: &mut Canvas, world: Pos2, recalculate: bool) {
        if !self.fill_enabled {
            self.status = "Activa un color de relleno".into();
            return;
        }
        let layer = self.layer;
        let frame = self.frame;
        let marks = &self.doc.layers[layer].frames[frame];
        match bucket_edit(
            marks,
            world,
            recalculate,
            self.gap,
            self.fill_color.to_array(),
        ) {
            Ok(edit) => {
                self.checkpoint();
                self.prepare_paint_key(layer, frame);
                self.apply_bucket_edit(
                    layer,
                    frame,
                    edit,
                    self.feature.gradient_on,
                    self.feature.gradient.clone(),
                );
                state.clear_selection();
                self.recover();
                self.status = "Relleno aplicado".into();
            }
            Err(error) => self.status = error,
        }
    }

    fn apply_bucket_edit(
        &mut self,
        layer: usize,
        frame: usize,
        edit: BucketEdit,
        gradient_on: bool,
        gradient: crate::features::Gradient,
    ) {
        let fit_gradient = |mark: &Mark| {
            gradient_on.then(|| {
                let mut gradient = gradient.clone();
                gradient.fit(crate::vector::control_bounds(mark));
                gradient
            })
        };
        match edit {
            BucketEdit::Recolor { index, color } => {
                let link = (!gradient_on)
                    .then(|| self.track_vector_color(color))
                    .flatten();
                let marks = &mut self.doc.layers[layer].frames[frame];
                let mark = &mut marks[index];
                mark.fill = Some(color);
                mark.style.palette_fill = link;
                mark.style.gradient = fit_gradient(mark);
                mark.style.shade = 0;
                mark.style.light = 0;
            }
            BucketEdit::Replace {
                shapes,
                replaced,
                outline,
            } => {
                let mut shapes = shapes;
                let bounds = shapes.iter().fold(Rect::NOTHING, |bounds, mark| {
                    bounds.union(crate::vector::control_bounds(mark))
                });
                for shape in &mut shapes {
                    shape.style.start_cap = self.feature.start_cap;
                    shape.style.end_cap = self.feature.end_cap;
                    shape.style.gradient = gradient_on.then(|| {
                        let mut gradient = gradient.clone();
                        gradient.fit(bounds);
                        gradient
                    });
                    self.link_drawn_colors(shape);
                }
                let marks = &mut self.doc.layers[layer].frames[frame];
                if let Some(index) = outline {
                    marks[index].fill = None;
                    marks[index].style.palette_fill = None;
                    marks[index].style.gradient = None;
                }
                for index in replaced.into_iter().rev() {
                    marks.remove(index);
                }
                marks.splice(0..0, shapes);
            }
        }
    }

    fn start_bucket_range(&mut self, ctx: &egui::Context, world: Pos2, recalculate: bool) {
        if self.feature.bucket_range.is_some() || !self.fill_enabled {
            return;
        }
        let confirm = std::mem::take(&mut self.feature.bucket_range_armed);
        let layer = self.layer;
        let snapshot = self.doc.layers.clone();
        let source = &snapshot[layer];
        let frames: Vec<_> = if confirm {
            let Some((_, a, b)) = self
                .feature
                .frame_selection
                .filter(|selection| selection.0 == layer)
            else {
                return;
            };
            (a.min(b)..=a.max(b).min(source.frames.len() - 1))
                .filter(|frame| source.key(*frame))
                .take(33)
                .collect()
        } else {
            following_bucket_keys(source, self.frame, self.feature.bucket_next_count)
        };
        if confirm && frames.len() > 32 {
            self.status = crate::i18n::tr("Select a range with at most 32 keyframes.").into();
            return;
        }
        if frames.is_empty() {
            self.status = crate::i18n::tr("No keyframes to fill").into();
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let worker_snapshot = snapshot.clone();
        let current_pose =
            (!confirm && !source.key(self.frame) && self.studio.prefs.auto_key_paint)
                .then(|| (self.frame, crate::animation::marks(source, self.frame)));
        let color = self.fill_color.to_array();
        let gap = self.gap;
        let ctx = ctx.clone();
        self.feature.bucket_range = Some(BucketRange {
            layer,
            snapshot,
            confirm,
            start_frame: self.frame,
            result: None,
            pending: rx,
            gradient_on: self.feature.gradient_on,
            gradient: self.feature.gradient.clone(),
        });
        std::thread::spawn(move || {
            let edits = frames
                .into_iter()
                .map(|frame| {
                    let marks = current_pose
                        .as_ref()
                        .filter(|(current, _)| *current == frame)
                        .map_or(&worker_snapshot[layer].frames[frame], |(_, marks)| marks);
                    (frame, bucket_edit(marks, world, recalculate, gap, color))
                })
                .collect();
            let _ = tx.send(edits);
            ctx.request_repaint();
        });
    }

    fn bucket_range_ui(&mut self, ctx: &egui::Context) {
        let Some(mut batch) = self.feature.bucket_range.take() else {
            return;
        };
        if batch.snapshot.identity() != self.doc.layers.identity() {
            return;
        }
        if batch.result.is_none() {
            match batch.pending.try_recv() {
                Ok(result) => batch.result = Some(result),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        let mut open = true;
        let mut apply = !batch.confirm && batch.result.is_some();
        if batch.confirm {
            egui::Window::new(crate::i18n::tr("Fill selected keyframes"))
                .id(egui::Id::unique("bucket-range-preview"))
                .open(&mut open)
                .show(ctx, |ui| {
                    if let Some(edits) = &batch.result {
                        egui::ScrollArea::vertical()
                            .max_height(260.)
                            .show(ui, |ui| {
                                for (frame, edit) in edits {
                                    ui.horizontal(|ui| {
                                        ui.label(format!("{:03}", frame + 1));
                                        if let Err(error) = edit {
                                            ui.label(crate::i18n::tr(error));
                                        } else {
                                            ui.label(crate::i18n::tr("Fill ready"));
                                        }
                                    });
                                }
                            });
                        if crate::design::button(
                            ui,
                            Icon::Bucket,
                            "Apply fills",
                            "Apply fills",
                            false,
                            edits.iter().any(|(_, edit)| edit.is_ok()),
                        )
                        .clicked()
                        {
                            apply = true;
                        }
                    } else {
                        ui.spinner();
                    }
                });
        }
        if apply && let Some(edits) = batch.result {
            let count = edits.iter().filter(|(_, edit)| edit.is_ok()).count();
            if count == 0 {
                self.status = edits
                    .into_iter()
                    .find_map(|(_, edit)| edit.err())
                    .unwrap_or_default();
                return;
            }
            let total = edits.len();
            self.checkpoint();
            for (frame, edit) in edits {
                if let Ok(edit) = edit {
                    if !batch.confirm && frame == batch.start_frame {
                        self.prepare_paint_key(batch.layer, frame);
                    }
                    self.apply_bucket_edit(
                        batch.layer,
                        frame,
                        edit,
                        batch.gradient_on,
                        batch.gradient.clone(),
                    );
                    let layer = &mut self.doc.layers[batch.layer];
                    let marks = layer.frames[frame].clone();
                    for held in layer.exposure(frame) {
                        layer.frames[held] = marks.clone();
                    }
                }
            }
            self.recover();
            self.status = format!("{} {count}/{total}", crate::i18n::tr("Keyframes filled"));
        } else if open {
            if !batch.confirm {
                self.status = crate::i18n::tr("Filling keyframes…").into();
            }
            self.feature.bucket_range = Some(batch);
        }
    }
}

fn following_bucket_keys(layer: &crate::document::Layer, frame: usize, count: usize) -> Vec<usize> {
    std::iter::once(frame)
        .chain(
            (frame.saturating_add(1)..layer.frames.len())
                .filter(|frame| layer.key(*frame))
                .take(count.clamp(1, 32)),
        )
        .collect()
}

enum BucketEdit {
    Recolor {
        index: usize,
        color: [u8; 4],
    },
    Replace {
        shapes: Vec<Mark>,
        replaced: Vec<usize>,
        outline: Option<usize>,
    },
}

type BucketEdits = Vec<(usize, Result<BucketEdit, String>)>;

pub(crate) struct BucketRange {
    layer: usize,
    confirm: bool,
    start_frame: usize,
    snapshot: crate::shared::SharedVec<crate::document::Layer>,
    gradient_on: bool,
    gradient: crate::features::Gradient,
    pending: std::sync::mpsc::Receiver<BucketEdits>,
    result: Option<BucketEdits>,
}

fn bucket_edit(
    marks: &[Mark],
    world: Pos2,
    recalculate: bool,
    gap: crate::bucket::Gap,
    color: [u8; 4],
) -> Result<BucketEdit, String> {
    let existing = marks.iter().rposition(|mark| {
        mark.style.instance.is_none()
            && mark.style.media_image.is_none()
            && !mark.style.raster_tile
            && !mark.style.guide
            && mark.fill.is_some()
            && crate::vector::control_bounds(mark).contains(world)
            && crate::vector::contains_fill(mark, world)
    });
    if !recalculate && let Some(index) = existing {
        return Ok(BucketEdit::Recolor { index, color });
    }
    let shapes = if let Some(index) = existing.filter(|&index| !marks[index].bucket) {
        let mut contours = marks.to_vec();
        if contours[index].stroke {
            contours[index].fill = None;
        } else {
            contours.remove(index);
        }
        crate::bucket::fill(&contours, world, gap, color)?
    } else {
        crate::bucket::fill(marks, world, gap, color)?
    };
    let mut replaced = crate::bucket::covered_bucket_fills(marks, &shapes);
    let outline = existing.filter(|&index| marks[index].stroke);
    if let Some(index) = existing
        && outline.is_none()
        && !replaced.contains(&index)
    {
        replaced.push(index);
        replaced.sort_unstable();
    }
    Ok(BucketEdit::Replace {
        shapes,
        replaced,
        outline,
    })
}

fn paint_width_handles(p: &egui::Painter, mark: &Mark, view: View) {
    let controls = if mark.width_profile.is_empty() {
        crate::width::Preset::Custom.stops()
    } else {
        mark.width_profile.clone()
    };
    for stop in &controls {
        if let Some((anchor, normal)) = crate::width::at_length(mark, stop.t) {
            let radius = mark.width * stop.width * 0.5;
            let left = view.screen(anchor - normal * radius);
            let right = view.screen(anchor + normal * radius);
            p.line_segment([left, right], Stroke::new(1., crate::themes::accent()));
            for handle in [left, right] {
                p.circle_filled(handle, 3.5, Color32::from_rgb(32, 35, 40));
                p.circle_stroke(handle, 3.5, Stroke::new(1., crate::themes::accent()));
            }
            p.circle_filled(view.screen(anchor), 2., Color32::WHITE);
        }
    }
}

#[cfg(test)]
mod styling_gesture_tests {
    use super::*;
    fn button(p: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: p,
            button: PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }
    fn tick(ctx: &egui::Context, e: &mut Editor, events: Vec<egui::Event>) {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000., 700.))),
                events,
                ..Default::default()
            },
            |ui| e.canvas_ui(ui),
        );
        output.textures_delta.clear();
    }
    fn run(test: impl FnOnce(&egui::Context, &mut Editor)) {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut e = Editor::new(&cc);
        e.doc = crate::Document::default();
        let path = std::env::temp_dir().join(format!(
            "libretoon-styling-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ));
        e.recovery = path.clone();
        tick(&ctx, &mut e, vec![]);
        test(&ctx, &mut e);
        drop(e);
        let _ = std::fs::remove_file(path);
    }
    #[test]
    fn resized_stage_keeps_world_screen_inverse_under_rotation() {
        let mut c = Canvas::default();
        c.stage_size = Vec2::new(3840., 2160.);
        c.zoom = 1.7;
        c.angle = 0.6;
        c.pan = Vec2::new(21., -44.);
        let v = c.view(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200., 800.)));
        let p = Pos2::new(-500., 3000.);
        assert!(v.world(v.screen(p)).distance(p) < 0.001);
        assert_eq!(v.stage_center, Pos2::new(1920., 1080.));
    }
    #[test]
    fn width_tool_widens_locally_and_undo_restores_uniform_centerline() {
        run(|ctx, e| {
            e.doc.layers[0].frames[0].push(Mark {
                points: vec![[100., 200.], [600., 200.]].into(),
                width: 20.,
                ..Default::default()
            });
            e.tool = Tool::Width;
            let view = e
                .canvas
                .view(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000., 700.)));
            let a = view.screen(Pos2::new(350., 200.));
            let b = view.screen(Pos2::new(350., 240.));
            tick(ctx, e, vec![egui::Event::PointerMoved(a), button(a, true)]);
            tick(ctx, e, vec![egui::Event::PointerMoved(b)]);
            tick(ctx, e, vec![button(b, false)]);
            let mark = &e.doc.layers[0].frames[0][0];
            assert!(crate::width::sample(&mark.width_profile, 0.5) > 3.5);
            assert_eq!(mark.points.len(), 2);
            e.history(false);
            assert!(e.doc.layers[0].frames[0][0].width_profile.is_empty());
            e.history(true);
            assert!(crate::width::sample(&e.doc.layers[0].frames[0][0].width_profile, 0.5) > 3.5);
        });
    }
    #[test]
    fn next_bucket_keyframes_include_current_and_skip_other_held_frames() {
        let mut doc = crate::Document::default();
        let layer = &mut doc.layers[0];
        layer.init_keys();
        layer.options.keys.fill(false);
        for key in [0, 3, 7, 9] {
            layer.options.keys[key] = true;
        }
        assert_eq!(following_bucket_keys(layer, 0, 2), vec![0, 3, 7]);
        assert_eq!(following_bucket_keys(layer, 1, 2), vec![1, 3, 7]);
        assert_eq!(following_bucket_keys(layer, 3, 2), vec![3, 7, 9]);
        assert_eq!(following_bucket_keys(layer, 9, 2), vec![9]);
    }

    #[test]
    fn bucket_click_creates_vector_fill_and_recolor_does_not_accumulate_faces() {
        run(|ctx, e| {
            e.doc.layers[0].frames[0].push(Mark {
                points: vec![[100., 100.], [500., 400.]].into(),
                rectangle: true,
                closed: true,
                width: 3.,
                ..Default::default()
            });
            e.tool = Tool::Bucket;
            let view = e
                .canvas
                .view(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000., 700.)));
            let point = view.screen(Pos2::new(300., 250.));
            tick(
                ctx,
                e,
                vec![egui::Event::PointerMoved(point), button(point, true)],
            );
            tick(ctx, e, vec![button(point, false)]);
            assert_eq!(e.doc.layers[0].frames[0].len(), 2);
            assert!(e.doc.layers[0].frames[0][0].bucket);
            // A legacy fill may stop short of the real contour.
            e.doc.layers[0].frames[0][0].points = vec![[250., 225.], [350., 275.]].into();
            e.doc.layers[0].frames[0][0].rectangle = true;
            e.fill_color = Color32::RED;
            tick(ctx, e, vec![button(point, true)]);
            tick(ctx, e, vec![button(point, false)]);
            assert_eq!(e.doc.layers[0].frames[0].len(), 2);
            assert_eq!(
                e.doc.layers[0].frames[0][0].fill,
                Some(Color32::RED.to_array())
            );
            assert_eq!(
                &*e.doc.layers[0].frames[0][0].points,
                &[[250., 225.], [350., 275.]]
            );
            let mut canvas = std::mem::take(&mut e.canvas);
            e.bucket_at(&mut canvas, Pos2::new(300., 250.), true);
            e.canvas = canvas;
            assert_eq!(e.doc.layers[0].frames[0].len(), 2);
            assert!(crate::vector::contains_fill(
                &e.doc.layers[0].frames[0][0],
                Pos2::new(225., 250.),
            ));
        });
    }
}

fn selected_node_hit(e: &Editor, state: &Canvas, world: Pos2, view: View) -> bool {
    state.selected.iter().any(|&(l, m)| {
        let mark = &e.doc.layers[l].frames[e.frame][m];
        mark.stroke
            && mark.nodes.iter().any(|n| {
                [n.position, n.in_handle, n.out_handle].iter().any(|p| {
                    view.screen(crate::vector::pos(*p))
                        .distance(view.screen(world))
                        < 8.
                })
            })
    })
}

#[cfg(test)]
mod new_tool_regressions {
    use super::*;
    use geo::Area;
    fn button(p: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: p,
            button: PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        }
    }
    fn tick(ctx: &egui::Context, e: &mut Editor, events: Vec<egui::Event>) -> egui::FullOutput {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000., 700.))),
                events,
                ..Default::default()
            },
            |ui| e.canvas_ui(ui),
        );
        out.textures_delta.clear();
        out
    }
    fn run(test: impl FnOnce(&egui::Context, &mut Editor)) {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut e = Editor::new(&cc);
        e.doc = crate::Document::default();
        let path = std::env::temp_dir().join(format!(
            "libretoon-new-tools-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ));
        e.recovery = path.clone();
        tick(&ctx, &mut e, vec![]);
        test(&ctx, &mut e);
        drop(e);
        let _ = std::fs::remove_file(path);
    }
    fn rect(a: [f32; 2], b: [f32; 2], c: [u8; 4]) -> Mark {
        Mark {
            points: vec![a, b].into(),
            rectangle: true,
            closed: true,
            fill: Some(c),
            stroke: false,
            ..Default::default()
        }
    }
    fn view(e: &Editor) -> View {
        e.canvas
            .view(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000., 700.)))
    }
    fn click(ctx: &egui::Context, e: &mut Editor, p: Pos2) {
        tick(ctx, e, vec![egui::Event::PointerMoved(p), button(p, true)]);
        tick(ctx, e, vec![button(p, false)]);
    }
    #[test]
    fn symbol_pivot_survives_deselection_and_serialization_without_moving_art() {
        run(|ctx, editor| {
            editor.doc.layers[0].frames[0].push(rect(
                [200., 160.],
                [360., 320.],
                [20, 60, 90, 255],
            ));
            editor.canvas.selected = vec![(0, 0)];
            editor.convert_symbol();
            editor.tool = Tool::Transform;
            let before = editor.doc.layers[0].frames[0][0]
                .style
                .instance
                .as_ref()
                .unwrap()
                .matrix;
            let v = view(editor);
            let bounds = editor.canvas.bounds(editor).unwrap();
            let from = v.screen(bounds.center());
            let to = from + Vec2::new(35., 15.);
            tick(
                ctx,
                editor,
                vec![egui::Event::PointerMoved(from), button(from, true)],
            );
            tick(ctx, editor, vec![egui::Event::PointerMoved(to)]);
            tick(ctx, editor, vec![button(to, false)]);
            let mark = &editor.doc.layers[0].frames[0][0];
            assert_eq!(mark.style.instance.as_ref().unwrap().matrix, before);
            assert!(mark.style.symbol_pivot.is_some());
            editor.doc = serde_json::from_slice(&serde_json::to_vec(&editor.doc).unwrap()).unwrap();
            editor.canvas.clear_selection();
            editor.canvas.selected.push((0, 0));
            let bounds = editor.canvas.bounds(editor).unwrap();
            assert!(
                v.screen(editor.canvas.transform_pivot(editor, bounds))
                    .distance(to)
                    < 0.01
            );
        });
    }

    #[test]
    fn camera_drag_moves_art_with_pointer_and_records_local_key() {
        run(|ctx, e| {
            e.doc = crate::fresh_document();
            e.add_camera();
            e.frame = 10;
            e.activate_tool(Tool::Camera);
            tick(ctx, e, vec![]);
            let area = Rect::from_min_size(Pos2::ZERO, Vec2::new(1000., 700.));
            let original = e.camera_view(e.canvas.view(area));
            let stage = e.canvas.view(area);
            let p = Pos2::new(500., 350.);
            let q = p + Vec2::new(60., 20.);
            let world = original.world(p);
            tick(ctx, e, vec![egui::Event::PointerMoved(p), button(p, true)]);
            tick(ctx, e, vec![egui::Event::PointerMoved(q)]);
            tick(ctx, e, vec![button(q, false)]);
            let v = e.camera_view(e.canvas.view(area));
            assert!(
                v.screen(world).distance(q) < 0.01,
                "camera screen {:?}, expected {:?}",
                v.screen(world),
                q
            );
            assert_eq!(v.center, stage.center);
            let c = &e.doc.extras.cameras[0];
            assert_eq!(c.animation.keys.len(), 1);
            assert_eq!(c.animation.keys[0].frame, 10);
            assert_eq!(
                c.center,
                [e.doc.width as f32 / 2., e.doc.height as f32 / 2.]
            );
            assert!(e.project_document().validate().is_ok());
        });
    }
    #[test]
    fn wheel_zoom_keeps_camera_and_parallax_art_anchored_to_cursor() {
        run(|ctx, e| {
            e.doc = crate::fresh_document();
            e.add_camera();
            let camera = &mut e.doc.extras.cameras[0];
            camera.zoom = 2.5;
            camera.center = [1100., 600.];
            camera.angle = 0.4;
            e.doc.layers[0].options.depth.distance = 450.;
            e.doc.layers[0].options.depth.keep_size = false;
            let camera_before = serde_json::to_value(&e.doc.extras.cameras).unwrap();
            let pointer = Pos2::new(730., 240.);
            for tool in [Tool::Camera, Tool::Select] {
                e.tool = tool;
                e.studio.stage_only = tool == Tool::Select;
                e.canvas.fit();
                tick(ctx, e, vec![egui::Event::PointerMoved(pointer)]);
                let anchor = e.canvas_edit_view(view(e)).world(pointer);
                for _ in 0..3 {
                    let zoom_before = e.canvas.zoom;
                    tick(
                        ctx,
                        e,
                        vec![egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            phase: egui::TouchPhase::Move,
                            source: egui::MouseWheelSource::Unknown,
                            delta: Vec2::new(0., 110.),
                            modifiers: egui::Modifiers::NONE,
                        }],
                    );
                    assert!(e.canvas.zoom > zoom_before);
                    assert!(e.canvas_edit_view(view(e)).screen(anchor).distance(pointer) < 0.01);
                }
            }
            assert_eq!(
                serde_json::to_value(&e.doc.extras.cameras).unwrap(),
                camera_before
            );
        });
    }

    #[test]
    fn moving_held_art_edits_exposure_without_creating_motion() {
        run(|ctx, e| {
            e.doc = crate::fresh_document();
            let mark = rect([100., 100.], [200., 200.], [90, 60, 220, 255]);
            for f in &mut e.doc.layers[0].frames {
                *f = vec![mark.clone()].into();
            }
            e.frame = 10;
            e.activate_tool(Tool::Transform);
            tick(ctx, e, vec![]);
            e.canvas.selected = vec![(0, 0)];
            let v = view(e);
            let p = v.screen(Pos2::new(130., 140.));
            let q = p + Vec2::new(70., 0.);
            tick(ctx, e, vec![egui::Event::PointerMoved(p), button(p, true)]);
            tick(ctx, e, vec![egui::Event::PointerMoved(q)]);
            tick(ctx, e, vec![button(q, false)]);
            assert_ne!(e.doc.layers[0].frames[0][0].points, mark.points);
            assert!(!e.doc.layers[0].key(10));
            assert!(e.doc.layers[0].options.tweens.is_empty());
            assert_eq!(
                e.doc.layers[0].frames[0][0].points,
                e.doc.layers[0].frames[10][0].points
            );
            e.history(false);
            assert_eq!(e.doc.layers[0].frames[0][0].points, mark.points);
        });
    }
    #[test]
    fn selected_fill_and_stroke_update_directly_and_undo_one_gesture() {
        run(|ctx, e| {
            let mut mark = rect([100., 100.], [300., 300.], [10, 20, 30, 255]);
            mark.stroke = true;
            mark.color = [90, 80, 70, 255];
            e.doc.layers[0].frames[0].push(mark);
            e.canvas.selected = vec![(0, 0)];
            e.sync_selected_style(&[(0, 0)]);
            assert_eq!(e.fill_color.to_array(), [10, 20, 30, 255]);
            assert_eq!(e.color.to_array(), [90, 80, 70, 255]);
            e.fill_color = Color32::RED;
            e.change_selected_style(true, false, false, false);
            e.width = 23.;
            e.change_selected_style(false, false, true, false);
            assert_eq!(e.undo.len(), 1);
            let m = &e.doc.layers[0].frames[0][0];
            assert_eq!(m.fill, Some(Color32::RED.to_array()));
            assert_eq!(m.width, 23.);
            assert_eq!(m.color, [90, 80, 70, 255]);
            e.history(false);
            assert_eq!(e.doc.layers[0].frames[0][0].fill, Some([10, 20, 30, 255]));
            tick(ctx, e, vec![]);
        });
    }
    #[test]
    fn select_tool_edits_visible_bezier_handles_without_switching_tools() {
        run(|ctx, e| {
            e.tool = Tool::Select;
            let curve = crate::vector::Cubic([
                Pos2::new(180., 200.),
                Pos2::new(220., 100.),
                Pos2::new(320., 100.),
                Pos2::new(360., 200.),
            ]);
            e.doc.layers[0].frames[0].push(crate::vector::from_cubics(&[curve], &Mark::default()));
            e.canvas.selected = vec![(0, 0)];
            tick(ctx, e, vec![]);
            let v = view(e);
            let a = v.screen(curve.0[1]);
            let b = a + Vec2::new(25., 30.);
            tick(ctx, e, vec![egui::Event::PointerMoved(a), button(a, true)]);
            tick(ctx, e, vec![egui::Event::PointerMoved(b), button(b, false)]);
            let changed = &e.doc.layers[0].frames[0][0];
            assert!(crate::vector::pos(changed.nodes[0].out_handle).distance(curve.0[1]) > 20.);
            assert_eq!(changed.nodes[0].position, [180., 200.]);
            assert_eq!(e.undo.len(), 1);
            e.history(false);
            assert_eq!(
                e.doc.layers[0].frames[0][0].nodes[0].out_handle,
                [220., 100.]
            );
        });
    }
    #[test]
    fn crossing_selection_deletes_only_the_chosen_segment_and_saves_curves() {
        run(|ctx, e| {
            e.tool = Tool::Select;
            let curve = crate::vector::Cubic([
                Pos2::new(100., 200.),
                Pos2::new(210., 150.),
                Pos2::new(390., 250.),
                Pos2::new(500., 200.),
            ]);
            e.doc.layers[0].frames[0].push(crate::vector::from_cubics(
                &[curve],
                &Mark {
                    width_profile: crate::width::Preset::Tapered.stops(),
                    ..Default::default()
                },
            ));
            e.doc.layers[0].frames[0].push(Mark {
                points: vec![[300., 100.], [300., 300.]].into(),
                ..Default::default()
            });
            let p = view(e).screen(curve.at(0.2));
            let original = e.doc.layers[0].frames[0].clone();
            tick(ctx, e, vec![egui::Event::PointerMoved(p), button(p, true)]);
            tick(ctx, e, vec![button(p + Vec2::new(0., -0.5), false)]);
            assert!(e.doc.layers[0].frames[0] == original);
            assert!(e.undo.is_empty());
            assert_eq!(e.canvas.selected.len(), 1);
            let id = e.canvas.selected[0];
            assert!(
                crate::vector::control_bounds(&e.canvas.partial[&id].1[0])
                    .max
                    .x
                    < 301.
            );
            tick(
                ctx,
                e,
                vec![egui::Event::Key {
                    key: egui::Key::Delete,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                }],
            );
            assert_eq!(e.doc.layers[0].frames[0].len(), 2);
            assert!(
                e.doc.layers[0].frames[0]
                    .iter()
                    .any(|m| crate::vector::control_bounds(m).min.x > 299.)
            );
            let copy = crate::fla::decode(&crate::fla::encode(&e.doc).unwrap()).unwrap();
            assert_eq!(copy.layers[0].frames[0].len(), 2);
            e.history(false);
            assert!(e.doc.layers[0].frames[0] == original);
        });
    }
    #[test]
    fn double_click_selects_the_connected_strokes_but_not_a_separate_line() {
        run(|ctx, e| {
            e.tool = Tool::Select;
            for points in [
                [[100., 200.], [500., 200.]],
                [[300., 100.], [300., 300.]],
                [[100., 350.], [500., 350.]],
            ] {
                e.doc.layers[0].frames[0].push(Mark {
                    points: points.to_vec().into(),
                    ..Default::default()
                });
            }
            let p = view(e).screen(Pos2::new(180., 200.));
            tick(ctx, e, vec![egui::Event::PointerMoved(p)]);
            click(ctx, e, p);
            click(ctx, e, p);
            assert!(ctx.input(|i| i.pointer.button_double_clicked(PointerButton::Primary)));
            assert!(e.canvas.selected.iter().any(|&(_, m)| {
                crate::vector::control_bounds(&e.doc.layers[0].frames[0][m]).height() > 100.
            }));
            assert!(e.canvas.selected.iter().all(|&(_, m)| {
                crate::vector::control_bounds(&e.doc.layers[0].frames[0][m])
                    .min
                    .y
                    < 350.
            }));
        });
    }
    #[test]
    fn lasso_and_polygon_defer_cut_until_edit_and_undo_it() {
        run(|ctx, e| {
            for tool in [Tool::Lasso, Tool::PolygonSelect] {
                e.doc = crate::Document::default();
                e.canvas.clear_selection();
                e.doc.layers[0].frames[0].push(rect([100., 100.], [400., 400.], [1, 2, 3, 255]));
                e.tool = tool;
                let v = view(e);
                let points = [
                    Pos2::new(150., 150.),
                    Pos2::new(350., 150.),
                    Pos2::new(150., 350.),
                ]
                .map(|p| v.screen(p));
                if tool == Tool::Lasso {
                    tick(
                        ctx,
                        e,
                        vec![
                            egui::Event::PointerMoved(points[0]),
                            button(points[0], true),
                        ],
                    );
                    tick(
                        ctx,
                        e,
                        vec![
                            egui::Event::PointerMoved(points[1]),
                            egui::Event::PointerMoved(points[2]),
                            button(points[0], false),
                        ],
                    );
                } else {
                    for p in points {
                        click(ctx, e, p);
                    }
                    tick(
                        ctx,
                        e,
                        vec![egui::Event::Key {
                            key: egui::Key::Enter,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: Default::default(),
                        }],
                    );
                }
                assert!(!e.canvas.selected.is_empty(), "{}", tool.name());
                let selected: f64 = e
                    .canvas
                    .selected
                    .iter()
                    .flat_map(|id| &e.canvas.partial[id].1)
                    .map(|mark| crate::vector::polygon(mark).unsigned_area())
                    .sum();
                assert!((selected - 20000.).abs() < 2., "{} {selected}", tool.name());
                let total: f64 = e.doc.layers[0].frames[0]
                    .iter()
                    .map(|m| crate::vector::polygon(m).unsigned_area())
                    .sum();
                assert!((total - 90000.).abs() < 2.);
                assert_eq!(e.doc.layers[0].frames[0].len(), 1);
                e.copy_selection(true);
                let remainder: f64 = e.doc.layers[0].frames[0]
                    .iter()
                    .map(|mark| crate::vector::polygon(mark).unsigned_area())
                    .sum();
                assert!((remainder - 70000.).abs() < 2.);
                e.history(false);
                assert_eq!(e.doc.layers[0].frames[0].len(), 1);
            }
        });
    }
    #[test]
    fn node_edit_with_pen_release_does_not_follow_hover_outside_canvas() {
        run(|ctx, e| {
            e.doc.layers[0].frames[0].push(Mark {
                nodes: vec![
                    crate::document::Node::corner([100., 100.]),
                    crate::document::Node::corner([400., 200.]),
                ]
                .into(),
                ..Default::default()
            });
            e.canvas.selected = vec![(0, 0)];
            e.tool = Tool::Node;
            e.drawing.pen.test_pressure(Some(0.2));
            let v = view(e);
            let a = v.screen(Pos2::new(100., 100.));
            let b = v.screen(Pos2::new(160., 140.));
            tick(ctx, e, vec![egui::Event::PointerMoved(a), button(a, true)]);
            tick(ctx, e, vec![egui::Event::PointerMoved(b)]);
            tick(
                ctx,
                e,
                vec![
                    button(b, false),
                    egui::Event::PointerMoved(Pos2::new(-9000., 9000.)),
                ],
            );
            assert!(
                crate::vector::pos(e.doc.layers[0].frames[0][0].nodes[0].position)
                    .distance(Pos2::new(160., 140.))
                    < 0.01
            );
            assert_eq!(e.doc.layers[0].frames[0][0].nodes[1].position, [400., 200.]);
            e.history(false);
            assert_eq!(e.doc.layers[0].frames[0][0].nodes[0].position, [100., 100.]);
        });
    }
    #[test]
    fn pen_nodes_support_the_same_undo_redo_actions_as_committed_art() {
        run(|ctx, e| {
            e.tool = Tool::Pen;
            let v = view(e);
            for p in [
                Pos2::new(100., 100.),
                Pos2::new(250., 150.),
                Pos2::new(400., 200.),
            ] {
                click(ctx, e, v.screen(p));
            }
            assert_eq!(e.canvas.pen.as_ref().unwrap().mark.nodes.len(), 3);
            let original = e.canvas.pen.as_ref().unwrap().mark.nodes.clone();
            for _ in 0..3 {
                e.perform_action(crate::shortcuts::Action::Undo, ctx);
            }
            assert!(e.canvas.pen.is_none());
            assert!(e.can_redo());
            for _ in 0..3 {
                e.perform_action(crate::shortcuts::Action::Redo, ctx);
            }
            assert_eq!(e.canvas.pen.as_ref().unwrap().mark.nodes, original);
            e.perform_action(crate::shortcuts::Action::Undo, ctx);
            click(ctx, e, v.screen(Pos2::new(500., 250.)));
            assert!(!e.can_redo());
            e.finish_pending_pen();
            assert_eq!(e.doc.layers[0].frames[0].len(), 1);
            e.perform_action(crate::shortcuts::Action::Undo, ctx);
            assert!(e.doc.layers[0].frames[0].is_empty());
            e.perform_action(crate::shortcuts::Action::Redo, ctx);
            assert_eq!(e.doc.layers[0].frames[0].len(), 1);
        });
    }
    #[test]
    fn brush_and_eraser_sizes_remain_independent_and_drive_actual_erasure() {
        run(|_ctx, e| {
            e.width = 3.;
            e.eraser_width = 40.;
            e.activate_tool(Tool::Eraser);
            e.doc.layers[0].frames[0].push(rect([100., 100.], [400., 400.], [10, 20, 30, 255]));
            assert!(e.erase_segment(Pos2::new(250., 80.), Pos2::new(250., 420.)));
            assert!(
                !e.doc.layers[0].frames[0]
                    .iter()
                    .any(|m| crate::vector::contains_fill(m, Pos2::new(265., 250.)))
            );
            assert!(
                e.doc.layers[0].frames[0]
                    .iter()
                    .any(|m| crate::vector::contains_fill(m, Pos2::new(280., 250.)))
            );
            e.eraser_width = 11.;
            e.activate_tool(Tool::Brush);
            assert_eq!(e.width, 3.);
            e.width = 7.;
            e.activate_tool(Tool::Eraser);
            assert_eq!(e.eraser_width, 11.);
            e.drawing.pen.test_eraser(true);
            assert_eq!(e.width, 7.);
            e.drawing.pen.test_eraser(false);
            assert_eq!(e.eraser_width, 11.);
        });
    }

    #[test]
    fn stylus_eraser_erases_without_replacing_the_selected_tool() {
        run(|ctx, e| {
            e.doc.layers[0].frames[0].push(rect([100., 100.], [400., 400.], [10, 20, 30, 255]));
            e.tool = Tool::Brush;
            e.width = 3.;
            e.eraser_width = 40.;
            e.drawing.pen.test_eraser(true);
            let v = view(e);
            let a = v.screen(Pos2::new(250., 80.));
            let b = v.screen(Pos2::new(250., 420.));
            tick(ctx, e, vec![egui::Event::PointerMoved(a), button(a, true)]);
            tick(ctx, e, vec![egui::Event::PointerMoved(b), button(b, false)]);
            assert!(
                !e.doc.layers[0].frames[0]
                    .iter()
                    .any(|m| crate::vector::contains_fill(m, Pos2::new(250., 250.)))
            );
            assert!(e.tool == Tool::Brush);
            assert_eq!(e.undo.len(), 1);
            e.drawing.pen.test_eraser(false);
            e.history(false);
            assert_eq!(e.doc.layers[0].frames[0].len(), 1);
        });
    }
    #[test]
    fn arranging_objects_keeps_selection_geometry_and_undo() {
        run(|_, e| {
            for i in 0..4 {
                e.doc.layers[0].frames[0].push(rect(
                    [i as f32 * 100., 100.],
                    [i as f32 * 100. + 50., 200.],
                    [i, 20, 30, 255],
                ));
            }
            e.canvas.selected = vec![(0, 1), (0, 2)];
            let before = e.doc.clone();
            let mut state = std::mem::take(&mut e.canvas);
            e.arrange_selection(&mut state, crate::ordering::Order::Front);
            assert_eq!(state.selected, vec![(0, 2), (0, 3)]);
            assert_eq!(
                e.doc.layers[0].frames[0]
                    .iter()
                    .map(|m| m.fill.unwrap()[0])
                    .collect::<Vec<_>>(),
                vec![0, 3, 1, 2]
            );
            assert_eq!(
                e.doc.layers[0].frames[0][2].points,
                before.layers[0].frames[0][1].points
            );
            e.canvas = state;
            e.history(false);
            assert_eq!(
                e.doc.layers[0].frames[0]
                    .iter()
                    .map(|m| m.fill.unwrap()[0])
                    .collect::<Vec<_>>(),
                vec![0, 1, 2, 3]
            );
        });
    }
    #[test]
    fn eraser_removes_fill_and_axis_sections_in_a_single_undo_step() {
        run(|ctx, e| {
            e.doc.layers[0].frames[0].push(rect([100., 100.], [400., 400.], [10, 20, 30, 255]));
            e.doc.layers[0].frames[0].push(Mark {
                points: vec![[80., 250.], [450., 250.]].into(),
                ..Default::default()
            });
            e.tool = Tool::Eraser;
            e.eraser_width = 40.;
            let v = view(e);
            let a = v.screen(Pos2::new(250., 80.));
            let b = v.screen(Pos2::new(250., 420.));
            tick(ctx, e, vec![egui::Event::PointerMoved(a), button(a, true)]);
            tick(ctx, e, vec![egui::Event::PointerMoved(b), button(b, false)]);
            assert!(
                !e.doc.layers[0].frames[0]
                    .iter()
                    .any(|m| crate::vector::contains_fill(m, Pos2::new(250., 250.)))
            );
            assert!(!e.doc.layers[0].frames[0].iter().any(|m| stroke_hit(
                m,
                Pos2::new(250., 250.),
                0.
            )));
            assert_eq!(e.undo.len(), 1);
            e.history(false);
            assert_eq!(e.doc.layers[0].frames[0].len(), 2);
        });
    }
    #[test]
    fn new_shapes_are_editable_and_roundtrip_native_fla() {
        run(|ctx, e| {
            for tool in [Tool::Line, Tool::Ellipse, Tool::Polygon] {
                e.doc = crate::Document::default();
                e.canvas.clear_selection();
                e.tool = tool;
                e.polygon_star = true;
                e.polygon_sides = 7;
                let a = Pos2::new(200., 200.);
                let b = Pos2::new(480., 400.);
                tick(ctx, e, vec![egui::Event::PointerMoved(a), button(a, true)]);
                tick(ctx, e, vec![egui::Event::PointerMoved(b), button(b, false)]);
                assert_eq!(e.doc.layers[0].frames[0].len(), 1);
                let m = &e.doc.layers[0].frames[0][0];
                assert_eq!(
                    m.nodes.len(),
                    match tool {
                        Tool::Line => 2,
                        Tool::Ellipse => 4,
                        _ => 14,
                    }
                );
                if tool == Tool::Ellipse {
                    assert_ne!(m.nodes[0].out_handle, m.nodes[0].position);
                }
                let copy = crate::fla::decode(&crate::fla::encode(&e.doc).unwrap()).unwrap();
                assert_eq!(copy.layers[0].frames[0][0].nodes, m.nodes);
            }
        });
    }
    #[test]
    fn eyedropper_samples_transformed_bitmap_and_ignores_transparent_pixels() {
        run(|ctx, e| {
            e.doc.layers[0].frames[0].push(rect([100., 100.], [300., 300.], [30, 40, 50, 255]));
            let asset = crate::media::bitmap(
                vec![210, 20, 40, 255, 0, 200, 0, 0],
                2,
                1,
                "Pasted image".into(),
            );
            let mut image = Mark {
                points: vec![[240., 120.], [240., 260.], [120., 260.], [120., 120.]].into(),
                fill: Some([0; 4]),
                stroke: false,
                closed: true,
                ..Mark::default()
            };
            image.style.media_image = Some(asset.id);
            e.doc.extras.media.assets.push(asset);
            e.doc.layers[0].frames[0].push(image);
            e.activate_tool(Tool::Eyedropper);
            let view = view(e);
            click(ctx, e, view.screen(Pos2::new(180., 150.)));
            assert_eq!(e.fill_color, Color32::from_rgb(210, 20, 40));
            click(ctx, e, view.screen(Pos2::new(180., 230.)));
            assert_eq!(e.fill_color, Color32::from_rgb(30, 40, 50));

            e.activate_tool(Tool::Brush);
            let fill = e.fill_color;
            e.studio.color_picker = Some(false);
            click(ctx, e, view.screen(Pos2::new(180., 150.)));
            assert_eq!(e.color, Color32::from_rgb(210, 20, 40));
            assert_eq!(e.fill_color, fill);
            assert_eq!(e.doc.layers[0].frames[0].len(), 2);
            assert!(e.undo.is_empty());
        });
    }
    #[test]
    fn wand_eyedropper_and_connected_object_selection_are_distinct() {
        run(|ctx, e| {
            e.doc.layers[0].frames[0].extend([
                rect([100., 100.], [200., 200.], [255, 0, 0, 255]),
                rect([300., 100.], [400., 200.], [255, 0, 0, 255]),
                rect([500., 100.], [600., 200.], [0, 0, 255, 255]),
            ]);
            e.tool = Tool::Wand;
            click(ctx, e, view(e).screen(Pos2::new(150., 150.)));
            assert_eq!(e.canvas.selected, vec![(0, 1), (0, 0)]);
            e.canvas.clear_selection(); // This case samples without applying to a selection.
            e.activate_tool(Tool::Eyedropper);
            click(ctx, e, view(e).screen(Pos2::new(550., 150.)));
            assert_eq!(e.fill_color, Color32::BLUE);
            e.doc.layers[0].frames[0].push(Mark {
                points: vec![[190., 150.], [310., 150.]].into(),
                width: 4.,
                ..Default::default()
            });
            e.activate_tool(Tool::Object);
            click(ctx, e, view(e).screen(Pos2::new(150., 150.)));
            assert_eq!(e.canvas.selected.len(), 3);
            assert!(!e.canvas.selected.contains(&(0, 2)));
        });
    }
    #[test]
    fn fill_face_selection_conserves_area_and_leaves_crossing_contour_untouched() {
        run(|ctx, e| {
            e.doc.layers[0].frames[0].extend([
                rect([100., 100.], [400., 400.], [255, 0, 0, 255]),
                Mark {
                    points: vec![[250., 80.], [250., 420.]].into(),
                    ..Default::default()
                },
            ]);
            e.tool = Tool::Select;
            click(ctx, e, view(e).screen(Pos2::new(150., 150.)));
            assert_eq!(e.canvas.selected.len(), 1);
            let (l, m) = e.canvas.selected[0];
            assert!(
                crate::vector::control_bounds(&e.doc.layers[l].frames[0][m])
                    .max
                    .x
                    < 251.
            );
            assert_eq!(
                e.doc.layers[0].frames[0]
                    .iter()
                    .filter(|m| m.stroke)
                    .count(),
                1
            );
            let total: f64 = e.doc.layers[0].frames[0]
                .iter()
                .filter(|m| m.fill.is_some())
                .map(|m| crate::vector::polygon(m).unsigned_area())
                .sum();
            assert!((total - 90000.).abs() < 0.2);
        });
    }
    #[test]
    fn hand_and_zoom_preserve_art_and_zoom_anchors_at_pointer() {
        run(|ctx, e| {
            e.tool = Tool::Pan;
            let a = Pos2::new(300., 300.);
            let b = Pos2::new(400., 350.);
            tick(ctx, e, vec![egui::Event::PointerMoved(a), button(a, true)]);
            tick(ctx, e, vec![egui::Event::PointerMoved(b), button(b, false)]);
            assert!(e.canvas.pan.length() > 100.);
            assert!(e.doc.layers[0].frames[0].is_empty());
            e.tool = Tool::Zoom;
            let before = view(e).world(a);
            let old = e.canvas.zoom;
            click(ctx, e, a);
            assert!(e.canvas.zoom > old);
            assert!(view(e).world(a).distance(before) < 0.01);
            e.zoom_out = true;
            click(ctx, e, a);
            assert!((e.canvas.zoom - old).abs() < 0.01);
        });
    }
    #[test]
    fn brush_cursor_uses_high_contrast_radius_and_recovers_default_outside_canvas() {
        run(|ctx, e| {
            e.tool = Tool::Brush;
            e.width = 30.;
            let p = Pos2::new(300., 300.);
            let out = tick(ctx, e, vec![egui::Event::PointerMoved(p)]);
            assert_eq!(out.platform_output.cursor_icon, CursorIcon::None);
            let expected = e.width * e.canvas.actual_scale * 0.5;
            assert!(out.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Circle(c) if (c.radius-expected).abs()<0.01&&c.stroke.color==Color32::WHITE)));
            let out = tick(
                ctx,
                e,
                vec![egui::Event::PointerMoved(Pos2::new(-20., -20.))],
            );
            assert_eq!(out.platform_output.cursor_icon, CursorIcon::Default);
        });
    }
}

#[cfg(test)]
mod node_drag_regressions {
    use super::*;
    #[test]
    fn smooth_handle_keeps_opposite_length_and_corner_does_not_grow_a_tangent() {
        let mark = Mark {
            nodes: vec![crate::document::Node {
                position: [100., 100.],
                in_handle: [95., 100.],
                out_handle: [130., 100.],
            }]
            .into(),
            ..Default::default()
        };
        let edited = move_node(&mark, 0, 2, Vec2::new(200., 40.), false);
        assert!(
            (crate::vector::pos(edited.nodes[0].in_handle).distance(Pos2::new(100., 100.)) - 5.)
                .abs()
                < 0.001
        );
        let mut corner = mark.clone();
        corner.nodes[0].in_handle = [100., 100.];
        assert_eq!(
            move_node(&corner, 0, 2, Vec2::new(200., 40.), false).nodes[0].in_handle,
            [100., 100.]
        );
        assert_eq!(
            move_node(&mark, 0, 2, Vec2::new(200., 40.), true).nodes[0].in_handle,
            [95., 100.]
        );
        let translated = move_node(&mark, 0, 0, Vec2::new(-500., 300.), false);
        assert_eq!(translated.nodes[0].position, [-400., 400.]);
        assert_eq!(translated.nodes[0].in_handle, [-405., 400.]);
    }
    #[test]
    fn releasing_pen_or_mouse_ignores_later_hover_positions() {
        let at = Pos2::new(250., 300.);
        let release = egui::Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        };
        assert_eq!(
            node_contact_position(
                &[
                    egui::Event::PointerMoved(at),
                    release,
                    egui::Event::PointerMoved(Pos2::new(9999., 9999.))
                ],
                false
            ),
            Some(at)
        );
        let end = egui::Event::Touch {
            device_id: egui::TouchDeviceId(0),
            id: egui::TouchId(0),
            phase: egui::TouchPhase::End,
            pos: Pos2::ZERO,
            force: Some(0.),
        };
        assert_eq!(
            node_contact_position(
                &[
                    egui::Event::PointerMoved(at),
                    end,
                    egui::Event::PointerMoved(Pos2::ZERO)
                ],
                false
            ),
            Some(at)
        );
        assert_eq!(
            node_contact_position(&[egui::Event::PointerMoved(at)], false),
            None
        );
    }
}

#[cfg(test)]
mod shape_feedback_regressions {
    use super::*;
    #[test]
    fn shape_cursor_and_grid_snapping_work_through_real_canvas_input() {
        let ctx = egui::Context::default();
        let mut e = Editor::new(&eframe::CreationContext::_new_kittest(ctx.clone()));
        e.doc = crate::fresh_document();
        e.doc.width = 1000;
        e.doc.height = 800;
        e.tool = Tool::Ellipse;
        e.studio.prefs.view = crate::view_options::Config::default();
        e.studio.prefs.view.grid.size = [50., 50.];
        e.studio.prefs.view.grid.subdivisions = 1;
        e.studio.prefs.view.snaps.grid = true;
        e.recovery = std::env::temp_dir().join(format!("shape-snap-{}.json", std::process::id()));
        let tick = |e: &mut Editor, events: Vec<egui::Event>| {
            let mut area = Rect::NOTHING;
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000., 800.))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    area = ui.available_rect_before_wrap();
                    e.canvas_ui(ui);
                },
            );
            let cursor = out.platform_output.cursor_icon;
            out.textures_delta.clear();
            (area, cursor)
        };
        let (area, _) = tick(&mut e, vec![]);
        let view = e.canvas.view(area);
        let a = view.screen(Pos2::new(103., 98.));
        let b = view.screen(Pos2::new(197., 202.));
        let (_, cursor) = tick(&mut e, vec![egui::Event::PointerMoved(a)]);
        assert_eq!(cursor, CursorIcon::None);
        let press = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        tick(&mut e, vec![press(a, true)]);
        tick(&mut e, vec![egui::Event::PointerMoved(b)]);
        tick(&mut e, vec![press(b, false)]);
        let bounds = crate::vector::control_bounds(&e.doc.layers[0].frames[0][0]);
        assert!((bounds.left() - 100.).abs() < 0.01);
        assert!((bounds.right() - 200.).abs() < 0.01);
        assert!((bounds.top() - 100.).abs() < 0.01);
        assert!((bounds.bottom() - 200.).abs() < 0.01);
    }
}
