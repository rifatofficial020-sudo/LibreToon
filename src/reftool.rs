//! Project-owned reference images in independent, persistent floating windows.
use crate::{
    Editor,
    design::{Icon, button},
    features::id,
    i18n::tr,
    shared::SharedVec,
};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Vec2};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    io::Cursor,
    path::Path,
    sync::mpsc,
};
const IMAGE_LIMIT: usize = 16 * 1024 * 1024;
const TOTAL_LIMIT: usize = 32 * 1024 * 1024;
const PIXEL_LIMIT: u64 = 16 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
pub struct Reference {
    pub id: u64,
    pub name: String,
    pub data: SharedVec<u8>,
    pub dimensions: [u32; 2],
    pub position: [f32; 2],
    pub size: [f32; 2],
    #[serde(default)]
    pub zoom: f32,
}
pub fn validate(refs: &[Reference]) -> Result<(), String> {
    let mut ids = HashSet::new();
    let mut bytes = 0usize;
    let mut pixels = 0u64;
    if refs.len() > 32 {
        return Err("Too many RefTool windows".into());
    }
    for r in refs {
        bytes = bytes
            .checked_add(r.data.len())
            .ok_or("Reference size overflow")?;
        pixels += r.dimensions[0] as u64 * r.dimensions[1] as u64;
        if r.id == 0
            || !ids.insert(r.id)
            || r.name.len() > 1024
            || r.data.len() > IMAGE_LIMIT
            || r.position.iter().any(|v| !v.is_finite() || v.abs() > 1e6)
            || r.size
                .iter()
                .any(|v| !v.is_finite() || !(100. ..=16384.).contains(v))
            || !r.zoom.is_finite()
            || !(0. ..=32.).contains(&r.zoom)
            || r.dimensions.iter().any(|v| *v > 8192)
            || ((r.dimensions == [0, 0]) != r.data.is_empty())
            || (!r.data.is_empty() && r.dimensions.contains(&0))
        {
            return Err("Invalid RefTool reference".into());
        }
    }
    if bytes > TOTAL_LIMIT || pixels > PIXEL_LIMIT {
        return Err("RefTool image budget exceeded".into());
    }
    Ok(())
}
struct Loaded {
    data: SharedVec<u8>,
    image: image::RgbaImage,
    name: String,
}
fn decode(data: SharedVec<u8>, name: String) -> Result<Loaded, String> {
    if data.len() > IMAGE_LIMIT {
        return Err(tr("The reference image exceeds 16 MiB.").into());
    }
    let mut reader = image::ImageReader::new(Cursor::new(data.as_slice()))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().map_err(|e| e.to_string())?;
    if image.width() as u64 * image.height() as u64 > PIXEL_LIMIT {
        return Err(tr("The reference images exceed the pixel budget.").into());
    }
    Ok(Loaded {
        data,
        image: image.to_rgba8(),
        name,
    })
}
fn load(path: &Path) -> Result<Loaded, String> {
    if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > IMAGE_LIMIT as u64 {
        return Err(tr("The reference image exceeds 16 MiB.").into());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    decode(
        bytes.into(),
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .chars()
            .take(250)
            .collect(),
    )
}
struct Cached {
    owner: SharedVec<u8>,
    pixels: image::RgbaImage,
    texture: egui::TextureHandle,
}
struct Job {
    id: u64,
    generation: u64,
    replace: bool,
    rx: mpsc::Receiver<Result<Loaded, String>>,
}
struct Layout {
    position: [f32; 2],
    size: [f32; 2],
    nonce: u64,
}
#[derive(Default)]
pub struct State {
    generation: u64,
    job: Option<Job>,
    cache: HashMap<u64, Cached>,
    errors: HashMap<u64, String>,
    layouts: HashMap<u64, Layout>,
    transactions: HashSet<u64>,
    window_gestures: HashSet<u64>,
}
impl State {
    pub fn reset_document(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.cache.clear();
        self.errors.clear();
        self.layouts.clear();
        self.transactions.clear();
        self.window_gestures.clear();
    }
    pub(crate) fn importing(&self) -> bool {
        self.job
            .as_ref()
            .is_some_and(|j| j.replace && j.generation == self.generation)
    }
}
fn sample(image: &image::RgbaImage, rect: Rect, p: Pos2) -> Option<Color32> {
    if !rect.contains(p) || !rect.is_positive() {
        return None;
    }
    let x = (((p.x - rect.left()) / rect.width()) * image.width() as f32)
        .floor()
        .clamp(0., (image.width() - 1) as f32) as u32;
    let y = (((p.y - rect.top()) / rect.height()) * image.height() as f32)
        .floor()
        .clamp(0., (image.height() - 1) as f32) as u32;
    let [r, g, b, a] = image.get_pixel(x, y).0;
    // The image is displayed over white. Sample its visible color at full opacity.
    let channel = |v: u8| ((v as u32 * a as u32 + 255 * (255 - a as u32) + 127) / 255) as u8;
    Some(Color32::from_rgb(channel(r), channel(g), channel(b)))
}

impl Editor {
    fn reference_checkpoint(&mut self) {
        let selected = self.canvas.selected.clone();
        self.checkpoint();
        self.canvas.selected = selected;
    }
    pub(crate) fn new_reference_window(&mut self) {
        if self.doc.extras.references.len() >= 32 {
            self.status = tr("You can open up to 32 RefTool windows per project.").into();
            return;
        }
        if self.hub.active.is_none() {
            self.add_document(crate::fresh_document(), None);
        }
        self.hub.home = false;
        let index = self.doc.extras.references.len();
        self.reference_checkpoint();
        self.doc.extras.references.push(Reference {
            id: id(),
            name: format!("{} {}", tr("Reference"), index + 1),
            data: Default::default(),
            dimensions: [0, 0],
            position: [
                140. + (index % 8) as f32 * 24.,
                120. + (index % 8) as f32 * 24.,
            ],
            size: [360., 360.],
            zoom: 0.,
        });
        self.recover();
    }
    fn reference_job(
        &mut self,
        ctx: &egui::Context,
        id: u64,
        replace: bool,
        work: impl FnOnce() -> Result<Loaded, String> + Send + 'static,
    ) {
        if self.references.job.is_some() {
            return;
        }
        self.references.errors.remove(&id);
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        self.references.job = Some(Job {
            id,
            generation: self.references.generation,
            replace,
            rx,
        });
        if replace {
            self.hub.dirty();
        }
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
                .unwrap_or_else(|_| Err("Reference image decoding failed".into()));
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
    fn poll_references(&mut self, ctx: &egui::Context) {
        let result = self
            .references
            .job
            .as_ref()
            .and_then(|j| match j.rx.try_recv() {
                Ok(r) => Some(r),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Reference worker stopped".into()))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            });
        let Some(result) = result else {
            return;
        };
        let job = self.references.job.take().unwrap();
        if job.generation != self.references.generation {
            return;
        }
        let Some(index) = self
            .doc
            .extras
            .references
            .iter()
            .position(|r| r.id == job.id)
        else {
            return;
        };
        match result {
            Err(e) => {
                self.references.errors.insert(job.id, e);
            }
            Ok(loaded) => {
                if job.replace {
                    let mut candidate = self.doc.extras.references.clone();
                    let r = &mut candidate[index];
                    r.name = loaded.name;
                    r.dimensions = [loaded.image.width(), loaded.image.height()];
                    r.data = loaded.data.clone();
                    r.zoom = 0.;
                    if let Err(e) = validate(&candidate) {
                        self.references.errors.insert(job.id, e);
                        return;
                    }
                    self.reference_checkpoint();
                    self.doc.extras.references = candidate;
                    self.recover();
                } else if self.doc.extras.references[index].dimensions
                    != [loaded.image.width(), loaded.image.height()]
                    || self.doc.extras.references[index].data.identity() != loaded.data.identity()
                {
                    self.references.errors.insert(
                        job.id,
                        "Reference image metadata changed while loading".into(),
                    );
                    return;
                }
                let texture = ctx.load_texture(
                    format!("reference-{}", job.id),
                    egui::ColorImage::from_rgba_unmultiplied(
                        [
                            loaded.image.width() as usize,
                            loaded.image.height() as usize,
                        ],
                        loaded.image.as_raw(),
                    ),
                    egui::TextureOptions::NEAREST,
                );
                self.references.cache.insert(
                    job.id,
                    Cached {
                        owner: loaded.data,
                        pixels: loaded.image,
                        texture,
                    },
                );
            }
        }
    }
    pub(crate) fn reference_windows(&mut self, ctx: &egui::Context) {
        self.poll_references(ctx);
        if self.hub.home {
            return;
        }
        let refs = self.doc.extras.references.clone();
        let ids: HashSet<_> = refs.iter().map(|r| r.id).collect();
        self.references.cache.retain(|id, _| ids.contains(id));
        self.references.layouts.retain(|id, _| ids.contains(id));
        self.references
            .window_gestures
            .retain(|id| ids.contains(id));
        self.references.errors.retain(|id, _| ids.contains(id));
        let mut recover = false;
        for reference in &refs {
            let id = reference.id;
            let layout = self.references.layouts.entry(id).or_insert(Layout {
                position: reference.position,
                size: reference.size,
                nonce: 0,
            });
            if layout.position != reference.position || layout.size != reference.size {
                layout.nonce = layout.nonce.wrapping_add(1);
                layout.position = reference.position;
                layout.size = reference.size;
            }
            let nonce = layout.nonce;
            let mut open = true;
            let mut pick = false;
            let mut load_image = false;
            let mut zoom = reference.zoom;
            let mut sampled = None;
            let ready = self
                .references
                .cache
                .get(&id)
                .is_some_and(|c| c.owner.identity() == reference.data.identity());
            let cached = ready.then(|| self.references.cache.get(&id).unwrap());
            let busy = self.references.job.is_some();
            let picker = self.studio.color_picker;
            let eyedropper = picker.is_some() || self.tool == crate::canvas::Tool::Eyedropper;
            let response = egui::Window::new(format!("RefTool · {}", reference.name))
                .id(egui::Id::unique((
                    "reference",
                    self.references.generation,
                    id,
                    nonce,
                )))
                .open(&mut open)
                .collapsible(false)
                .resizable(true)
                .default_pos(Pos2::new(reference.position[0], reference.position[1]))
                .default_size(Vec2::new(reference.size[0], reference.size[1]))
                .min_size([230., 180.])
                .show(ctx, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        load_image =
                            button(ui, Icon::Open, "Load image…", "Load image…", false, !busy)
                                .clicked();
                        pick = button(
                            ui,
                            Icon::Eyedropper,
                            "",
                            "Pick reference color",
                            eyedropper,
                            ready,
                        )
                        .clicked();
                        if button(ui, Icon::Fit, "", "Fit reference", zoom == 0., ready).clicked() {
                            zoom = 0.;
                        }
                        if ready {
                            let mut percent = if zoom == 0. { 100. } else { zoom * 100. };
                            if ui
                                .add(
                                    egui::DragValue::new(&mut percent)
                                        .range(2. ..=3200.)
                                        .suffix(" %"),
                                )
                                .changed()
                            {
                                zoom = percent / 100.;
                            }
                        }
                    });
                    if let Some(error) = self.references.errors.get(&id) {
                        ui.colored_label(Color32::LIGHT_RED, error);
                    }
                    if let Some(cached) = cached {
                        let available = ui.available_size().max(Vec2::splat(32.));
                        let native =
                            Vec2::new(cached.pixels.width() as f32, cached.pixels.height() as f32);
                        let scale = if zoom == 0. {
                            (available.x / native.x)
                                .min(available.y / native.y)
                                .min(1.)
                                .max(0.001)
                        } else {
                            zoom
                        };
                        egui::ScrollArea::both()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let (rect, response) =
                                    ui.allocate_exact_size(native * scale, Sense::click_and_drag());
                                ui.painter().rect_filled(rect, 0., Color32::WHITE);
                                ui.painter().image(
                                    cached.texture.id(),
                                    rect,
                                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
                                    Color32::WHITE,
                                );
                                if eyedropper && response.hovered() {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                                }
                                if eyedropper
                                    && (response.clicked() || response.is_pointer_button_down_on())
                                    && ui.input(|i| {
                                        i.pointer.primary_down() || i.pointer.primary_released()
                                    })
                                {
                                    if let Some(p) = response.interact_pointer_pos() {
                                        sampled = sample(&cached.pixels, rect, p);
                                    }
                                }
                            });
                    } else {
                        ui.add_space(20.);
                        if reference.data.is_empty() {
                            ui.label(tr("Load an image to use as a reference."));
                        } else if self.references.errors.get(&id).is_none() {
                            ui.spinner();
                            ui.label(tr("Loading reference…"));
                        }
                    }
                });
            if pick {
                self.activate_tool(crate::canvas::Tool::Eyedropper);
            }
            if let Some(color) = sampled {
                if let Some(fill) = picker {
                    self.apply_picker_color(color, fill);
                    self.studio.color_picker = None;
                } else {
                    self.color = color;
                    self.apply_picker_color(color, true);
                    self.stroke_enabled = true;
                }
            }
            if !open {
                self.reference_checkpoint();
                self.doc.extras.references.retain(|r| r.id != id);
                self.references.cache.remove(&id);
                self.references.transactions.remove(&id);
                self.references.window_gestures.remove(&id);
                recover = true;
                continue;
            }
            let rect = response.map(|r| r.response.rect);
            let origin = ctx.input(|i| i.pointer.press_origin());
            let pressed_on_frame = rect.is_some_and(|r| {
                origin.is_some_and(|p| {
                    r.contains(p)
                        && (p.y < r.top() + 32.
                            || p.x < r.left() + 8.
                            || p.x > r.right() - 8.
                            || p.y > r.bottom() - 8.)
                })
            });
            if pressed_on_frame && ctx.input(|i| i.pointer.primary_pressed()) {
                self.references.window_gestures.insert(id);
            }
            let moving = self.references.window_gestures.contains(&id);
            let geometry = rect
                .filter(|_| {
                    moving
                        && ctx.input(|i| i.pointer.primary_down() || i.pointer.primary_released())
                })
                .filter(|r| {
                    r.min
                        .distance(Pos2::new(reference.position[0], reference.position[1]))
                        > 0.5
                        || (r.size() - Vec2::new(reference.size[0], reference.size[1])).length()
                            > 0.5
                });
            if geometry.is_some() || zoom != reference.zoom {
                if self.references.transactions.insert(id) {
                    self.reference_checkpoint();
                }
                if let Some(r) = self.doc.extras.references.iter_mut().find(|r| r.id == id) {
                    if let Some(rect) = geometry {
                        r.position = [rect.left(), rect.top()];
                        r.size = [rect.width(), rect.height()];
                    }
                    r.zoom = zoom;
                    if let Some(layout) = self.references.layouts.get_mut(&id) {
                        layout.position = r.position;
                        layout.size = r.size;
                    }
                }
                if !ctx.input(|i| i.pointer.primary_down()) {
                    self.references.transactions.remove(&id);
                    recover = true;
                }
            }
            if !ctx.input(|i| i.pointer.primary_down()) && self.references.transactions.remove(&id)
            {
                recover = true;
            }
            if !ctx.input(|i| i.pointer.primary_down()) {
                self.references.window_gestures.remove(&id);
            }
            if load_image {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Images", &["png", "jpg", "jpeg", "bmp", "tif", "tiff"])
                    .pick_file()
                {
                    self.reference_job(ctx, id, true, move || load(&path));
                }
            } else if !reference.data.is_empty()
                && !ready
                && !busy
                && !self.references.errors.contains_key(&id)
            {
                let data = reference.data.clone();
                let name = reference.name.clone();
                self.reference_job(ctx, id, false, move || decode(data, name));
            }
        }
        if recover {
            self.recover();
        }
        if self.references.job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn reference() -> Reference {
        let image = image::RgbaImage::from_fn(2, 2, |x, y| {
            image::Rgba([x as u8 * 100, y as u8 * 100, 20, 255])
        });
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        Reference {
            id: 42,
            name: "Reference test".into(),
            data: bytes.into_inner().into(),
            dimensions: [2, 2],
            position: [123., 234.],
            size: [350., 400.],
            zoom: 2.,
        }
    }
    fn editor() -> Editor {
        let mut e = Editor::new(&eframe::CreationContext::_new_kittest(
            egui::Context::default(),
        ));
        e.doc = crate::fresh_document();
        e.recovery = std::env::temp_dir().join(format!(
            "reftool-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ));
        e
    }
    #[test]
    fn embedded_images_and_window_layout_survive_json_and_fla() {
        let mut doc = crate::fresh_document();
        doc.extras.references.push(reference());
        let json = serde_json::to_vec(&doc).unwrap();
        let restored: crate::Document = serde_json::from_slice(&json).unwrap();
        assert!(restored.validate().is_ok());
        let packed = crate::fla::encode(&doc).unwrap();
        let restored = crate::fla::decode(&packed).unwrap();
        assert_eq!(
            serde_json::to_vec(&restored.extras.references).unwrap(),
            serde_json::to_vec(&doc.extras.references).unwrap()
        );
        let r = &restored.extras.references[0];
        let decoded = decode(r.data.clone(), r.name.clone()).unwrap();
        assert_eq!(decoded.image.get_pixel(1, 1).0, [100, 100, 20, 255]);
    }
    #[test]
    fn eyedropper_uses_original_pixels_at_any_display_size_and_opaque_visible_color() {
        let image = image::RgbaImage::from_fn(2, 1, |x, _| {
            if x == 0 {
                image::Rgba([10, 20, 30, 255])
            } else {
                image::Rgba([255, 0, 0, 128])
            }
        });
        for width in [2., 20., 2000.] {
            let rect = Rect::from_min_size(egui::pos2(30., 40.), egui::vec2(width, 100.));
            assert_eq!(
                sample(&image, rect, egui::pos2(30. + width * 0.25, 70.))
                    .unwrap()
                    .to_array(),
                [10, 20, 30, 255]
            );
            assert_eq!(
                sample(&image, rect, egui::pos2(30. + width * 0.75, 70.))
                    .unwrap()
                    .to_array(),
                [255, 127, 127, 255]
            );
            assert!(sample(&image, rect, egui::pos2(29., 70.)).is_none());
        }
    }
    #[test]
    fn invalid_layout_duplicate_ids_and_bad_image_are_rejected() {
        let r = reference();
        assert!(validate(std::slice::from_ref(&r)).is_ok());
        assert!(validate(&[r.clone(), r.clone()]).is_err());
        let mut invalid = r;
        invalid.position[0] = f32::NAN;
        assert!(validate(&[invalid]).is_err());
        assert!(decode(vec![1, 2, 3].into(), "bad".into()).is_err());
    }
    #[test]
    fn stale_worker_and_failed_replacement_do_not_overwrite_reference() {
        let ctx = egui::Context::default();
        let mut e = editor();
        e.doc.extras.references.push(reference());
        let before = serde_json::to_vec(&e.doc.extras.references).unwrap();
        let (tx, rx) = mpsc::channel();
        tx.send(Err("Unsupported image".into())).unwrap();
        e.references.job = Some(Job {
            id: 42,
            generation: e.references.generation,
            replace: true,
            rx,
        });
        e.poll_references(&ctx);
        assert_eq!(
            serde_json::to_vec(&e.doc.extras.references).unwrap(),
            before
        );
        assert!(e.references.errors.contains_key(&42));
        let (tx, rx) = mpsc::channel();
        let r = reference();
        tx.send(Ok(decode(r.data, r.name).unwrap())).unwrap();
        e.references.job = Some(Job {
            id: 42,
            generation: e.references.generation,
            replace: true,
            rx,
        });
        e.references.reset_document();
        assert!(!e.references.importing());
        e.poll_references(&ctx);
        assert!(e.references.cache.is_empty());
        assert_eq!(
            serde_json::to_vec(&e.doc.extras.references).unwrap(),
            before
        );
    }
    #[test]
    fn reference_window_draws_and_eyedropper_samples_without_editing_artwork() {
        let ctx = egui::Context::default();
        let mut e = editor();
        e.hub.home = false;
        let mut r = reference();
        r.zoom = 32.;
        let loaded = decode(r.data.clone(), r.name.clone()).unwrap();
        let texture = ctx.load_texture(
            "reference-ui-test",
            egui::ColorImage::from_rgba_unmultiplied([2, 2], loaded.image.as_raw()),
            egui::TextureOptions::NEAREST,
        );
        let texture_id = texture.id();
        e.references.cache.insert(
            r.id,
            Cached {
                owner: r.data.clone(),
                pixels: loaded.image,
                texture,
            },
        );
        e.doc.extras.references.push(r);
        e.tool = crate::canvas::Tool::Eyedropper;
        let before = serde_json::to_vec(&e.doc).unwrap();
        let mut run = |events: Vec<egui::Event>| {
            ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900., 700.))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    e.reference_windows(ui.ctx());
                },
            )
        };
        let mut rect = Rect::NOTHING;
        for _ in 0..3 {
            let mut output = run(vec![]);
            for shape in &output.shapes {
                if let egui::Shape::Mesh(mesh) = &shape.shape {
                    if mesh.texture_id == texture_id {
                        for vertex in &mesh.vertices {
                            rect.extend_with(vertex.pos);
                        }
                    }
                }
            }
            output.textures_delta.clear();
        }
        assert!(rect.is_positive(), "reference image must be rendered");
        let p = rect.min + rect.size() * 0.75;
        for pressed in [true, false] {
            let mut output = run(vec![
                egui::Event::PointerMoved(p),
                egui::Event::PointerButton {
                    pos: p,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ]);
            output.textures_delta.clear();
        }
        drop(run);
        assert_eq!(e.color.to_array(), [100, 100, 20, 255]);
        assert_eq!(e.fill_color, e.color);
        assert_eq!(
            serde_json::to_vec(&e.doc).unwrap(),
            before,
            "sampling must not recolor selected artwork or move windows"
        );
    }
    #[test]
    fn floating_window_drag_is_persisted_and_close_is_undoable() {
        let ctx = egui::Context::default();
        let mut e = editor();
        e.hub.home = false;
        let mut r = reference();
        r.data = Default::default();
        r.dimensions = [0, 0];
        e.doc.extras.references.push(r);
        let tick = |e: &mut Editor, events: Vec<egui::Event>| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000., 800.))),
                    events,
                    ..Default::default()
                },
                |ui| e.reference_windows(ui.ctx()),
            );
            output.textures_delta.clear();
        };
        for _ in 0..3 {
            tick(&mut e, vec![]);
        }
        let window_id = egui::Id::unique(("reference", e.references.generation, 42u64, 0u64));
        let rect = ctx.memory(|m| m.area_rect(window_id)).unwrap();
        let from = rect.min + egui::vec2(70., 12.);
        let to = from + egui::vec2(180., -110.);
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        tick(
            &mut e,
            vec![egui::Event::PointerMoved(from), button(from, true)],
        );
        tick(&mut e, vec![egui::Event::PointerMoved(to)]);
        tick(&mut e, vec![button(to, false)]);
        let moved = &e.doc.extras.references[0];
        assert!(
            moved.position[0] > rect.left() + 100.,
            "window drag must update saved position"
        );
        let rect = ctx.memory(|m| m.area_rect(window_id)).unwrap();
        let close = egui::pos2(rect.right() - 12., rect.top() + 12.);
        tick(
            &mut e,
            vec![egui::Event::PointerMoved(close), button(close, true)],
        );
        tick(&mut e, vec![button(close, false)]);
        assert!(
            e.doc.extras.references.is_empty(),
            "X must remove the reference"
        );
        e.history(false);
        assert_eq!(e.doc.extras.references.len(), 1);
    }
}
