//! Camera-space layer planes and bounded, premultiplied layer compositing.
use crate::{
    Document, Editor, Mark,
    canvas::{Tool, View},
    design::{self, Icon},
    i18n::tr,
};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
use serde::{Deserialize, Serialize};
const REFERENCE: f32 = 1000.;
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LayerDepth {
    pub distance: f32,
    pub keep_size: bool,
}
impl Default for LayerDepth {
    fn default() -> Self {
        Self {
            distance: REFERENCE,
            keep_size: true,
        }
    }
}
impl LayerDepth {
    pub fn valid(&self) -> bool {
        self.distance.is_finite() && (1. ..=1e6).contains(&self.distance)
    }
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlurMethod {
    #[default]
    Gaussian,
    Lens,
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    Low,
    #[default]
    Medium,
    High,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Optics {
    pub position: f32,
    pub method: BlurMethod,
    pub strength: f32,
    pub quality: Quality,
    pub focus: f32,
    pub range: f32,
}
impl Default for Optics {
    fn default() -> Self {
        Self {
            position: 0.,
            method: BlurMethod::Gaussian,
            strength: 0.,
            quality: Quality::Medium,
            focus: REFERENCE,
            range: 100.,
        }
    }
}
impl Optics {
    pub fn interpolate(&self, to: &Self, t: f32) -> Self {
        let t = t.clamp(0., 1.);
        let lerp = |a: f32, b: f32| a + (b - a) * t;
        // Discrete settings belong to the preceding key, even if easing reaches 1 early.
        let discrete = self;
        Self {
            position: lerp(self.position, to.position),
            focus: lerp(self.focus, to.focus),
            range: lerp(self.range, to.range),
            strength: lerp(self.strength, to.strength),
            method: discrete.method,
            quality: discrete.quality,
        }
    }
    pub fn valid(&self) -> bool {
        self.position.is_finite()
            && self.position.abs() <= 1e6
            && self.strength.is_finite()
            && (0. ..=100.).contains(&self.strength)
            && self.focus.is_finite()
            && (0. ..=2e6).contains(&self.focus)
            && self.range.is_finite()
            && (0. ..=2e6).contains(&self.range)
    }
    pub fn radius(&self, distance: f32) -> f32 {
        let t = ((distance - self.focus).abs() - self.range).max(0.) / self.range.max(100.);
        let t = t.clamp(0., 1.);
        t * t * (3. - 2. * t) * self.strength * 0.32
    }
}
pub fn camera(doc: &Document, frame: usize) -> Option<&crate::features::Camera> {
    doc.extras
        .cameras
        .iter()
        .find(|c| frame >= c.start && frame < c.start + c.duration)
}
fn factors(layer: &LayerDepth, optics: &Optics) -> (f32, f32) {
    let distance = (layer.distance - optics.position).max(1.);
    (
        if layer.keep_size {
            layer.distance / distance
        } else {
            REFERENCE / distance
        },
        REFERENCE / distance,
    )
}
pub fn project_mark(
    doc: &Document,
    pose: &crate::animation::CameraKey,
    optics: &Optics,
    layer: &LayerDepth,
    mark: &Mark,
) -> Mark {
    let (size, motion) = factors(layer, optics);
    let center = Pos2::new(doc.width as f32 / 2., doc.height as f32 / 2.);
    let offset = (crate::vector::pos(pose.center) - center) * (1. - motion);
    if size == 1. && offset == Vec2::ZERO {
        return mark.clone();
    }
    let mut result = crate::vector::transform(mark, |p| center + (p - center) * size + offset);
    result.width *= size;
    result
}
pub fn layer_view(doc: &Document, frame: usize, layer: usize, base: View) -> View {
    let Some(layer) = doc.layers.get(layer) else {
        return base;
    };
    view_for_plane(doc, frame, &layer.options.depth, base)
}
pub fn view_for_plane(doc: &Document, frame: usize, plane: &LayerDepth, base: View) -> View {
    let Some(c) = camera(doc, frame) else {
        return base;
    };
    let pose = crate::animation::camera_at(c, frame);
    let (size, motion) = factors(plane, &pose.optics(c));
    let center = Pos2::new(doc.width as f32 / 2., doc.height as f32 / 2.);
    View {
        scale: base.scale * size,
        stage_center: center + (crate::vector::pos(pose.center) - center) * (motion / size),
        ..base
    }
}
pub fn active(editor: &Editor) -> bool {
    editor.playing || editor.studio.stage_only || editor.tool == Tool::Camera
}
fn label(ui: &mut egui::Ui, icon: Icon, text: &str) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(Vec2::splat(18.), egui::Sense::hover());
        design::draw(ui.painter(), r, icon, crate::themes::accent());
        ui.label(tr(text));
    });
}
const HELP: &str = "Las distancias se miden desde la cámara. Foco 200 y amplitud 100 mantienen nítido de 100 a 300; fuera de ese intervalo el desenfoque aumenta suavemente. Mantener tamaño conserva el tamaño inicial de las capas al cambiar su profundidad. Las capas cercanas se desplazan más al mover la cámara. Ocultar el desenfoque solo afecta al editor, nunca a la exportación.";
fn help(ui: &mut egui::Ui) {
    let id = ui.scope_id().with("depth-help-open");
    let mut open = ui.data(|data| data.get_temp::<bool>(id).unwrap_or(false));
    let response =
        design::button(ui, Icon::Info, "", "Ayuda de cámara", open, true).on_hover_text(tr(HELP));
    if response.clicked() {
        open = !open;
    }
    egui::Popup::from_response(&response)
        .width(340.)
        .open_bool(&mut open)
        .show(|ui| {
            ui.label(tr(HELP));
        });
    ui.data_mut(|data| data.insert_temp(id, open));
}
pub fn optics_ui(ui: &mut egui::Ui, optics: &mut Optics) -> bool {
    design::category(ui, "Profundidad de campo");
    ui.horizontal(|ui| {
        label(ui, Icon::Camera, "Ajustes de enfoque");
        help(ui);
    });
    let mut changed = false;
    label(ui, Icon::Blur, "Método de desenfoque");
    egui::ComboBox::from_id_salt("camera-blur-method")
        .selected_text(tr(if optics.method == BlurMethod::Gaussian {
            "Gaussiano"
        } else {
            "Lente"
        }))
        .show_ui(ui, |ui| {
            changed |= ui
                .selectable_value(&mut optics.method, BlurMethod::Gaussian, tr("Gaussiano"))
                .changed();
            changed |= ui
                .selectable_value(&mut optics.method, BlurMethod::Lens, tr("Lente"))
                .changed();
        });
    label(ui, Icon::Blur, "Fuerza de desenfoque");
    changed |= ui
        .add(egui::Slider::new(&mut optics.strength, 0. ..=100.))
        .changed();
    label(ui, Icon::Settings, "Calidad del desenfoque");
    egui::ComboBox::from_id_salt("camera-blur-quality")
        .selected_text(tr(match optics.quality {
            Quality::Low => "Baja",
            Quality::Medium => "Media",
            Quality::High => "Alta",
        }))
        .show_ui(ui, |ui| {
            for (q, s) in [
                (Quality::Low, "Baja"),
                (Quality::Medium, "Media"),
                (Quality::High, "Alta"),
            ] {
                changed |= ui.selectable_value(&mut optics.quality, q, tr(s)).changed();
            }
        });
    for (icon, title, value) in [
        (Icon::Focus, "Punto de foco", &mut optics.focus),
        (Icon::Depth, "Amplitud de enfoque", &mut optics.range),
    ] {
        label(ui, icon, title);
        changed |= ui
            .add(egui::DragValue::new(value).range(0. ..=2e6).speed(1.))
            .changed();
    }
    ui.small(format!(
        "{}: {:.0} – {:.0}",
        tr("Zona nítida"),
        (optics.focus - optics.range).max(0.),
        optics.focus + optics.range
    ));
    changed
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum ViewportQuality {
    #[default]
    Full,
    Half,
    Quarter,
    Eighth,
}
impl ViewportQuality {
    pub const ALL: [Self; 4] = [Self::Eighth, Self::Quarter, Self::Half, Self::Full];
    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "Full quality",
            Self::Half => "1/2",
            Self::Quarter => "1/4",
            Self::Eighth => "1/8",
        }
    }
    fn factor(self) -> f32 {
        match self {
            Self::Full => 1.,
            Self::Half => 0.5,
            Self::Quarter => 0.25,
            Self::Eighth => 0.125,
        }
    }
}

const PREVIEW_TEXTURE_BYTES: usize = 96 * 1024 * 1024;
const PREVIEW_LOOKAHEAD: usize = 12;
const PREVIEW_IDLE: std::time::Duration = std::time::Duration::from_millis(200);

struct CachedPreview {
    key: Vec<usize>,
    pixels: Vec<u8>,
    size: [usize; 2],
    texture: Option<egui::TextureHandle>,
}

fn preview_identity(doc: &Document) -> [usize; 25] {
    let mut identity = [0; 25];
    identity[..11].copy_from_slice(&[
        doc.layers.identity(),
        doc.extras.cameras.identity(),
        doc.extras.symbols.identity(),
        doc.extras.folders.identity(),
        doc.extras.rig.bones.identity(),
        doc.extras.rig.bindings.identity(),
        doc.extras.media.assets.identity(),
        doc.extras.media.tracks.identity(),
        doc.extras.animation_clips.identity(),
        doc.width as usize,
        doc.height as usize,
    ]);
    identity[11..].copy_from_slice(&doc.extras.rig.character_turns.identity());
    identity
}

// Only retain a composited frame across changes to light or FX parameters.
// Artwork, camera, visibility and other document changes use the live stage.
fn only_compositing_changed(before: &Document, after: &Document) -> bool {
    let previous = preview_identity(before);
    let current = preview_identity(after);
    if previous[1..] != current[1..]
        || before.fps != after.fps
        || before.layers.len() != after.layers.len()
    {
        return false;
    }
    let mut changed = false;
    for (old, new) in before.layers.iter().zip(after.layers.iter()) {
        if old.frames.identity() != new.frames.identity()
            || old.visible != new.visible
            || old.name != new.name
            || old.options.light.is_some() != new.options.light.is_some()
            || old.options.adjustment.is_some() != new.options.adjustment.is_some()
        {
            return false;
        }
        let (mut old, mut new) = (old.options.clone(), new.options.clone());
        changed |= old.adjustment != new.adjustment
            || old.adjustment_bands != new.adjustment_bands
            || old.light != new.light;
        for options in [&mut old, &mut new] {
            options.adjustment = None;
            options.adjustment_bands = None;
            options.light = None;
        }
        if old != new {
            return false;
        }
    }
    changed
}

#[derive(Default)]
pub struct DepthEditor {
    pub open: bool,
    pub hide_blur: bool,
    pub preview: Option<egui::TextureHandle>,
    job: Option<std::sync::mpsc::Receiver<(Vec<usize>, Vec<u8>, [usize; 2])>>,
    requested: Vec<usize>,
    work: Option<std::sync::Arc<PreviewWork>>,
    gpu: Option<std::sync::Arc<crate::gpu_lens::GpuLens>>,
    job_is_prefetch: bool,
    owner: Option<Document>,
    render_doc: Option<Document>,
    independent_clock: Option<bool>,
    cached: std::collections::VecDeque<CachedPreview>,
    prepare: Option<(usize, usize)>,
    shown: Vec<usize>,
    held_preview: bool,
    interactive_job: bool,
    focus: Option<(Vec<usize>, std::time::Instant)>,
    preview_scope: Option<(
        usize,
        Option<u64>,
        u32,
        u32,
        bool,
        bool,
        bool,
        bool,
        Option<usize>,
        ViewportQuality,
    )>,
    preview_epoch: usize,
    render_cache: std::sync::Arc<std::sync::Mutex<crate::paint::RenderCache>>,
}
#[derive(Default)]
pub(crate) struct PreviewWork {
    canceled: std::sync::atomic::AtomicBool,
    progress: std::sync::atomic::AtomicU8,
    phase: std::sync::atomic::AtomicU8,
}
impl PreviewWork {
    pub(crate) fn canceled(&self) -> bool {
        self.canceled.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub(crate) fn phase(&self, phase: u8) {
        self.phase
            .store(phase, std::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn report(&self, progress: u8) {
        self.progress
            .store(progress, std::sync::atomic::Ordering::Relaxed);
    }
}

fn preview_texture(ctx: &egui::Context, size: [usize; 2], pixels: &[u8]) -> egui::TextureHandle {
    ctx.load_texture(
        "composited-stage-preview",
        egui::ColorImage::from_rgb(size, pixels),
        egui::TextureOptions::LINEAR,
    )
}

impl DepthEditor {
    pub(crate) fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            gpu: cc
                .wgpu_render_state
                .as_ref()
                .and_then(crate::gpu_lens::GpuLens::from_renderer)
                .map(std::sync::Arc::new),
            ..Self::default()
        }
    }

    pub(crate) fn invalidate_preview(&mut self) {
        self.cancel_job();
        self.preview = None;
        self.prepare = None;
        self.shown.clear();
        self.held_preview = false;
        self.interactive_job = false;
        self.cached.clear();
        self.focus = None;
        self.owner = None;
        self.render_doc = None;
        self.independent_clock = None;
        self.render_cache = Default::default();
        self.requested.clear();
        self.preview_scope = None;
        self.preview_epoch = self.preview_epoch.wrapping_add(1);
    }

    fn sync_document(&mut self, doc: &Document) {
        if self
            .owner
            .as_ref()
            .is_none_or(|owner| preview_identity(owner) != preview_identity(doc))
        {
            let editing = self
                .owner
                .as_ref()
                .is_some_and(|owner| only_compositing_changed(owner, doc));
            let displayed_latest = self.shown.first() == Some(&self.preview_epoch);
            let requested_latest = self.requested.first() == Some(&self.preview_epoch);
            self.held_preview = editing && (displayed_latest || self.held_preview);
            self.interactive_job =
                editing && !self.job_is_prefetch && (requested_latest || self.interactive_job);
            if !self.interactive_job {
                self.cancel_job();
            }
            self.preview_epoch = self.preview_epoch.wrapping_add(1);
            self.cached.clear();

            self.owner = Some(doc.clone());
            self.render_doc = None;
            self.independent_clock = None;
            self.prepare = None;
        }
    }

    fn cache_clock(&mut self, doc: &Document, frame: usize, playback_clock: usize) -> usize {
        if frame == playback_clock {
            return frame;
        }
        let independent = self.independent_clock.get_or_insert_with(|| {
            // Most projects only use scene-synchronized symbols. Inspect shared
            // cels once per revision, with a conservative fallback on large files.
            let mut visited = std::collections::HashSet::new();
            let mut remaining = 100_000usize;
            let layers = doc.layers.iter().chain(
                doc.extras
                    .symbols
                    .iter()
                    .flat_map(|symbol| symbol.layers.iter()),
            );
            for cel in layers.flat_map(|layer| layer.frames.iter()) {
                if remaining == 0 {
                    return true;
                }
                remaining -= 1;
                if !visited.insert(cel.identity()) {
                    continue;
                }
                if cel.len() > remaining {
                    return true;
                }
                remaining -= cel.len();
                if cel.iter().any(|mark| {
                    mark.style.instance.as_ref().is_some_and(|instance| {
                        instance.clock == crate::features::SymbolClock::Independent
                    })
                }) {
                    return true;
                }
            }
            false
        });
        if *independent { playback_clock } else { frame }
    }

    fn cancel_job(&self) {
        if let Some(work) = &self.work {
            work.canceled
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    pub(crate) fn cache_usage(&self) -> (usize, usize) {
        (
            self.cached.iter().map(|frame| frame.pixels.len()).sum(),
            self.cached.len(),
        )
    }

    fn trim_cache(&mut self, limit: usize) {
        let mut bytes = self.cache_usage().0;
        while bytes > limit {
            let index = self
                .cached
                .iter()
                .position(|entry| entry.key != self.shown)
                .unwrap_or(0);
            bytes -= self.cached.remove(index).unwrap().pixels.len();
        }
    }

    fn texture_bytes(&self) -> usize {
        self.cached
            .iter()
            .filter_map(|f| f.texture.as_ref())
            .map(egui::TextureHandle::byte_size)
            .sum()
    }

    fn trim_textures(&mut self) {
        let mut bytes = self.texture_bytes();
        for frame in &mut self.cached {
            if bytes <= PREVIEW_TEXTURE_BYTES {
                break;
            }
            if frame.key != self.shown
                && let Some(texture) = frame.texture.take()
            {
                bytes -= texture.byte_size();
            }
        }
    }

    fn show_cached(&mut self, key: &[usize], ctx: &egui::Context) {
        if self.shown == key {
            return;
        }
        if let Some(index) = self.cached.iter().position(|frame| frame.key == key) {
            let mut frame = self.cached.remove(index).unwrap();
            let texture = frame
                .texture
                .get_or_insert_with(|| preview_texture(ctx, frame.size, &frame.pixels));
            self.preview = Some(texture.clone());
            self.shown.clone_from(&frame.key);
            self.cached.push_back(frame);
            self.trim_textures();
        }
    }

    pub(crate) fn ready_frames(&self, doc: &Document) -> impl Iterator<Item = usize> + '_ {
        let valid = self
            .owner
            .as_ref()
            .is_some_and(|owner| preview_identity(owner) == preview_identity(doc));
        let focus = self.focus.as_ref().map(|(key, _)| key);
        self.cached.iter().filter_map(move |cached| {
            let key = &cached.key;
            let current_size = focus.is_some_and(|focus| focus[3..] == key[3..]);
            let current_clock = focus.is_some_and(|focus| {
                focus[2].wrapping_sub(focus[1]) == key[2].wrapping_sub(key[1])
            });
            (valid && current_size && current_clock).then_some(key[1])
        })
    }
}

impl Editor {
    pub fn depth_window(&mut self, ctx: &egui::Context) {
        if !self.depth.open {
            return;
        }
        let mut open = true;
        egui::Window::new(tr("Profundidad de capas"))
            .id(egui::Id::unique("layer-depth"))
            .open(&mut open)
            .default_size([740., 600.])
            .vscroll(true)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    label(ui, Icon::Camera, "Cámara");
                    if self.doc.extras.cameras.is_empty() {
                        if design::button(
                            ui,
                            Icon::Plus,
                            "Añadir cámara",
                            "Añadir cámara",
                            false,
                            true,
                        )
                        .clicked()
                        {
                            self.add_camera();
                        }
                    } else {
                        let selected = camera(&self.doc, self.frame)
                            .map(|c| c.id)
                            .or(self.feature.selected_camera);
                        egui::ComboBox::from_id_salt("depth-camera")
                            .selected_text(
                                self.doc
                                    .extras
                                    .cameras
                                    .iter()
                                    .find(|c| Some(c.id) == selected)
                                    .map(|c| c.name.as_str())
                                    .unwrap_or("Camera"),
                            )
                            .show_ui(ui, |ui| {
                                for c in &self.doc.extras.cameras {
                                    if ui
                                        .selectable_label(Some(c.id) == selected, &c.name)
                                        .clicked()
                                    {
                                        self.feature.selected_camera = Some(c.id);
                                        self.frame = c.start;
                                    }
                                }
                            });
                    }
                    help(ui);
                });
                let index = self
                    .feature
                    .selected_camera
                    .and_then(|id| self.doc.extras.cameras.iter().position(|c| c.id == id))
                    .or_else(|| {
                        self.doc.extras.cameras.iter().position(|c| {
                            self.frame >= c.start && self.frame < c.start + c.duration
                        })
                    });
                let mut position = index.map_or(0., |i| {
                    crate::animation::camera_optics(&self.doc.extras.cameras[i], self.frame)
                        .position
                });
                ui.horizontal(|ui| {
                    label(ui, Icon::Depth, "Posición de cámara");
                    if ui
                        .add_enabled(
                            index.is_some(),
                            egui::DragValue::new(&mut position).range(-1e6..=1e6),
                        )
                        .changed()
                    {
                        self.checkpoint();
                        let camera = &mut self.doc.extras.cameras[index.unwrap()];
                        let mut key = crate::animation::camera_at(camera, self.frame);
                        let mut optics = key.optics(camera);
                        optics.position = position;
                        key.optics = Some(optics);
                        key.frame = self
                            .frame
                            .saturating_sub(camera.start)
                            .min(camera.duration - 1);
                        crate::animation::camera_record(camera, key);
                        self.recover();
                    }
                });
                let mut keep = self.doc.layers.iter().all(|l| l.options.depth.keep_size)
                    && self.video_depth_rows().iter().all(|(_, _, p)| p.keep_size);
                if ui.checkbox(&mut keep, tr("Mantener tamaño")).changed() {
                    self.checkpoint();
                    for l in &mut self.doc.layers {
                        l.options.depth.keep_size = keep;
                    }
                    for track in &mut self.doc.extras.media.tracks {
                        track.depth.keep_size = keep;
                    }
                    self.recover();
                }
                ui.small(tr("Vista lateral · distancias desde la cámara"));
                let (rect, _) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), 170.),
                    egui::Sense::hover(),
                );
                self.depth_diagram(ui, rect, position);
                egui::ScrollArea::vertical()
                    .max_height(250.)
                    .show(ui, |ui| {
                        let mut edit = None;
                        for (i, l) in self.doc.layers.iter().enumerate() {
                            ui.horizontal(|ui| {
                                label(ui, Icon::Layers, &l.name);
                                let mut value = l.options.depth.distance - position;
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut value)
                                            .range(1. ..=1e6)
                                            .prefix("↔ "),
                                    )
                                    .on_hover_text(tr("Distancia a la cámara"))
                                    .changed()
                                {
                                    edit = Some((i, (value + position).clamp(1., 1e6)));
                                }
                                if !l.visible {
                                    label(ui, Icon::EyeOff, "Oculta");
                                }
                            });
                        }
                        for (index, name, mut plane) in self.video_depth_rows() {
                            ui.horizontal(|ui| {
                                label(ui, Icon::Film, &name);
                                let mut distance = plane.distance - position;
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut distance)
                                            .range(1. ..=1e6)
                                            .prefix("↔ "),
                                    )
                                    .changed()
                                {
                                    plane.distance = (distance + position).clamp(1., 1e6);
                                    self.set_video_depth(index, plane);
                                }
                            });
                        }
                        if let Some((i, value)) = edit {
                            self.checkpoint();
                            self.doc.layers[i].options.depth.distance = value;
                            self.recover();
                        }
                    });
                ui.separator();
                label(ui, Icon::Eye, "Previsualización de cámara");
                if let Some(texture) = self
                    .depth
                    .preview
                    .as_ref()
                    .filter(|_| !self.depth.hide_blur)
                {
                    ui.add(
                        egui::Image::new(texture).max_size(Vec2::new(ui.available_width(), 230.)),
                    );
                } else {
                    ui.small(tr("Activa la cámara para previsualizar el resultado"));
                }
            });
        self.depth.open = open;
    }
    fn depth_diagram(&self, ui: &egui::Ui, rect: Rect, camera: f32) {
        let p = ui.painter();
        p.rect_filled(rect, 5., crate::themes::panel());
        let x = rect.left() + 38.;
        let mid = rect.center().y;
        design::draw(
            p,
            Rect::from_center_size(Pos2::new(x, mid), Vec2::splat(26.)),
            Icon::Camera,
            crate::themes::accent(),
        );
        let max = self
            .doc
            .layers
            .iter()
            .map(|l| (l.options.depth.distance - camera).max(1.))
            .fold(100., f32::max);
        p.line_segment(
            [Pos2::new(x + 18., mid), Pos2::new(rect.right() - 15., mid)],
            Stroke::new(1., crate::themes::muted()),
        );
        for (i, l) in self.doc.layers.iter().enumerate() {
            let distance = l.options.depth.distance - camera;
            let xx = x + 35. + distance.max(0.) / max * (rect.width() - 105.);
            let y = rect.top() + 25. + (i % 4) as f32 * 24.;
            let color = if i == self.layer {
                crate::themes::accent()
            } else {
                Color32::from_rgb(115, 153, 180)
            };
            p.line_segment(
                [Pos2::new(xx, y), Pos2::new(xx, rect.bottom() - 18.)],
                Stroke::new(2., color),
            );
            p.text(
                Pos2::new(xx + 4., y),
                egui::Align2::LEFT_TOP,
                format!("{} · {:.0}", l.name, distance),
                egui::FontId::proportional(11.),
                color,
            );
        }
    }
}
// All channels are premultiplied: transparent pixels never acquire dark halos.
pub fn blur(pixels: Vec<u8>, w: u32, h: u32, radius: f32, optics: &Optics) -> Vec<u8> {
    if radius < 0.1 {
        return pixels;
    }
    let factor = match optics.quality {
        Quality::Low => 0.25,
        Quality::Medium => 0.5,
        Quality::High => 1.,
    };
    let image = image::RgbaImage::from_raw(w, h, pixels).unwrap();
    let (sw, sh) = (
        ((w as f32 * factor).round() as u32).max(1),
        ((h as f32 * factor).round() as u32).max(1),
    );
    let small = if factor < 1. {
        image::imageops::resize(&image, sw, sh, image::imageops::FilterType::Triangle)
    } else {
        image
    };
    let result = if optics.method == BlurMethod::Gaussian {
        image::imageops::blur(&small, (radius * factor * 0.5).max(0.1))
    } else {
        let samples = match optics.quality {
            Quality::Low => 8,
            Quality::Medium => 20,
            Quality::High => 48,
        };
        let offsets: Vec<_> = (0..samples)
            .map(|i| {
                let a = i as f32 * 2.399963;
                let r = ((i as f32 + 0.5) / samples as f32).sqrt() * radius * factor;
                ((a.cos() * r).round() as i32, (a.sin() * r).round() as i32)
            })
            .collect();
        crate::gpu_lens::blur_on_worker(&small, &offsets)
            .unwrap_or_else(|| lens_blur(&small, &offsets))
    };
    if factor < 1. {
        image::imageops::resize(&result, w, h, image::imageops::FilterType::Triangle).into_raw()
    } else {
        result.into_raw()
    }
}
pub(crate) fn lens_blur(image: &image::RgbaImage, offsets: &[(i32, i32)]) -> image::RgbaImage {
    let (w, h) = image.dimensions();
    let stride = w as usize * 4;
    let mut output = image::RgbaImage::new(w, h);
    let draw = |first: usize, rows: &mut [u8]| {
        for (index, out) in rows.chunks_exact_mut(4).enumerate() {
            let x = (index % w as usize) as i32;
            let y = (first + index / w as usize) as i32;
            let mut sum = [0u32; 4];
            for &(dx, dy) in offsets {
                let (xx, yy) = (x + dx, y + dy);
                if xx >= 0 && yy >= 0 && xx < w as i32 && yy < h as i32 {
                    let color = image.get_pixel(xx as u32, yy as u32);
                    for k in 0..4 {
                        sum[k] += u32::from(color[k]);
                    }
                }
            }
            for (dst, value) in out.iter_mut().zip(sum) {
                *dst = (value / offsets.len() as u32) as u8;
            }
        }
    };
    let workers = if u64::from(w) * u64::from(h) * offsets.len() as u64 >= 8_000_000 {
        std::thread::available_parallelism().map_or(1, |n| n.get().min(4))
    } else {
        1
    };
    if workers == 1 {
        draw(0, &mut output);
    } else {
        let rows_per_chunk = (h as usize).div_ceil(workers);
        std::thread::scope(|scope| {
            for (part, rows) in output.chunks_mut(rows_per_chunk * stride).enumerate() {
                scope.spawn(move || draw(part * rows_per_chunk, rows));
            }
        });
    }
    output
}

impl Editor {
    pub(crate) fn prepare_preview(&mut self, range: Option<(usize, usize)>) {
        let last = self.doc.layers[0].frames.len() - 1;
        let (first, end) = range.unwrap_or((0, last));
        let range = (first.min(last), end.clamp(first.min(last), last));
        if self.doc.extras.media.video_intersects(range.0, range.1) {
            self.status = tr("The selected range contains video. Choose a range without video to prepare the preview.").into();
            return;
        }
        self.playing = false;
        self.restart_media_audio();
        self.studio.stage_only = true;
        self.frame = range.0;
        self.depth.prepare = Some(range);
    }

    pub(crate) fn paint_preview_progress(&self, painter: &egui::Painter, area: egui::Rect) {
        let Some(work) = self.depth.work.as_ref() else {
            return;
        };
        if area.width() < 180. || area.height() < 48. {
            return;
        }
        let Some(&frame) = self.depth.requested.get(1) else {
            return;
        };
        let canceled = work.canceled();
        let percent = if canceled {
            0
        } else {
            work.progress.load(std::sync::atomic::Ordering::Relaxed)
        };
        let label = if canceled {
            tr("Waiting to render frame")
        } else if percent < 20 {
            tr("Preparing artwork")
        } else if percent < 88 {
            match work.phase.load(std::sync::atomic::Ordering::Relaxed) {
                1 => tr("Calculating lights"),
                2 => tr("Applying effects"),
                _ => tr("Compositing layers"),
            }
        } else {
            tr("Finishing preview")
        };
        let number = if canceled { self.frame } else { frame };
        let prepared = self
            .depth
            .prepare
            .map(|(first, last)| {
                let ready = self
                    .depth
                    .ready_frames(&self.doc)
                    .filter(|f| (first..=last).contains(f))
                    .count();
                format!(" · {ready}/{}", last - first + 1)
            })
            .unwrap_or_default();
        let text = format!("{label} {} · {percent}%{prepared}", number + 1);
        let font = egui::FontId::proportional(11.);
        let text_color = painter
            .ctx()
            .style_of(painter.ctx().theme())
            .visuals
            .text_color();
        let galley = painter.layout_no_wrap(text, font, text_color);
        let position = area.left_top() + egui::vec2(10., 10.);
        let rect = egui::Rect::from_min_size(position, galley.size() + egui::vec2(16., 12.));
        painter.rect_filled(rect, 4., crate::themes::color(egui::Color32::from_gray(35)));
        painter.galley(position + egui::vec2(8., 4.), galley, text_color);
        let start = rect.left_bottom() + egui::vec2(6., -3.);
        painter.line_segment(
            [
                start,
                start + egui::vec2((rect.width() - 12.) * f32::from(percent) / 100., 0.),
            ],
            egui::Stroke::new(2., crate::themes::accent()),
        );
        painter
            .ctx()
            .request_repaint_after(std::time::Duration::from_millis(100));
    }

    pub(crate) fn stop_preparing_preview(&mut self) {
        self.depth.prepare = None;
        if self.depth.job_is_prefetch {
            self.depth.cancel_job();
        }
    }

    pub(crate) fn preparing_preview(&self) -> bool {
        self.depth.prepare.is_some()
    }

    pub fn camera_depth_view(&self, base: View, layer: usize) -> View {
        let base = if active(self) {
            layer_view(&self.doc, self.frame, layer, base)
        } else {
            base
        };
        crate::folder_groups::view(
            base,
            self.doc
                .layers
                .get(layer)
                .map(|l| crate::folder_groups::layer_matrix(&self.doc, l))
                .unwrap_or(crate::rig::IDENTITY),
        )
    }
    pub fn camera_blur_preview(&mut self, painter: &egui::Painter, stage: View) -> bool {
        let use_camera = active(self) && !self.doc.extras.cameras.is_empty();
        let optics =
            camera(&self.doc, self.frame).map(|c| crate::animation::camera_optics(c, self.frame));
        let allow_blur = (use_camera || self.depth.open) && !self.depth.hide_blur;
        let blur = allow_blur && optics.as_ref().is_some_and(|o| o.strength > 0.);
        let fx = !self.post.hidden
            && self.doc.layers.iter().any(|layer| {
                layer.options.adjustment.is_some() && crate::post::visible_effects(layer)
            });
        let lights = !self.post.hide_lights
            && self
                .doc
                .layers
                .iter()
                .any(|layer| layer.visible && layer.options.light.is_some());
        let solo = (self.post.solo_light && self.doc.layers[self.layer].options.light.is_some())
            .then_some(self.layer);
        let post = fx || lights;
        let quality = self.studio.prefs.viewport_quality;
        let reduced = quality != ViewportQuality::Full;
        if !post && !blur && !reduced {
            if self.depth.prepare.take().is_some() {
                self.status = tr("No composited preview to prepare at the current frame.").into();
            }
            if self.depth.preview_scope.is_some() {
                self.depth.invalidate_preview();
            }
            return false;
        }
        let scope = (
            self.doc.extras.active_scene,
            self.feature.symbol_stack.last().map(|entry| entry.3),
            self.doc.width,
            self.doc.height,
            use_camera,
            fx,
            lights,
            allow_blur,
            solo,
            quality,
        );
        let prepare = self.depth.prepare;
        let scope_changed = self.depth.preview_scope != Some(scope);
        if scope_changed {
            self.depth.invalidate_preview();
            self.depth.preview_scope = Some(scope);
        }
        let longest = self.doc.width.max(self.doc.height) as f32;
        let max = if lights {
            // Keep the light sampling grid fixed while the user zooms the canvas.
            longest.min(2048.)
        } else if post || reduced {
            let physical_zoom = stage.scale * painter.ctx().pixels_per_point();
            let cap = 3072.;
            ((longest * physical_zoom * 1.5 / 128.).ceil() * 128.).clamp(900., cap)
        } else {
            match optics.as_ref().map(|o| o.quality).unwrap_or_default() {
                Quality::Low => 480.,
                Quality::Medium => 800.,
                Quality::High => 1200.,
            }
        };
        let scale = if post || reduced {
            max / longest
        } else {
            (max / longest).min(1.)
        };
        let scale = scale * quality.factor();
        let size = [
            (self.doc.width as f32 * scale).round().max(1.) as usize,
            (self.doc.height as f32 * scale).round().max(1.) as usize,
        ];
        self.depth.sync_document(&self.doc);
        if scope_changed {
            self.depth.prepare = prepare;
        }
        let cache_limit = self.studio.prefs.preview_cache_mib.clamp(128, 1024) * 1048576;
        self.depth.trim_cache(cache_limit);
        let Some(video) = self.depth_video_frames(use_camera || blur) else {
            return false;
        };
        let has_video = !video.is_empty();
        let clock = if self.playing {
            self.depth
                .cache_clock(&self.doc, self.frame, self.animation.clock)
        } else {
            self.frame
        };
        // The scope and document identity are covered by the epoch. The frame
        // and symbol clock can diverge during playback and must both match.
        let mut key = vec![
            self.depth.preview_epoch,
            self.frame,
            clock,
            size[0],
            size[1],
        ];
        key.extend(
            video
                .iter()
                .map(|clip| std::sync::Arc::as_ptr(&clip.pixels) as usize),
        );
        let now = std::time::Instant::now();
        if self
            .depth
            .focus
            .as_ref()
            .is_none_or(|(focus, _)| *focus != key)
        {
            if (self.depth.job_is_prefetch || !self.playing)
                && (self.depth.requested.get(1) != Some(&self.frame)
                    || self.depth.interactive_job && self.depth.requested.get(1..) != key.get(1..))
            {
                self.depth.interactive_job = false;
                self.depth.cancel_job();
            }
            self.depth.focus = Some((key.clone(), now));
        }
        if let Some(job) = &self.depth.job {
            match job.try_recv() {
                Ok((done, pixels, size)) => {
                    self.depth.work = None;
                    self.depth.job_is_prefetch = false;
                    let interactive = std::mem::take(&mut self.depth.interactive_job);
                    // Publish intermediate results for the same shot while the
                    // user keeps dragging. Only exact results enter the cache.
                    if interactive
                        && !self.playing
                        && done != key
                        && done.get(1..) == key.get(1..)
                        && !pixels.is_empty()
                    {
                        self.depth.preview = Some(preview_texture(painter.ctx(), size, &pixels));
                        self.depth.shown.clone_from(&done);
                        self.depth.held_preview = true;
                    }
                    if done.first() == Some(&self.depth.preview_epoch) && !pixels.is_empty() {
                        let show = done == key;
                        let texture = (show
                            || (!has_video
                                && self.depth.texture_bytes() + size[0] * size[1] * 4
                                    <= PREVIEW_TEXTURE_BYTES))
                            .then(|| preview_texture(painter.ctx(), size, &pixels));
                        if show {
                            self.depth.held_preview = false;
                            self.depth.preview = texture.clone();
                            self.depth.shown.clone_from(&done);
                        }
                        if !has_video && pixels.len() <= cache_limit {
                            self.depth.cached.retain(|frame| frame.key != done);
                            self.depth.cached.push_back(CachedPreview {
                                key: done,
                                pixels,
                                size,
                                texture,
                            });
                            self.depth.trim_cache(cache_limit);
                            self.depth.trim_textures();
                        }
                    }
                    self.depth.job = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.depth.job = None;
                    self.depth.work = None;
                    self.depth.job_is_prefetch = false;
                    self.depth.requested.clear();
                    self.depth.interactive_job = false;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        self.depth.show_cached(&key, painter.ctx());
        if self.depth.shown == key {
            self.depth.held_preview = false;
        }
        let mut next = None;
        if self.depth.job.is_none() {
            if self.depth.shown != key {
                next = Some((key.clone(), false));
            } else if let Some((first, last)) =
                self.depth.prepare.filter(|_| !self.playing && !has_video)
            {
                let capacity = cache_limit / (size[0] * size[1] * 3);
                let last_fit = last.min(first + capacity.saturating_sub(1));
                next = (first..=last_fit).find_map(|frame| {
                    let candidate = vec![self.depth.preview_epoch, frame, frame, size[0], size[1]];
                    (!self
                        .depth
                        .cached
                        .iter()
                        .any(|cached| cached.key == candidate))
                    .then_some((candidate, true))
                });
                if next.is_none() {
                    self.depth.prepare = None;
                    self.status = tr(if last_fit == last {
                        "Preview ready."
                    } else {
                        "Preview cache full. Increase its memory or prepare a shorter range."
                    })
                    .into();
                }
            } else if !self.playing
                && !self.doc.extras.media.has_video_tracks()
                && (use_camera || self.doc.extras.cameras.is_empty())
                && (post || reduced)
            {
                let idle = now.duration_since(self.depth.focus.as_ref().unwrap().1);
                let capacity = (cache_limit / (size[0] * size[1] * 3)).saturating_sub(1);
                let end = self
                    .loop_range()
                    .filter(|&(start, end)| (start..=end).contains(&self.frame))
                    .map_or(self.doc.layers[0].frames.len() - 1, |(_, end)| end);
                let limit = end.min(self.frame + capacity.min(PREVIEW_LOOKAHEAD));
                if self.frame < limit {
                    if idle < PREVIEW_IDLE {
                        painter.ctx().request_repaint_after(PREVIEW_IDLE - idle);
                    } else {
                        next = (self.frame + 1..=limit).find_map(|frame| {
                            let candidate =
                                vec![self.depth.preview_epoch, frame, frame, size[0], size[1]];
                            (!self
                                .depth
                                .cached
                                .iter()
                                .any(|cached| cached.key == candidate))
                            .then_some((candidate, true))
                        });
                    }
                }
            }
        }
        if let Some((key, prefetch)) = next {
            let doc = self
                .depth
                .render_doc
                .get_or_insert_with(|| {
                    let mut doc =
                        crate::animation::scene_doc(&self.doc, self.doc.extras.active_scene);
                    if !lights || !fx || solo.is_some() {
                        for (index, layer) in doc.layers.iter_mut().enumerate() {
                            if !crate::post::is_adjustment(layer) {
                                continue;
                            }
                            let use_light = lights
                                && solo.is_none_or(|selected| selected == index)
                                && layer.options.light.is_some();
                            let use_fx = fx && layer.options.adjustment.is_some();
                            if !use_light && !use_fx {
                                layer.visible = false;
                            } else {
                                if !use_light {
                                    layer.options.light = None;
                                }
                                if !use_fx {
                                    layer.options.adjustment = None;
                                }
                            }
                        }
                    }
                    doc
                })
                .clone();
            self.depth.requested = key.clone();
            let frame = key[1];
            let clock = key[2];
            let render_camera = use_camera
                || (allow_blur
                    && camera(&doc, frame)
                        .is_some_and(|c| crate::animation::camera_optics(c, frame).strength > 0.));
            let work = std::sync::Arc::new(PreviewWork::default());
            self.depth.work = Some(work.clone());
            self.depth.job_is_prefetch = prefetch;
            let (tx, rx) = std::sync::mpsc::channel();
            self.depth.job = Some(rx);
            let ctx = painter.ctx().clone();
            let submitted = key.clone();
            let render_cache = self.depth.render_cache.clone();
            let gpu = self.depth.gpu.clone();
            std::thread::spawn(move || {
                crate::gpu_lens::use_on_worker(gpu);
                let render = egui::Context::default();
                let mut cache = render_cache
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                let pixels = crate::exporter::raster_preview(
                    &doc,
                    frame,
                    clock,
                    size[0] as u32,
                    size[1] as u32,
                    &render,
                    &mut cache,
                    &video,
                    render_camera,
                    post,
                    allow_blur,
                    !reduced,
                    Some(&work),
                );
                work.report(100);
                let _ = tx.send((submitted, pixels, size));
                ctx.request_repaint();
            });
        }
        let hold =
            !self.playing && self.depth.held_preview && self.depth.shown.get(1..) == key.get(1..);
        if (!use_camera && !post && !reduced) || (self.depth.shown != key && !hold) {
            return false;
        }
        let Some(texture) = &self.depth.preview else {
            return false;
        };
        let corners = [
            Pos2::ZERO,
            Pos2::new(self.doc.width as f32, 0.),
            Pos2::new(self.doc.width as f32, self.doc.height as f32),
            Pos2::new(0., self.doc.height as f32),
        ];
        let mut mesh = egui::Mesh::with_texture(texture.id());
        for (p, uv) in corners.into_iter().zip([
            Pos2::new(0., 0.),
            Pos2::new(1., 0.),
            Pos2::new(1., 1.),
            Pos2::new(0., 1.),
        ]) {
            mesh.vertices.push(egui::epaint::Vertex {
                pos: stage.screen(p),
                uv,
                color: Color32::WHITE,
            });
        }
        mesh.indices = vec![0, 1, 2, 0, 2, 3];
        painter.add(mesh);
        true
    }
}
impl Editor {
    pub(crate) fn depth_point(&self, point: Pos2, from: usize, to: usize) -> Pos2 {
        if from == to {
            return point;
        }
        let base = View {
            center: Pos2::ZERO,
            stage_center: Pos2::ZERO,
            scale: 1.,
            angle: 0.,
        };
        let a = self.camera_depth_view(base, from);
        let b = self.camera_depth_view(base, to);
        b.world(a.screen(point))
    }
    pub(crate) fn depth_mark(&self, mark: &Mark, from: usize, to: usize) -> Mark {
        if from == to {
            return mark.clone();
        }
        let mut result = crate::vector::transform(mark, |p| self.depth_point(p, from, to));
        let origin = self.depth_point(Pos2::ZERO, from, to);
        result.width *= self
            .depth_point(Pos2::new(1., 0.), from, to)
            .distance(origin);
        result
    }
    pub(crate) fn depth_hit(&self, layer: usize, mark: &Mark, point: Pos2, tolerance: f32) -> bool {
        if mark.style.raster_tile
            || !crate::features::object_visible(&self.doc.layers[layer], mark, self.frame)
        {
            return false;
        }
        crate::folder_groups::layer_active(&self.doc, &self.doc.layers[layer], self.frame)
            && self.selection_hit(mark, self.depth_point(point, self.layer, layer), tolerance)
    }
}

pub fn visible(plane: &LayerDepth, optics: &Optics) -> bool {
    plane.distance - optics.position >= 1.
}
#[cfg(test)]
mod tests {
    use super::*;
    fn doc() -> Document {
        let mut d = crate::fresh_document();
        d.width = 96;
        d.height = 64;
        d.extras.cameras.push(crate::features::Camera {
            optics: Optics::default(),
            animation: Default::default(),
            id: 1,
            name: "Camera".into(),
            start: 0,
            duration: 24,
            center: [48., 32.],
            zoom: 1.,
            angle: 0.,
        });
        d
    }
    fn square(x: f32, color: [u8; 4]) -> Mark {
        Mark {
            points: vec![[x, 20.], [x + 8., 44.]].into(),
            rectangle: true,
            closed: true,
            stroke: false,
            fill: Some(color),
            ..Default::default()
        }
    }
    #[test]
    fn default_depth_keeps_legacy_projection_and_maintain_size_preserves_art() {
        let mut d = doc();
        let mark = square(30., [0, 0, 0, 255]);
        d.layers[0].frames[0].push(mark.clone());
        let original = crate::features::resolve_export(&d, 0, true).remove(0).2;
        assert!(
            crate::vector::control_bounds(&original)
                .center()
                .distance(crate::vector::control_bounds(&mark).center())
                < 0.001
        );
        for distance in [200., 500., 2000.] {
            d.layers[0].options.depth.distance = distance;
            let m = crate::features::resolve_export(&d, 0, true).remove(0).2;
            assert!((crate::vector::control_bounds(&m).width() - 8.).abs() < 0.001);
        }
        d.layers[0].options.depth.keep_size = false;
        let m = crate::features::resolve_export(&d, 0, true).remove(0).2;
        assert!((crate::vector::control_bounds(&m).width() - 4.).abs() < 0.001);
    }
    #[test]
    fn nearby_layers_move_more_and_editor_projection_matches_export() {
        let mut d = doc();
        let mark = square(30., [0, 0, 0, 255]);
        d.extras.cameras[0].center[0] += 10.;
        let pose = crate::animation::camera_at(&d.extras.cameras[0], 0);
        let base = View {
            center: Pos2::new(48., 32.),
            stage_center: crate::vector::pos(pose.center),
            scale: pose.zoom,
            angle: -pose.angle,
        };
        let mut movement = vec![];
        for distance in [200., 1000., 2000.] {
            d.layers[0].options.depth.distance = distance;
            d.layers[0].frames[0] = vec![mark.clone()].into();
            let m = crate::features::resolve_export(&d, 0, true).remove(0).2;
            let projected = crate::vector::flatten(&m, 0.1)[0];
            let preview = layer_view(&d, 0, 0, base).screen(Pos2::new(30., 20.));
            assert!(preview.distance(projected) < 0.001);
            movement.push((preview.x - 30.).abs());
        }
        assert!(movement[0] > movement[1] && movement[1] > movement[2]);
        d.extras.cameras[0].optics.position = 3000.;
        assert!(crate::features::resolve_export(&d, 0, true).is_empty());
    }
    #[test]
    fn focus_interval_is_inclusive_and_transitions_continuously() {
        let o = Optics {
            focus: 200.,
            range: 100.,
            strength: 100.,
            ..Default::default()
        };
        for d in [100., 200., 300.] {
            assert_eq!(o.radius(d), 0.);
        }
        assert!(o.radius(301.) > 0. && o.radius(301.) < o.radius(310.));
        assert!(o.radius(310.) < o.radius(350.));
        assert_eq!(o.radius(0.), o.radius(400.));
    }
    #[test]
    fn both_blur_methods_preserve_premultiplied_color_and_output_size() {
        for method in [BlurMethod::Gaussian, BlurMethod::Lens] {
            for quality in [Quality::Low, Quality::Medium, Quality::High] {
                let mut pixels = vec![0; 32 * 32 * 4];
                for y in 12..20 {
                    for x in 12..20 {
                        pixels[(y * 32 + x) * 4] = 255;
                        pixels[(y * 32 + x) * 4 + 3] = 255;
                    }
                }
                let o = Optics {
                    method,
                    quality,
                    ..Default::default()
                };
                let blurred = blur(pixels, 32, 32, 6., &o);
                assert_eq!(blurred.len(), 32 * 32 * 4);
                assert!(
                    blurred
                        .chunks_exact(4)
                        .all(|p| p[0] == p[3] && p[1] == 0 && p[2] == 0)
                );
                assert!(blurred[(16 * 32 + 10) * 4 + 3] > 0);
            }
        }
    }
    #[test]
    fn depth_and_optics_roundtrip_and_old_projects_keep_defaults() {
        let mut d = doc();
        d.layers[0].options.depth.distance = 200.;
        d.extras.cameras[0].optics = Optics {
            focus: 200.,
            strength: 60.,
            method: BlurMethod::Lens,
            ..Default::default()
        };
        let json = serde_json::to_vec(&d).unwrap();
        let restored: Document = serde_json::from_slice(&json).unwrap();
        assert!(restored.validate().is_ok());
        assert!(restored.layers[0].options.depth == d.layers[0].options.depth);
        assert!(restored.extras.cameras[0].optics == d.extras.cameras[0].optics);
        let fla = crate::fla::encode(&d).unwrap();
        let restored = crate::fla::decode(&fla).unwrap();
        assert_eq!(restored.layers[0].options.depth.distance, 200.);
        assert_eq!(restored.extras.cameras[0].optics.focus, 200.);
        let mut legacy = serde_json::to_value(&d).unwrap();
        legacy["layers"][0]["options"]
            .as_object_mut()
            .unwrap()
            .remove("depth");
        legacy["extras"]["cameras"][0]
            .as_object_mut()
            .unwrap()
            .remove("optics");
        let restored: Document = serde_json::from_value(legacy).unwrap();
        assert!(restored.layers[0].options.depth.keep_size);
        assert_eq!(restored.extras.cameras[0].optics.strength, 0.);
    }
    #[test]
    fn export_blurs_only_unfocused_planes_and_svg_has_layer_filters() {
        let mut d = doc();
        d.layers[0].frames[0].push(square(20., [0, 0, 0, 255]));
        let mut layer = d.layers[0].clone();
        layer.name = "Focus".into();
        layer.options.depth.distance = 200.;
        layer.frames[0] = vec![square(70., [0, 0, 0, 255])].into();
        d.layers.push(layer);
        let ctx = egui::Context::default();
        let mut cache = crate::paint::RenderCache::default();
        let sharp = crate::exporter::raster(&d, 0, 96, 64, &ctx, &mut cache);
        for method in [BlurMethod::Gaussian, BlurMethod::Lens] {
            d.extras.cameras[0].optics = Optics {
                focus: 200.,
                range: 100.,
                strength: 50.,
                quality: Quality::High,
                method,
                ..Default::default()
            };
            let pixels = crate::exporter::raster(&d, 0, 96, 64, &ctx, &mut cache);
            assert_eq!(pixels.len(), 96 * 64 * 3);
            assert!(pixels[(32 * 96 + 17) * 3] < sharp[(32 * 96 + 17) * 3]);
            assert_eq!(pixels[(32 * 96 + 74) * 3], sharp[(32 * 96 + 74) * 3]);
            assert!(pixels[(32 * 96 + 69) * 3].abs_diff(sharp[(32 * 96 + 69) * 3]) <= 1);
            let svg = d.svg(0);
            assert!(svg.contains("dof-0"));
            assert!(!svg.contains("dof-1"));
        }
    }
    #[test]
    fn post_preview_draws_live_artwork_instead_of_the_previous_frame_while_pending() {
        let ctx = egui::Context::default();
        let mut e = Editor::new(&eframe::CreationContext::_new_kittest(ctx.clone()));
        e.doc = doc();
        let mut layer = e.doc.layers[0].clone();
        layer.options.adjustment =
            Some(vec![crate::post::Effect::new(crate::post::Kind::Hue)].into());
        e.doc.layers.push(layer);
        let view = View {
            center: Pos2::ZERO,
            scale: 1.,
            stage_center: Pos2::ZERO,
            angle: 0.,
        };
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            e.camera_blur_preview(ui.painter(), view);
            let (sender, receiver) = std::sync::mpsc::channel();
            e.depth.job = Some(receiver);
            sender
                .send((e.depth.requested.clone(), vec![0, 0, 0], [1, 1]))
                .unwrap();
            assert!(e.camera_blur_preview(ui.painter(), view));
            let texture = e.depth.preview.as_ref().unwrap().id();
            let (_pending, receiver) = std::sync::mpsc::channel();
            e.depth.job = Some(receiver);
            let old_work = std::sync::Arc::new(PreviewWork::default());
            e.depth.work = Some(old_work.clone());
            e.frame = 1;
            assert!(!e.camera_blur_preview(ui.painter(), view));
            assert_eq!(e.depth.preview.as_ref().unwrap().id(), texture);
            assert!(old_work.canceled());
            e.frame = 2;
            assert!(!e.camera_blur_preview(ui.painter(), view));
            e.frame = 3;
            assert!(!e.camera_blur_preview(ui.painter(), view));
            drop(_pending);
            assert!(!e.camera_blur_preview(ui.painter(), view));
            assert_eq!(e.depth.requested[1], 3);
            e.post.hidden = true;
            assert!(!e.camera_blur_preview(ui.painter(), view));
        });
        output.textures_delta.clear();
    }

    #[test]
    fn compositing_edits_keep_displaying_completed_previews_while_dragging() {
        let ctx = egui::Context::default();
        let mut e = Editor::new(&eframe::CreationContext::_new_kittest(ctx.clone()));
        e.doc = doc();
        e.frame = e.doc.layers[0].frames.len() - 1;
        let mut layer = e.doc.layers[0].clone();
        layer.options.adjustment =
            Some(vec![crate::post::Effect::new(crate::post::Kind::Hue)].into());
        e.doc.layers.push(layer);
        let view = View {
            center: Pos2::ZERO,
            scale: 1.,
            stage_center: Pos2::ZERO,
            angle: 0.,
        };
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let complete = |e: &mut Editor| {
                let (tx, rx) = std::sync::mpsc::channel();
                tx.send((e.depth.requested.clone(), vec![30, 60, 90], [1, 1]))
                    .unwrap();
                e.depth.job = Some(rx);
            };
            let preview = |e: &mut Editor| e.camera_blur_preview(ui.painter(), view);
            preview(&mut e);
            complete(&mut e);
            assert!(preview(&mut e));
            let first = e.depth.preview.as_ref().unwrap().id();
            e.doc.layers[1].options.adjustment =
                Some(vec![crate::post::Effect::new(crate::post::Kind::Curves)].into());
            assert!(preview(&mut e));
            assert_eq!(e.depth.preview.as_ref().unwrap().id(), first);
            let work = e.depth.work.as_ref().unwrap().clone();
            complete(&mut e);
            e.doc.layers[1].options.adjustment =
                Some(vec![crate::post::Effect::new(crate::post::Kind::Levels)].into());
            assert!(preview(&mut e));
            assert!(!work.canceled());
            assert_ne!(e.depth.preview.as_ref().unwrap().id(), first);
            assert_ne!(e.depth.shown[0], e.depth.preview_epoch);
            assert_eq!(e.depth.ready_frames(&e.doc).count(), 0);
            e.doc.layers[0].frames[0].push(Mark::default());
            assert!(!preview(&mut e));
        });
        output.textures_delta.clear();
    }

    #[test]
    fn composited_preview_reuses_frames_and_rejects_results_after_editing() {
        let ctx = egui::Context::default();
        let mut e = Editor::new(&eframe::CreationContext::_new_kittest(ctx.clone()));
        e.doc = doc();
        e.studio.stage_only = true;
        let mut layer = e.doc.layers[0].clone();
        layer.options.adjustment =
            Some(vec![crate::post::Effect::new(crate::post::Kind::Hue)].into());
        e.doc.layers.push(layer);
        let view = View {
            center: Pos2::ZERO,
            scale: 1.,
            stage_center: Pos2::ZERO,
            angle: 0.,
        };
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let complete = |e: &mut Editor| {
                let (tx, rx) = std::sync::mpsc::channel();
                tx.send((e.depth.requested.clone(), vec![0, 0, 0], [1, 1]))
                    .unwrap();
                e.depth.job = Some(rx);
            };
            e.camera_blur_preview(ui.painter(), view);
            complete(&mut e);
            e.camera_blur_preview(ui.painter(), view);
            let first = e.depth.preview.as_ref().unwrap().id();
            e.frame = 1;
            e.camera_blur_preview(ui.painter(), view);
            complete(&mut e);
            e.camera_blur_preview(ui.painter(), view);
            let second = e.depth.preview.as_ref().unwrap().id();
            assert_ne!(first, second);

            e.frame = 0;
            e.camera_blur_preview(ui.painter(), view);
            assert_eq!(e.depth.preview.as_ref().unwrap().id(), first);
            assert!(e.depth.job.is_none());
            assert_eq!(e.depth.ready_frames(&e.doc).count(), 2);

            e.frame = e.doc.layers[0].frames.len() - 1;
            e.animation.clock = e.frame;
            e.playing = true;
            e.advance_animation(1);
            assert!(e.animation.clock > e.frame);
            assert!(e.camera_blur_preview(ui.painter(), view));
            assert_eq!(e.depth.preview.as_ref().unwrap().id(), first);
            assert!(e.depth.job.is_none());

            e.frame = 2;
            e.animation.clock = 9;
            e.camera_blur_preview(ui.painter(), view);
            assert_eq!(e.depth.requested[2], 2);
            complete(&mut e);
            e.doc.layers[0].name.push('!');
            e.camera_blur_preview(ui.painter(), view);
            assert_eq!(e.depth.preview.as_ref().unwrap().id(), first);
            assert_eq!(e.depth.ready_frames(&e.doc).count(), 0);
            assert_ne!(e.depth.requested[0], e.depth.shown[0]);
        });
        out.textures_delta.clear();
    }

    #[test]
    fn independent_symbol_uses_its_playback_clock_in_the_preview_key() {
        let mut doc = doc();
        let mut depth = DepthEditor::default();
        depth.sync_document(&doc);
        assert_eq!(depth.cache_clock(&doc, 1, 25), 1);
        let mut mark = Mark::default();
        mark.style.instance = Some(crate::features::Instance {
            clock: crate::features::SymbolClock::Independent,
            symbol: 1,
            matrix: crate::rig::IDENTITY,
            mode: crate::features::Playback::Loop,
            first: 0,
            born: 0,
        });
        doc.layers[0].frames[0].push(mark);
        depth.sync_document(&doc);
        assert_eq!(depth.cache_clock(&doc, 1, 25), 25);
    }

    #[test]
    fn preparing_a_range_fills_earlier_and_later_frames_and_survives_play() {
        let ctx = egui::Context::default();
        let mut e = Editor::new(&eframe::CreationContext::_new_kittest(ctx.clone()));
        e.doc = doc();
        let mut layer = e.doc.layers[0].clone();
        layer.options.adjustment =
            Some(vec![crate::post::Effect::new(crate::post::Kind::Hue)].into());
        e.doc.layers.push(layer);
        e.prepare_preview(Some((3, 5)));
        let view = View {
            center: Pos2::ZERO,
            scale: 1.,
            stage_center: Pos2::ZERO,
            angle: 0.,
        };
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            for frame in 3..=5 {
                e.camera_blur_preview(ui.painter(), view);
                assert_eq!(e.depth.requested[1], frame);
                let (tx, rx) = std::sync::mpsc::channel();
                tx.send((e.depth.requested.clone(), vec![0; 3], [1, 1]))
                    .unwrap();
                e.depth.job = Some(rx);
            }
            e.camera_blur_preview(ui.painter(), view);
            assert!(!e.preparing_preview());
            assert_eq!(e.depth.ready_frames(&e.doc).count(), 3);
            e.playing = true;
            e.frame = 5;
            e.animation.clock = 5;
            e.camera_blur_preview(ui.painter(), view);
            assert_eq!(e.depth.shown[1], 5);
            assert!(e.depth.job.is_none());
            e.depth.invalidate_preview();
            assert_eq!(e.depth.cache_usage(), (0, 0));
        });
        out.textures_delta.clear();
    }

    #[test]
    #[ignore]
    fn benchmark_dbgt_preview_cache() {
        let path = std::env::var_os("LIBRETOON_BENCHMARK_PROJECT")
            .expect("Set LIBRETOON_BENCHMARK_PROJECT to DBGT-Rebuild-v1.ltoon");
        let source = std::fs::read(path).unwrap();
        let ctx = egui::Context::default();
        let mut e = Editor::new(&eframe::CreationContext::_new_kittest(ctx.clone()));
        e.doc = crate::native_project::decode_if_native(&source)
            .unwrap()
            .unwrap();
        e.studio.stage_only = true;
        let full_scene = std::env::var_os("LIBRETOON_BENCHMARK_FULL_SCENE").is_some();
        let count = e.doc.layers[0].frames.len();
        let prepared = if full_scene {
            e.studio.prefs.preview_cache_mib = 512;
            e.prepare_preview(None);
            count
        } else {
            e.lip.preview_range = Some((0, 1));
            2
        };
        let view = View {
            center: Pos2::ZERO,
            scale: 1.,
            stage_center: Pos2::ZERO,
            angle: 0.,
        };
        let tick = |e: &mut Editor| {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                e.camera_blur_preview(ui.painter(), view);
            });
            let uploads = out.textures_delta.set.len();
            out.textures_delta.clear();
            uploads
        };
        let start = std::time::Instant::now();
        tick(&mut e);
        while e.depth.cached.len() < prepared {
            assert!(start.elapsed() < std::time::Duration::from_secs(300));
            std::thread::sleep(std::time::Duration::from_millis(10));
            tick(&mut e);
        }
        let warmup = start.elapsed();
        let start = std::time::Instant::now();
        e.lip.preview_range = None;
        e.playing = true;
        e.frame = count - 1;
        e.animation.clock = e.frame;
        e.advance_animation(1);
        tick(&mut e);
        assert_eq!(e.depth.shown[1], 0);
        assert!(e.depth.job.is_none());
        if full_scene {
            for _ in 0..2 * count {
                e.advance_animation(1);
                tick(&mut e);
                assert_eq!(e.depth.shown[1], e.frame);
                assert!(e.depth.job.is_none());
            }
            eprintln!(
                "DBGT: {count} prepared frames in {warmup:?}; every frame reused across two playback loops."
            );
            return;
        }
        for i in 0..100 {
            e.frame = i % 2;
            e.animation.clock = e.frame;
            assert_eq!(tick(&mut e), 0);
            assert!(e.depth.job.is_none());
        }
        eprintln!(
            "DBGT Full HD warmup {warmup:?}, 100 cached frame switches {:?} (no render jobs or uploads)",
            start.elapsed()
        );
    }

    #[test]
    fn hidden_editor_blur_does_not_schedule_rendering_or_change_export() {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut e = Editor::new(&cc);
        e.doc = doc();
        e.doc.layers[0].frames[0].push(square(20., [0, 0, 0, 255]));
        e.doc.extras.cameras[0].optics.strength = 30.;
        e.studio.stage_only = true;
        e.depth.hide_blur = true;
        let before = serde_json::to_vec(&e.doc).unwrap();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let view = View {
                center: Pos2::ZERO,
                scale: 1.,
                stage_center: Pos2::ZERO,
                angle: 0.,
            };
            assert!(!e.camera_blur_preview(ui.painter(), view));
        });
        out.textures_delta.clear();
        assert!(e.depth.job.is_none());
        assert!(e.depth.preview.is_none());
        assert_eq!(before, serde_json::to_vec(&e.doc).unwrap());
    }
}
#[cfg(test)]
mod integration_tests {
    use super::*;
    #[test]
    fn offstage_art_can_blur_into_the_camera_frame() {
        let mut d = crate::fresh_document();
        d.width = 64;
        d.height = 64;
        d.layers[0].frames[0].push(Mark {
            points: vec![[-10., 20.], [-1., 44.]].into(),
            rectangle: true,
            closed: true,
            stroke: false,
            fill: Some([0, 0, 0, 255]),
            ..Default::default()
        });
        let ctx = egui::Context::default();
        let mut cache = crate::paint::RenderCache::default();
        d.extras.cameras.push(crate::features::Camera {
            optics: Optics {
                strength: 50.,
                focus: 200.,
                quality: Quality::High,
                ..Default::default()
            },
            animation: Default::default(),
            id: 1,
            name: "Camera".into(),
            start: 0,
            duration: 24,
            center: [32., 32.],
            zoom: 1.,
            angle: 0.,
        });
        let pixels = crate::exporter::raster(&d, 0, 64, 64, &ctx, &mut cache);
        assert!(pixels[(32 * 64) * 3] < 250);
    }
    #[test]
    fn depth_window_and_camera_properties_render_on_a_laptop_sized_ui() {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut e = Editor::new(&cc);
        e.doc = crate::fresh_document();
        e.add_camera();
        e.depth.open = true;
        let before = serde_json::to_vec(&e.doc).unwrap();
        for _ in 0..2 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1366., 768.))),
                    ..Default::default()
                },
                |ui| {
                    e.depth_window(ui.ctx());
                    egui::Panel::right("camera-properties-test").show(ui, |ui| {
                        e.camera_properties(ui);
                    });
                },
            );
            assert!(output.shapes.len() > 20);
            output.textures_delta.clear();
        }
        assert_eq!(before, serde_json::to_vec(&e.doc).unwrap());
        assert!(e.doc.layers[0].options.depth.keep_size);
    }
    #[test]
    #[ignore = "manual release benchmark; software compositing, not whole-app FPS"]
    fn benchmark_camera_depth_and_blur() {
        let mut d = crate::fresh_document();
        d.width = 640;
        d.height = 360;
        d.layers[0].frames[0].push(Mark {
            points: vec![[100., 80.], [450., 260.]].into(),
            rectangle: true,
            closed: true,
            stroke: false,
            fill: Some([40, 120, 220, 255]),
            ..Default::default()
        });
        let mut second = d.layers[0].clone();
        second.options.depth.distance = 200.;
        d.layers.push(second);
        d.extras.cameras.push(crate::features::Camera {
            optics: Optics {
                strength: 60.,
                focus: 200.,
                ..Default::default()
            },
            animation: Default::default(),
            id: 1,
            name: "Camera".into(),
            start: 0,
            duration: 24,
            center: [320., 180.],
            zoom: 1.,
            angle: 0.,
        });
        for method in [BlurMethod::Gaussian, BlurMethod::Lens] {
            for quality in [Quality::Low, Quality::Medium, Quality::High] {
                d.extras.cameras[0].optics.method = method;
                d.extras.cameras[0].optics.quality = quality;
                let ctx = egui::Context::default();
                let mut cache = crate::paint::RenderCache::default();
                let start = std::time::Instant::now();
                for _ in 0..3 {
                    std::hint::black_box(crate::exporter::raster(
                        &d, 0, 640, 360, &ctx, &mut cache,
                    ));
                }
                eprintln!(
                    "640x360, 2 layers, {} / {}: {:?}/frame",
                    if method == BlurMethod::Gaussian {
                        "Gaussian"
                    } else {
                        "Lens"
                    },
                    match quality {
                        Quality::Low => "low",
                        Quality::Medium => "medium",
                        Quality::High => "high",
                    },
                    start.elapsed() / 3
                );
            }
        }
    }
}
#[cfg(test)]
mod focus_key_tests {
    use super::*;
    fn project() -> Document {
        let mut d = crate::fresh_document();
        d.width = 96;
        d.height = 64;
        let mut camera = crate::features::Camera {
            optics: Optics {
                focus: 200.,
                range: 100.,
                strength: 50.,
                ..Default::default()
            },
            animation: Default::default(),
            id: 1,
            name: "Camera".into(),
            start: 0,
            duration: 24,
            center: [48., 32.],
            zoom: 1.,
            angle: 0.,
        };
        let a = crate::animation::camera_at(&camera, 0);
        crate::animation::camera_record(&mut camera, a);
        let mut b = crate::animation::camera_at(&camera, 20);
        b.frame = 20;
        b.optics.as_mut().unwrap().focus = 1000.;
        crate::animation::camera_record(&mut camera, b);
        d.extras.cameras.push(camera);
        d
    }
    #[test]
    fn keys_interpolate_all_numeric_optics_and_step_method_and_quality() {
        let mut d = project();
        let c = &mut d.extras.cameras[0];
        let end = c.animation.keys[1].optics.as_mut().unwrap();
        end.position = 100.;
        end.range = 40.;
        end.strength = 90.;
        end.method = BlurMethod::Lens;
        end.quality = Quality::High;
        let middle = crate::animation::camera_optics(c, 10);
        assert_eq!(
            (middle.focus, middle.range, middle.position, middle.strength),
            (600., 70., 50., 70.)
        );
        assert!(middle.method == BlurMethod::Gaussian && middle.quality == Quality::Medium);
        let end = crate::animation::camera_optics(c, 20);
        assert!(end.method == BlurMethod::Lens && end.quality == Quality::High);
        let restored: Document = serde_json::from_slice(&serde_json::to_vec(&d).unwrap()).unwrap();
        assert!(crate::animation::camera_optics(&restored.extras.cameras[0], 10) == middle);
        let restored = crate::fla::decode(&crate::fla::encode(&d).unwrap()).unwrap();
        assert!(crate::animation::camera_optics(&restored.extras.cameras[0], 10) == middle);
        let mut old = serde_json::to_value(&d).unwrap();
        for k in old["extras"]["cameras"][0]["animation"]["keys"]
            .as_array_mut()
            .unwrap()
        {
            k.as_object_mut().unwrap().remove("optics");
        }
        let old: Document = serde_json::from_value(old).unwrap();
        assert_eq!(
            crate::animation::camera_optics(&old.extras.cameras[0], 10).focus,
            200.
        );
    }
    #[test]
    fn focus_pull_changes_the_exported_sharp_plane() {
        let mut d = project();
        let rectangle = |x| Mark {
            points: vec![[x, 20.], [x + 8., 44.]].into(),
            rectangle: true,
            closed: true,
            stroke: false,
            fill: Some([0, 0, 0, 255]),
            ..Default::default()
        };
        d.layers[0].frames[0] = vec![rectangle(20.)].into();
        d.layers[0].frames[20] = d.layers[0].frames[0].clone();
        let mut layer = d.layers[0].clone();
        layer.options.depth.distance = 200.;
        layer.frames[0] = vec![rectangle(70.)].into();
        layer.frames[20] = layer.frames[0].clone();
        d.layers.push(layer);
        let ctx = egui::Context::default();
        let mut cache = crate::paint::RenderCache::default();
        let a = crate::exporter::raster(&d, 0, 96, 64, &ctx, &mut cache);
        let b = crate::exporter::raster(&d, 20, 96, 64, &ctx, &mut cache);
        assert!(a[(32 * 96 + 17) * 3] < b[(32 * 96 + 17) * 3]);
        assert!(a[(32 * 96 + 67) * 3] > b[(32 * 96 + 67) * 3]);
    }
    #[test]
    fn splitting_and_trimming_a_camera_preserve_evaluated_focus_and_undo() {
        let ctx = egui::Context::default();
        let mut e = Editor::new(&eframe::CreationContext::_new_kittest(ctx));
        e.doc = project();
        let original = e.doc.extras.cameras[0].clone();
        let expected = crate::animation::camera_optics(&original, 10);
        e.camera_timeline_action(1, 10, 4);
        assert_eq!(e.doc.extras.cameras.len(), 2);
        let right = &e.doc.extras.cameras[1];
        assert!(crate::animation::camera_optics(right, 10) == expected);
        assert_eq!(
            crate::animation::camera_optics(right, 15).focus,
            crate::animation::camera_optics(&original, 15).focus
        );
        e.history(false);
        assert_eq!(e.doc.extras.cameras.len(), 1);
        assert!(crate::animation::camera_optics(&e.doc.extras.cameras[0], 10) == expected);
    }
}
