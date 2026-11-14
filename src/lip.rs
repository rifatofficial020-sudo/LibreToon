//! Editable mouth poses and local audio-to-pose suggestions.
use crate::{
    Editor,
    design::{self, Icon},
    i18n::tr,
    shared::SharedVec,
};
use eframe::egui;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
enum Shape {
    Rest,
    Closed,
    Open,
    Wide,
    Round,
    Fricative,
    WideHigh,
    RoundHigh,
    Sibilant,
}

impl Shape {
    const ALL: [(Self, &'static str, &'static [&'static str]); 9] = [
        (Self::Rest, "Rest", &["rest", "reposo", "silence"]),
        (
            Self::Closed,
            "Closed",
            &["m", "p", "b", "closed", "cerrada"],
        ),
        (Self::Open, "Open", &["a", "open", "abierta"]),
        (Self::Wide, "Wide", &["e", "ñ", "wide", "estirada"]),
        (Self::Round, "Round", &["o", "eau", "round", "redonda"]),
        (Self::Fricative, "Fricative", &["f", "v"]),
        (Self::WideHigh, "Narrow smile (I)", &["i"]),
        (Self::RoundHigh, "Tight round (U)", &["u"]),
        (Self::Sibilant, "Sibilant (S)", &["s", "z", "sh"]),
    ];
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Pose {
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shape: Option<Shape>,
}
impl From<&str> for Pose {
    fn from(name: &str) -> Self {
        Self {
            name: name.into(),
            shape: None,
        }
    }
}
impl From<String> for Pose {
    fn from(name: String) -> Self {
        Self { name, shape: None }
    }
}
const DEFAULT_POSES: [&str; 9] = ["Rest", "M/P/B", "A", "E", "I", "O", "U", "F/V", "S"];

pub(crate) fn default_poses(frame_count: usize) -> SharedVec<Pose> {
    (0..frame_count.max(DEFAULT_POSES.len()))
        .map(|frame| {
            DEFAULT_POSES
                .get(frame)
                .map_or_else(|| format!("Pose {}", frame + 1).into(), |&name| name.into())
        })
        .collect()
}

impl Pose {
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
    pub fn valid(&self) -> bool {
        !self.name.is_empty() && self.name.len() <= 48
    }

    fn shape(&self) -> Option<Shape> {
        self.shape.or_else(|| {
            let name = self.name.to_lowercase();
            Shape::ALL
                .iter()
                .find(|(_, _, aliases)| {
                    name.split(|c: char| !c.is_alphabetic())
                        .any(|word| aliases.contains(&word))
                })
                .map(|(shape, _, _)| *shape)
        })
    }
}

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Key {
    pub frame: usize,
    pub pose: usize,
    pub manual: bool,
}

pub fn pose_at(keys: &[Key], frame: usize) -> Option<usize> {
    keys.get(
        keys.partition_point(|key| key.frame <= frame)
            .checked_sub(1)?,
    )
    .map(|key| key.pose)
}

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Slot {
    layer: usize,
    mark: usize,
    pub(crate) symbol: u64,
    pub(crate) track: u64,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Route {
    pub(crate) path: Vec<Slot>,
    keys: SharedVec<Key>,
}

pub fn valid_routes(routes: &[Route]) -> bool {
    routes.len() <= 64
        && routes.iter().all(|route| {
            (1..=8).contains(&route.path.len())
                && route.keys.len() <= 10_000
                && route
                    .keys
                    .windows(2)
                    .all(|pair| pair[0].frame < pair[1].frame)
                && route
                    .keys
                    .iter()
                    .all(|key| key.frame < 10_000 && key.pose < 128)
        })
}

pub(crate) fn has_mouth_key(mark: &crate::Mark, frame: usize) -> bool {
    let contains = |keys: &[Key]| keys.binary_search_by_key(&frame, |key| key.frame).is_ok();
    contains(&mark.style.mouth_keys)
        || mark
            .style
            .mouth_routes
            .iter()
            .any(|route| contains(&route.keys))
}

pub(crate) struct RouteScope<'a> {
    routes: &'a [Route],
    frame: usize,
    prefix: Vec<Slot>,
}

impl<'a> RouteScope<'a> {
    pub(crate) fn new(routes: &'a [Route], frame: usize) -> Self {
        Self {
            routes,
            frame,
            prefix: Vec::new(),
        }
    }

    pub(crate) fn pose(&self) -> Option<usize> {
        let route = self.routes.iter().find(|route| {
            route.path.len() == self.prefix.len() && Self::matches(&route.path, &self.prefix)
        })?;
        pose_at(&route.keys, self.frame)
    }

    fn matches(path: &[Slot], prefix: &[Slot]) -> bool {
        path.iter().zip(prefix).all(|(expected, actual)| {
            expected.symbol == actual.symbol
                && if expected.track != 0 {
                    expected.track == actual.track
                } else {
                    expected.layer == actual.layer && expected.mark == actual.mark
                }
        })
    }

    pub(crate) fn child(&self, layer: usize, mark: usize, artwork: &crate::Mark) -> Option<Self> {
        let symbol = artwork.style.instance.as_ref()?.symbol;
        let mut prefix = self.prefix.clone();
        prefix.push(Slot {
            layer,
            mark,
            symbol,
            track: artwork.style.track,
        });
        self.routes
            .iter()
            .any(|route| route.path.len() >= prefix.len() && Self::matches(&route.path, &prefix))
            .then_some(Self {
                routes: self.routes,
                frame: self.frame,
                prefix,
            })
    }
}

#[derive(Clone)]
struct Target {
    root: Slot,
    path: Vec<Slot>,
    name: String,
    poses: SharedVec<Pose>,
    drawn: Vec<bool>,
}

#[derive(Clone, Copy)]
struct AnalysisOptions {
    sensitivity: f32,
    min_hold: usize,
    lead: usize,
}

impl Default for AnalysisOptions {
    fn default() -> Self {
        Self {
            sensitivity: 1.,
            min_hold: 2,
            lead: 0,
        }
    }
}

struct Session {
    audio: Audio,
    targets: Vec<Target>,
    target: usize,
    pose: usize,
    options: AnalysisOptions,
    waveform: SharedVec<u8>,
    frame: usize,
    missing: usize,
    phrase: (usize, usize),
    zoom_phrase: bool,
    media: SharedVec<crate::media::Track>,
}

#[derive(Clone)]
pub(crate) struct Audio {
    pub name: String,
    pub data: SharedVec<u8>,
    pub peaks: SharedVec<u8>,
    pub start: usize,
    pub offset: usize,
    pub length: usize,
}

type Analysis = Result<Vec<Key>, &'static str>;

struct Job {
    layer: usize,
    mark: usize,
    frame: usize,
    path: Vec<Slot>,
    audio_start: usize,
    audio_end: usize,
    owner: SharedVec<crate::document::Layer>,
    media: SharedVec<crate::media::Track>,
    symbols: SharedVec<crate::features::Symbol>,
    receiver: std::sync::mpsc::Receiver<Analysis>,
}

#[derive(Default)]
pub(crate) struct State {
    pose_name: String,
    audio: usize,
    job: Option<Job>,
    session: Option<Session>,
    pub(crate) preview_range: Option<(usize, usize)>,
}

impl Editor {
    pub(crate) fn open_lip_session(&mut self, audio: Audio) {
        let frame = self.frame;
        let mut targets = Vec::new();
        let mut budget = 1024;
        for (layer, data) in self.doc.layers.iter().enumerate().rev() {
            if !data.visible || data.options.locked {
                continue;
            }
            for (mark, artwork) in data.frames[frame].iter().enumerate() {
                if targets.len() >= 64 || budget == 0 {
                    break;
                }
                if let Some(instance) = artwork.style.instance.as_ref() {
                    let root = Slot {
                        layer,
                        mark,
                        symbol: instance.symbol,
                        track: artwork.style.track,
                    };
                    collect_targets(
                        &self.doc,
                        root,
                        instance,
                        &mut Vec::new(),
                        &format!("{} · {}", data.name, mark + 1),
                        frame,
                        &mut targets,
                        &mut budget,
                    );
                }
            }
        }
        let selected = self
            .canvas
            .selected
            .first()
            .and_then(|&(layer, mark)| {
                targets
                    .iter()
                    .position(|target| target.root.layer == layer && target.root.mark == mark)
            })
            .unwrap_or(0);
        let waveform = Wave::read(&audio.data)
            .map(|wav| {
                let start = audio.offset * wav.rate / self.doc.fps as usize;
                let end = (audio.offset + audio.length) * wav.rate / self.doc.fps as usize;
                (0..360)
                    .map(|i| {
                        let a = start + (end - start) * i / 360;
                        let b = start + (end - start) * (i + 1) / 360;
                        let peak = (a..b)
                            .step_by(((b - a) / 32).max(1))
                            .map(|sample| wav.sample(sample).abs())
                            .fold(0f32, f32::max);
                        (peak * 255.) as u8
                    })
                    .collect()
            })
            .unwrap_or_else(|| {
                if audio.peaks.is_empty() {
                    return Default::default();
                }
                (0..360)
                    .map(|i| {
                        let source = audio.offset as f32 + audio.length as f32 * i as f32 / 360.;
                        let peak = (source * 50. / self.doc.fps as f32) as usize;
                        audio.peaks.get(peak).copied().unwrap_or(0)
                    })
                    .collect()
            });
        let start = audio.start;
        let end = start + audio.length.saturating_sub(1);
        let phrase = self
            .feature
            .frame_selection
            .map_or((start, end), |(_, a, b)| {
                let lo = a.min(b).max(start);
                let hi = a.max(b).min(end);
                if lo <= hi { (lo, hi) } else { (start, end) }
            });
        self.lip.session = Some(Session {
            audio,
            targets,
            target: selected,
            pose: 0,
            options: Default::default(),
            waveform,
            frame,
            missing: 0,
            phrase,
            zoom_phrase: false,
            media: self.doc.extras.media.tracks.clone(),
        });
    }

    pub(crate) fn close_lip_session(&mut self) {
        self.lip.session = None;
        self.lip.preview_range = None;
    }

    pub(crate) fn lip_session_ui(&mut self, ctx: &egui::Context) {
        let Some(mut session) = self.lip.session.take() else {
            return;
        };
        if session.media.identity() != self.doc.extras.media.tracks.identity() {
            if self.lip.preview_range.take().is_some() {
                self.playing = false;
                self.restart_media_audio();
            }
            self.status = tr("The target changed. Run Lip Sync again.").into();
            return;
        }
        let mut open = true;
        let mut generate = false;
        let mut edit = None;
        let mut edit_frame = self.frame;
        let end = (session.audio.start + session.audio.length).min(self.doc.layers[0].frames.len());
        let start = session.audio.start;
        let mut run_preview = false;
        egui::Window::new(tr("Dialogue Lip Sync"))
            .id(egui::Id::unique("dialogue-lip-sync"))
            .open(&mut open)
            .default_width(620.)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading(&session.audio.name);
                let Some(target) = session.targets.get(session.target) else {
                    ui.label(tr("No editable Lip Sync mouths at the playhead."));
                    return;
                };
                egui::ComboBox::from_id_salt("dialogue-target")
                    .selected_text(&target.name)
                    .width(350.)
                    .show_ui(ui, |ui| {
                        for (i, target) in session.targets.iter().enumerate() {
                            ui.selectable_value(&mut session.target, i, &target.name);
                        }
                    });
                let target = &session.targets[session.target];
                let _ = self.symbol_thumbnail(
                    ui,
                    target
                        .path
                        .last()
                        .map_or(target.root.symbol, |slot| slot.symbol),
                    session.pose.min(target.poses.len() - 1),
                    egui::vec2(100., 52.),
                );
                let Some(artwork) = self
                    .doc
                    .layers
                    .get(target.root.layer)
                    .filter(|layer| !layer.options.locked)
                    .and_then(|layer| layer.frames.get(session.frame))
                    .and_then(|cel| cel.get(target.root.mark))
                    .filter(|mark| {
                        mark.style
                            .instance
                            .as_ref()
                            .is_some_and(|i| i.symbol == target.root.symbol)
                            && (target.root.track == 0 || target.root.track == mark.style.track)
                    })
                else {
                    ui.label(tr("The target changed. Run Lip Sync again."));
                    return;
                };
                let keys = channel_keys(artwork, &target.path);
                let count = end.saturating_sub(start);
                if count == 0 {
                    return;
                }
                let mut first = session.phrase.0.clamp(start, end - 1) + 1;
                let mut last = session.phrase.1.clamp(first - 1, end - 1) + 1;
                ui.horizontal(|ui| {
                    ui.label(tr("Phrase range"));
                    ui.add(egui::DragValue::new(&mut first).range(start + 1..=end));
                    last = last.max(first);
                    ui.add(egui::DragValue::new(&mut last).range(first..=end));
                });
                session.phrase = (first - 1, last - 1);
                ui.checkbox(&mut session.zoom_phrase, tr("Zoom to phrase"));
                let (view_start, view_end) = if session.zoom_phrase {
                    (first - 1, last)
                } else {
                    (start, end)
                };
                let view_count = view_end - view_start;
                let (wave, response) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width().max(260.), 70.),
                    egui::Sense::click_and_drag(),
                );
                let x_at = |frame: usize| {
                    wave.left()
                        + ((frame as f32 - view_start as f32) / view_count as f32) * wave.width()
                };
                let frame_at = |x: f32| {
                    view_start
                        + (((x - wave.left()) / wave.width()) * view_count as f32)
                            .floor()
                            .clamp(0., (view_count - 1) as f32) as usize
                };
                let painter = ui.painter().with_clip_rect(wave);
                painter.rect_filled(wave, 4., crate::themes::panel());
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(x_at(session.phrase.0), wave.top()),
                        egui::pos2(x_at(session.phrase.1 + 1), wave.bottom()),
                    ),
                    3.,
                    crate::themes::accent().gamma_multiply(0.18),
                );
                for (i, peak) in session.waveform.iter().enumerate() {
                    let frame =
                        start as f32 + i as f32 * count as f32 / session.waveform.len() as f32;
                    if frame < view_start as f32 || frame >= view_end as f32 {
                        continue;
                    }
                    let x = wave.left()
                        + (frame - view_start as f32) / view_count as f32 * wave.width();
                    let amplitude = f32::from(*peak) / 255. * 29.;
                    painter.line_segment(
                        [
                            egui::pos2(x, wave.center().y - amplitude),
                            egui::pos2(x, wave.center().y + amplitude),
                        ],
                        egui::Stroke::new(1., crate::themes::muted()),
                    );
                }
                if (response.clicked() || response.dragged())
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    edit_frame = frame_at(pos.x);
                }
                if (view_start..view_end).contains(&edit_frame) {
                    let x = x_at(edit_frame);
                    painter.line_segment(
                        [egui::pos2(x, wave.top()), egui::pos2(x, wave.bottom())],
                        egui::Stroke::new(1., crate::themes::accent()),
                    );
                }
                let (lane, _) =
                    ui.allocate_exact_size(egui::vec2(wave.width(), 25.), egui::Sense::hover());
                for key in keys
                    .iter()
                    .filter(|key| (view_start..view_end).contains(&key.frame))
                {
                    let x = x_at(key.frame);
                    let pose = target.poses.get(key.pose).map_or("?", |p| p.name.as_str());
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(x, lane.top()),
                        egui::vec2(12., lane.height()),
                    );
                    let widget = ui
                        .interact(
                            rect,
                            ui.scope_id().with(("mouth-key", session.target, key.frame)),
                            egui::Sense::click_and_drag(),
                        )
                        .on_hover_cursor(egui::CursorIcon::Grab);
                    ui.painter().text(
                        rect.left_center(),
                        egui::Align2::LEFT_CENTER,
                        pose,
                        egui::FontId::proportional(11.),
                        crate::themes::accent(),
                    );
                    if widget.clicked() {
                        edit_frame = key.frame;
                        session.pose = key.pose;
                    }
                    if widget.dragged()
                        && let Some(pos) = widget.interact_pointer_pos()
                    {
                        edit_frame = frame_at(pos.x);
                    }
                    if widget.drag_stopped()
                        && let Some(pos) = widget.interact_pointer_pos()
                    {
                        let to = frame_at(pos.x);
                        if !keys.iter().any(|key| key.frame == to) {
                            edit = Some((key.frame, Some((to, key.pose))));
                        }
                    }
                }
                ui.horizontal_wrapped(|ui| {
                    session.pose = session.pose.min(target.poses.len().saturating_sub(1));
                    egui::ComboBox::from_id_salt("dialogue-pose")
                        .selected_text(&target.poses[session.pose].name)
                        .show_ui(ui, |ui| {
                            for (i, pose) in target.poses.iter().enumerate() {
                                ui.selectable_value(&mut session.pose, i, &pose.name);
                            }
                        });
                    let in_range = (start..end).contains(&edit_frame);
                    if design::button(ui, Icon::Mouth, "Set mouth pose", "", false, in_range)
                        .clicked()
                    {
                        edit = Some((edit_frame, Some((edit_frame, session.pose))));
                    }
                    if design::button(
                        ui,
                        Icon::Minus,
                        "",
                        "Delete mouth key",
                        false,
                        in_range && keys.iter().any(|key| key.frame == edit_frame),
                    )
                    .clicked()
                    {
                        edit = Some((edit_frame, None));
                    }
                    run_preview = design::button(
                        ui,
                        Icon::Play,
                        "Preview phrase",
                        "",
                        self.lip.preview_range.is_some() && self.playing,
                        true,
                    )
                    .clicked();
                });
                design::category(ui, "AUTOMATED SYNC");
                let empty = target.drawn.iter().filter(|&&drawn| !drawn).count();
                if empty > 0 {
                    ui.label(format!("{}: {empty}", tr("Empty mouth poses")));
                    egui::ComboBox::from_id_salt("missing-pose")
                        .selected_text(tr("Fallback for empty poses"))
                        .show_ui(ui, |ui| {
                            for (i, pose) in target
                                .poses
                                .iter()
                                .enumerate()
                                .filter(|(i, _)| target.drawn[*i])
                            {
                                ui.selectable_value(&mut session.missing, i, &pose.name);
                            }
                        });
                }
                for (label, value, range) in [
                    ("Minimum pose length", &mut session.options.min_hold, 1..=12),
                    ("Mouth lead (frames)", &mut session.options.lead, 0..=8),
                ] {
                    ui.horizontal(|ui| {
                        ui.label(tr(label));
                        ui.add(egui::DragValue::new(value).range(range));
                    });
                }
                ui.add(
                    egui::Slider::new(&mut session.options.sensitivity, 0.4..=3.)
                        .text(tr("Sensitivity")),
                );
                generate = design::button(
                    ui,
                    Icon::Audio,
                    "Generate mouth keys",
                    "",
                    false,
                    self.lip.job.is_none() && target.drawn.iter().any(|&drawn| drawn),
                )
                .clicked();
                if self.lip.job.is_some() {
                    ui.spinner();
                }
            });
        if !open {
            if self.lip.preview_range.take().is_some() {
                self.playing = false;
                self.restart_media_audio();
            }
            return;
        }
        if edit_frame != self.frame {
            self.frame = edit_frame;
            self.playing = false;
            self.scrub_media_audio(ctx);
        }
        if let Some(target) = session.targets.get(session.target) {
            if let Some((from, key)) = edit {
                self.change_mouth_channel(
                    target.root.layer,
                    target.root.mark,
                    session.frame,
                    &target.path,
                    |keys| {
                        keys.retain(|key| key.frame != from);
                        if let Some((frame, pose)) = key {
                            keys.push(Key {
                                frame,
                                pose,
                                manual: true,
                            });
                        }
                    },
                );
            }
            if generate {
                let fallback = if target.drawn.get(session.missing) == Some(&true) {
                    session.missing
                } else {
                    target.drawn.iter().position(|&drawn| drawn).unwrap_or(0)
                };
                let mut audio = session.audio.clone();
                audio.offset += session.phrase.0 - audio.start;
                audio.start = session.phrase.0;
                audio.length = session.phrase.1 - session.phrase.0 + 1;
                self.start_lip_job(
                    ctx,
                    audio,
                    target.root.layer,
                    target.root.mark,
                    target.poses.clone(),
                    &target.path,
                    session.options,
                    session.frame,
                    target.drawn.clone(),
                    fallback,
                );
            }
        }
        if run_preview && end > start {
            if self.lip.preview_range.take().is_some() && self.playing {
                self.playing = false;
                self.restart_media_audio();
            } else {
                self.frame = session.phrase.0;
                self.lip.preview_range = Some(session.phrase);
                self.playing = false;
                self.toggle_timeline_play();
            }
        }
        self.lip.session = Some(session);
    }

    fn lip_symbol_context(&self) -> Option<usize> {
        let id = self.feature.symbol_stack.last()?.3;
        self.doc
            .extras
            .symbols
            .iter()
            .position(|symbol| symbol.id == id && !symbol.mouth_poses.is_empty())
    }

    pub(crate) fn lip_frame_label(&self, frame: usize) -> Option<String> {
        let symbol = &self.doc.extras.symbols[self.lip_symbol_context()?];
        symbol
            .mouth_poses
            .get(frame)
            .map(|pose| tr(&pose.name).to_owned())
    }

    pub(crate) fn enable_lip_symbol_on_selection(&mut self) -> bool {
        let [(layer, mark)] = self.canvas.selected.as_slice() else {
            return false;
        };
        let Some(instance) = self.doc.layers[*layer].frames[self.frame]
            .get(*mark)
            .and_then(|mark| mark.style.instance.as_ref())
        else {
            return false;
        };
        let id = instance.symbol;
        let Some(index) = self
            .doc
            .extras
            .symbols
            .iter()
            .position(|symbol| symbol.id == id)
        else {
            return false;
        };
        let count = self.doc.extras.symbols[index].layers[0].frames.len();
        if count > 128 {
            self.status = tr("Lip Sync symbols support up to 128 poses.").into();
            return true;
        }
        if self.doc.extras.symbols[index].mouth_poses.is_empty() {
            self.checkpoint();
            let symbol = &mut self.doc.extras.symbols[index];
            symbol.mouth_poses = default_poses(count);
            for layer in &mut symbol.layers {
                layer.init_keys();
                layer
                    .frames
                    .resize_with(symbol.mouth_poses.len(), Default::default);
                layer.options.keys.resize(symbol.mouth_poses.len(), true);
            }
            self.recover();
        }
        true
    }

    pub(crate) fn lip_pose_properties(&mut self, ui: &mut egui::Ui) {
        let Some(index) = self.lip_symbol_context() else {
            return;
        };
        design::category(ui, "Lip Sync poses");
        if let Some(label) = self.lip_frame_label(self.frame) {
            ui.label(label);
        }
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.lip.pose_name)
                    .desired_width(115.)
                    .hint_text(tr("Pose or sound")),
            );
            ui.menu_button(tr("Common poses"), |ui| {
                for pose in DEFAULT_POSES.into_iter().chain(["Ñ", "EAU"]) {
                    if ui.button(tr(pose)).clicked() {
                        self.lip.pose_name = pose.into();
                        ui.close();
                    }
                }
            });
        });
        let name: String = self
            .lip
            .pose_name
            .trim()
            .chars()
            .filter(|c| !c.is_control())
            .take(12)
            .collect();
        let count = self.doc.layers[0].frames.len();
        let mut add = false;
        let mut rename = false;
        let original = self.doc.extras.symbols[index].mouth_poses.get(self.frame);
        let mut shape = original.and_then(|pose| pose.shape);
        let inferred = original.and_then(Pose::shape);
        let selected = shape
            .and_then(|value| {
                Shape::ALL
                    .iter()
                    .find(|(s, _, _)| *s == value)
                    .map(|(_, label, _)| *label)
            })
            .unwrap_or("From pose name");
        if original.is_some() {
            ui.label(tr("Automatic mouth shape"));
            egui::ComboBox::from_id_salt("lip-pose-shape")
                .selected_text(tr(selected))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut shape, None, tr("From pose name"));
                    for (value, label, _) in Shape::ALL {
                        ui.selectable_value(&mut shape, Some(value), tr(label));
                    }
                });
            if shape.is_none()
                && let Some((_, label, _)) =
                    Shape::ALL.iter().find(|(s, _, _)| Some(*s) == inferred)
            {
                ui.small(tr(label));
            }
        }
        let update_shape = shape != original.and_then(|pose| pose.shape);
        ui.horizontal_wrapped(|ui| {
            add = design::button(
                ui,
                Icon::Plus,
                "Add pose",
                "",
                false,
                !name.is_empty() && count < 128,
            )
            .clicked();
            rename = design::button(
                ui,
                Icon::Rename,
                "Name this pose",
                "",
                false,
                !name.is_empty() && self.frame < 128,
            )
            .clicked();
        });
        if add || rename || update_shape {
            self.finish_pending_pen();
            self.checkpoint();
            if add {
                for layer in &mut self.doc.layers {
                    layer.init_keys();
                    layer.frames.push(Default::default());
                    layer.options.keys.push(true);
                }
                self.frame = count;
            }
            let poses = &mut self.doc.extras.symbols[index].mouth_poses;
            let length = poses.len().max(self.frame + 1);
            poses.resize_with(length, || "Rest".into());
            if add || rename {
                poses[self.frame] = Pose {
                    name,
                    shape: if add { None } else { shape },
                };
                self.lip.pose_name.clear();
            } else {
                poses[self.frame].shape = shape;
            }
            self.canvas.clear_selection();
            self.recover();
        }
        ui.separator();
    }

    pub(crate) fn lip_instance_properties(
        &mut self,
        ui: &mut egui::Ui,
        l: usize,
        m: usize,
        symbol: u64,
    ) -> bool {
        let Some(poses) = self
            .doc
            .extras
            .symbols
            .iter()
            .find(|s| s.id == symbol)
            .filter(|s| !s.mouth_poses.is_empty())
            .map(|s| s.mouth_poses.clone())
        else {
            return false;
        };
        let Some(mark) = self.doc.layers[l].frames[self.frame].get(m) else {
            return false;
        };
        let current = pose_at(&mark.style.mouth_keys, self.frame).unwrap_or_else(|| {
            mark.style
                .instance
                .as_ref()
                .map_or(0, |instance| instance.first)
        });
        let has_key = mark
            .style
            .mouth_keys
            .binary_search_by_key(&self.frame, |k| k.frame)
            .is_ok();
        let has_generated = mark.style.mouth_keys.iter().any(|key| !key.manual);
        let unlocked = !self.doc.layers[l].options.locked;
        design::category(ui, "LIPSYNC");
        if design::button(ui, Icon::Node, "Editar símbolo", "", false, true).clicked() {
            self.enter_symbol(symbol);
            return true;
        }
        let previous = mark
            .style
            .mouth_keys
            .iter()
            .rev()
            .find(|key| key.frame < self.frame)
            .map(|key| key.frame);
        let next = mark
            .style
            .mouth_keys
            .iter()
            .find(|key| key.frame > self.frame && key.frame < self.doc.layers[0].frames.len())
            .map(|key| key.frame);
        let mut jump = None;
        ui.horizontal(|ui| {
            for (target, icon, tip) in [
                (previous, Icon::Previous, "Previous mouth key"),
                (next, Icon::Next, "Next mouth key"),
            ] {
                if design::button(ui, icon, "", tip, false, target.is_some()).clicked()
                    && let Some(target) = target
                {
                    jump = Some(target);
                }
            }
        });
        if let Some(target) = jump {
            self.frame = target;
            self.playing = false;
            self.scrub_media_audio(ui.ctx());
            return true;
        }
        ui.add_enabled_ui(unlocked, |ui| {
            ui.small(tr("Edit the symbol to add more mouth poses."));
            ui.horizontal_wrapped(|ui| {
                for (pose, drawing) in poses.iter().enumerate() {
                    if design::button(ui, Icon::Mouth, &drawing.name, "", current == pose, true)
                        .clicked()
                    {
                        let frame = self.frame;
                        self.change_mouth_keys(l, m, frame, |keys| {
                            keys.retain(|key| key.frame != frame);
                            keys.push(Key {
                                frame,
                                pose,
                                manual: true,
                            });
                        });
                    }
                }
                if design::button(ui, Icon::Minus, "", "Delete mouth key", false, has_key).clicked()
                {
                    let frame = self.frame;
                    self.change_mouth_keys(l, m, frame, |keys| {
                        keys.retain(|key| key.frame != frame)
                    });
                }
            });
            if design::button(
                ui,
                Icon::Trash,
                "Clear generated mouth keys",
                "",
                false,
                has_generated,
            )
            .clicked()
            {
                self.change_mouth_keys(l, m, self.frame, |keys| keys.retain(|key| key.manual));
            }
            let audio = self.doc.extras.media.dialogue_clips();
            design::category(ui, "AUTOMATED SYNC");
            ui.small(tr(
                "Right-click an audio clip to sync it with the selected Lip Sync symbol.",
            ));
            if audio.is_empty() {
                ui.small(tr("Add an audio clip to this scene."));
            } else {
                self.lip.audio = self.lip.audio.min(audio.len() - 1);
                egui::ComboBox::from_id_salt("lip-audio")
                    .selected_text(&audio[self.lip.audio].name)
                    .show_ui(ui, |ui| {
                        for (i, clip) in audio.iter().enumerate() {
                            ui.selectable_value(&mut self.lip.audio, i, &clip.name);
                        }
                    });
                if design::button(
                    ui,
                    Icon::Audio,
                    "Generate mouth keys",
                    "",
                    false,
                    self.lip.job.is_none(),
                )
                .clicked()
                {
                    self.open_lip_session(audio[self.lip.audio].clone());
                }
                if self.lip.job.is_some() {
                    ui.spinner();
                }
            }
        });
        true
    }

    pub(crate) fn mouth_timeline_menu(
        &mut self,
        ui: &mut egui::Ui,
        layer: usize,
        frame: usize,
        inspector: bool,
    ) {
        if self.doc.layers[layer].options.locked {
            return;
        }
        let mut channels = Vec::new();
        for (index, mark) in self.doc.layers[layer].frames[frame].iter().enumerate() {
            let Some(instance) = &mark.style.instance else {
                continue;
            };
            let name = self
                .doc
                .extras
                .symbols
                .iter()
                .find(|symbol| symbol.id == instance.symbol)
                .map_or("Symbol", |symbol| symbol.name.as_str());
            let mut targets = Vec::new();
            let root = Slot {
                layer,
                mark: index,
                symbol: instance.symbol,
                track: mark.style.track,
            };
            collect_targets(
                &self.doc,
                root,
                instance,
                &mut Vec::new(),
                name,
                frame,
                &mut targets,
                &mut 256,
            );
            for (channel, path, keys) in std::iter::once((0, &[][..], &mark.style.mouth_keys))
                .chain(
                    mark.style
                        .mouth_routes
                        .iter()
                        .enumerate()
                        .map(|(i, route)| (i + 1, route.path.as_slice(), &route.keys)),
                )
            {
                if let Ok(at) = keys.binary_search_by_key(&frame, |key| key.frame) {
                    let poses = targets
                        .iter()
                        .find(|target| target.path == path)
                        .map(|target| target.poses.clone());
                    channels.push((
                        index,
                        channel,
                        name.to_owned(),
                        path.to_vec(),
                        keys.clone(),
                        keys[at].pose,
                        poses,
                    ));
                }
            }
        }
        for (index, channel, name, path, keys, pose, poses) in channels {
            let label = format!("{} · {} · {}", tr("Lip Sync"), name, channel + 1);
            let actions = |ui: &mut egui::Ui| {
                if let Some(poses) = poses {
                    let choose_pose = |ui: &mut egui::Ui| {
                        for (choice, drawing) in poses.iter().enumerate() {
                            if design::button(
                                ui,
                                Icon::Mouth,
                                &drawing.name,
                                "",
                                choice == pose,
                                true,
                            )
                            .clicked()
                            {
                                self.change_mouth_channel(layer, index, frame, &path, |keys| {
                                    keys.retain(|key| key.frame != frame);
                                    keys.push(Key {
                                        frame,
                                        pose: choice,
                                        manual: true,
                                    });
                                });
                                ui.close();
                            }
                        }
                    };
                    if inspector {
                        egui::ComboBox::from_id_salt(("timeline-mouth-pose", index, channel))
                            .selected_text(
                                poses
                                    .get(pose)
                                    .map_or(tr("Set mouth pose"), |p| p.name.as_str()),
                            )
                            .show_ui(ui, choose_pose);
                    } else {
                        ui.menu_button(tr("Set mouth pose"), choose_pose);
                    }
                }
                for (icon, label, target) in [
                    (Icon::Previous, "Move mouth key left", frame.checked_sub(1)),
                    (
                        Icon::Next,
                        "Move mouth key right",
                        (frame + 1 < self.doc.layers[layer].frames.len()).then_some(frame + 1),
                    ),
                    (Icon::Trash, "Delete mouth key", None),
                ] {
                    let delete = label == "Delete mouth key";
                    let enabled =
                        delete || target.is_some_and(|to| !keys.iter().any(|key| key.frame == to));
                    if design::button(ui, icon, label, "", false, enabled).clicked() {
                        self.change_mouth_channel(layer, index, frame, &path, |keys| {
                            keys.retain(|key| key.frame != frame);
                            if let Some(to) = target {
                                keys.push(Key {
                                    frame: to,
                                    pose,
                                    manual: true,
                                });
                            }
                        });
                        if let Some(to) = target {
                            self.frame = to;
                            self.feature.frame_selection = Some((layer, to, to));
                        }
                        if !inspector {
                            ui.close();
                        }
                    }
                }
                if design::button(
                    ui,
                    Icon::Trash,
                    "Clear generated mouth keys",
                    "",
                    false,
                    keys.iter().any(|key| !key.manual),
                )
                .clicked()
                {
                    self.change_mouth_channel(layer, index, frame, &path, |keys| {
                        keys.retain(|key| key.manual)
                    });
                    if !inspector {
                        ui.close();
                    }
                }
            };
            if inspector {
                ui.label(label);
                actions(ui);
            } else {
                let button =
                    design::button(ui, Icon::Mouth, &format!("{label}  ›"), "", false, true);
                egui::containers::menu::SubMenu::new().show(ui, &button, actions);
            }
        }
    }

    fn change_mouth_keys(
        &mut self,
        layer: usize,
        mark: usize,
        frame: usize,
        edit: impl FnOnce(&mut Vec<Key>),
    ) {
        self.change_mouth_channel(layer, mark, frame, &[], edit);
    }

    fn change_mouth_channel(
        &mut self,
        layer: usize,
        mark: usize,
        frame: usize,
        path: &[Slot],
        edit: impl FnOnce(&mut Vec<Key>),
    ) {
        let old = &self.doc.layers[layer].frames[frame][mark];
        let Some(symbol) = old.style.instance.as_ref().map(|instance| instance.symbol) else {
            return;
        };
        let track = old.style.track;
        let mouth_track = if track == 0 {
            crate::features::id()
        } else {
            track
        };
        let exposure = self.doc.layers[layer].exposure(frame);
        if !path.is_empty()
            && old.style.mouth_routes.len() >= 64
            && !old
                .style
                .mouth_routes
                .iter()
                .any(|route| route.path == path)
        {
            return;
        }
        let mut keys = channel_keys(old, path).to_vec();
        edit(&mut keys);
        keys.sort_by_key(|key| key.frame);
        let keys: SharedVec<_> = keys.into();
        let selection = self.canvas.selected.clone();
        self.checkpoint();
        let layer = &mut self.doc.layers[layer];
        let mut edited = std::collections::HashMap::new();
        for (f, cel) in layer.frames.iter_mut().enumerate() {
            if track == 0 && !exposure.contains(&f) {
                continue;
            }
            let next = edited.entry(cel.identity()).or_insert_with(|| {
                let mut updated = cel.clone();
                if track == 0 {
                    if let Some(mouth) = updated.get_mut(mark) {
                        mouth.style.track = mouth_track;
                        store_channel(mouth, path, &keys);
                    }
                } else if cel.iter().any(|m| {
                    m.style.track == track
                        && m.style
                            .instance
                            .as_ref()
                            .is_some_and(|i| i.symbol == symbol)
                }) {
                    for mouth in &mut updated {
                        if mouth.style.track == track
                            && mouth
                                .style
                                .instance
                                .as_ref()
                                .is_some_and(|i| i.symbol == symbol)
                        {
                            store_channel(mouth, path, &keys);
                        }
                    }
                }
                updated
            });
            *cel = next.clone();
        }
        self.recover();
        self.canvas.selected = selection;
    }

    fn start_lip_job(
        &mut self,
        ctx: &egui::Context,
        audio: Audio,
        layer: usize,
        mark: usize,
        poses: SharedVec<Pose>,
        path: &[Slot],
        options: AnalysisOptions,
        anchor_frame: usize,
        drawn: Vec<bool>,
        fallback: usize,
    ) {
        if self.lip.job.is_some() {
            return;
        }
        let (tx, receiver) = std::sync::mpsc::channel();
        let fps = self.doc.fps;
        let frame_count = self.doc.layers[0].frames.len();
        let ctx = ctx.clone();
        self.lip.job = Some(Job {
            layer,
            mark,
            frame: anchor_frame,
            path: path.to_vec(),
            audio_start: audio.start,
            audio_end: (audio.start + audio.length).min(frame_count),
            owner: self.doc.layers.clone(),
            media: self.doc.extras.media.tracks.clone(),
            symbols: self.doc.extras.symbols.clone(),
            receiver,
        });
        let ffmpeg = crate::exporter::ffmpeg_path(&self.studio.prefs.ffmpeg);
        std::thread::spawn(move || {
            let result = (|| {
                let mut audio = audio;
                if Wave::read(&audio.data).is_none() {
                    audio.data = crate::media::decode_lip_audio(&audio, fps, &ffmpeg)?;
                    audio.offset = 0;
                }
                analyze_with(&audio, fps, frame_count, &poses, &drawn, fallback, options)
            })();
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    pub(crate) fn poll_lip_analysis(&mut self) {
        let Some(job) = self.lip.job.as_ref() else {
            return;
        };
        let result = match job.receiver.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(_) => Err("Lip Sync analysis failed."),
        };
        let job = self.lip.job.take().unwrap();
        if self.doc.layers.identity() != job.owner.identity()
            || self.doc.extras.media.tracks.identity() != job.media.identity()
            || self.doc.extras.symbols.identity() != job.symbols.identity()
        {
            self.status = tr("The target changed. Run Lip Sync again.").into();
            return;
        }
        match result {
            Ok(suggestions) => {
                self.change_mouth_channel(job.layer, job.mark, job.frame, &job.path, |keys| {
                    let resume = pose_at(keys, job.audio_end).unwrap_or(0);
                    keys.retain(|key| {
                        key.manual || !(job.audio_start..job.audio_end).contains(&key.frame)
                    });
                    let retained: std::collections::HashSet<_> =
                        keys.iter().map(|key| key.frame).collect();
                    keys.extend(
                        suggestions
                            .into_iter()
                            .filter(|key| !retained.contains(&key.frame)),
                    );
                    if job.audio_end < 10_000 && !keys.iter().any(|key| key.frame == job.audio_end)
                    {
                        keys.push(Key {
                            frame: job.audio_end,
                            pose: resume,
                            manual: false,
                        });
                    }
                });
                self.status = tr("Lip Sync keys generated.").into();
            }
            Err(error) => self.status = tr(error).into(),
        }
    }
}

fn collect_targets(
    doc: &crate::Document,
    root: Slot,
    instance: &crate::features::Instance,
    path: &mut Vec<Slot>,
    label: &str,
    frame: usize,
    out: &mut Vec<Target>,
    budget: &mut usize,
) {
    if path.len() > 8 || out.len() >= 64 || *budget == 0 {
        return;
    }
    *budget -= 1;
    let Some(symbol) = doc.extras.symbols.iter().find(|s| s.id == instance.symbol) else {
        return;
    };
    let label = format!("{label} › {}", symbol.name);
    if !symbol.mouth_poses.is_empty() {
        let drawn = (0..symbol.mouth_poses.len())
            .map(|pose| {
                symbol.layers.iter().any(|layer| {
                    layer.visible && layer.frames.get(pose).is_some_and(|cel| !cel.is_empty())
                })
            })
            .collect();
        out.push(Target {
            root,
            path: path.clone(),
            name: label,
            poses: symbol.mouth_poses.clone(),
            drawn,
        });
        return;
    }
    let local =
        crate::features::symbol_frame(instance, symbol.layers[0].frames.len(), frame, frame);
    for (layer, data) in symbol
        .layers
        .iter()
        .enumerate()
        .filter(|(_, data)| data.visible)
    {
        for (mark, artwork) in data.frames[local].iter().enumerate() {
            if *budget == 0 || out.len() >= 64 {
                return;
            }
            if let Some(instance) = &artwork.style.instance {
                path.push(Slot {
                    layer,
                    mark,
                    symbol: instance.symbol,
                    track: artwork.style.track,
                });
                collect_targets(
                    doc,
                    root,
                    instance,
                    path,
                    &format!("{label} › {} · {}", data.name, mark + 1),
                    local,
                    out,
                    budget,
                );
                path.pop();
            }
        }
    }
}

fn channel_keys<'a>(mark: &'a crate::Mark, path: &[Slot]) -> &'a [Key] {
    if path.is_empty() {
        &mark.style.mouth_keys
    } else {
        mark.style
            .mouth_routes
            .iter()
            .find(|route| route.path == path)
            .map_or(&[], |route| &route.keys)
    }
}

fn store_channel(mark: &mut crate::Mark, path: &[Slot], keys: &SharedVec<Key>) {
    if path.is_empty() {
        mark.style.mouth_keys = keys.clone();
    } else if let Some(route) = mark
        .style
        .mouth_routes
        .iter_mut()
        .find(|route| route.path == path)
    {
        route.keys = keys.clone();
    } else {
        mark.style.mouth_routes.push(Route {
            path: path.to_vec(),
            keys: keys.clone(),
        });
    }
}

pub(crate) struct Wave<'a> {
    bytes: &'a [u8],
    pub(crate) rate: usize,
    pub(crate) channels: usize,
    width: usize,
    float: bool,
}

impl<'a> Wave<'a> {
    pub(crate) fn read(bytes: &'a [u8]) -> Option<Self> {
        if bytes.get(..4)? != b"RIFF" || bytes.get(8..12)? != b"WAVE" {
            return None;
        }
        let mut data = None;
        let mut format = None;
        let mut cursor = 12;
        while let Some(header) = bytes.get(cursor..cursor + 8) {
            let len = u32::from_le_bytes(header[4..8].try_into().ok()?) as usize;
            let end = cursor.checked_add(8)?.checked_add(len)?;
            let chunk = bytes.get(cursor + 8..end)?;
            match &header[..4] {
                b"fmt " => format = Some(chunk),
                b"data" => data = Some(chunk),
                _ => {}
            }
            cursor = end + len % 2;
        }
        let fmt = format?;
        let u16_at = |i| Some(u16::from_le_bytes(fmt.get(i..i + 2)?.try_into().ok()?));
        let mut kind = u16_at(0)?;
        if kind == 0xfffe
            && fmt.get(26..40)? == [0, 0, 0, 0, 16, 0, 128, 0, 0, 170, 0, 56, 155, 113]
        {
            kind = u16_at(24)?;
        }
        let channels = usize::from(u16_at(2)?);
        let rate = u32::from_le_bytes(fmt.get(4..8)?.try_into().ok()?) as usize;
        let bits = u16_at(14)?;
        let width = usize::from(bits / 8);
        if !(1..=32).contains(&channels)
            || !(4000..=384000).contains(&rate)
            || !(kind == 1 && matches!(bits, 8 | 16 | 24 | 32) || kind == 3 && bits == 32)
            || usize::from(u16_at(12)?) != width * channels
        {
            return None;
        }
        Some(Self {
            bytes: data?,
            rate,
            channels,
            width,
            float: kind == 3,
        })
    }

    pub(crate) fn frames(&self) -> usize {
        self.bytes.len() / (self.width * self.channels)
    }

    pub(crate) fn channel(&self, index: usize, channel: usize) -> f32 {
        let start = (index * self.channels + channel.min(self.channels - 1)) * self.width;
        let Some(raw) = self.bytes.get(start..start + self.width) else {
            return 0.;
        };
        let sample = match raw {
            [a] => (f32::from(*a) - 128.) / 128.,
            [a, b] => i16::from_le_bytes([*a, *b]) as f32 / 32768.,
            [a, b, c] => i32::from_le_bytes([0, *a, *b, *c]) as f32 / 2147483648.,
            [a, b, c, d] if self.float => f32::from_le_bytes([*a, *b, *c, *d]),
            [a, b, c, d] => i32::from_le_bytes([*a, *b, *c, *d]) as f32 / 2147483648.,
            _ => 0.,
        };
        if sample.is_finite() {
            sample.clamp(-1., 1.)
        } else {
            0.
        }
    }

    pub(crate) fn sample(&self, index: usize) -> f32 {
        (0..self.channels)
            .map(|channel| self.channel(index, channel))
            .sum::<f32>()
            / self.channels as f32
    }
}

fn shape_poses(poses: &[Pose], drawn: &[bool], fallback: usize) -> [usize; 9] {
    use Shape::*;
    let find = |shape| {
        poses
            .iter()
            .enumerate()
            .filter(|(i, _)| drawn.is_empty() || drawn.get(*i) == Some(&true))
            .filter(|(_, pose)| pose.shape() == Some(shape))
            .min_by_key(|(_, pose)| pose.shape != Some(shape))
            .map(|(i, _)| i)
    };
    let similar: [&[Shape]; 9] = [
        &[Rest, Closed],
        &[Closed, Rest],
        &[Open, Wide, Round],
        &[Wide, WideHigh, Open],
        &[Round, RoundHigh, Open],
        &[Fricative, Sibilant, Wide],
        &[WideHigh, Wide, Open],
        &[RoundHigh, Round, Open],
        &[Sibilant, Fricative, Wide],
    ];
    similar.map(|shapes| {
        shapes
            .iter()
            .find_map(|&shape| find(shape))
            .unwrap_or(fallback)
    })
}

#[cfg(test)]
fn analyze(audio: &Audio, fps: u32, count: usize, poses: &[Pose]) -> Analysis {
    analyze_with(
        audio,
        fps,
        count,
        poses,
        &[],
        0,
        AnalysisOptions {
            min_hold: 1,
            ..Default::default()
        },
    )
}

struct SpeechWindow {
    energy: f32,
    shape: Shape,
}

const LPC_ORDER: usize = 12;
const SPECTRUM_BINS: usize = 43;

fn speech_window(
    samples: &mut [f32],
    basis: &[[(f32, f32); LPC_ORDER]; SPECTRUM_BINS],
) -> SpeechWindow {
    let mut power = 0.;
    let mut high = 0.;
    let mut previous = 0.;
    let last = samples.len().saturating_sub(1).max(1) as f32;
    for (i, sample) in samples.iter_mut().enumerate() {
        let value = *sample;
        power += value * value;
        high += (value - previous).powi(2);
        *sample = (value - previous * 0.94)
            * (0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / last).cos());
        previous = value;
    }
    let energy = (power / samples.len().max(1) as f32).sqrt();
    let shape = if high > power * 1.35 {
        Shape::Sibilant
    } else if high > power * 0.68 {
        Shape::Fricative
    } else if energy < 0.001 {
        Shape::Rest
    } else {
        vowel_shape(samples, basis)
    };
    SpeechWindow { energy, shape }
}

fn vowel_shape(samples: &[f32], basis: &[[(f32, f32); LPC_ORDER]; SPECTRUM_BINS]) -> Shape {
    let correlations: [f32; LPC_ORDER + 1] = std::array::from_fn(|lag| {
        samples
            .iter()
            .zip(samples.iter().skip(lag))
            .map(|(a, b)| a * b)
            .sum()
    });
    let mut error = correlations[0] * 1.001 + 1e-9;
    let mut filter = [0.; LPC_ORDER];
    for order in 0..LPC_ORDER {
        let partial = correlations[order + 1]
            + (0..order)
                .map(|i| filter[i] * correlations[order - i])
                .sum::<f32>();
        let reflection = (-partial / error.max(1e-9)).clamp(-0.97, 0.97);
        let old = filter;
        for i in 0..order {
            filter[i] += reflection * old[order - 1 - i];
        }
        filter[order] = reflection;
        error *= 1. - reflection * reflection;
    }
    // Use the spectral envelope rather than a few individual harmonics. A change
    // in pitch can move the harmonics without changing the shape of the mouth.
    let envelope: [f32; SPECTRUM_BINS] = basis.map(|band| {
        let (real, imaginary) = filter
            .iter()
            .zip(band)
            .fold((1., 0.), |(r, i), (a, (c, s))| (r + a * c, i + a * s));
        (real * real + imaginary * imaginary).max(1e-8).recip()
    });
    let frequency = |bin| 200 + bin * 75;
    let peak = |min_hz, max_hz| {
        (1..SPECTRUM_BINS - 1)
            .filter(|&i| {
                (min_hz..=max_hz).contains(&frequency(i))
                    && envelope[i] >= envelope[i - 1]
                    && envelope[i] >= envelope[i + 1]
            })
            .max_by(|&a, &b| envelope[a].total_cmp(&envelope[b]))
            .map(frequency)
    };
    let first = peak(240, 1050);
    let second = peak(first.map_or(800, |f| (f + 300).max(800)), 3050);
    match (first, second) {
        (Some(a), Some(b)) if a >= 625 && b >= 1150 => Shape::Open,
        (Some(a), Some(b)) if b <= 1450 => {
            if a < 430 {
                Shape::RoundHigh
            } else {
                Shape::Round
            }
        }
        (Some(a), Some(b)) if b >= 1750 => {
            if a < 450 {
                Shape::WideHigh
            } else {
                Shape::Wide
            }
        }
        (Some(a), _) if a < 400 => Shape::RoundHigh,
        (Some(a), _) if a < 550 => Shape::Round,
        _ => Shape::Open,
    }
}

fn analyze_with(
    audio: &Audio,
    fps: u32,
    count: usize,
    poses: &[Pose],
    drawn: &[bool],
    fallback: usize,
    options: AnalysisOptions,
) -> Analysis {
    let wave =
        Wave::read(&audio.data).ok_or("Automatic Lip Sync requires a PCM or float WAV clip.")?;
    let mapped = shape_poses(poses, drawn, fallback);
    let end = (audio.start + audio.length).min(count);
    if end <= audio.start {
        return Err("The audio clip is outside the scene.");
    }
    let step = (wave.rate / 8000).max(1);
    let sample_rate = wave.rate as f32 / step as f32;
    let basis = std::array::from_fn(|bin| {
        let omega = std::f32::consts::TAU * (200. + 75. * bin as f32) / sample_rate;
        std::array::from_fn(|k| {
            let (s, c) = (omega * (k + 1) as f32).sin_cos();
            (c, s)
        })
    });
    let clip_start = audio.offset * wave.rate / fps as usize;
    let clip_end = (audio.offset + end - audio.start) * wave.rate / fps as usize;
    let half_window = wave.rate * 18 / 1000;
    let mut samples = Vec::with_capacity(640);
    let mut windows = Vec::with_capacity(end - audio.start);
    for frame in audio.start..end {
        let source = audio.offset + frame - audio.start;
        let center = ((2 * source + 1) * wave.rate) / (2 * fps as usize);
        let start = center.saturating_sub(half_window).max(clip_start);
        let stop = (center + half_window).min(clip_end);
        samples.clear();
        samples.extend((start..stop).step_by(step).map(|i| wave.sample(i)));
        windows.push(speech_window(&mut samples, &basis));
    }
    let mut levels: Vec<_> = windows.iter().map(|w| w.energy).collect();
    let reference = levels.len() * 85 / 100;
    let (_, level, _) = levels.select_nth_unstable_by(reference, f32::total_cmp);
    let gate = ((*level * 0.09).max(0.004) / options.sensitivity).max(0.001);
    let mut result: Vec<Key> = Vec::new();
    for (i, window) in windows.iter().enumerate() {
        let next = windows.get(i + 1).map_or(0., |w| w.energy);
        let previous = i.checked_sub(1).map_or(0., |j| windows[j].energy);
        let shape = if window.energy < gate {
            if next >= gate && (previous >= gate || window.energy > gate * 0.35) {
                Shape::Closed
            } else {
                Shape::Rest
            }
        } else if previous > 2.5 * window.energy && next > 2.5 * window.energy {
            Shape::Closed
        } else {
            window.shape
        };
        let pose = mapped[shape as usize];
        if result.last().is_none_or(|key| key.pose != pose) {
            result.push(Key {
                frame: audio.start + i,
                pose,
                manual: false,
            });
        }
    }
    // Merge isolated flicker. A brief change between two different poses can be
    // a real closure or consonant and must not be absorbed into the first pose.
    if options.min_hold > 1 {
        let mut i = 1;
        while i + 1 < result.len() {
            if result[i + 1].frame - result[i].frame < options.min_hold
                && result[i - 1].pose == result[i + 1].pose
            {
                result.drain(i..=i + 1);
            } else {
                i += 1;
            }
        }
    }
    for key in &mut result {
        key.frame = key.frame.saturating_sub(options.lead).max(audio.start);
    }
    result.dedup_by(|next, previous| {
        if next.frame == previous.frame {
            previous.pose = next.pose;
            true
        } else {
            false
        }
    });
    result.dedup_by_key(|key| key.pose);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_mapping_prefers_drawn_poses_and_keeps_vowels_distinct() {
        let poses = default_poses(0);
        let all = [true; 9];
        let mapped = shape_poses(&poses, &all, 0);
        assert_eq!(mapped, [0, 1, 2, 3, 5, 7, 4, 6, 8]);
        let mut sparse = all;
        sparse[2] = false;
        sparse[4] = false;
        assert_eq!(shape_poses(&poses, &sparse, 0)[Shape::Open as usize], 3);
        assert_eq!(shape_poses(&poses, &sparse, 0)[Shape::WideHigh as usize], 3);
    }

    #[test]
    #[ignore = "manual speech sample, requires LIBRETOON_TEST_VOICE pointing to a WAV file"]
    fn manual_lip_sync_speech_pose_distribution() {
        let path = std::env::var_os("LIBRETOON_TEST_VOICE").expect("provide a speech WAV");
        let data = std::fs::read(path).unwrap();
        let wave = Wave::read(&data).unwrap();
        let length = (wave.frames() * 24 / wave.rate).min(10_000);
        let audio = Audio {
            name: String::new(),
            data: data.into(),
            peaks: Default::default(),
            start: 0,
            offset: 0,
            length,
        };
        let poses = default_poses(0);
        let keys = analyze_with(
            &audio,
            24,
            length,
            &poses,
            &[true; 9],
            0,
            AnalysisOptions::default(),
        )
        .unwrap();
        let mut counts = [0; 9];
        for key in keys {
            counts[key.pose] += 1;
        }
        for (pose, count) in poses.iter().zip(counts) {
            println!("{}: {count}", pose.name);
        }
    }

    #[test]
    fn trimmed_wav_analysis_emits_keys_at_scene_frames() {
        let mut wav = crate::media::test_wav();
        for i in 2000..4000 {
            let sample = ((i as f32 * 0.35).sin() * 12000.) as i16;
            wav[44 + i * 2..46 + i * 2].copy_from_slice(&sample.to_le_bytes());
        }
        let audio = Audio {
            name: "dialogue".into(),
            data: wav.into(),
            peaks: Default::default(),
            start: 10,
            offset: 4,
            length: 10,
        };
        let keys = analyze(&audio, 24, 24, &["Rest".into(), "A".into()]).unwrap();
        assert_eq!(pose_at(&keys, 10), Some(0));
        assert_eq!(pose_at(&keys, 13), Some(1));
        assert_eq!(pose_at(&keys, 19), Some(0));
        assert!(
            keys.iter()
                .all(|k| (10..20).contains(&k.frame) && !k.manual)
        );
    }

    #[test]
    fn nested_mouth_channel_resolves_only_for_its_character_instance() {
        let mut editor = Editor::new(&eframe::CreationContext::_new_kittest(
            egui::Context::default(),
        ));
        editor.doc.layers[0].frames[0].push(crate::Mark {
            points: vec![[0., 0.], [10., 10.]].into(),
            rectangle: true,
            ..Default::default()
        });
        editor.canvas.selected = vec![(0, 0)];
        editor.convert_symbol_kind(true);
        let mouth = editor.doc.extras.symbols[0].id;
        let layer = &mut editor.doc.extras.symbols[0].layers[0];
        let mut open = layer.frames[0].clone();
        open[0].points[1][1] = 20.;
        layer.frames[2] = open;
        editor.convert_symbol();
        let twin = editor.doc.layers[0].frames[0][0].clone();
        editor.doc.layers[0].frames[0].push(twin);
        let path = [Slot {
            layer: 0,
            mark: 0,
            symbol: mouth,
            track: 0,
        }];
        editor.change_mouth_channel(0, 0, 0, &path, |keys| {
            keys.push(Key {
                frame: 0,
                pose: 2,
                manual: true,
            })
        });
        let rendered = crate::features::resolve(&editor.doc, 0, false);
        assert_eq!(crate::vector::control_bounds(&rendered[0].2).height(), 20.);
        assert_eq!(crate::vector::control_bounds(&rendered[1].2).height(), 10.);
    }

    #[test]
    fn shared_symbol_mouths_keep_independent_keys_and_roundtrip() {
        let mut editor = Editor::new(&eframe::CreationContext::_new_kittest(
            egui::Context::default(),
        ));
        editor.doc.layers[0].frames[0].push(crate::Mark {
            points: vec![[0., 0.], [10., 10.]].into(),
            rectangle: true,
            ..Default::default()
        });
        editor.canvas.selected = vec![(0, 0)];
        editor.convert_symbol_kind(true);
        let symbol = &mut editor.doc.extras.symbols[0];
        let layer = &mut symbol.layers[0];
        assert_eq!(symbol.mouth_poses.len(), DEFAULT_POSES.len());
        assert_eq!(symbol.mouth_poses[2].name, "A");
        assert!(layer.frames.iter().skip(1).all(|frame| frame.is_empty()));
        assert!((0..layer.frames.len()).all(|frame| layer.key(frame)));
        let mut open = layer.frames[0].clone();
        open[0].points[1][1] = 20.;
        layer.frames[2] = open;
        let second = editor.doc.layers[0].frames[0][0].clone();
        editor.doc.layers[0].frames[0].push(second);
        editor.change_mouth_keys(0, 0, 0, |keys| {
            keys.push(Key {
                frame: 0,
                pose: 2,
                manual: true,
            })
        });
        assert_eq!(editor.canvas.selected, vec![(0, 0)]);
        editor.change_mouth_keys(0, 0, 0, |keys| {
            keys[0].pose = 0;
        });
        assert_eq!(editor.canvas.selected, vec![(0, 0)]);
        editor.change_mouth_keys(0, 0, 0, |keys| {
            keys[0].pose = 2;
        });
        let data = crate::native_project::encode(&editor.doc).unwrap();
        let decoded = crate::native_project::decode_if_native(&data)
            .unwrap()
            .unwrap();
        let resolved = crate::features::resolve(&decoded, 0, false);
        assert_eq!(crate::vector::control_bounds(&resolved[0].2).height(), 20.);
        assert_eq!(crate::vector::control_bounds(&resolved[1].2).height(), 10.);
    }
}
