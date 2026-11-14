use crate::{Editor, canvas::View, features::id, i18n::tr, shared::SharedVec};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Vec2};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

const ASSET_LIMIT: usize = 64 * 1024 * 1024;
const LIBRARY_LIMIT: usize = 256 * 1024 * 1024;
const DEMUXERS: &str =
    "mov,matroska,avi,mp3,wav,ogg,flac,aac,png_pipe,jpeg_pipe,bmp_pipe,tiff_pipe,webp_pipe,image2";
type StagedVideo = std::sync::Arc<MediaFile>;
type VideoFrame = (u64, StagedVideo, image::RgbaImage);

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum Kind {
    Image,
    Video,
    Audio,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Asset {
    pub(crate) id: u64,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) folder: Option<u64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) raster_tile: bool,
    kind: Kind,
    pub(crate) data: SharedVec<u8>,
    size: [u32; 2],
    seconds: f64,
    #[serde(default)]
    peaks: SharedVec<u8>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Clip {
    asset: u64,
    start: usize,
    offset: usize,
    length: usize,
    #[serde(default)]
    transform: VideoTransform,
    #[serde(default)]
    audio: AudioMix,
}

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct AudioMix {
    gain: f32,
    fade_in: usize,
    fade_out: usize,
}
impl Default for AudioMix {
    fn default() -> Self {
        Self {
            gain: 1.,
            fade_in: 0,
            fade_out: 0,
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
struct VideoTransform {
    offset: [f32; 2],
    scale_log2: f32,
    angle: f32,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Track {
    #[serde(default)]
    pub(crate) depth: crate::depth::LayerDepth,
    id: u64,
    #[serde(default)]
    pub(crate) slot: usize,
    name: String,
    kind: Kind,
    clips: SharedVec<Clip>,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    locked: bool,
}

impl Track {
    fn replacement_fits(&self, old: u64, frames: usize) -> bool {
        self.clips
            .iter()
            .filter(|clip| clip.asset == old)
            .all(|clip| clip.offset.saturating_add(clip.length) <= frames)
    }

    fn replace_asset_reference(&mut self, old: u64, new: u64) {
        if self.asset_use_count(old) > 0 {
            for clip in self.clips.iter_mut().filter(|clip| clip.asset == old) {
                clip.asset = new;
            }
        }
    }

    pub(crate) fn asset_use_count(&self, id: u64) -> usize {
        self.clips.iter().filter(|clip| clip.asset == id).count()
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct MediaLibrary {
    pub(crate) assets: SharedVec<Asset>,
    pub(crate) tracks: SharedVec<Track>,
}

impl MediaLibrary {
    fn lip_audio(&self, clip: &Clip) -> Option<crate::lip::Audio> {
        let asset = self
            .assets
            .iter()
            .find(|asset| asset.id == clip.asset && asset.kind == Kind::Audio)?;
        Some(crate::lip::Audio {
            name: format!("{} · {}", asset.name, clip.start + 1),
            data: asset.data.clone(),
            peaks: asset.peaks.clone(),
            start: clip.start,
            offset: clip.offset,
            length: clip.length,
        })
    }

    pub(crate) fn dialogue_clips(&self) -> Vec<crate::lip::Audio> {
        self.tracks
            .iter()
            .filter(|track| track.kind == Kind::Audio)
            .flat_map(|track| track.clips.iter().filter_map(|clip| self.lip_audio(clip)))
            .collect()
    }
}

#[derive(Default)]
pub struct MediaEditor {
    audio_destination: Option<(u64, usize)>,
    importing: Option<(
        PathBuf,
        std::sync::mpsc::Receiver<Vec<Result<Asset, String>>>,
    )>,
    audio: AudioPreview,
    videos: std::collections::HashMap<u64, SceneVideoPreview>,
    scene_video_cache: std::collections::VecDeque<CachedVideoSample>,
    selected_video: Option<u64>,
    waveform_cache: std::collections::HashMap<u64, SharedVec<u8>>,
    waveform_job: Option<std::sync::mpsc::Receiver<(u64, SharedVec<u8>)>>,
    video_drag: Option<(u64, usize, Pos2, VideoTransform)>,
    video_drag_started: bool,
    video_drag_modifiers: egui::Modifiers,
    video_resize_center: Option<Pos2>,
    library_video: Option<(u64, std::time::Instant)>,
    library_job: Option<std::sync::mpsc::Receiver<Option<VideoFrame>>>,
    library_preview: Option<(u64, egui::TextureHandle)>,
    library_file: Option<(u64, StagedVideo)>,
    library_request: Option<(u64, usize)>,
    textures: std::collections::HashMap<u64, egui::TextureHandle>,
}

#[derive(Default)]
struct SceneVideoPreview {
    request: Option<VideoSampleKey>,
    request_owner: Option<SharedVec<u8>>,
    file_owner: Option<SharedVec<u8>>,
    job: Option<std::sync::mpsc::Receiver<Option<VideoFrame>>>,
    file: Option<(u64, StagedVideo)>,
    texture: Option<(u64, egui::TextureHandle)>,
    pixels: Option<std::sync::Arc<image::RgbaImage>>,
    presented: Option<VideoSampleKey>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct VideoSampleKey {
    asset: u64,
    frame: usize,
    fps: u32,
    data: usize,
}

struct CachedVideoSample {
    key: VideoSampleKey,
    _owner: SharedVec<u8>,
    pixels: std::sync::Arc<image::RgbaImage>,
}

impl SceneVideoPreview {
    fn present(
        &mut self,
        ctx: &egui::Context,
        key: VideoSampleKey,
        pixels: std::sync::Arc<image::RgbaImage>,
    ) {
        let size = [pixels.width() as usize, pixels.height() as usize];
        let color = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
        self.pixels = Some(pixels);
        self.presented = Some(key);
        let id = key.asset;
        if let Some((asset, texture)) = &mut self.texture
            && *asset == id
        {
            texture.set(color, egui::TextureOptions::LINEAR);
        } else {
            self.texture = Some((
                id,
                ctx.load_texture("video-preview", color, egui::TextureOptions::LINEAR),
            ));
        }
    }
}

#[derive(Clone)]
pub struct MediaDrag(pub u64);

#[derive(Clone)]
pub(crate) struct MediaTrackDrag(pub usize);

pub(crate) fn bitmap(data: Vec<u8>, width: u32, height: u32, name: String) -> Asset {
    Asset {
        id: id(),
        name,
        folder: None,
        raster_tile: false,
        kind: Kind::Image,
        data: data.into(),
        size: [width, height],
        seconds: 0.,
        peaks: Default::default(),
    }
}

pub(crate) fn imported_audio(data: Vec<u8>, name: String) -> Option<Asset> {
    if data.is_empty() || data.len() > ASSET_LIMIT {
        return None;
    }
    let mut seconds = None;
    if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WAVE") {
        let mut at = 12usize;
        let mut rate = 0u32;
        let mut length = 0usize;
        while let Some(header) = data.get(at..at.saturating_add(8)) {
            let size = u32::from_le_bytes(header[4..8].try_into().ok()?) as usize;
            let end = (at + 8).checked_add(size)?;
            let chunk = data.get(at + 8..end)?;
            if &header[..4] == b"fmt " && chunk.len() >= 16 {
                rate = u32::from_le_bytes(chunk[8..12].try_into().ok()?);
            } else if &header[..4] == b"data" {
                length = size;
            }
            at = end + size % 2;
        }
        if rate != 0 && length != 0 {
            seconds = Some(length as f64 / f64::from(rate));
        }
    }
    let seconds = match seconds {
        Some(seconds) => seconds,
        None => {
            let file = MediaFile::new(&data).ok()?;
            let mut command = Command::new(crate::exporter::ffmpeg_path(""));
            crate::exporter::hidden(&mut command);
            let result = command
                .args([
                    "-hide_banner",
                    "-nostdin",
                    "-protocol_whitelist",
                    "file,pipe",
                    "-format_whitelist",
                    DEMUXERS,
                    "-i",
                ])
                .arg(&file.0)
                .args(["-t", "0", "-f", "null", "-"])
                .output()
                .ok()?;
            let details = String::from_utf8_lossy(&result.stderr);
            duration(&details)
                .filter(|_| details.contains("Audio:") && !details.contains("Video:"))?
        }
    };
    Some(Asset {
        id: id(),
        name,
        folder: None,
        raster_tile: false,
        kind: Kind::Audio,
        data: data.into(),
        size: [0; 2],
        seconds,
        peaks: Default::default(),
    })
}

pub(crate) fn imported_sound_track(
    name: String,
    entries: Vec<(&Asset, usize, usize, usize)>,
    fps: u32,
) -> Option<Track> {
    let clips: SharedVec<_> = entries
        .into_iter()
        .filter_map(|(asset, start, offset, limit)| {
            let length = ((asset.seconds * f64::from(fps)).ceil() as usize)
                .saturating_sub(offset)
                .min(limit)
                .min(10000usize.saturating_sub(offset));
            (length > 0).then(|| Clip {
                asset: asset.id,
                start,
                offset,
                length,
                transform: Default::default(),
                audio: Default::default(),
            })
        })
        .collect();
    (!clips.is_empty()).then(|| Track {
        id: id(),
        name,
        kind: Kind::Audio,
        clips,
        slot: 0,
        depth: Default::default(),
        hidden: false,
        locked: false,
    })
}

fn duration(text: &str) -> Option<f64> {
    let value = text.split_once("Duration: ")?.1.split(',').next()?.trim();
    let mut seconds = 0.;
    for part in value.split(':') {
        seconds = seconds * 60. + part.parse::<f64>().ok()?;
    }
    (seconds.is_finite() && seconds > 0.).then_some(seconds)
}

impl Editor {
    fn read_media(path: &Path, ffmpeg_path: &Path) -> Result<Asset, String> {
        let name = path
            .file_name()
            .ok_or("Invalid file name")?
            .to_string_lossy()
            .into_owned();
        let kind = match path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "png" | "jpg" | "jpeg" | "bmp" | "tif" | "tiff" | "webp" => Kind::Image,
            "mp4" | "avi" | "mov" | "mkv" | "webm" => Kind::Video,
            "wav" | "ogg" | "mp3" | "flac" | "m4a" | "aac" => Kind::Audio,
            _ => return Err("Unsupported media format".into()),
        };
        if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > ASSET_LIMIT as u64 {
            return Err("Media file exceeds 64 MiB".into());
        }
        if kind == Kind::Audio
            && path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
        {
            let data = std::fs::read(path).map_err(|error| error.to_string())?;
            if data.len() > ASSET_LIMIT {
                return Err("Media file exceeds 64 MiB".into());
            }
            if let Some(wave) = crate::lip::Wave::read(&data)
                && wave.frames() > 0
            {
                let seconds = wave.frames() as f64 / wave.rate as f64;
                let end = wave.frames().min(420 * wave.rate);
                let step = (wave.rate / 8000).max(1);
                let peaks = (0..end)
                    .step_by((wave.rate / 50).max(1))
                    .map(|start| {
                        let stop = (start + wave.rate / 50).min(end);
                        let peak = (start..stop)
                            .step_by(step)
                            .map(|sample| wave.sample(sample).abs())
                            .fold(0f32, f32::max);
                        (peak * 255.).round() as u8
                    })
                    .collect();
                return Ok(Asset {
                    id: id(),
                    name,
                    folder: None,
                    raster_tile: false,
                    kind,
                    data: data.into(),
                    size: [0; 2],
                    seconds,
                    peaks,
                });
            }
        }
        let ffmpeg = || {
            let mut command = Command::new(ffmpeg_path);
            command
                .args([
                    "-hide_banner",
                    "-nostdin",
                    "-protocol_whitelist",
                    "file,pipe",
                    "-format_whitelist",
                    DEMUXERS,
                    "-i",
                ])
                .arg(path);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
            }
            command
        };
        let asset = if kind == Kind::Image {
            let decoded = image::open(path).or_else(|_| {
                let result = ffmpeg().args(["-frames:v", "1", "-vf", "scale=w=min(iw\\,4096):h=min(ih\\,4096):force_original_aspect_ratio=decrease", "-f", "image2pipe", "-vcodec", "png", "pipe:1"]).output()
                    .map_err(image::ImageError::IoError)?;
                image::load_from_memory(&result.stdout)
            }).map_err(|error| format!("Cannot decode image. JPEG and WebP require FFmpeg: {error}"))?.to_rgba8();
            if decoded.width() > 4096 || decoded.height() > 4096 {
                return Err("Image dimensions exceed 4096 pixels".into());
            }
            let (w, h) = decoded.dimensions();
            bitmap(decoded.into_raw(), w, h, name)
        } else {
            let output = ffmpeg()
                .args(["-t", "0", "-f", "null", "-"])
                .output()
                .map_err(|e| format!("FFmpeg is required to import video and audio: {e}"))?;
            let details = String::from_utf8_lossy(&output.stderr);
            let stream = if kind == Kind::Video {
                "Video:"
            } else {
                "Audio:"
            };
            let seconds = duration(&details)
                .filter(|_| details.contains(stream))
                .ok_or("Cannot determine media duration or find a compatible stream")?;
            Asset {
                id: id(),
                name,
                folder: None,
                raster_tile: false,
                kind,
                data: std::fs::read(path).map_err(|e| e.to_string())?.into(),
                size: [0; 2],
                seconds,
                peaks: if kind == Kind::Audio {
                    ffmpeg()
                        .args([
                            "-map", "0:a:0", "-t", "420", "-ac", "1", "-ar", "8000", "-f", "s16le",
                            "pipe:1",
                        ])
                        .output()
                        .ok()
                        .filter(|output| output.status.success())
                        .map(|output| waveform(&output.stdout))
                        .unwrap_or_default()
                } else {
                    Default::default()
                },
            }
        };
        Ok(asset)
    }

    pub fn media_library_ui(&mut self, ui: &mut egui::Ui) {
        if ui
            .add_enabled(
                self.media.importing.is_none(),
                egui::Button::new(tr("Importar medios…")),
            )
            .clicked()
        {
            self.import_media(ui.ctx());
        }
        if self.media.importing.is_some() {
            ui.spinner();
        }
    }

    pub(crate) fn import_media(&mut self, ctx: &egui::Context) {
        if self.media.importing.is_some() {
            return;
        }
        let files = rfd::FileDialog::new()
            .add_filter(
                tr("Medios"),
                &[
                    "png", "jpg", "jpeg", "bmp", "tif", "tiff", "webp", "mp4", "avi", "mov", "mkv",
                    "webm", "wav", "ogg", "mp3", "flac", "m4a", "aac",
                ],
            )
            .pick_files();
        if let Some(files) = files {
            let ffmpeg = crate::exporter::ffmpeg_path(&self.studio.prefs.ffmpeg);
            let ctx = ctx.clone();
            let (send, receive) = std::sync::mpsc::channel();
            self.media.importing = Some((self.recovery.clone(), receive));
            std::thread::spawn(move || {
                let mut bytes = 0usize;
                let results = files
                    .into_iter()
                    .map(|path| {
                        let asset = Self::read_media(&path, &ffmpeg)
                            .map_err(|error| format!("{}: {error}", path.display()))?;
                        if bytes.saturating_add(asset.data.len()) > LIBRARY_LIMIT {
                            return Err("Media library exceeds 256 MiB".into());
                        }
                        bytes += asset.data.len();
                        Ok(asset)
                    })
                    .collect();
                let _ = send.send(results);
                ctx.request_repaint();
            });
        }
    }

    pub(crate) fn media_replacement_choices(&self, old: u64) -> Vec<(u64, String)> {
        if !self.feature.symbol_stack.is_empty() {
            return Vec::new();
        }
        let assets = &self.doc.extras.media.assets;
        let Some(source) = assets
            .iter()
            .find(|asset| asset.id == old && !asset.raster_tile)
        else {
            return Vec::new();
        };
        assets
            .iter()
            .filter(|asset| asset.id != old && !asset.raster_tile && asset.kind == source.kind)
            .map(|asset| (asset.id, asset.name.clone()))
            .collect()
    }

    pub(crate) fn replace_media_placements(&mut self, old: u64, new: u64) {
        if !self
            .media_replacement_choices(old)
            .iter()
            .any(|(id, _)| *id == new)
        {
            return;
        }
        let asset = self
            .doc
            .extras
            .media
            .assets
            .iter()
            .find(|asset| asset.id == new)
            .unwrap();
        if asset.kind != Kind::Image {
            let available = (asset.seconds * f64::from(self.doc.fps)).ceil() as usize;
            let tracks = self.doc.extras.media.tracks.iter().chain(
                self.doc
                    .extras
                    .scenes
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| *index != self.doc.extras.active_scene)
                    .flat_map(|(_, scene)| scene.media_tracks.iter()),
            );
            if !tracks
                .into_iter()
                .all(|track| track.replacement_fits(old, available))
            {
                self.status = tr("Replacement media is too short for an existing clip.").into();
                return;
            }
        }
        fn images(
            layers: &mut SharedVec<crate::document::Layer>,
            old: u64,
            new: u64,
            cels: &mut std::collections::HashMap<usize, Option<SharedVec<crate::document::Mark>>>,
        ) {
            for layer in layers {
                for cel in &mut layer.frames {
                    let replacement = cels.entry(cel.identity()).or_insert_with(|| {
                        if !cel.iter().any(|mark| mark.style.media_image == Some(old)) {
                            return None;
                        }
                        let mut updated = cel.clone();
                        for mark in updated
                            .iter_mut()
                            .filter(|mark| mark.style.media_image == Some(old))
                        {
                            mark.style.media_image = Some(new);
                        }
                        Some(updated)
                    });
                    if let Some(replacement) = replacement {
                        *cel = replacement.clone();
                    }
                }
            }
        }
        let image = asset.kind == Kind::Image;
        self.checkpoint();
        if image {
            let mut cels = std::collections::HashMap::new();
            images(&mut self.doc.layers, old, new, &mut cels);
            for scene in &mut self.doc.extras.scenes {
                images(&mut scene.layers, old, new, &mut cels);
            }
            for symbol in &mut self.doc.extras.symbols {
                images(&mut symbol.layers, old, new, &mut cels);
            }
        } else {
            for track in &mut self.doc.extras.media.tracks {
                track.replace_asset_reference(old, new);
            }
            for scene in &mut self.doc.extras.scenes {
                for track in &mut scene.media_tracks {
                    track.replace_asset_reference(old, new);
                }
            }
            self.restart_media_audio();
        }
        self.recover();
    }

    pub(crate) fn delete_library_media(&mut self, id: u64) {
        fn references(doc: &crate::Document, id: u64) -> bool {
            let layers = |layers: &[crate::document::Layer]| {
                layers
                    .iter()
                    .flat_map(|layer| layer.frames.iter().flatten())
                    .any(|mark| mark.style.media_image == Some(id))
            };
            let tracks = |tracks: &[Track]| {
                tracks
                    .iter()
                    .any(|track| track.clips.iter().any(|clip| clip.asset == id))
            };
            layers(&doc.layers)
                || tracks(&doc.extras.media.tracks)
                || doc.extras.scenes.iter().enumerate().any(|(index, scene)| {
                    index != doc.extras.active_scene
                        && (layers(&scene.layers) || tracks(&scene.media_tracks))
                })
                || doc
                    .extras
                    .symbols
                    .iter()
                    .any(|symbol| layers(&symbol.layers))
        }
        if !self
            .doc
            .extras
            .media
            .assets
            .iter()
            .any(|asset| asset.id == id)
        {
            return;
        }
        if references(&self.doc, id)
            || self
                .feature
                .symbol_stack
                .iter()
                .any(|(doc, _, _, _)| references(doc, id))
        {
            self.status = tr("Media is in use. Remove its instances and clips before deleting it from the library.").into();
            return;
        }
        self.checkpoint();
        self.doc.extras.media.assets.retain(|asset| asset.id != id);
        self.media.textures.remove(&id);
        self.media.waveform_cache.remove(&id);
        self.clear_media_asset_preview();
        self.assets.selected = None;
        self.recover();
    }

    pub fn clear_media_asset_preview(&mut self) {
        self.media.library_video = None;
        self.media.library_preview = None;
        self.media.library_request = None;
        self.media.library_job = None;
        self.media.library_file = None;
    }

    pub fn media_asset_preview(&mut self, ui: &mut egui::Ui, id: u64) {
        let Some(asset) = self
            .doc
            .extras
            .media
            .assets
            .iter()
            .find(|asset| asset.id == id)
        else {
            return;
        };
        if asset.kind != Kind::Video {
            self.media.library_video = None;
            self.media.library_file = None;
            self.media.library_job = None;
        }
        if asset.kind == Kind::Image {
            let Some((size, pixels)) = self.doc.extras.media.pixels(id) else {
                return;
            };
            let texture = self.media.textures.entry(id).or_insert_with(|| {
                ui.ctx().load_texture(
                    format!("media-{id}"),
                    egui::ColorImage::from_rgba_unmultiplied(size, pixels),
                    egui::TextureOptions::LINEAR,
                )
            });
            show_preview(ui, texture);
        } else if asset.kind == Kind::Video {
            if self
                .media
                .library_video
                .is_none_or(|(selected, _)| selected != id)
            {
                self.media.library_video = Some((id, std::time::Instant::now()));
                self.media.library_preview = None;
                self.media.library_request = None;
                self.media.library_file = None;
                self.media.library_job = None;
            }
            if let Some(job) = &self.media.library_job
                && let Ok(result) = job.try_recv()
            {
                self.media.library_job = None;
                if let Some((id, file, image)) = result {
                    self.media.library_file = Some((id, file));
                    let size = [image.width() as usize, image.height() as usize];
                    self.media.library_preview = Some((
                        id,
                        ui.ctx().load_texture(
                            "library-video",
                            egui::ColorImage::from_rgba_unmultiplied(size, &image),
                            egui::TextureOptions::LINEAR,
                        ),
                    ));
                }
            }
            let elapsed = self.media.library_video.unwrap().1.elapsed().as_secs_f64();
            let duration = asset.seconds.min(3.);
            let sample = (elapsed.min((duration - 0.15).max(0.)) * 6.) as usize;
            if self.media.library_job.is_none() && self.media.library_request != Some((id, sample))
            {
                self.media.library_request = Some((id, sample));
                let asset = asset.clone();
                let ffmpeg = crate::exporter::ffmpeg_path(&self.studio.prefs.ffmpeg);
                let ctx = ui.ctx().clone();
                let (send, receive) = std::sync::mpsc::channel();
                self.media.library_job = Some(receive);
                let cached = self
                    .media
                    .library_file
                    .as_ref()
                    .filter(|(asset_id, _)| *asset_id == id)
                    .map(|(_, file)| file.clone());
                std::thread::spawn(move || {
                    let result = (|| {
                        let file = match cached {
                            Some(file) => file,
                            None => StagedVideo::new(MediaFile::new(&asset.data).ok()?),
                        };
                        let image = decode_video_file(&file, sample as f64 / 6., &ffmpeg, true)?;
                        Some((id, file, image))
                    })();
                    let _ = send.send(result);
                    ctx.request_repaint();
                });
            }
            if elapsed < duration {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(100));
            }
            if let Some((preview_id, texture)) = &self.media.library_preview
                && *preview_id == id
            {
                show_preview(ui, texture);
            }
        } else {
            ui.label(format!("{:.1} s", asset.seconds));
        }
    }

    pub fn paste_media_image(&mut self) -> bool {
        if self.doc.layers[self.layer].options.locked {
            return false;
        }
        let Ok(image) = arboard::Clipboard::new().and_then(|mut clipboard| clipboard.get_image())
        else {
            return false;
        };
        if self.raster_active() || self.adjustment_active() {
            self.status = tr("Select a vector layer to place a symbol or image").into();
            return true;
        }
        if image.width == 0 || image.height == 0 || image.width > 4096 || image.height > 4096 {
            self.status = "Clipboard image exceeds supported dimensions".into();
            return true;
        }
        let asset = bitmap(
            image.bytes.into_owned(),
            image.width as u32,
            image.height as u32,
            tr("Imagen del portapapeles").into(),
        );
        if !self.doc.extras.media.can_add(&asset) {
            self.status = "Media library exceeds 256 MiB".into();
            return true;
        }
        self.checkpoint();
        self.insert_image(&asset, self.frame, None);
        self.doc.extras.media.assets.push(asset);
        self.recover();
        true
    }

    fn insert_image(&mut self, asset: &Asset, frame: usize, position: Option<Pos2>) {
        if self.raster_active() || self.adjustment_active() {
            self.status = tr("Select a vector layer to place a symbol or image").into();
            return;
        }
        let size = Vec2::new(asset.size[0] as f32, asset.size[1] as f32);
        let center = position
            .unwrap_or_else(|| Pos2::new(self.doc.width as f32 / 2., self.doc.height as f32 / 2.));
        let rect = Rect::from_center_size(center, size);
        let mut mark = crate::Mark {
            points: vec![crate::vector::xy(rect.min), crate::vector::xy(rect.max)].into(),
            rectangle: true,
            closed: true,
            stroke: false,
            fill: Some([0, 0, 0, 0]),
            ..Default::default()
        };
        mark.style.media_image = Some(asset.id);
        self.doc.layers[self.layer].frames[frame].push(mark);
    }

    pub fn paint_media_video(
        &mut self,
        painter: &egui::Painter,
        view: View,
        frame: usize,
        slot: usize,
    ) {
        let depth_active = crate::depth::active(self);
        let library = &self.doc.extras.media;
        let layers = self.doc.layers.len();
        for track in library.tracks.iter().filter(|track| {
            track.kind == Kind::Video && !track.hidden && track.slot.min(layers) == slot
        }) {
            let Some(clip) = track
                .clips
                .iter()
                .find(|clip| frame >= clip.start && frame - clip.start < clip.length)
            else {
                continue;
            };
            if depth_active
                && crate::depth::camera(&self.doc, frame).is_some_and(|c| {
                    !crate::depth::visible(&track.depth, &crate::animation::camera_optics(c, frame))
                })
            {
                continue;
            }
            let Some(asset) = library.assets.iter().find(|asset| asset.id == clip.asset) else {
                continue;
            };
            let key = VideoSampleKey {
                asset: clip.asset,
                frame: clip.offset + frame - clip.start,
                fps: self.doc.fps,
                data: asset.data.identity(),
            };
            let preview = self.media.videos.entry(track.id).or_default();
            let cache = &mut self.media.scene_video_cache;
            if let Some(job) = &preview.job {
                match job.try_recv() {
                    Ok(result) => {
                        preview.job = None;
                        if let Some((id, file, image)) = result {
                            let pixels = std::sync::Arc::new(image);
                            preview.file = Some((id, file));
                            preview.file_owner = preview.request_owner.clone();
                            if let (Some(decoded), Some(owner)) =
                                (preview.request, preview.request_owner.take())
                            {
                                let mut bytes = pixels.len()
                                    + cache.iter().map(|frame| frame.pixels.len()).sum::<usize>();
                                const LIMIT: usize = 48 * 1024 * 1024;
                                if pixels.len() <= LIMIT {
                                    while bytes > LIMIT || cache.len() >= 24 {
                                        bytes -= cache.pop_front().unwrap().pixels.len();
                                    }
                                    cache.push_back(CachedVideoSample {
                                        key: decoded,
                                        _owner: owner,
                                        pixels: pixels.clone(),
                                    });
                                }
                            }
                            if let Some(decoded) = preview.request {
                                preview.present(painter.ctx(), decoded, pixels);
                            }
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => preview.job = None,
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                }
            }
            if preview.job.is_none() && preview.request != Some(key) {
                preview.request = Some(key);
                if let Some(index) = cache.iter().position(|sample| sample.key == key) {
                    let sample = cache.remove(index).unwrap();
                    preview.present(painter.ctx(), key, sample.pixels.clone());
                    cache.push_back(sample);
                } else {
                    preview.request_owner = Some(asset.data.clone());
                    let asset = asset.clone();
                    let time = key.frame as f64 / key.fps as f64;
                    let ffmpeg = crate::exporter::ffmpeg_path(&self.studio.prefs.ffmpeg);
                    let (sender, receiver) = std::sync::mpsc::channel();
                    let ctx = painter.ctx().clone();
                    preview.job = Some(receiver);
                    let cached = preview
                        .file
                        .as_ref()
                        .filter(|(id, _)| {
                            *id == key.asset
                                && preview
                                    .file_owner
                                    .as_ref()
                                    .is_some_and(|owner| owner.identity() == key.data)
                        })
                        .map(|(_, file)| file.clone());
                    std::thread::spawn(move || {
                        let frame = (|| {
                            let file = match cached {
                                Some(file) => file,
                                None => StagedVideo::new(MediaFile::new(&asset.data).ok()?),
                            };
                            let image = decode_video_file(&file, time, &ffmpeg, true)?;
                            Some((key.asset, file, image))
                        })();
                        let _ = sender.send(frame);
                        ctx.request_repaint();
                    });
                }
            }

            if let Some((preview_id, texture)) = &preview.texture
                && *preview_id == key.asset
                && preview.presented == Some(key)
            {
                let mark = transformed_video_mark(&self.doc, texture.size_vec2(), clip.transform);
                let view = if depth_active {
                    crate::depth::view_for_plane(&self.doc, frame, &track.depth, view)
                } else {
                    view
                };
                if let Some(mesh) = image_mesh(&mark, view, texture.id()) {
                    painter.add(mesh);
                }
            }
        }
    }

    fn video_selection_mark(&self, track: &Track, clip: &Clip) -> crate::Mark {
        let dimensions = self
            .media
            .videos
            .get(&track.id)
            .and_then(|preview| preview.texture.as_ref())
            .map_or(
                Vec2::new(self.doc.width as f32, self.doc.height as f32),
                |(_, texture)| texture.size_vec2(),
            );
        transformed_video_mark(&self.doc, dimensions, clip.transform)
    }

    pub(crate) fn paint_video_selection(&self, painter: &egui::Painter, view: View) {
        if self.playing
            || !matches!(
                self.tool,
                crate::canvas::Tool::Select
                    | crate::canvas::Tool::Object
                    | crate::canvas::Tool::Transform
            )
        {
            return;
        }
        let Some((t, c)) = self
            .media
            .selected_video
            .and_then(|id| self.video_clip_indices(id))
        else {
            return;
        };
        let track = &self.doc.extras.media.tracks[t];
        if track.hidden || track.locked {
            return;
        }
        let view = if crate::depth::active(self) {
            crate::depth::view_for_plane(&self.doc, self.frame, &track.depth, view)
        } else {
            view
        };
        let mark = self.video_selection_mark(track, &track.clips[c]);
        let points: Vec<_> = crate::canvas::vertices(&mark)
            .into_iter()
            .take(4)
            .map(|point| view.screen(point))
            .collect();
        if points.len() < 4 {
            return;
        }
        let accent = crate::themes::accent();
        painter.add(egui::Shape::closed_line(
            points.clone(),
            egui::Stroke::new(1., accent),
        ));
        if self.tool == crate::canvas::Tool::Transform {
            for point in points {
                painter.rect_filled(Rect::from_center_size(point, Vec2::splat(7.)), 1., accent);
            }
        }
    }

    pub(crate) fn video_transform_input(&mut self, ui: &egui::Ui, area: Rect, view: View) -> bool {
        let snap_view = self.camera_depth_view(view, self.layer);
        if !matches!(
            self.tool,
            crate::canvas::Tool::Select
                | crate::canvas::Tool::Object
                | crate::canvas::Tool::Transform
        ) || self.playing
        {
            return false;
        }
        if self.media.video_drag.is_none()
            && self
                .doc
                .extras
                .media
                .active_videos(self.frame, self.doc.layers.len())
                .next()
                .is_none()
        {
            self.media.selected_video = None;
            return false;
        }
        let (position, pressed, down, released, modifiers) = ui.input(|i| {
            (
                i.pointer.interact_pos(),
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.primary_released(),
                i.modifiers,
            )
        });
        let Some(position) = position else {
            return false;
        };
        let depth_active = crate::depth::active(self);
        if pressed
            && area.contains(position)
            && ui.ctx().layer_id_at(position) == Some(ui.layer_id())
        {
            let layer_above = self
                .doc
                .layers
                .iter()
                .enumerate()
                .rev()
                .filter(|(_, layer)| layer.visible && !layer.options.locked)
                .find_map(|(index, layer)| {
                    let view = if depth_active {
                        crate::depth::layer_view(&self.doc, self.frame, index, view)
                    } else {
                        view
                    };
                    let world = view.world(position);
                    crate::animation::marks(layer, self.frame)
                        .iter()
                        .rev()
                        .any(|mark| self.selection_hit(mark, world, 4. / view.scale))
                        .then_some(index)
                });
            let tracks = &self.doc.extras.media;
            let hit = |track: &Track, clip: &Clip, handles: bool| {
                if track.locked || track.hidden {
                    return false;
                }
                let view = if depth_active {
                    crate::depth::view_for_plane(&self.doc, self.frame, &track.depth, view)
                } else {
                    view
                };
                let world = view.world(position);
                let mark = self.video_selection_mark(track, clip);
                crate::vector::contains_fill(&mark, world)
                    || (handles
                        && crate::canvas::vertices(&mark)
                            .into_iter()
                            .take(4)
                            .any(|p| view.screen(p).distance(position) <= 9.))
            };
            let explicit = self
                .media
                .selected_video
                .filter(|_| self.tool == crate::canvas::Tool::Transform)
                .and_then(|id| self.video_clip_indices(id))
                .filter(|&(t, c)| hit(&tracks.tracks[t], &tracks.tracks[t].clips[c], true));
            let picked = explicit.or_else(|| {
                let (track, _) = tracks
                    .active_videos(self.frame, self.doc.layers.len())
                    .filter(|(track, clip)| {
                        layer_above.is_none_or(|l| track.slot > l) && hit(track, clip, false)
                    })
                    .last()?;
                self.video_clip_indices(track.id)
            });
            if let Some((t, c)) = picked {
                let track = &tracks.tracks[t];
                let clip = &track.clips[c];
                let view = if depth_active {
                    crate::depth::view_for_plane(&self.doc, self.frame, &track.depth, view)
                } else {
                    view
                };
                let world = view.world(position);
                let mark = self.video_selection_mark(track, clip);
                self.media.video_resize_center = (self.tool == crate::canvas::Tool::Transform
                    && crate::canvas::vertices(&mark)
                        .into_iter()
                        .take(4)
                        .any(|p| view.screen(p).distance(position) <= 9.))
                .then(|| crate::vector::control_bounds(&mark).center());
                self.media.selected_video = Some(track.id);
                self.media.video_drag = Some((track.id, c, world, clip.transform));
                self.media.video_drag_started = false;
                self.media.video_drag_modifiers = modifiers;
                self.view_options.video_bounds = Some(Rect::from_points(
                    &crate::canvas::vertices(&mark)
                        .iter()
                        .map(|p| snap_view.world(view.screen(*p)))
                        .collect::<Vec<_>>(),
                ));
                self.prepare_snapping(&[]);
            } else {
                self.media.selected_video = None;
            }
        }
        if let Some((id, c, origin, initial)) = self.media.video_drag {
            let view = if depth_active {
                self.doc
                    .extras
                    .media
                    .tracks
                    .iter()
                    .find(|t| t.id == id)
                    .map_or(view, |t| {
                        crate::depth::view_for_plane(&self.doc, self.frame, &t.depth, view)
                    })
            } else {
                view
            };
            let world = view.world(position);
            let mut delta = world - origin;
            if self.studio.prefs.view.snaps.any()
                && self.media.video_resize_center.is_none()
                && !self.media.video_drag_modifiers.shift
                && !self.media.video_drag_modifiers.alt
            {
                if let Some(bounds) = self.view_options.video_bounds {
                    let anchor = snap_view.world(view.screen(origin));
                    let destination = snap_view.world(view.screen(world));
                    let snapped = self.snap_translation(
                        bounds,
                        destination - anchor,
                        snap_view,
                        modifiers.alt,
                    );
                    delta = view.world(snap_view.screen(anchor + snapped)) - origin;
                }
            }
            if !self.media.video_drag_started && delta.length() * view.scale > 2. {
                self.checkpoint();
                self.media.video_drag_started = true;
            }
            if self.media.video_drag_started
                && let Some(t) = self
                    .doc
                    .extras
                    .media
                    .tracks
                    .iter()
                    .position(|track| track.id == id)
                && let Some(clip) = self.doc.extras.media.tracks[t].clips.get_mut(c)
            {
                clip.transform = initial;
                if let Some(center) = self.media.video_resize_center {
                    let ratio =
                        (world.distance(center) / origin.distance(center).max(0.1)).max(0.01);
                    clip.transform.scale_log2 = (initial.scale_log2 + ratio.log2()).clamp(-5., 5.);
                } else if self.media.video_drag_modifiers.shift {
                    clip.transform.scale_log2 =
                        (initial.scale_log2 + delta.x / 180.).clamp(-5., 5.);
                } else if self.media.video_drag_modifiers.alt {
                    clip.transform.angle = initial.angle + delta.x / 140.;
                } else {
                    clip.transform.offset =
                        [initial.offset[0] + delta.x, initial.offset[1] + delta.y];
                }
            }
            if released || !down {
                self.media.video_drag = None;
                self.media.video_resize_center = None;
                if self.media.video_drag_started {
                    self.recover();
                }
                self.media.video_drag_started = false;
            }
            return true;
        }
        false
    }

    fn video_clip_indices(&self, track_id: u64) -> Option<(usize, usize)> {
        let t = self
            .doc
            .extras
            .media
            .tracks
            .iter()
            .position(|track| track.id == track_id && track.kind == Kind::Video)?;
        let c = self.doc.extras.media.tracks[t]
            .clips
            .iter()
            .position(|clip| self.frame >= clip.start && self.frame - clip.start < clip.length)?;
        Some((t, c))
    }

    pub(crate) fn update_raster_texture(&mut self, id: u64, rgba: &[u8], side: usize) {
        if let Some(texture) = self.media.textures.get_mut(&id) {
            texture.set(
                egui::ColorImage::from_rgba_unmultiplied([side; 2], rgba),
                egui::TextureOptions::LINEAR,
            );
        }
    }
    pub(crate) fn invalidate_raster_texture(&mut self, id: u64) {
        self.media.textures.remove(&id);
    }
    pub fn paint_media_image(
        &mut self,
        painter: &egui::Painter,
        view: View,
        mark: &crate::Mark,
    ) -> bool {
        let Some(id) = mark.style.media_image else {
            return false;
        };
        let Some(asset) = self
            .doc
            .extras
            .media
            .assets
            .iter()
            .find(|asset| asset.id == id)
        else {
            return false;
        };
        let dimensions = asset.size.map(|n| n as usize);
        if dimensions[0]
            .checked_mul(dimensions[1])
            .and_then(|n| n.checked_mul(4))
            != Some(asset.data.len())
        {
            return false;
        }
        let texture = self.media.textures.entry(id).or_insert_with(|| {
            let image = egui::ColorImage::from_rgba_unmultiplied(dimensions, &asset.data);
            painter
                .ctx()
                .load_texture(format!("media-{id}"), image, egui::TextureOptions::LINEAR)
        });
        if let Some(mesh) = image_mesh(mark, view, texture.id()) {
            painter.add(mesh);
            true
        } else {
            false
        }
    }

    pub fn place_media(&mut self, asset_id: u64, frame: usize, track_id: Option<u64>) {
        self.place_media_at(asset_id, frame, track_id, None);
    }

    pub fn place_media_at(
        &mut self,
        asset_id: u64,
        frame: usize,
        track_id: Option<u64>,
        position: Option<Pos2>,
    ) {
        let Some(asset) = self
            .doc
            .extras
            .media
            .assets
            .iter()
            .find(|asset| asset.id == asset_id)
            .cloned()
        else {
            return;
        };
        match asset.kind {
            Kind::Image => {
                if self.raster_active() || self.adjustment_active() {
                    self.status = tr("Select a vector layer to place a symbol or image").into();
                    return;
                }
                if self.doc.layers[self.layer].options.locked
                    || frame >= self.doc.layers[self.layer].frames.len()
                {
                    return;
                }
                self.checkpoint();
                self.frame = frame;
                self.insert_image(&asset, frame, position);
                self.recover();
            }
            Kind::Audio if track_id.is_none() => {
                self.media.audio_destination = Some((asset_id, frame))
            }
            _ => self.add_media_clip(
                &asset,
                frame,
                track_id,
                if asset.kind == Kind::Video {
                    tr("Pista de vídeo")
                } else {
                    tr("Sonido")
                },
            ),
        }
    }

    fn add_media_clip(&mut self, asset: &Asset, frame: usize, track_id: Option<u64>, name: &str) {
        if !self.feature.symbol_stack.is_empty() {
            self.status = "Add audio and video to a scene timeline.".into();
            return;
        }
        let length = (asset.seconds * self.doc.fps as f64)
            .ceil()
            .clamp(1., 10000.) as usize;
        if frame >= 10000 {
            return;
        }
        let length = length.min(10000 - frame);
        let index = track_id.and_then(|id| {
            self.doc
                .extras
                .media
                .tracks
                .iter()
                .position(|track| track.id == id && track.kind == asset.kind && !track.locked)
        });
        if track_id.is_some() && index.is_none() {
            return;
        }
        if let Some(index) = index
            && self.doc.extras.media.tracks[index]
                .clips
                .iter()
                .any(|clip| overlaps(clip, frame, length))
        {
            self.status = "Choose a free span on this track.".into();
            return;
        }
        self.checkpoint();
        self.extend_media_timeline(frame + length);
        let tracks = &mut self.doc.extras.media.tracks;
        let index = index.unwrap_or_else(|| {
            tracks.push(Track {
                depth: Default::default(),
                id: id(),
                name: name.into(),
                kind: asset.kind,
                clips: Default::default(),
                slot: 0,
                hidden: false,
                locked: false,
            });
            tracks.len() - 1
        });
        tracks[index].clips.push(Clip {
            asset: asset.id,
            start: frame,
            offset: 0,
            length,
            transform: Default::default(),
            audio: Default::default(),
        });
        self.recover();
    }

    fn extend_media_timeline(&mut self, end: usize) {
        if end <= self.doc.layers[0].frames.len() {
            return;
        }
        for layer in &mut self.doc.layers {
            if crate::animation_clips::resize_track(layer, end) {
                continue;
            }
            layer.init_keys();
            let held = layer.frames.last().cloned().unwrap_or_default();
            layer.frames.resize(end, held);
            layer.options.keys.resize(end, false);
        }
        self.fit_camera_duration();
    }

    pub fn media_dialog(&mut self, ctx: &egui::Context) {
        if let Some((document, result)) =
            self.media
                .importing
                .as_ref()
                .and_then(|(document, job)| match job.try_recv() {
                    Ok(result) => Some((document.clone(), result)),
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => Some((
                        document.clone(),
                        vec![Err("Media import interrupted".into())],
                    )),
                    Err(std::sync::mpsc::TryRecvError::Empty) => None,
                })
        {
            self.media.importing = None;
            if document == self.recovery && !self.hub.home {
                let mut added = false;
                for asset in result {
                    match asset {
                        Ok(asset) if self.doc.extras.media.can_add(&asset) => {
                            if !added {
                                self.checkpoint();
                            }
                            added = true;
                            self.doc.extras.media.assets.push(asset);
                        }
                        Ok(_) => self.status = "Media library exceeds 256 MiB".into(),
                        Err(error) => self.status = error,
                    }
                }
                if added {
                    self.recover();
                }
            } else {
                self.status = "Import cancelled because the active document changed.".into();
            }
        }
        let Some((id, frame)) = self.media.audio_destination else {
            return;
        };
        let Some(asset) = self
            .doc
            .extras
            .media
            .assets
            .iter()
            .find(|asset| asset.id == id)
            .cloned()
        else {
            self.media.audio_destination = None;
            return;
        };
        let mut open = true;
        egui::Window::new(tr("Añadir audio a una pista"))
            .id(egui::Id::unique("audio-destination"))
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .order(egui::Order::Foreground)
            .min_width(320.)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(egui::RichText::new(&asset.name).strong());
                ui.add_space(6.);
                for name in [tr("Diálogo"), tr("Música"), tr("Sonido")] {
                    if ui.button(name).clicked() {
                        self.add_media_clip(&asset, frame, None, name);
                        self.media.audio_destination = None;
                    }
                }
                let tracks = self.doc.extras.media.tracks.clone();
                if tracks.iter().any(|track| track.kind == Kind::Audio) {
                    ui.separator();
                    ui.label(tr("Pista existente"));
                }
                egui::ScrollArea::vertical()
                    .max_height(180.)
                    .show(ui, |ui| {
                        for track in tracks
                            .iter()
                            .filter(|track| track.kind == Kind::Audio && !track.locked)
                        {
                            if ui.button(&track.name).clicked() {
                                self.add_media_clip(&asset, frame, Some(track.id), &track.name);
                                self.media.audio_destination = None;
                            }
                        }
                    });
            });
        if !open {
            self.media.audio_destination = None;
        }
    }

    pub(crate) fn media_track_ui(
        &mut self,
        ui: &mut egui::Ui,
        t: usize,
        row: Rect,
        width: f32,
        cell: f32,
        total: usize,
        track_at: impl Fn(Pos2) -> Option<(usize, f32)>,
    ) {
        let tracks = self.doc.extras.media.tracks.clone();
        let Some(track) = tracks.get(t) else {
            return;
        };
        let original_clip = ui.clip_rect();
        let label = Rect::from_min_size(
            Pos2::new(original_clip.left(), row.top()),
            Vec2::new(width, row.height()),
        );
        let grid_clip = Rect::from_min_max(
            Pos2::new(
                label.right().min(original_clip.right()),
                original_clip.top(),
            ),
            original_clip.max,
        );
        ui.set_clip_rect(Rect::from_min_max(
            original_clip.min,
            Pos2::new(
                label.right().min(original_clip.right()),
                original_clip.bottom(),
            ),
        ));
        let frame_at = |pos: Pos2| ((pos.x - row.left() - width) / cell).floor().max(0.) as usize;
        let title = ui.interact(
            Rect::from_min_size(label.min, Vec2::new(width - 55., row.height())),
            ui.scope_id().with(("media-row-title", track.id)),
            Sense::click_and_drag(),
        );
        if !track.locked && title.drag_started_by(egui::PointerButton::Primary) {
            title.dnd_set_drag_payload(MediaTrackDrag(t));
        }
        let mut delete_track = false;
        title.context_menu(|ui| {
            let key = title.id.with("track-name");
            let mut name = ui
                .data_mut(|data| data.get_temp::<String>(key))
                .unwrap_or_else(|| track.name.clone());
            ui.label(tr("Renombrar"));
            let edit = ui.add_enabled(!track.locked, egui::TextEdit::singleline(&mut name));
            ui.data_mut(|data| data.insert_temp(key, name.clone()));
            if ui
                .add_enabled(
                    !track.locked && !name.trim().is_empty() && name.trim() != track.name,
                    egui::Button::new(tr("Renombrar")),
                )
                .clicked()
                || (edit.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && !track.locked
                    && !name.trim().is_empty())
            {
                self.checkpoint();
                self.doc.extras.media.tracks[t].name = name.trim().chars().take(100).collect();
                self.recover();
                ui.close();
            }
            if ui
                .add_enabled(!track.locked, egui::Button::new(tr("Borrar pista")))
                .clicked()
            {
                delete_track = true;
                ui.close();
            }
        });
        if delete_track {
            self.checkpoint();
            self.doc.extras.media.tracks.remove(t);
            self.restart_media_audio();
            self.recover();
            return;
        }
        if egui::DragAndDrop::has_payload_of_type::<MediaTrackDrag>(ui.ctx())
            && let Some(source) = title.dnd_release_payload::<MediaTrackDrag>()
        {
            self.move_media_track(source.0, track.slot, Some(t));
            return;
        }
        let color = self.studio.prefs.layer_color(if track.kind == Kind::Video {
            crate::studio::LayerColor::Video
        } else {
            crate::studio::LayerColor::Audio
        });
        ui.painter()
            .rect_filled(row, 3., color.gamma_multiply(0.45));
        ui.painter().text(
            label.left_center() + Vec2::new(28., 0.),
            egui::Align2::LEFT_CENTER,
            &track.name,
            egui::FontId::proportional(12.),
            Color32::WHITE,
        );
        crate::design::draw(
            ui.painter(),
            Rect::from_center_size(label.left_center() + Vec2::new(16., 0.), Vec2::splat(17.)),
            if track.kind == Kind::Video {
                crate::design::Icon::Film
            } else {
                crate::design::Icon::Audio
            },
            Color32::WHITE,
        );
        for (column, glyph, active) in [
            (
                0,
                if track.hidden {
                    crate::design::Icon::EyeOff
                } else {
                    crate::design::Icon::Eye
                },
                track.hidden,
            ),
            (1, crate::design::Icon::Lock, track.locked),
        ] {
            let rect = Rect::from_center_size(
                label.left_center() + Vec2::new(width - 42. + column as f32 * 23., 0.),
                Vec2::splat(20.),
            );
            let response = ui.interact(
                rect,
                ui.scope_id().with(("media-state", track.id, column)),
                Sense::click(),
            );
            crate::design::draw(
                ui.painter(),
                rect.shrink(3.),
                glyph,
                if active {
                    crate::themes::accent()
                } else {
                    Color32::WHITE
                },
            );
            if response
                .on_hover_text(if column == 0 {
                    tr("Mostrar u ocultar pista")
                } else {
                    tr("Bloquear o desbloquear pista")
                })
                .clicked()
            {
                self.checkpoint();
                if column == 0 {
                    self.doc.extras.media.tracks[t].hidden = !track.hidden;
                } else {
                    self.doc.extras.media.tracks[t].locked = !track.locked;
                }
                self.restart_media_audio();
                self.recover();
            }
        }
        ui.set_clip_rect(grid_clip);
        ui.painter()
            .rect_filled(row, 3., color.gamma_multiply(0.45));
        let row_response = ui.interact(
            Rect::from_min_max(row.min + Vec2::new(width, 0.), row.max),
            ui.scope_id().with(("media-row-drop", track.id)),
            Sense::hover(),
        );
        if row_response.container_contains_pointer()
            && let Some(payload) = egui::DragAndDrop::payload::<MediaDrag>(ui.ctx())
            && let Some(pos) = ui.input(|i| i.pointer.interact_pos())
        {
            let frame = frame_at(pos).min(total - 1);
            let valid = !track.locked
                && self
                    .doc
                    .extras
                    .media
                    .assets
                    .iter()
                    .find(|asset| asset.id == payload.0)
                    .is_some_and(|asset| {
                        let length = (asset.seconds * self.doc.fps as f64)
                            .ceil()
                            .clamp(1., 10000.) as usize;
                        asset.kind == track.kind
                            && !track.clips.iter().any(|clip| overlaps(clip, frame, length))
                    });
            crate::sequencer::timeline_drop_marker(
                ui.painter(),
                Rect::from_min_size(
                    Pos2::new(row.left() + width + frame as f32 * cell, row.top()),
                    Vec2::new(cell, row.height()),
                ),
                valid,
            );
        }
        if !track.locked
            && egui::DragAndDrop::has_payload_of_type::<MediaDrag>(ui.ctx())
            && let Some(asset) = row_response.dnd_release_payload::<MediaDrag>()
        {
            let frame = ui
                .input(|i| i.pointer.interact_pos())
                .map_or(self.frame, frame_at);
            self.place_media(asset.0, frame.min(total - 1), Some(track.id));
        }
        for (c, clip) in track.clips.iter().enumerate() {
            let rect = Rect::from_min_size(
                row.min + Vec2::new(width + clip.start as f32 * cell, 1.),
                Vec2::new(clip.length as f32 * cell, 22.),
            );
            if !ui.is_rect_visible(rect) {
                continue;
            }
            let Some(asset) = self
                .doc
                .extras
                .media
                .assets
                .iter()
                .find(|asset| asset.id == clip.asset)
            else {
                continue;
            };
            let name = asset.name.clone();
            let source = (asset.seconds * self.doc.fps as f64)
                .ceil()
                .clamp(1., 10000.) as usize;
            let peaks = if track.kind == Kind::Audio && asset.peaks.is_empty() {
                let asset = asset.clone();
                self.media_waveform(ui.ctx(), asset)
            } else {
                asset.peaks.clone()
            };

            let response = ui.interact(
                rect,
                ui.scope_id().with((track.id, c)),
                Sense::click_and_drag(),
            );
            let grab = response.id.with("clip-edge");
            if response.clicked()
                && let Some(pos) = response.interact_pointer_pos()
            {
                self.frame = frame_at(pos).min(total - 1);
                self.media.selected_video = (track.kind == Kind::Video).then_some(track.id);
                self.playing = false;
            }

            if response.hovered() {
                let edge = ui
                    .input(|i| i.pointer.hover_pos())
                    .is_some_and(|p| (p.x - rect.left()).min(rect.right() - p.x) < 6.);
                ui.ctx().set_cursor_icon(if edge {
                    egui::CursorIcon::ResizeHorizontal
                } else {
                    egui::CursorIcon::Grab
                });
            }
            if response.drag_started_by(egui::PointerButton::Primary) && !track.locked {
                let origin = ui
                    .input(|i| i.pointer.press_origin())
                    .unwrap_or(rect.center());
                let edge = if origin.x - rect.left() < 6. {
                    -1i8
                } else if rect.right() - origin.x < 6. {
                    1
                } else {
                    0
                };
                ui.data_mut(|data| data.insert_temp(grab, (edge, origin.x)));
            }
            let mut preview = clip.clone();
            let mut preview_y = row.top();
            let mut valid_preview = true;
            if let Some((edge, origin)) = ui.data(|data| data.get_temp::<(i8, f32)>(grab))
                && let Some(pos) = response.interact_pointer_pos()
            {
                let delta = ((pos.x - origin) / cell).round() as isize;
                if edge < 0 {
                    let delta = delta.clamp(
                        -(clip.start.min(clip.offset) as isize),
                        clip.length.saturating_sub(1) as isize,
                    );
                    preview.start = clip.start.saturating_add_signed(delta);
                    preview.offset = clip.offset.saturating_add_signed(delta);
                    preview.length = clip.length.saturating_add_signed(-delta);
                } else if edge > 0 {
                    preview.length = clip.length.saturating_add_signed(delta).clamp(
                        1,
                        source
                            .saturating_sub(clip.offset)
                            .min(10000 - clip.start)
                            .max(1),
                    );
                } else {
                    preview.start = clip
                        .start
                        .saturating_add_signed(delta)
                        .min(10000usize.saturating_sub(clip.length));
                }
                let candidate = if edge == 0 {
                    track_at(pos)
                } else {
                    Some((t, row.top()))
                };
                let (destination, y) = candidate.unwrap_or((t, row.top()));
                preview_y = y;
                valid_preview = candidate.is_some()
                    && tracks.get(destination).is_some_and(|target| {
                        target.kind == track.kind
                            && !target.locked
                            && !target.clips.iter().enumerate().any(|(i, other)| {
                                (destination != t || i != c)
                                    && overlaps(other, preview.start, preview.length)
                            })
                    });
                if response.drag_stopped_by(egui::PointerButton::Primary) {
                    ui.data_mut(|data| {
                        data.remove_temp::<(i8, f32)>(grab);
                    });
                    if valid_preview {
                        self.checkpoint();
                        self.extend_media_timeline(preview.start + preview.length);
                        if destination == t {
                            self.doc.extras.media.tracks[t].clips[c] = preview.clone();
                        } else {
                            self.doc.extras.media.tracks[t].clips.remove(c);
                            self.doc.extras.media.tracks[destination]
                                .clips
                                .push(preview.clone());
                            self.doc
                                .extras
                                .media
                                .tracks
                                .retain(|track| !track.clips.is_empty());
                        }
                        self.recover();
                    } else {
                        preview = clip.clone();
                        preview_y = row.top();
                    }
                }
            }
            let display = Rect::from_min_size(
                Pos2::new(
                    row.left() + width + preview.start as f32 * cell,
                    preview_y + 1.,
                ),
                Vec2::new(preview.length as f32 * cell, 22.),
            );
            ui.painter().rect_filled(
                display,
                3.,
                if track.hidden {
                    color.gamma_multiply(0.45)
                } else {
                    color
                },
            );
            if response.dragged_by(egui::PointerButton::Primary) {
                crate::sequencer::timeline_drop_marker(
                    &ui.ctx()
                        .layer_painter(egui::LayerId::new(egui::Order::Foreground, response.id))
                        .with_clip_rect(ui.clip_rect()),
                    display,
                    valid_preview,
                );
            }
            if track.kind == Kind::Audio {
                draw_waveform(ui, display, &peaks, &preview, self.doc.fps);
            }
            ui.painter()
                .with_clip_rect(display.intersect(ui.clip_rect()))
                .text(
                    display.left_center() + Vec2::new(7., 0.),
                    egui::Align2::LEFT_CENTER,
                    name,
                    egui::FontId::proportional(12.),
                    Color32::WHITE,
                );
            response.context_menu(|ui| {
                if track.kind == Kind::Audio {
                    if crate::design::button(
                        ui,
                        crate::design::Icon::Mouth,
                        "Dialogue Lip Sync",
                        "",
                        false,
                        true,
                    )
                    .clicked()
                        && let Some(audio) = self.doc.extras.media.lip_audio(clip)
                    {
                        self.open_lip_session(audio);
                        ui.close();
                    }
                }
                if !track.locked {
                    let split = self.frame > clip.start && self.frame < clip.start + clip.length;
                    if ui
                        .add_enabled(split, egui::Button::new(tr("Split clip at playhead")))
                        .clicked()
                    {
                        let mut left = clip.clone();
                        let mut right = clip.clone();
                        left.length = self.frame - clip.start;
                        left.audio.fade_out = 0;
                        right.start = self.frame;
                        right.offset += left.length;
                        right.length -= left.length;
                        right.audio.fade_in = 0;
                        self.checkpoint();
                        let clips = &mut self.doc.extras.media.tracks[t].clips;
                        clips[c] = left;
                        clips.insert(c + 1, right);
                        self.restart_media_audio();
                        self.recover();
                        ui.close();
                        return;
                    }
                    if track.kind == Kind::Audio {
                        let mut mix = clip.audio;
                        let mut percent = mix.gain * 100.;
                        ui.horizontal(|ui| {
                            ui.label(tr("Clip volume"));
                            ui.add(
                                egui::DragValue::new(&mut percent)
                                    .range(0. ..=200.)
                                    .suffix(" %")
                                    .update_while_editing(false),
                            );
                        });
                        mix.gain = percent / 100.;
                        for (label, duration) in [
                            ("Fade in (frames)", &mut mix.fade_in),
                            ("Fade out (frames)", &mut mix.fade_out),
                        ] {
                            ui.horizontal(|ui| {
                                ui.label(tr(label));
                                ui.add(
                                    egui::DragValue::new(duration)
                                        .range(0..=clip.length)
                                        .update_while_editing(false),
                                );
                            });
                        }
                        if mix != clip.audio {
                            self.checkpoint();
                            self.doc.extras.media.tracks[t].clips[c].audio = mix;
                            self.restart_media_audio();
                            self.recover();
                        }
                    }
                }
                if track.kind == Kind::Video && !track.locked {
                    if ui.button(tr("Transformar vídeo")).clicked() {
                        self.frame = clip.start.min(total - 1);
                        self.playing = false;
                        self.media.selected_video = Some(track.id);
                        self.activate_tool(crate::canvas::Tool::Transform);
                        ui.close();
                    }
                    let mut value = clip.transform;
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut value.offset[0]).prefix("X "));
                        ui.add(egui::DragValue::new(&mut value.offset[1]).prefix("Y "));
                    });
                    let mut percent = value.scale_log2.exp2() * 100.;
                    if ui
                        .add(
                            egui::DragValue::new(&mut percent)
                                .range(3.125..=3200.)
                                .prefix(tr("Escala"))
                                .suffix(" %"),
                        )
                        .changed()
                    {
                        value.scale_log2 = (percent / 100.).log2();
                    }
                    let mut angle = value.angle.to_degrees();
                    if ui
                        .add(egui::DragValue::new(&mut angle).suffix("°"))
                        .changed()
                    {
                        value.angle = angle.to_radians();
                    }
                    if value != clip.transform {
                        self.checkpoint();
                        self.doc.extras.media.tracks[t].clips[c].transform = value;
                        self.recover();
                    }
                }
                if ui
                    .add_enabled(!track.locked, egui::Button::new(tr("Borrar clip")))
                    .clicked()
                {
                    self.delete_media_clip(track.id, c);
                    ui.close();
                }
            });
        }
    }

    fn delete_media_clip(&mut self, track_id: u64, clip: usize) {
        let Some(index) = self
            .doc
            .extras
            .media
            .tracks
            .iter()
            .position(|track| track.id == track_id)
        else {
            return;
        };
        let track = &self.doc.extras.media.tracks[index];
        if track.locked || clip >= track.clips.len() {
            return;
        }
        self.checkpoint();
        self.doc.extras.media.tracks[index].clips.remove(clip);
        self.doc
            .extras
            .media
            .tracks
            .retain(|track| !track.clips.is_empty());
        self.restart_media_audio();
        self.recover();
    }

    fn media_waveform(&mut self, ctx: &egui::Context, asset: Asset) -> SharedVec<u8> {
        if let Some(job) = &self.media.waveform_job {
            match job.try_recv() {
                Ok((id, peaks)) => {
                    self.media.waveform_job = None;
                    self.media.waveform_cache.insert(id, peaks);
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.media.waveform_job = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(peaks) = self.media.waveform_cache.get(&asset.id) {
            return peaks.clone();
        }
        if self.media.waveform_job.is_none() {
            let (send, receive) = std::sync::mpsc::channel();
            self.media.waveform_job = Some(receive);
            let ffmpeg = crate::exporter::ffmpeg_path(&self.studio.prefs.ffmpeg);
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let peaks = (|| {
                    let file = MediaFile::new(&asset.data).ok()?;
                    let mut command = Command::new(ffmpeg);
                    crate::exporter::hidden(&mut command);
                    let output = command
                        .args([
                            "-hide_banner",
                            "-loglevel",
                            "error",
                            "-nostdin",
                            "-protocol_whitelist",
                            "file,pipe",
                            "-format_whitelist",
                            DEMUXERS,
                            "-i",
                        ])
                        .arg(&file.0)
                        .args([
                            "-map", "0:a:0", "-t", "420", "-ac", "1", "-ar", "8000", "-f", "s16le",
                            "pipe:1",
                        ])
                        .output()
                        .ok()?;
                    output.status.success().then(|| waveform(&output.stdout))
                })()
                .unwrap_or_default();
                let _ = send.send((asset.id, peaks));
                ctx.request_repaint();
            });
        }
        Default::default()
    }

    pub(crate) fn move_media_track(&mut self, source: usize, slot: usize, target: Option<usize>) {
        let tracks = &self.doc.extras.media.tracks;
        if source >= tracks.len() || tracks[source].locked || target == Some(source) {
            return;
        }
        self.checkpoint();
        let tracks = &mut self.doc.extras.media.tracks;
        let mut moved = tracks.remove(source);
        moved.slot = slot.min(self.doc.layers.len());
        let at = target
            .map(|target| target - usize::from(source < target))
            .unwrap_or(tracks.len());
        let at = at.min(tracks.len());
        tracks.insert(at, moved);
        self.recover();
    }
}

impl MediaLibrary {
    pub(crate) fn timeline_end(&self) -> usize {
        self.tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .map(|clip| clip.start + clip.length)
            .max()
            .unwrap_or(1)
    }
    pub(crate) fn remove_empty_tracks(&mut self) {
        self.tracks.retain(|track| !track.clips.is_empty());
    }
    pub(crate) fn can_add(&self, asset: &Asset) -> bool {
        asset.data.len() <= ASSET_LIMIT
            && self
                .assets
                .iter()
                .map(|asset| asset.data.len())
                .fold(asset.data.len(), usize::saturating_add)
                <= LIBRARY_LIMIT
    }

    pub(crate) fn validate(&self, scenes: &[crate::animation::Scene]) -> Result<(), String> {
        use std::collections::HashMap;
        let mut assets = HashMap::new();
        let mut total = 0usize;
        for asset in &self.assets {
            total = total.saturating_add(asset.data.len());
            if asset.id == 0
                || asset.data.is_empty()
                || asset.data.len() > ASSET_LIMIT
                || total > LIBRARY_LIMIT
                || assets.insert(asset.id, asset.kind).is_some()
                || (asset.kind == Kind::Image && self.pixels(asset.id).is_none())
                || (asset.kind != Kind::Image
                    && (!asset.seconds.is_finite() || asset.seconds <= 0.))
            {
                return Err("Invalid or oversized media asset".into());
            }
        }
        for tracks in
            std::iter::once(&self.tracks).chain(scenes.iter().map(|scene| &scene.media_tracks))
        {
            for track in tracks {
                if !track.depth.valid()
                    || track.kind == Kind::Image
                    || track.clips.iter().any(|clip| {
                        assets.get(&clip.asset) != Some(&track.kind)
                            || !clip.audio.gain.is_finite()
                            || !(0. ..=2.).contains(&clip.audio.gain)
                            || clip.audio.fade_in > 10000
                            || clip.audio.fade_out > 10000
                            || clip.length == 0
                            || clip.start.saturating_add(clip.length) > 10000
                            || clip.offset.saturating_add(clip.length) > 10000
                    })
                {
                    return Err("Invalid media track or clip".into());
                }
            }
        }
        Ok(())
    }

    pub(crate) fn image_pixel(&self, mark: &crate::Mark, point: Pos2) -> Option<[u8; 4]> {
        let (size, pixels) = self.pixels(mark.style.media_image?)?;
        if mark.color[3] == 0 || !crate::vector::contains_fill(mark, point) {
            return None;
        }
        let [origin, right, bottom] = if let Some(basis) = mark.style.texture_basis {
            basis.map(crate::vector::pos)
        } else {
            let corners = crate::canvas::vertices(mark);
            [*corners.first()?, *corners.get(1)?, *corners.get(3)?]
        };
        let (x, y, d) = (right - origin, bottom - origin, point - origin);
        let cross = |a: Vec2, b: Vec2| a.x * b.y - a.y * b.x;
        let det = cross(x, y);
        if det.abs() < 1e-8 || !det.is_finite() {
            return None;
        }
        let u = (cross(d, y) / det).clamp(0., 1.);
        let v = (cross(x, d) / det).clamp(0., 1.);
        let x = ((u * size[0] as f32) as usize).min(size[0] - 1);
        let y = ((v * size[1] as f32) as usize).min(size[1] - 1);
        let pixel: [u8; 4] = pixels[(y * size[0] + x) * 4..][..4].try_into().ok()?;
        (pixel[3] != 0).then_some(pixel)
    }

    pub(crate) fn pixels(&self, id: u64) -> Option<([usize; 2], &[u8])> {
        let asset = self
            .assets
            .iter()
            .find(|asset| asset.id == id && asset.kind == Kind::Image)?;
        let size = asset.size.map(|dimension| dimension as usize);
        if size.contains(&0) || size[0].checked_mul(size[1])?.checked_mul(4)? != asset.data.len() {
            return None;
        }
        Some((size, &asset.data))
    }
}

pub(crate) fn image_mesh(
    mark: &crate::Mark,
    view: View,
    texture: egui::TextureId,
) -> Option<egui::Mesh> {
    let points = crate::canvas::vertices(mark);
    if let Some(basis) = mark.style.texture_basis {
        return crate::raster::clipped_mesh(mark, view, texture, basis);
    }
    if points.len() < 4 {
        return None;
    }
    let mut mesh = egui::Mesh::with_texture(texture);
    for (point, uv) in points.into_iter().zip([
        Pos2::ZERO,
        Pos2::new(1., 0.),
        Pos2::new(1., 1.),
        Pos2::new(0., 1.),
    ]) {
        mesh.vertices.push(egui::epaint::Vertex {
            pos: view.screen(point),
            uv,
            color: Color32::from_white_alpha(mark.color[3]),
        });
    }
    mesh.indices.extend([0, 1, 2, 0, 2, 3]);
    Some(mesh)
}

fn decode_video_file(
    file: &MediaFile,
    time: f64,
    ffmpeg: &Path,
    preview: bool,
) -> Option<image::RgbaImage> {
    let mut command = Command::new(ffmpeg);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-protocol_whitelist",
            "file,pipe",
            "-format_whitelist",
            DEMUXERS,
            "-ss",
        ])
        .arg(format!("{time:.6}"))
        .arg("-i")
        .arg(&file.0)
        .args(["-frames:v", "1"]);
    if preview {
        command.args(["-vf", "scale=1280:720:force_original_aspect_ratio=decrease"]);
    }
    let output = command
        .args(["-f", "image2pipe", "-vcodec", "png", "pipe:1"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(image::load_from_memory(&output.stdout).ok()?.to_rgba8())
}

pub(crate) struct MediaFile(std::path::PathBuf);

impl MediaFile {
    fn new(data: &[u8]) -> Result<Self, String> {
        use std::io::Write;
        let nonce = id();
        let path =
            std::env::temp_dir().join(format!("libretoon-media-{}-{nonce}", std::process::id()));
        let mut file = std::fs::File::create_new(&path).map_err(|e| e.to_string())?;
        let temporary = Self(path);
        file.write_all(data).map_err(|e| e.to_string())?;
        Ok(temporary)
    }
}
impl Drop for MediaFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub(crate) fn decode_lip_audio(
    audio: &crate::lip::Audio,
    fps: u32,
    ffmpeg: &Path,
) -> Result<SharedVec<u8>, &'static str> {
    let error = "Could not decode this audio. Check FFmpeg in Program Preferences.";
    let source = MediaFile::new(&audio.data).map_err(|_| error)?;
    let output = MediaFile::new(&[]).map_err(|_| error)?;
    let mut command = Command::new(ffmpeg);
    crate::exporter::hidden(&mut command);
    let offset = audio.offset as f64 / f64::from(fps);
    let duration = audio.length as f64 / f64::from(fps);
    let result = command
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-y",
            "-protocol_whitelist",
            "file,pipe",
            "-format_whitelist",
            DEMUXERS,
            "-ss",
        ])
        .arg(format!("{offset:.9}"))
        .arg("-i")
        .arg(&source.0)
        .args(["-map", "0:a:0", "-t"])
        .arg(format!("{duration:.9}"))
        .args([
            "-ac",
            "1",
            "-ar",
            "8000",
            "-c:a",
            "pcm_s16le",
            "-f",
            "wav",
            "-fs",
            "160001024",
        ])
        .arg(&output.0)
        .output()
        .map_err(|_| error)?;
    if !result.status.success() {
        return Err(error);
    }
    let decoded = std::fs::read(&output.0).map_err(|_| error)?;
    Ok(decoded.into())
}

pub(crate) fn audio_export(
    doc: &crate::Document,
    command: &mut Command,
    first: usize,
    last: usize,
    avi: bool,
    include_video: bool,
) -> Result<Vec<MediaFile>, String> {
    let mut files = Vec::new();
    let mut filters = Vec::new();
    let fps = doc.fps as f64;
    let mut scene_start = 0usize;
    for scene_index in 0..doc.extras.scenes.len().max(1) {
        let scene = crate::animation::scene_doc(doc, scene_index);
        let scene_end = scene_start + scene.layers[0].frames.len();
        for track in scene
            .extras
            .media
            .tracks
            .iter()
            .filter(|track| track.kind == Kind::Audio && !track.hidden)
        {
            for clip in &track.clips {
                let start = scene_start.saturating_add(clip.start);
                let from = start.max(first);
                let to = start.saturating_add(clip.length).min(last).min(scene_end);
                if from >= to {
                    continue;
                }
                let Some(asset) = doc
                    .extras
                    .media
                    .assets
                    .iter()
                    .find(|asset| asset.id == clip.asset && asset.kind == Kind::Audio)
                else {
                    continue;
                };
                let file = MediaFile::new(&asset.data)?;
                let offset = (clip.offset.saturating_add(from - start)) as f64 / fps;
                let duration = (to - from) as f64 / fps;
                let delay = ((from - first) as f64 * 1000. / fps).round() as usize;
                command
                    .args([
                        "-protocol_whitelist",
                        "file,pipe",
                        "-format_whitelist",
                        DEMUXERS,
                        "-ss",
                    ])
                    .arg(format!("{offset:.6}"))
                    .arg("-t")
                    .arg(format!("{duration:.6}"))
                    .arg("-i")
                    .arg(&file.0);
                files.push(file);
                let input = files.len();
                let elapsed = (from - start) as f64 / fps;
                let length = clip.length as f64 / fps;
                let fade_in = clip.audio.fade_in.min(clip.length) as f64 / fps;
                let fade_out = clip.audio.fade_out.min(clip.length) as f64 / fps;
                let mut gain = format!("{:.6}", clip.audio.gain);
                if fade_in > 0. {
                    gain.push_str(&format!("*clip((t+{elapsed:.6})/{fade_in:.6},0,1)"));
                }
                if fade_out > 0. {
                    gain.push_str(&format!(
                        "*clip(({length:.6}-t-{elapsed:.6})/{fade_out:.6},0,1)"
                    ));
                }
                filters.push(format!(
                    "[{input}:a:0]asetpts=PTS-STARTPTS,volume='{gain}':eval=frame,adelay={delay}:all=1[a{input}]"
                ));
            }
        }
        scene_start = scene_end;
    }
    if files.is_empty() {
        command.arg("-an");
    } else {
        let labels = (1..=files.len())
            .map(|i| format!("[a{i}]"))
            .collect::<String>();
        let duration = (last - first) as f64 / fps;
        filters.push(format!(
            "{labels}amix=inputs={}:normalize=0,apad,atrim=duration={duration:.6}[sound]",
            files.len()
        ));
        command.arg("-filter_complex").arg(filters.join(";"));
        if include_video {
            command.args(["-map", "0:v:0"]);
        }
        command.args([
            "-map",
            "[sound]",
            "-c:a",
            if avi { "pcm_s16le" } else { "aac" },
        ]);
    }
    Ok(files)
}

fn overlaps(clip: &Clip, start: usize, length: usize) -> bool {
    start < clip.start.saturating_add(clip.length) && clip.start < start.saturating_add(length)
}

impl MediaLibrary {
    pub(crate) fn video_intersects(&self, first: usize, last: usize) -> bool {
        self.tracks.iter().any(|track| {
            track.kind == Kind::Video
                && !track.hidden
                && track.clips.iter().any(|clip| {
                    clip.length > 0
                        && clip.start <= last
                        && first < clip.start.saturating_add(clip.length)
                })
        })
    }

    pub(crate) fn has_video_tracks(&self) -> bool {
        self.tracks.iter().any(|track| track.kind == Kind::Video)
    }

    fn active_videos(&self, frame: usize, layers: usize) -> impl Iterator<Item = (&Track, &Clip)> {
        let mut videos: Vec<_> = self
            .tracks
            .iter()
            .filter(|track| track.kind == Kind::Video && !track.hidden)
            .filter_map(|track| {
                let clip = track
                    .clips
                    .iter()
                    .find(|clip| frame >= clip.start && frame - clip.start < clip.length)?;
                Some((track, clip))
            })
            .collect();
        videos.sort_by_key(|(track, _)| track.slot.min(layers));
        videos.into_iter()
    }
}

pub(crate) fn video_mark(doc: &crate::Document, dimensions: Vec2) -> crate::Mark {
    let stage = Vec2::new(doc.width as f32, doc.height as f32);
    let fit = (stage.x / dimensions.x).min(stage.y / dimensions.y);
    let rect = Rect::from_center_size(stage.to_pos2() / 2., dimensions * fit);
    crate::Mark {
        rectangle: true,
        points: vec![crate::vector::xy(rect.min), crate::vector::xy(rect.max)].into(),
        ..Default::default()
    }
}

fn transformed_video_mark(
    doc: &crate::Document,
    dimensions: Vec2,
    transform: VideoTransform,
) -> crate::Mark {
    let mut mark = video_mark(doc, dimensions);
    mark.closed = true;
    mark.fill = Some([0, 0, 0, 0]);
    let center = Pos2::new(doc.width as f32 / 2., doc.height as f32 / 2.);
    crate::vector::transform(&mark, |point| {
        center
            + crate::canvas::rotate(
                (point - center) * transform.scale_log2.exp2(),
                transform.angle,
            )
            + Vec2::new(transform.offset[0], transform.offset[1])
    })
}

#[derive(Clone)]
pub(crate) struct ExportVideoFrame {
    pub depth: crate::depth::LayerDepth,
    pub slot: usize,
    pub mark: crate::Mark,
    pub pixels: std::sync::Arc<image::RgbaImage>,
}

#[derive(Default)]
pub(crate) struct VideoExport {
    files: std::collections::HashMap<u64, MediaFile>,
    last: std::collections::HashMap<u64, ((u64, usize), std::sync::Arc<image::RgbaImage>)>,
}

impl VideoExport {
    pub(crate) fn frames(
        &mut self,
        doc: &crate::Document,
        frame: usize,
        ffmpeg: &Path,
    ) -> Result<Vec<ExportVideoFrame>, String> {
        let (scene, local) = crate::animation::locate(doc, frame);
        let scene = crate::animation::scene_doc(doc, scene);
        let media = &scene.extras.media;
        let mut result = Vec::new();
        for (track, clip) in media.active_videos(local, scene.layers.len()) {
            let id = clip.asset;
            let source = clip.offset + local - clip.start;
            let key = (id, source);
            if self
                .last
                .get(&track.id)
                .is_none_or(|(cached, _)| *cached != key)
            {
                let asset = media
                    .assets
                    .iter()
                    .find(|asset| asset.id == id)
                    .ok_or("Video asset is missing")?;
                let file = match self.files.entry(id) {
                    std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(MediaFile::new(&asset.data)?)
                    }
                };
                let time = source as f64 / doc.fps as f64;
                let image = decode_video_file(file, time, ffmpeg, false)
                    .or_else(|| {
                        decode_video_file(
                            file,
                            (time - 0.5 / doc.fps as f64).max(0.),
                            ffmpeg,
                            false,
                        )
                    })
                    .ok_or_else(|| {
                        format!("Could not decode video {} at {time:.2}s", asset.name)
                    })?;
                self.last
                    .insert(track.id, (key, std::sync::Arc::new(image)));
            }
            let pixels = self.last[&track.id].1.clone();
            let mut mark = transformed_video_mark(
                doc,
                Vec2::new(pixels.width() as f32, pixels.height() as f32),
                clip.transform,
            );
            if let Some(camera) = scene
                .extras
                .cameras
                .iter()
                .find(|camera| local >= camera.start && local - camera.start < camera.duration)
            {
                let optics = crate::animation::camera_optics(camera, local);
                if !crate::depth::visible(&track.depth, &optics) {
                    continue;
                }
                let pose = crate::animation::camera_at(camera, local);
                mark = crate::depth::project_mark(&scene, &pose, &optics, &track.depth, &mark);
                let camera = pose;
                mark = crate::vector::transform(&mark, |p| {
                    Pos2::new(doc.width as f32 / 2., doc.height as f32 / 2.)
                        + crate::canvas::rotate(
                            p - crate::vector::pos(camera.center),
                            -camera.angle,
                        ) * camera.zoom
                });
            }
            result.push(ExportVideoFrame {
                depth: track.depth.clone(),
                slot: track.slot.min(scene.layers.len()),
                mark,
                pixels,
            });
        }
        Ok(result)
    }
}

const AUDIO_RATE: u32 = 48000;
const AUDIO_CHANNELS: u16 = 2;

#[derive(Default)]
struct AudioPreview {
    stream: Option<rodio::OutputStream>,
    sink: Option<rodio::Sink>,
    plan: Vec<u64>,
    plan_revision: Option<(usize, usize, usize, u32, usize, bool, usize)>,
    plan_snapshot: Option<crate::Document>,
    samples: Option<std::sync::Arc<[f32]>>,
    preparing: Option<std::sync::mpsc::Receiver<Result<Option<std::sync::Arc<[f32]>>, String>>>,
    failed: bool,
    continuous: bool,
    origin: usize,
    scrub: Option<(usize, std::time::Instant)>,
}

impl AudioPreview {
    fn stop(&mut self) {
        if let Some(sink) = self.sink.take() {
            sink.stop();
        }
        self.continuous = false;
    }

    fn play(&mut self, frame: usize, fps: u32, scrub: bool) -> Result<(), String> {
        use rodio::Source;
        self.stop();
        let Some(samples) = self.samples.clone() else {
            return Ok(());
        };
        let start = audio_sample_offset(frame, fps).min(samples.len());
        let end = if scrub {
            (start + AUDIO_RATE as usize * AUDIO_CHANNELS as usize * 70 / 1000).min(samples.len())
        } else {
            samples.len()
        };
        if start >= end {
            return Ok(());
        }
        if self.stream.is_none() {
            let mut stream = rodio::OutputStreamBuilder::open_default_stream()
                .map_err(|e| format!("Audio device: {e}"))?;
            stream.log_on_drop(false);
            self.stream = Some(stream);
        }
        let sink = rodio::Sink::connect_new(self.stream.as_ref().unwrap().mixer());
        let source = AudioSlice {
            samples,
            at: start,
            end,
            duration: std::time::Duration::from_secs_f64(
                (end - start) as f64 / (AUDIO_RATE as f64 * AUDIO_CHANNELS as f64),
            ),
        };
        sink.append(source.amplify(1.));
        self.origin = frame;
        self.continuous = !scrub;
        self.sink = Some(sink);
        Ok(())
    }
}

fn audio_sample_offset(frame: usize, fps: u32) -> usize {
    ((frame as u64 * AUDIO_RATE as u64 / fps.max(1) as u64) * AUDIO_CHANNELS as u64) as usize
}

struct AudioSlice {
    samples: std::sync::Arc<[f32]>,
    at: usize,
    end: usize,
    duration: std::time::Duration,
}

impl Iterator for AudioSlice {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.at >= self.end {
            return None;
        }
        let sample = self.samples[self.at];
        self.at += 1;
        Some(sample)
    }
}
impl rodio::Source for AudioSlice {
    fn current_span_len(&self) -> Option<usize> {
        Some(self.end - self.at)
    }
    fn channels(&self) -> u16 {
        AUDIO_CHANNELS
    }
    fn sample_rate(&self) -> u32 {
        AUDIO_RATE
    }
    fn total_duration(&self) -> Option<std::time::Duration> {
        Some(self.duration)
    }
}

impl Drop for MediaEditor {
    fn drop(&mut self) {
        self.audio.stop();
    }
}

impl Editor {
    fn audio_document(&self) -> crate::Document {
        if self.animation.play_all {
            self.doc.clone()
        } else {
            crate::animation::scene_doc(&self.doc, self.doc.extras.active_scene)
        }
    }

    fn audio_plan(&self, doc: &crate::Document) -> Vec<u64> {
        let mut plan = vec![
            doc.fps as u64,
            if self.animation.play_all {
                0
            } else {
                doc.extras.active_scene as u64
            },
            self.animation.play_all as u64,
        ];
        for asset in &doc.extras.media.assets {
            if asset.kind == Kind::Audio {
                plan.extend([asset.id, asset.data.identity() as u64]);
            }
        }
        for index in 0..doc.extras.scenes.len().max(1) {
            let scene = crate::animation::scene_doc(doc, index);
            plan.push(scene.layers[0].frames.len() as u64);
            for track in scene
                .extras
                .media
                .tracks
                .iter()
                .filter(|track| track.kind == Kind::Audio && !track.hidden)
            {
                plan.extend([track.id, track.clips.len() as u64]);
                for clip in &track.clips {
                    plan.extend([
                        clip.asset,
                        clip.start as u64,
                        clip.offset as u64,
                        clip.length as u64,
                        u64::from(clip.audio.gain.to_bits()),
                        clip.audio.fade_in as u64,
                        clip.audio.fade_out as u64,
                    ]);
                }
            }
            plan.push(u64::MAX);
        }
        plan
    }

    fn ensure_audio(&mut self, ctx: &egui::Context) -> bool {
        let doc = self.audio_document();
        let revision = (
            doc.extras.media.assets.identity(),
            doc.extras.media.tracks.identity(),
            if doc.extras.scenes.is_empty() {
                0
            } else {
                doc.extras.scenes.identity()
            },
            doc.fps,
            doc.layers[0].frames.len(),
            self.animation.play_all,
            doc.extras.active_scene,
        );
        if self.media.audio.plan_revision != Some(revision) {
            let plan = self.audio_plan(&doc);
            if self.media.audio.plan != plan {
                self.media.audio.stop();
                self.media.audio.plan = plan;
                self.media.audio.samples = None;
                self.media.audio.preparing = None;
                self.media.audio.failed = false;
            }
            self.media.audio.plan_revision = Some(revision);
            // Keep shared containers alive so subsequent edits change identity.
            self.media.audio.plan_snapshot = Some(doc.clone());
        }
        if let Some(job) = &self.media.audio.preparing {
            let result = match job.try_recv() {
                Ok(result) => Some(result),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Audio preparation stopped".into()))
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
            };
            if let Some(result) = result {
                self.media.audio.preparing = None;
                match result {
                    Ok(samples) => self.media.audio.samples = samples,
                    Err(error) => {
                        self.media.audio.failed = true;
                        self.status = error;
                    }
                }
                // A scene with no audible clips uses the animation clock.
                if self.media.audio.samples.is_none() {
                    self.media.audio.failed = true;
                }
            }
        }
        if self.media.audio.failed || self.media.audio.samples.is_some() {
            return true;
        }
        if self.media.audio.preparing.is_none() {
            let last = crate::animation::movie_count(&doc);
            let has_audio = (0..doc.extras.scenes.len().max(1)).any(|index| {
                crate::animation::scene_doc(&doc, index)
                    .extras
                    .media
                    .tracks
                    .iter()
                    .any(|track| {
                        track.kind == Kind::Audio && !track.hidden && !track.clips.is_empty()
                    })
            });
            if !has_audio {
                self.media.audio.failed = true;
                return true;
            }
            let ffmpeg = crate::exporter::ffmpeg_path(&self.studio.prefs.ffmpeg);
            let (send, receive) = std::sync::mpsc::channel();
            self.media.audio.preparing = Some(receive);
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let result = (|| {
                    if let Some(samples) = native_wav_mix(&doc, last) {
                        return Ok(Some(std::sync::Arc::from(samples)));
                    }
                    let Some(file) = prepare_audio(&doc, 0, last, &ffmpeg)? else {
                        return Ok(None);
                    };
                    let bytes = std::fs::read(&file.0).map_err(|e| e.to_string())?;
                    if bytes.len() % 4 != 0 || bytes.len() > 256 * 1024 * 1024 {
                        return Err("Audio preview exceeds the supported decoded size".into());
                    }
                    let samples: Vec<f32> = bytes
                        .chunks_exact(4)
                        .map(|sample| {
                            let value = f32::from_le_bytes(sample.try_into().unwrap());
                            if value.is_finite() {
                                value.clamp(-1., 1.)
                            } else {
                                0.
                            }
                        })
                        .collect();
                    Ok(Some(std::sync::Arc::from(samples)))
                })();
                let _ = send.send(result);
                ctx.request_repaint();
            });
        }
        false
    }

    pub(crate) fn media_audio_ready(&mut self, ctx: &egui::Context) -> bool {
        if !self.playing || self.hub.home {
            if self.media.audio.continuous {
                self.media.audio.stop();
            }
            if !self.hub.home {
                let ready = self.ensure_audio(ctx);
                if ready
                    && let Some((frame, requested)) = self.media.audio.scrub.take()
                    && requested.elapsed() < std::time::Duration::from_millis(250)
                    && let Err(error) = self.media.audio.play(frame, self.doc.fps, true)
                {
                    self.status = error;
                }
            }
            return false;
        }
        self.media.audio.scrub = None;
        if !self.ensure_audio(ctx) {
            return false;
        }
        if self.media.audio.samples.is_some() && !self.media.audio.continuous {
            let frame = if self.animation.play_all {
                crate::animation::movie_offset(&self.doc, self.frame)
            } else {
                self.frame
            };
            if let Err(error) = self.media.audio.play(frame, self.doc.fps, false) {
                self.status = error;
                self.media.audio.samples = None;
                self.media.audio.failed = true;
            }
            self.tick = std::time::Instant::now();
        }
        true
    }

    pub(crate) fn media_audio_steps(&self) -> Option<usize> {
        if !self.media.audio.continuous {
            return None;
        }
        let sink = self.media.audio.sink.as_ref()?;
        let current = if self.animation.play_all {
            crate::animation::movie_offset(&self.doc, self.frame)
        } else {
            self.frame
        };
        if sink.empty() {
            let frames = self.media.audio.samples.as_ref().map_or(0, |samples| {
                (samples.len() as u64 * self.doc.fps as u64)
                    .div_ceil(AUDIO_RATE as u64 * AUDIO_CHANNELS as u64) as usize
            });
            return Some(frames.saturating_sub(current));
        }
        let target =
            self.media.audio.origin.saturating_add(
                (sink.get_pos().as_secs_f64() * self.doc.fps as f64).floor() as usize,
            );
        Some(target.saturating_sub(current))
    }

    pub(crate) fn scrub_media_audio(&mut self, ctx: &egui::Context) {
        if self.playing {
            return;
        }
        let frame = if self.animation.play_all {
            crate::animation::movie_offset(&self.doc, self.frame)
        } else {
            self.frame
        };
        self.media.audio.scrub = Some((frame, std::time::Instant::now()));
        if self.ensure_audio(ctx) {
            self.media.audio.scrub = None;
            if let Err(error) = self.media.audio.play(frame, self.doc.fps, true) {
                self.status = error;
            }
        }
    }

    pub(crate) fn restart_media_audio(&mut self) {
        self.media.audio.stop();
    }
}

fn native_wav_mix(doc: &crate::Document, last: usize) -> Option<Vec<f32>> {
    let samples = audio_sample_offset(last, doc.fps);
    if samples > 256 * 1024 * 1024 / 4 {
        return None;
    }
    let mut sources = Vec::new();
    let mut scene_start = 0usize;
    for index in 0..doc.extras.scenes.len().max(1) {
        let scene = crate::animation::scene_doc(doc, index);
        let scene_end = scene_start + scene.layers[0].frames.len();
        for clip in scene
            .extras
            .media
            .tracks
            .iter()
            .filter(|t| t.kind == Kind::Audio && !t.hidden)
            .flat_map(|t| t.clips.iter())
        {
            let start = scene_start + clip.start;
            let end = (start + clip.length).min(scene_end).min(last);
            if start >= end {
                continue;
            }
            let asset = doc
                .extras
                .media
                .assets
                .iter()
                .find(|a| a.id == clip.asset && a.kind == Kind::Audio)?;
            let wave = crate::lip::Wave::read(&asset.data).filter(|wav| wav.channels <= 2)?;
            sources.push((start, end, clip.offset, clip.length, clip.audio, wave));
        }
        scene_start = scene_end;
    }
    if sources.is_empty() {
        return None;
    }
    let mut mix = vec![0.; samples];
    for (start, end, offset, length, audio, wave) in sources {
        let source_start = offset as f64 * wave.rate as f64 / doc.fps as f64;
        let ratio = wave.rate as f64 / AUDIO_RATE as f64;
        let frames_per_sample = doc.fps as f32 / AUDIO_RATE as f32;
        let a = audio_sample_offset(start, doc.fps);
        let b = audio_sample_offset(end, doc.fps);
        for (i, dst) in mix[a..b].chunks_exact_mut(2).enumerate() {
            let source = source_start + i as f64 * ratio;
            let sample = source as usize;
            let fraction = source.fract() as f32;
            let frame = i as f32 * frames_per_sample;
            let mut gain = audio.gain;
            if audio.fade_in > 0 {
                gain *= (frame / audio.fade_in.min(length) as f32).clamp(0., 1.);
            }
            if audio.fade_out > 0 {
                gain *= ((length as f32 - frame) / audio.fade_out.min(length) as f32).clamp(0., 1.);
            }
            for (channel, dst) in dst.iter_mut().enumerate() {
                let first = wave.channel(sample, channel);
                let next = wave.channel(sample + 1, channel);
                *dst += (first + (next - first) * fraction) * gain;
            }
        }
    }
    for sample in &mut mix {
        *sample = sample.clamp(-1., 1.);
    }
    Some(mix)
}

fn prepare_audio(
    doc: &crate::Document,
    first: usize,
    last: usize,
    ffmpeg: &Path,
) -> Result<Option<MediaFile>, String> {
    if audio_sample_offset(last.saturating_sub(first), doc.fps) > 256 * 1024 * 1024 / 4 {
        return Err("Audio preview exceeds the supported decoded size".into());
    }
    let mut command = Command::new(ffmpeg);
    crate::exporter::hidden(&mut command);
    command.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostdin",
        "-y",
        "-f",
        "lavfi",
        "-i",
        "anullsrc=r=48000:cl=stereo",
    ]);
    let inputs = audio_export(doc, &mut command, first, last, true, false)?;
    if inputs.is_empty() {
        return Ok(None);
    }
    let output = MediaFile::new(&[])?;
    let result = command
        .args([
            "-ar",
            "48000",
            "-ac",
            "2",
            "-c:a",
            "pcm_f32le",
            "-f",
            "f32le",
        ])
        .arg(&output.0)
        .output()
        .map_err(|e| e.to_string())?;
    if !result.status.success() {
        return Err(format!(
            "Audio preview failed: {}",
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    Ok(Some(output))
}

#[cfg(test)]
pub(crate) fn test_wav() -> Vec<u8> {
    let size = 16000u32;
    let mut bytes = b"RIFF".to_vec();
    bytes.extend_from_slice(&(size + 36).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&8000u32.to_le_bytes());
    bytes.extend_from_slice(&16000u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&size.to_le_bytes());
    bytes.resize(44 + size as usize, 0);
    bytes
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_wav_preview_uses_clip_offset_and_scene_position() {
        let mut wav = test_wav();
        for i in 2000..4000 {
            wav[44 + 2 * i..46 + 2 * i].copy_from_slice(&16384i16.to_le_bytes());
        }
        let asset = imported_audio(wav, "dialogue.wav".into()).unwrap();
        let mut doc = crate::Document::default();
        doc.fps = 24;
        let track =
            imported_sound_track("Dialogue".into(), vec![(&asset, 3, 6, 4)], doc.fps).unwrap();
        doc.extras.media.tracks.push(track);
        doc.extras.media.assets.push(asset);
        let mix = native_wav_mix(&doc, 12).unwrap();
        let start = audio_sample_offset(3, 24);
        assert!(mix[..start].iter().all(|sample| *sample == 0.));
        assert_eq!(&mix[start..start + 2], &[0.5, 0.5]);
        assert_eq!(mix[audio_sample_offset(7, 24)], 0.);
    }

    #[test]
    fn track_header_opens_delete_action_with_right_click() {
        let ctx = egui::Context::default();
        let mut editor = Editor::new(&eframe::CreationContext::_new_kittest(ctx.clone()));
        editor.doc.extras.media.tracks.push(Track {
            depth: Default::default(),
            id: 7,
            slot: 0,
            name: "Test audio".into(),
            kind: Kind::Audio,
            hidden: false,
            locked: false,
            clips: vec![Clip {
                asset: 9,
                start: 0,
                offset: 0,
                length: 3,
                transform: Default::default(),
                audio: Default::default(),
            }]
            .into(),
        });
        let render = |editor: &mut Editor, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800., 500.))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    editor.media_track_ui(
                        ui,
                        0,
                        Rect::from_min_size(Pos2::new(10., 10.), Vec2::new(700., 28.)),
                        300.,
                        22.,
                        24,
                        |_| Some((0, 10.)),
                    );
                },
            );
            output.textures_delta.clear();
            output.shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == tr("Borrar pista"))
            })
        };
        render(&mut editor, vec![]);
        let point = Pos2::new(80., 20.);
        let event = |pressed| egui::Event::PointerButton {
            pos: point,
            pressed,
            button: egui::PointerButton::Secondary,
            modifiers: egui::Modifiers::NONE,
        };
        render(
            &mut editor,
            vec![egui::Event::PointerMoved(point), event(true)],
        );
        let opened = render(&mut editor, vec![event(false)]);
        assert!(
            opened || render(&mut editor, vec![]),
            "right-click must expose track deletion"
        );
    }

    #[test]
    fn library_media_delete_is_undoable_and_protects_instances() {
        let mut editor = Editor::new(&eframe::CreationContext::_new_kittest(
            egui::Context::default(),
        ));
        editor.recovery =
            std::env::temp_dir().join(format!("delete-library-media-{}.json", std::process::id()));
        let asset = bitmap(vec![255; 16], 2, 2, "image".into());
        let id = asset.id;
        editor.doc.extras.media.assets.push(asset);
        let mut mark = crate::Mark::default();
        mark.style.media_image = Some(id);
        editor.doc.layers[0].frames[0].push(mark);
        editor.delete_library_media(id);
        assert_eq!(editor.doc.extras.media.assets.len(), 1);
        editor.doc.layers[0].frames[0].clear();
        editor.delete_library_media(id);
        assert!(editor.doc.extras.media.assets.is_empty());
        editor.history(false);
        assert_eq!(editor.doc.extras.media.assets[0].id, id);
        for kind in [Kind::Audio, Kind::Video] {
            editor.doc.extras.media.tracks.push(Track {
                id: crate::features::id(),
                depth: Default::default(),
                slot: 0,
                name: "used media".into(),
                kind,
                hidden: false,
                locked: false,
                clips: vec![Clip {
                    asset: id,
                    start: 0,
                    offset: 0,
                    length: 3,
                    transform: Default::default(),
                    audio: Default::default(),
                }]
                .into(),
            });
            editor.delete_library_media(id);
            assert_eq!(editor.doc.extras.media.assets.len(), 1);
            editor.doc.extras.media.tracks.clear();
        }
    }

    #[test]
    fn deleting_last_clip_removes_its_track_and_undo_restores_it() {
        let mut editor = Editor::new(&eframe::CreationContext::_new_kittest(
            egui::Context::default(),
        ));
        editor.recovery =
            std::env::temp_dir().join(format!("delete-media-track-{}.json", std::process::id()));
        for kind in [Kind::Audio, Kind::Video] {
            let track_id = id();
            editor.doc.extras.media.tracks.push(Track {
                depth: Default::default(),
                id: track_id,
                slot: 0,
                name: "fixture".into(),
                kind,
                hidden: false,
                locked: false,
                clips: vec![
                    Clip {
                        asset: 9,
                        start: 0,
                        offset: 0,
                        length: 3,
                        transform: Default::default(),
                        audio: Default::default(),
                    },
                    Clip {
                        asset: 9,
                        start: 3,
                        offset: 0,
                        length: 3,
                        transform: Default::default(),
                        audio: Default::default(),
                    },
                ]
                .into(),
            });
            editor.delete_media_clip(track_id, 0);
            assert_eq!(
                editor.doc.extras.media.tracks.last().unwrap().clips.len(),
                1
            );
            editor.delete_media_clip(track_id, 0);
            assert!(
                editor
                    .doc
                    .extras
                    .media
                    .tracks
                    .iter()
                    .all(|track| track.id != track_id)
            );
            editor.history(false);
            assert_eq!(editor.doc.extras.media.tracks.last().unwrap().id, track_id);
        }
    }

    #[test]
    fn native_audio_slice_starts_at_the_requested_frame_without_copying_the_mix() {
        let samples: std::sync::Arc<[f32]> = (0..9600).map(|i| i as f32).collect::<Vec<_>>().into();
        let start = audio_sample_offset(1, 24);
        assert_eq!(start, 4000);
        let slice = AudioSlice {
            samples: samples.clone(),
            at: start,
            end: start + 8,
            duration: std::time::Duration::from_millis(1),
        };
        assert_eq!(slice.collect::<Vec<_>>(), samples[start..start + 8]);
        assert_eq!(audio_sample_offset(0, 24), 0);
    }

    #[test]
    fn imported_bitmap_survives_native_serialization_and_renders_in_export() {
        let mut editor = Editor::new(&eframe::CreationContext::_new_kittest(
            egui::Context::default(),
        ));
        editor.doc = crate::fresh_document();
        editor.doc.width = 8;
        editor.doc.height = 8;
        let asset = bitmap([255, 0, 0, 255].repeat(16), 4, 4, "fixture.png".into());
        editor.insert_image(&asset, 0, None);
        editor.doc.extras.media.assets.push(asset);
        let restored = crate::fla::decode(&crate::fla::encode(&editor.doc).unwrap()).unwrap();
        restored.validate().unwrap();
        let pixels = crate::exporter::raster(
            &restored,
            0,
            8,
            8,
            &egui::Context::default(),
            &mut Default::default(),
        );
        assert_eq!(&pixels[(4 * 8 + 4) * 3..][..3], &[255, 0, 0]);
        assert_eq!(&pixels[..3], &[255, 255, 255]);
    }

    #[test]
    fn replacing_library_audio_preserves_clip_timing_and_is_undoable() {
        let ctx = egui::Context::default();
        let mut editor = Editor::new(&eframe::CreationContext::_new_kittest(ctx));
        let old = imported_audio(test_wav(), "old.wav".into()).unwrap();
        let new = imported_audio(test_wav(), "new.wav".into()).unwrap();
        let (old_id, new_id) = (old.id, new.id);
        let track = imported_sound_track("Dialogue".into(), vec![(&old, 3, 0, 3)], 24).unwrap();
        let clip = track.clips[0].clone();
        editor.doc.extras.media.assets.extend([old, new]);
        editor.doc.extras.media.tracks.push(track);
        editor.replace_media_placements(old_id, new_id);
        let current = &editor.doc.extras.media.tracks[0].clips[0];
        assert_eq!(current.asset, new_id);
        assert_eq!(
            (current.start, current.offset, current.length),
            (clip.start, clip.offset, clip.length)
        );
        assert!(current.audio == clip.audio);
        editor.history(false);
        assert_eq!(editor.doc.extras.media.tracks[0].clips[0].asset, old_id);
    }

    #[test]
    #[ignore = "requires configured FFmpeg"]
    fn final_video_sample_preserves_native_resolution() {
        let ffmpeg = crate::exporter::ffmpeg_path("");
        let fixture = Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=red:s=1920x1080",
                "-frames:v",
                "1",
                "-c:v",
                "ffv1",
                "-f",
                "matroska",
                "pipe:1",
            ])
            .output()
            .unwrap();
        assert!(
            fixture.status.success(),
            "{}",
            String::from_utf8_lossy(&fixture.stderr)
        );
        let file = MediaFile::new(&fixture.stdout).unwrap();
        let preview = decode_video_file(&file, 0., &ffmpeg, true).unwrap();
        let export = decode_video_file(&file, 0., &ffmpeg, false).unwrap();
        assert_eq!(preview.dimensions(), (1280, 720));
        assert_eq!(export.dimensions(), (1920, 1080));
        for (preview, export) in preview
            .get_pixel(500, 500)
            .0
            .into_iter()
            .zip(export.get_pixel(500, 500).0)
        {
            assert!(preview.abs_diff(export) <= 1);
        }
    }

    #[test]
    #[ignore = "requires configured FFmpeg"]
    fn media_decodes_video_and_builds_audio_mix() {
        let ffmpeg = crate::exporter::ffmpeg_path("");
        let audio = Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=0.5",
                "-f",
                "wav",
                "pipe:1",
            ])
            .output()
            .unwrap();
        assert!(audio.status.success());
        let video = Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=red:s=64x48:d=0.5",
                "-c:v",
                "mpeg4",
                "-f",
                "matroska",
                "pipe:1",
            ])
            .output()
            .unwrap();
        assert!(video.status.success());
        let mut doc = crate::fresh_document();
        doc.extras.media.assets.push(Asset {
            id: 1,
            name: "test.wav".into(),
            folder: None,
            raster_tile: false,
            kind: Kind::Audio,
            data: audio.stdout.into(),
            size: [0; 2],
            seconds: 0.5,
            peaks: Default::default(),
        });
        doc.extras.media.assets.push(Asset {
            id: 2,
            name: "test.mkv".into(),
            folder: None,
            raster_tile: false,
            kind: Kind::Video,
            data: video.stdout.into(),
            size: [0; 2],
            seconds: 0.5,
            peaks: Default::default(),
        });
        for (asset, kind) in [(1, Kind::Audio), (2, Kind::Video)] {
            doc.extras.media.tracks.push(Track {
                depth: Default::default(),
                id: asset + 2,
                name: "fixture".into(),
                slot: 0,
                hidden: false,
                locked: false,
                kind,
                clips: vec![Clip {
                    asset,
                    start: 0,
                    offset: 2,
                    length: 6,
                    transform: Default::default(),
                    audio: Default::default(),
                }]
                .into(),
            });
        }
        let mix = prepare_audio(&doc, 0, 6, &ffmpeg).unwrap().unwrap();
        assert!(std::fs::metadata(&mix.0).unwrap().len() > 1024);
        let peak = |bytes: &[u8]| {
            bytes
                .chunks_exact(4)
                .map(|sample| f32::from_le_bytes(sample.try_into().unwrap()).abs())
                .fold(0.0_f32, f32::max)
        };
        let original = std::fs::read(&mix.0).unwrap();
        doc.extras.media.tracks[0].clips[0].audio = AudioMix {
            gain: 0.5,
            fade_in: 2,
            fade_out: 2,
        };
        let quieter = prepare_audio(&doc, 0, 6, &ffmpeg).unwrap().unwrap();
        let quieter = std::fs::read(&quieter.0).unwrap();
        let frame_bytes = 48000 / 24 * 2 * 4;
        assert!(peak(&quieter[..frame_bytes]) < peak(&quieter[2 * frame_bytes..4 * frame_bytes]));
        assert!(peak(&quieter) < peak(&original) * 0.55);
        assert!(
            peak(&quieter[5 * frame_bytes..]) < peak(&quieter[2 * frame_bytes..4 * frame_bytes])
        );
        let mut decoder = VideoExport::default();
        let frames = decoder.frames(&doc, 1, &ffmpeg).unwrap();
        let frame = &frames[0].pixels;
        let [r, g, b, _] = frame.get_pixel(10, 10).0;
        assert!(r > 200 && g < 20 && b < 20);
    }
}

fn waveform(samples: &[u8]) -> SharedVec<u8> {
    samples
        .chunks(320)
        .map(|block| {
            block
                .chunks_exact(2)
                .map(|sample| i16::from_le_bytes([sample[0], sample[1]]).unsigned_abs())
                .max()
                .unwrap_or(0)
                .div_ceil(129) as u8
        })
        .collect()
}

fn draw_waveform(ui: &egui::Ui, rect: Rect, peaks: &[u8], clip: &Clip, fps: u32) {
    let visible = rect.intersect(ui.clip_rect());
    if !visible.is_positive() || peaks.is_empty() {
        return;
    }
    for x in (visible.left().ceil() as i32..visible.right().floor() as i32).step_by(2) {
        let x = x as f32;
        let frame = clip.offset as f32 + (x - rect.left()) / rect.width() * clip.length as f32;
        let index = (frame * 50. / fps.max(1) as f32) as usize;
        let height = peaks.get(index).copied().unwrap_or(0) as f32 / 255. * rect.height() * 0.38;
        let center = rect.center().y;
        ui.painter().line_segment(
            [Pos2::new(x, center - height), Pos2::new(x, center + height)],
            egui::Stroke::new(1., Color32::from_white_alpha(100)),
        );
    }
}

impl Track {
    pub(crate) fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }
}

impl Asset {
    pub(crate) fn icon(&self) -> &'static str {
        match self.kind {
            Kind::Image => "▧",
            Kind::Video => "▶",
            Kind::Audio => "♪",
        }
    }
}

fn show_preview(ui: &mut egui::Ui, texture: &egui::TextureHandle) {
    let size = texture.size_vec2();
    let scale = (ui.available_width() / size.x).min(100. / size.y).min(1.);
    ui.image((texture.id(), size * scale));
}

impl Editor {
    pub(crate) fn depth_video_frames(&self, use_camera: bool) -> Option<Vec<ExportVideoFrame>> {
        let mut result = vec![];
        for (track, clip) in self
            .doc
            .extras
            .media
            .active_videos(self.frame, self.doc.layers.len())
        {
            if use_camera
                && crate::depth::camera(&self.doc, self.frame).is_some_and(|camera| {
                    !crate::depth::visible(
                        &track.depth,
                        &crate::animation::camera_optics(camera, self.frame),
                    )
                })
            {
                continue;
            }
            let asset = self
                .doc
                .extras
                .media
                .assets
                .iter()
                .find(|asset| asset.id == clip.asset)?;
            let key = VideoSampleKey {
                asset: clip.asset,
                frame: clip.offset + self.frame - clip.start,
                fps: self.doc.fps,
                data: asset.data.identity(),
            };
            let preview = self
                .media
                .videos
                .get(&track.id)
                .filter(|preview| preview.presented == Some(key))?;
            if let Some(pixels) = preview.pixels.clone() {
                let mut mark = transformed_video_mark(
                    &self.doc,
                    Vec2::new(pixels.width() as f32, pixels.height() as f32),
                    clip.transform,
                );
                if use_camera && let Some(camera) = crate::depth::camera(&self.doc, self.frame) {
                    let pose = crate::animation::camera_at(camera, self.frame);
                    mark = crate::depth::project_mark(
                        &self.doc,
                        &pose,
                        &pose.optics(camera),
                        &track.depth,
                        &mark,
                    );
                    mark = crate::vector::transform(&mark, |p| {
                        Pos2::new(self.doc.width as f32 / 2., self.doc.height as f32 / 2.)
                            + crate::canvas::rotate(
                                p - crate::vector::pos(pose.center),
                                -pose.angle,
                            ) * pose.zoom
                    });
                }
                result.push(ExportVideoFrame {
                    depth: track.depth.clone(),
                    slot: track.slot,
                    mark,
                    pixels,
                });
            }
        }
        result.sort_by_key(|c| c.slot);
        Some(result)
    }
    pub(crate) fn video_depth_rows(&self) -> Vec<(usize, String, crate::depth::LayerDepth)> {
        self.doc
            .extras
            .media
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.kind == Kind::Video)
            .map(|(i, t)| (i, t.name.clone(), t.depth.clone()))
            .collect()
    }
    pub(crate) fn set_video_depth(&mut self, index: usize, value: crate::depth::LayerDepth) {
        self.checkpoint();
        self.doc.extras.media.tracks[index].depth = value;
        self.recover();
    }
}

impl Editor {
    pub(crate) fn video_snap_rects(&self) -> Vec<Rect> {
        let base = View {
            center: Pos2::ZERO,
            stage_center: Pos2::ZERO,
            scale: 1.,
            angle: 0.,
        };
        let target = self.camera_depth_view(base, self.layer);
        let depth = crate::depth::active(self);
        self.doc
            .extras
            .media
            .active_videos(self.frame, self.doc.layers.len())
            .filter(|(track, _)| {
                self.media
                    .video_drag
                    .is_none_or(|(id, _, _, _)| track.id != id)
            })
            .map(|(track, clip)| {
                let view = if depth {
                    crate::depth::view_for_plane(&self.doc, self.frame, &track.depth, base)
                } else {
                    base
                };
                let mark = self.video_selection_mark(track, clip);
                Rect::from_points(
                    &crate::canvas::vertices(&mark)
                        .iter()
                        .map(|p| target.world(view.screen(*p)))
                        .collect::<Vec<_>>(),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod view_snap_regressions {
    use super::*;
    #[test]
    fn dragging_video_snaps_its_edge_without_needing_a_decoder() {
        let ctx = egui::Context::default();
        let mut e = Editor::new(&eframe::CreationContext::_new_kittest(ctx.clone()));
        e.doc = crate::fresh_document();
        e.doc.width = 400;
        e.doc.height = 300;
        e.tool = crate::canvas::Tool::Object;
        e.studio.prefs.view = Default::default();
        e.studio.prefs.view.snaps.objects = true;
        e.recovery = std::env::temp_dir().join(format!("video-snap-{}.json", std::process::id()));
        e.doc.layers[0].frames[0] = vec![crate::Mark {
            points: vec![[320., 75.], [360., 225.]].into(),
            rectangle: true,
            fill: Some([20, 40, 200, 255]),
            stroke: false,
            ..Default::default()
        }]
        .into();
        e.doc.extras.media.assets.push(Asset {
            id: 42,
            name: "Video".into(),
            folder: None,
            raster_tile: false,
            kind: Kind::Video,
            data: vec![1, 2].into(),
            size: [400, 300],
            seconds: 1.,
            peaks: Default::default(),
        });
        e.doc.extras.media.tracks.push(Track {
            id: 43,
            name: "Video".into(),
            depth: Default::default(),
            slot: 1,
            kind: Kind::Video,
            hidden: false,
            locked: false,
            clips: vec![Clip {
                asset: 42,
                start: 0,
                offset: 0,
                length: 24,
                audio: Default::default(),
                transform: VideoTransform {
                    scale_log2: -1.,
                    ..Default::default()
                },
            }]
            .into(),
        });
        let tick = |e: &mut Editor, events: Vec<egui::Event>| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800., 600.))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    e.video_transform_input(
                        ui,
                        ui.available_rect_before_wrap(),
                        View {
                            center: Pos2::ZERO,
                            stage_center: Pos2::ZERO,
                            scale: 1.,
                            angle: 0.,
                        },
                    );
                },
            );
            output.textures_delta.clear();
        };
        let a = Pos2::new(150., 150.);
        let b = Pos2::new(171., 150.);
        let press = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        tick(&mut e, vec![]);
        tick(&mut e, vec![egui::Event::PointerMoved(a)]);
        tick(&mut e, vec![press(a, true)]);
        tick(&mut e, vec![egui::Event::PointerMoved(b)]);
        tick(&mut e, vec![press(b, false)]);
        assert!((e.doc.extras.media.tracks[0].clips[0].transform.offset[0] - 20.).abs() < 0.01);
    }
}
