//! Editable animation assets backed by shared symbol layers and native rigs.
//! Track exposures are the single source of truth; drag previews never copy a project.
use crate::{
    Document, Editor, Mark,
    design::{self, Icon, button},
    document::Layer,
    features::{Instance, Playback, Symbol, SymbolClock, id},
    i18n::tr,
    shared::SharedVec,
};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct Asset {
    #[serde(default)]
    pub folder: Option<u64>,
    pub id: u64,
    pub name: String,
    pub resource: String,
    pub symbol: u64,
    pub start: usize,
    pub end: usize,
    pub looping: bool,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Collection {
    pub folders: SharedVec<crate::library::Folder>,
    pub order: SharedVec<Entry>,
}
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Entry {
    Folder(u64),
    Clip(u64),
}
#[derive(Clone, Copy)]
struct FolderDrag(u64);
impl Collection {
    pub fn is_empty(&self) -> bool {
        self.folders.is_empty() && self.order.is_empty()
    }
    fn can_parent(&self, folder: u64, parent: Option<u64>) -> bool {
        crate::library::Library {
            folders: self.folders.clone(),
            order: Default::default(),
        }
        .can_parent(folder, parent)
    }
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Timing {
    pub asset: u64,
    pub offset: f64,
    pub rate: f64,
    pub looping: bool,
    pub reversed: bool,
    #[serde(default)]
    pub ping_pong: bool,
}
#[derive(Clone, Copy)]
pub struct ClipDrag(pub u64);
#[derive(Clone)]
struct Span {
    start: usize,
    end: usize,
    mark: Mark,
}
#[derive(Clone)]
struct Gesture {
    layer: usize,
    destination: usize,
    drop_area: Rect,
    original: Span,
    preview: Span,
    mode: i8,
    stretch: bool,
    pointer: f32,
    valid: bool,
}
enum Operation {
    Change(usize, usize, Option<Span>),
    Transfer(usize, usize, usize, Span),
    Edit(u64),
    Place(u64, usize, Option<usize>),
}
struct Create {
    name: String,
    resource: String,
    start: usize,
    end: usize,
    layers: Vec<bool>,
    rig: bool,
    root_motion: bool,
    looping: bool,
}
struct CachedTrack {
    frames: SharedVec<SharedVec<Mark>>,
    keys: SharedVec<bool>,
    spans: Vec<Span>,
}
#[derive(Default)]
pub struct Manager {
    pub open: bool,
    #[cfg(test)]
    test_rows: std::collections::HashMap<Entry, Rect>,
    selected_folder: Option<u64>,
    folder_color_transaction: bool,
    rename_focus: bool,
    selected: Option<u64>,
    delete_asset: Option<u64>,
    search: String,
    create: Option<Create>,
    playing: bool,
    clock: Option<std::time::Instant>,
    gesture: Option<Gesture>,
    selected_span: Option<(usize, usize)>,
    cache: std::collections::HashMap<usize, CachedTrack>,
    preview_cache: crate::paint::RenderCache,
    preview_bounds: Option<(u64, SharedVec<Symbol>, Rect)>,
    pending: Vec<Operation>,
}
fn spans(layer: &Layer) -> Vec<Span> {
    let keys: Vec<_> = (0..layer.frames.len()).filter(|&f| layer.key(f)).collect();
    keys.iter()
        .enumerate()
        .filter_map(|(i, &start)| {
            let mark = layer.frames[start].first()?.clone();
            mark.style.animation_clip.as_ref()?;
            Some(Span {
                start,
                end: keys.get(i + 1).copied().unwrap_or(layer.frames.len()),
                mark,
            })
        })
        .collect()
}
fn clear_of_other_clips(list: &[Span], original: usize, preview: &Span) -> bool {
    let first = list.partition_point(|span| span.end <= preview.start);
    list[first..]
        .iter()
        .take_while(|span| span.start < preview.end)
        .all(|span| span.start == original)
}
pub(crate) fn track_end(layer: &Layer) -> usize {
    spans(layer).iter().map(|span| span.end).max().unwrap_or(1)
}
pub(crate) fn resize_track(layer: &mut Layer, count: usize) -> bool {
    if !layer.options.animation_track {
        return false;
    }
    let previous = layer.frames.len();
    layer.init_keys();
    layer.frames.resize(count, Default::default());
    layer.options.keys.resize(count, false);
    if previous < count {
        layer.options.keys[previous] = true;
    }
    true
}
pub fn source_frame(doc: &Document, timing: &Timing, born: usize, frame: usize) -> Option<usize> {
    let asset = doc
        .extras
        .animation_clips
        .iter()
        .find(|a| a.id == timing.asset)?;
    let count = asset.end.checked_sub(asset.start)?.max(1);
    let phase = timing.offset + frame.saturating_sub(born) as f64 * timing.rate;
    let position = if timing.ping_pong && count > 1 {
        let period = 2 * (count - 1);
        let step = stable_floor(phase.rem_euclid(period as f64));
        if step < count { step } else { period - step }
    } else if timing.looping {
        stable_floor(phase.rem_euclid(count as f64)) % count
    } else {
        stable_floor(phase).min(count - 1)
    };
    Some(
        asset.start
            + if timing.reversed {
                count - 1 - position
            } else {
                position
            },
    )
}
fn stable_floor(value: f64) -> usize {
    let rounded = value.round();
    (if (value - rounded).abs() < 1e-8 {
        rounded
    } else {
        value
    })
    .floor()
    .max(0.) as usize
}
fn frame_duration(value: f64) -> usize {
    let rounded = value.round();
    (if (value - rounded).abs() < 1e-8 {
        rounded
    } else {
        value
    })
    .ceil()
    .clamp(1., 10000.) as usize
}
impl Timing {
    fn repeats(&self) -> bool {
        self.looping || self.ping_pong
    }
    fn period(&self, count: usize) -> f64 {
        if self.ping_pong && count > 1 {
            2. * (count - 1) as f64
        } else {
            count as f64
        }
    }
}
fn edit_edge(original: &Span, mode: i8, delta: isize, stretch: bool, count: usize) -> Span {
    let mut preview = original.clone();
    if mode == 0 {
        let length = preview.end - preview.start;
        preview.start =
            (preview.start as isize + delta).clamp(0, (10000 - length) as isize) as usize;
        preview.end = preview.start + length;
    } else if mode == 1 {
        preview.end =
            (preview.end as isize + delta).clamp((preview.start + 1) as isize, 10000) as usize;
        let timing = preview.mark.style.animation_clip.as_mut().unwrap();
        if stretch {
            timing.rate *=
                (original.end - original.start) as f64 / (preview.end - preview.start) as f64;
        } else if !timing.repeats() {
            preview.end = preview.end.min(
                preview.start
                    + frame_duration((count as f64 - timing.offset).max(0.) / timing.rate),
            );
        }
    } else {
        let timing = preview.mark.style.animation_clip.as_mut().unwrap();
        let lower = if stretch || timing.repeats() {
            -(preview.start as isize)
        } else {
            -(stable_floor(timing.offset / timing.rate) as isize)
        };
        let delta = delta.clamp(
            lower.max(-(preview.start as isize)),
            (preview.end - preview.start - 1) as isize,
        );
        preview.start = (preview.start as isize + delta) as usize;
        if stretch {
            timing.rate *=
                (original.end - original.start) as f64 / (preview.end - preview.start) as f64;
        } else {
            let offset = timing.offset + delta as f64 * timing.rate;
            timing.offset = if timing.repeats() {
                offset.rem_euclid(timing.period(count))
            } else {
                offset.max(0.)
            };
        }
    }
    preview
}
fn retime_span(original: &Span, rate: f64) -> Span {
    let mut span = original.clone();
    let timing = span.mark.style.animation_clip.as_mut().unwrap();
    span.end =
        span.start + frame_duration((original.end - original.start) as f64 * timing.rate / rate);
    timing.rate = rate;
    span
}

pub(crate) fn normalize_asset_ranges(doc: &mut Document, symbol_id: u64) {
    let Some(count) = doc
        .extras
        .symbols
        .iter()
        .find(|s| s.id == symbol_id)
        .and_then(|s| s.layers.first())
        .map(|l| l.frames.len())
        .filter(|count| *count > 0)
    else {
        return;
    };
    let updates: Vec<_> = doc
        .extras
        .animation_clips
        .iter()
        .enumerate()
        .filter(|(_, asset)| {
            asset.symbol == symbol_id && (asset.start >= count || asset.end > count)
        })
        .map(|(index, _)| index)
        .collect();
    for index in updates {
        let asset = &mut doc.extras.animation_clips[index];
        asset.start = asset.start.min(count - 1);
        asset.end = asset.end.min(count).max(asset.start + 1);
    }
}
pub fn validate(doc: &Document) -> Result<(), String> {
    validate_collection(doc)?;
    let mut ids = std::collections::HashSet::new();
    if doc.extras.animation_clips.len() > 1000 {
        return Err("Too many animation clips".into());
    }
    for asset in &doc.extras.animation_clips {
        let symbol = doc
            .extras
            .symbols
            .iter()
            .find(|s| s.id == asset.symbol)
            .ok_or("Missing animation source")?;
        if asset.id == 0
            || !ids.insert(asset.id)
            || asset.name.len() > 1024
            || asset.resource.len() > 1024
            || asset.start >= asset.end
            || asset.end > symbol.layers.first().map_or(0, |l| l.frames.len())
        {
            return Err("Invalid animation asset".into());
        }
    }
    let mut seen = std::collections::HashSet::new();
    for layer in doc
        .layers
        .iter()
        .chain(doc.extras.scenes.iter().flat_map(|s| s.layers.iter()))
        .chain(doc.extras.symbols.iter().flat_map(|s| s.layers.iter()))
    {
        for frame in layer
            .frames
            .iter()
            .filter(|frame| seen.insert(frame.identity()))
        {
            for mark in frame {
                if let Some(t) = &mark.style.animation_clip {
                    let asset = doc
                        .extras
                        .animation_clips
                        .iter()
                        .find(|a| a.id == t.asset)
                        .ok_or("Missing animation asset")?;
                    if !t.rate.is_finite()
                        || !(0.01..=100.).contains(&t.rate)
                        || !t.offset.is_finite()
                        || !(0. ..20000.).contains(&t.offset)
                        || mark
                            .style
                            .instance
                            .as_ref()
                            .is_none_or(|i| i.symbol != asset.symbol)
                    {
                        return Err("Invalid animation clip timing".into());
                    }
                }
            }
        }
    }
    Ok(())
}
impl Editor {
    pub(crate) fn begin_animation_clip(&mut self) {
        let count = self.doc.layers[0].frames.len();
        let (layer, a, b) = self
            .feature
            .frame_selection
            .unwrap_or((self.layer, 0, count - 1));
        let rig = !self.doc.extras.rig.is_empty();
        let targets: std::collections::HashSet<_> = self
            .doc
            .extras
            .rig
            .bindings
            .iter()
            .map(|b| b.target)
            .collect();
        self.clips.create = Some(Create {
            name: tr("Animation clip").into(),
            resource: tr("Animated resource").into(),
            start: a.min(count - 1),
            end: b.min(count - 1),
            layers: self
                .doc
                .layers
                .iter()
                .enumerate()
                .map(|(i, l)| {
                    !l.options.animation_track
                        && l.options.mannequin.is_none()
                        && (if rig && !targets.is_empty() {
                            l.frames
                                .iter()
                                .flatten()
                                .any(|m| targets.contains(&m.style.track))
                        } else {
                            i == layer
                        })
                })
                .collect(),
            rig,
            root_motion: true,
            looping: false,
        });
    }
    fn capture_animation_clip(&mut self, create: &Create) -> Result<u64, String> {
        let count = self.doc.layers[0].frames.len();
        if create.start > create.end
            || create.end >= count
            || create.name.trim().is_empty()
            || create.resource.trim().is_empty()
        {
            return Err(tr("Choose a name and a valid frame range.").into());
        }
        let layers: SharedVec<Layer> = self
            .doc
            .layers
            .iter()
            .enumerate()
            .filter(|(i, l)| {
                create.layers.get(*i) == Some(&true)
                    && !l.options.animation_track
                    && l.options.mannequin.is_none()
            })
            .map(|(_, l)| {
                let mut layer = l.clone();
                layer.options.folder = None;
                layer
            })
            .collect();
        if layers.is_empty() {
            return Err(tr("Select at least one layer.").into());
        }
        let targets: std::collections::HashSet<_> = layers
            .iter()
            .flat_map(|l| l.frames.iter().flatten())
            .map(|m| m.style.track)
            .filter(|id| *id != 0)
            .collect();
        let mut rig = if create.rig {
            self.doc.extras.rig.clone()
        } else {
            Default::default()
        };
        rig.bindings.retain(|b| targets.contains(&b.target));
        if !create.root_motion {
            for bone in &mut rig.bones {
                if bone.parent.is_none() {
                    let translation = crate::rig::bone_value(bone, create.start).translation;
                    for key in &mut bone.keys {
                        key.translation = translation;
                    }
                }
            }
        }
        let source = id();
        let asset = id();
        let mut candidate = self.doc.clone();
        candidate.extras.symbols.push(Symbol {
            id: source,
            name: create.name.trim().into(),
            layers,
            rig,
            library_folder: None,
            mouth_poses: Default::default(),
            folders: Default::default(),
            perspective: self.doc.extras.perspective.clone(),
        });
        candidate.extras.animation_clips.push(Asset {
            folder: self.clips.selected_folder,
            id: asset,
            name: create.name.trim().into(),
            resource: create.resource.trim().into(),
            symbol: source,
            start: create.start,
            end: create.end + 1,
            looping: create.looping,
        });
        candidate.validate()?;
        self.checkpoint();
        self.doc = candidate;
        self.clips.selected = Some(asset);
        self.clips.selected_folder = None;
        self.clips.open = true;
        self.feature.library = false;
        self.recover();
        Ok(asset)
    }
    pub(crate) fn place_animation_clip(
        &mut self,
        asset_id: u64,
        at: usize,
        track: Option<usize>,
        point: Option<Pos2>,
    ) -> bool {
        let Some(asset) = self
            .doc
            .extras
            .animation_clips
            .iter()
            .find(|a| a.id == asset_id)
            .cloned()
        else {
            return false;
        };
        if !self.symbol_insert_allowed(asset.symbol) {
            return false;
        }
        let at = at.min(9999);
        let length = (asset.end - asset.start).min(10000 - at);
        let bounds = self
            .animation_source_bounds(asset.symbol, asset.start)
            .unwrap_or(Rect::from_min_size(Pos2::ZERO, Vec2::splat(32.)));
        let offset = point.map_or(Vec2::ZERO, |p| p - bounds.center());
        let mut mark = Mark {
            points: vec![
                crate::vector::xy(bounds.min + offset),
                crate::vector::xy(bounds.max + offset),
            ]
            .into(),
            rectangle: true,
            stroke: false,
            ..Default::default()
        };
        mark.style.track = id();
        mark.style.instance = Some(Instance {
            symbol: asset.symbol,
            matrix: [1., 0., 0., 1., offset.x, offset.y],
            mode: Playback::Once,
            clock: SymbolClock::Scene,
            first: asset.start,
            born: at,
        });
        mark.style.animation_clip = Some(Timing {
            asset: asset_id,
            offset: 0.,
            rate: 1.,
            looping: asset.looping,
            reversed: false,
            ping_pong: false,
        });
        let destination = track.filter(|&l| {
            self.doc
                .layers
                .get(l)
                .is_some_and(|l| l.options.animation_track && !l.options.locked)
        });
        if let Some(l) = destination {
            if spans(&self.doc.layers[l])
                .iter()
                .any(|s| at < s.end && at + length > s.start)
            {
                self.status = tr("Clips cannot overlap on the same track.").into();
                return false;
            }
        }
        self.checkpoint();
        self.finish_pending_pen();
        let end = at + length;
        let count = self.doc.layers[0].frames.len().max(end);
        for layer in &mut self.doc.layers {
            if resize_track(layer, count) {
                continue;
            }
            layer.init_keys();
            let held = if layer.options.animation_track {
                Default::default()
            } else {
                layer.frames.last().cloned().unwrap_or_default()
            };
            layer.frames.resize(count, held);
            layer.options.keys.resize(count, false);
        }
        let layer = if let Some(l) = destination {
            l
        } else {
            let mut layer = Layer {
                name: asset.resource.clone(),
                visible: true,
                frames: vec![SharedVec::default(); count].into(),
                options: Default::default(),
            };
            layer.options.animation_track = true;
            layer.options.keys = vec![false; count].into();
            layer.options.keys[0] = true;
            self.doc.layers.push(layer);
            self.doc.layers.len() - 1
        };
        let mut list = spans(&self.doc.layers[layer]);
        list.push(Span {
            start: at,
            end,
            mark,
        });
        self.write_clip_spans(layer, list);
        self.layer = layer;
        self.frame = at;
        self.canvas.selected = vec![(layer, 0)];
        self.playing = false;
        self.clips.selected = Some(asset_id);
        self.clips.selected_folder = None;
        self.clips.selected_span = Some((layer, at));
        self.activate_tool(crate::canvas::Tool::Object);
        self.fit_camera_duration();
        self.recover();
        true
    }
    fn write_clip_spans(&mut self, layer: usize, list: Vec<Span>) {
        let count = self.doc.layers[0]
            .frames
            .len()
            .max(list.iter().map(|s| s.end).max().unwrap_or(1));
        let target = &mut self.doc.layers[layer];
        target.frames = vec![SharedVec::default(); count].into();
        target.options.keys = vec![false; count].into();
        target.options.keys[0] = true;
        for mut span in list {
            if let Some(i) = &mut span.mark.style.instance {
                i.born = span.start;
            }
            let frame: SharedVec<Mark> = vec![span.mark].into();
            for f in span.start..span.end.min(count) {
                target.frames[f] = frame.clone();
            }
            target.options.keys[span.start] = true;
            if span.end < count {
                target.options.keys[span.end] = true;
            }
        }
        self.clips.cache.remove(&layer);
    }
    fn clip_drop_valid(&self, layer: usize, original: usize, preview: &Span) -> bool {
        !self.doc.layers[layer].options.locked
            && preview.start < preview.end
            && preview.end <= 10000
            && spans(&self.doc.layers[layer])
                .iter()
                .all(|s| s.start == original || preview.end <= s.start || preview.start >= s.end)
    }
    fn change_animation_span(
        &mut self,
        layer: usize,
        original: usize,
        preview: Option<Span>,
    ) -> bool {
        if layer >= self.doc.layers.len()
            || !self.doc.layers[layer].options.animation_track
            || self.doc.layers[layer].options.locked
        {
            return false;
        }
        if preview
            .as_ref()
            .is_some_and(|p| !self.clip_drop_valid(layer, original, p))
        {
            return false;
        }
        let mut list = spans(&self.doc.layers[layer]);
        let index = list.iter().position(|s| s.start == original);
        if index.is_none() && original != usize::MAX {
            return false;
        }
        self.checkpoint();
        if let Some(index) = index {
            list.remove(index);
        }
        let selected = preview.as_ref().map(|p| p.start);
        if let Some(preview) = preview {
            list.push(preview);
        }
        if list.is_empty() {
            self.doc.layers.remove(layer);
            if self.doc.layers.is_empty() {
                let mut fresh = crate::fresh_document().layers[0].clone();
                fresh.frames.resize(self.frame + 1, Default::default());
                fresh.init_keys();
                self.doc.layers.push(fresh);
            }
            self.layer = self.layer.min(self.doc.layers.len() - 1);
            self.frame = self.frame.min(self.doc.layers[0].frames.len() - 1);
            self.clips.selected_span = None;
        } else {
            let count = self.doc.layers[0]
                .frames
                .len()
                .max(list.iter().map(|s| s.end).max().unwrap_or(1));
            for other in &mut self.doc.layers {
                if resize_track(other, count) {
                    continue;
                }
                other.init_keys();
                let held = if other.options.animation_track {
                    Default::default()
                } else {
                    other.frames.last().cloned().unwrap_or_default()
                };
                other.frames.resize(count, held);
                other.options.keys.resize(count, false);
            }
            self.write_clip_spans(layer, list);
            self.clips.selected_span = selected.map(|s| (layer, s));
        }
        self.canvas.clear_selection();
        self.feature.frame_selection = None;
        self.clips.cache.clear();
        self.fit_camera_duration();
        self.recover();
        true
    }
    fn clip_in_use(&self, id: u64) -> bool {
        let refs = |doc: &Document| {
            doc.layers
                .iter()
                .chain(
                    doc.extras
                        .scenes
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| *index != doc.extras.active_scene)
                        .flat_map(|(_, s)| s.layers.iter()),
                )
                .chain(doc.extras.symbols.iter().flat_map(|s| s.layers.iter()))
                .any(|l| {
                    l.frames.iter().flatten().any(|m| {
                        m.style
                            .animation_clip
                            .as_ref()
                            .is_some_and(|t| t.asset == id)
                    })
                })
        };
        refs(&self.doc)
            || self
                .feature
                .symbol_stack
                .iter()
                .any(|(doc, _, _, _)| refs(doc))
    }
    fn delete_animation_asset(&mut self, id: u64) {
        if self.clip_in_use(id) {
            self.clips.delete_asset = Some(id);
            return;
        }
        self.remove_animation_asset(id);
    }
    fn remove_animation_asset(&mut self, id: u64) {
        // Resource deletion is global. Return to the root before checkpointing so
        // Undo restores the whole project, including parent scenes and symbols.
        while !self.feature.symbol_stack.is_empty() {
            self.exit_symbol();
        }
        let Some(asset) = self
            .doc
            .extras
            .animation_clips
            .iter()
            .find(|a| a.id == id)
            .cloned()
        else {
            return;
        };
        self.checkpoint();
        remove_asset_instances(&mut self.doc, id);
        self.layer = self.layer.min(self.doc.layers.len() - 1);
        self.clips.reset_document();
        self.feature.frame_selection = None;
        self.doc.extras.animation_clips.retain(|a| a.id != id);
        self.doc
            .extras
            .animation_library
            .order
            .retain(|entry| *entry != Entry::Clip(id));
        if !self.symbol_in_use(asset.symbol)
            && !self
                .doc
                .extras
                .animation_clips
                .iter()
                .any(|a| a.symbol == asset.symbol)
        {
            self.doc.extras.symbols.retain(|s| s.id != asset.symbol);
        }
        self.clips.selected = None;
        self.recover();
    }
    fn animation_delete_dialog(&mut self, ctx: &egui::Context) {
        let Some(id) = self.clips.delete_asset else {
            return;
        };
        let mut confirm = false;
        let mut cancel = false;
        let response=egui::Modal::new(egui::Id::unique("delete-animation-resource")).show(ctx,|ui|{
            ui.heading(tr("Delete animation?"));
            ui.label(tr("This animation is in use. Delete it and all its placed clips? Other animation resources and artwork are kept. Undo restores the deletion."));
            ui.horizontal(|ui|{
                confirm=button(ui,Icon::Trash,"Delete animation and clips","Delete animation and clips",false,true).clicked();
                cancel=button(ui,Icon::Close,"Cancelar","Cancelar",false,true).clicked();
            });
        });
        cancel |= response.should_close();
        if confirm {
            self.clips.delete_asset = None;
            self.remove_animation_asset(id);
        } else if cancel {
            self.clips.delete_asset = None;
        }
    }
    fn saved_rig_poses_ui(&mut self, ui: &mut egui::Ui) {
        if !self.feature.symbol_stack.is_empty() {
            return;
        }
        let frame = self.group_rig_frame();
        let rig = &self.doc.extras.rig;
        let mut apply = None;
        let mut remove = None;
        let mut save = None;
        egui::CollapsingHeader::new(tr("Rig poses")).show(ui, |ui| {
            ui.horizontal(|ui| {
                let draft_id = ui.scope_id().with("rig-pose-name");
                let mut name = ui
                    .data(|data| data.get_temp::<String>(draft_id))
                    .unwrap_or_default();
                ui.add(
                    egui::TextEdit::singleline(&mut name)
                        .char_limit(64)
                        .hint_text(tr("Pose name")),
                );
                if button(
                    ui,
                    Icon::Save,
                    "",
                    "Save current rig pose",
                    false,
                    !name.trim().is_empty()
                        && !rig.bones.is_empty()
                        && self.doc.extras.rig_poses.len() < 100,
                )
                .clicked()
                {
                    save = Some(crate::rig::SavedPose {
                        name: name.trim().to_owned(),
                        bones: rig
                            .bones
                            .iter()
                            .map(|bone| (bone.id, crate::rig::bone_value(bone, frame)))
                            .collect(),
                    });
                    name.clear();
                }
                ui.data_mut(|data| data.insert_temp(draft_id, name));
            });
            egui::ScrollArea::vertical()
                .max_height(120.)
                .show(ui, |ui| {
                    for (index, pose) in self.doc.extras.rig_poses.iter().enumerate() {
                        ui.horizontal(|ui| {
                            let compatible = !pose.bones.is_empty()
                                && pose
                                    .bones
                                    .iter()
                                    .all(|(id, _)| rig.bones.iter().any(|bone| bone.id == *id));
                            if button(
                                ui,
                                Icon::Play,
                                &pose.name,
                                "Apply pose at playhead",
                                false,
                                compatible,
                            )
                            .clicked()
                            {
                                apply = Some(index);
                            }
                            if button(ui, Icon::Trash, "", "Delete saved pose", false, true)
                                .clicked()
                            {
                                remove = Some(index);
                            }
                        });
                    }
                });
        });
        if let Some(pose) = save {
            self.checkpoint();
            self.doc.extras.rig_poses.push(pose);
            self.recover();
        }
        if let Some(index) = apply {
            let mut pose = self.doc.extras.rig_poses[index].clone();
            self.checkpoint();
            for (id, key) in &mut pose.bones {
                key.frame = frame;
                self.doc.extras.rig.set_bone_key(*id, key.clone());
            }
            self.recover();
        }
        if let Some(index) = remove {
            self.checkpoint();
            self.doc.extras.rig_poses.remove(index);
            self.recover();
        }
    }

    pub(crate) fn animation_manager_ui(&mut self, ui: &mut egui::Ui) {
        self.saved_rig_poses_ui(ui);
        if ui.rect_contains_pointer(ui.max_rect())
            && !ui.ctx().egui_wants_keyboard_input()
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete))
        {
            if let Some(id) = self.clips.selected_folder {
                self.delete_animation_folder(id);
            } else if let Some(id) = self.clips.selected {
                self.delete_animation_asset(id);
            }
        }
        if !ui.input(|i| i.pointer.primary_down()) {
            self.clips.folder_color_transaction = false;
        }
        ui.horizontal_wrapped(|ui| {
            if button(
                ui,
                Icon::FolderPlus,
                "",
                "Nueva carpeta",
                false,
                self.doc.extras.animation_library.folders.len() < 200,
            )
            .clicked()
            {
                self.create_animation_folder();
            }
            if button(
                ui,
                Icon::Rename,
                "",
                "Renombrar",
                false,
                self.clips.selected.is_some() || self.clips.selected_folder.is_some(),
            )
            .clicked()
            {
                self.clips.rename_focus = true;
            }
            if button(ui, Icon::Plus, "", "Create Animation Clip…", false, true).clicked() {
                self.begin_animation_clip();
            }
            if button(
                ui,
                Icon::Copy,
                "",
                "Duplicate animation",
                false,
                self.clips.selected.is_some(),
            )
            .clicked()
            {
                if let Some(mut asset) = self
                    .doc
                    .extras
                    .animation_clips
                    .iter()
                    .find(|a| Some(a.id) == self.clips.selected)
                    .cloned()
                {
                    if let Some(mut symbol) = self
                        .doc
                        .extras
                        .symbols
                        .iter()
                        .find(|s| s.id == asset.symbol)
                        .cloned()
                    {
                        self.checkpoint();
                        symbol.id = id();
                        asset.id = id();
                        asset.symbol = symbol.id;
                        asset.name = format!("{} {}", asset.name, tr("Copy"));
                        self.doc.extras.symbols.push(symbol);
                        self.clips.selected = Some(asset.id);
                        self.clips.selected_folder = None;
                        self.doc.extras.animation_clips.push(asset);
                        self.recover();
                    }
                }
            }
            if button(
                ui,
                Icon::Trash,
                "",
                if self.clips.selected_folder.is_some() {
                    "Eliminar carpeta"
                } else {
                    "Delete animation"
                },
                false,
                self.clips.selected.is_some() || self.clips.selected_folder.is_some(),
            )
            .clicked()
            {
                if let Some(id) = self.clips.selected_folder {
                    self.delete_animation_folder(id);
                } else if let Some(id) = self.clips.selected {
                    self.delete_animation_asset(id);
                }
            }
            ui.label(egui::RichText::new(tr("Animation Manager")).strong());
        });
        ui.add(
            egui::TextEdit::singleline(&mut self.clips.search)
                .hint_text(tr("Search animations…"))
                .desired_width(f32::INFINITY),
        );
        let mut edit = self.animation_collection_ui(ui);
        self.animation_delete_dialog(ui.ctx());
        if self.doc.extras.animation_clips.is_empty() {
            ui.add_space(12.);
            ui.label(tr("Select a frame range, then create an animation clip. Characters, objects and effects are supported."));
        }
        if let Some(asset) = self
            .doc
            .extras
            .animation_clips
            .iter()
            .find(|a| Some(a.id) == self.clips.selected)
            .cloned()
        {
            ui.separator();
            let mut name = asset.name.clone();
            let mut resource = asset.resource.clone();
            ui.horizontal(|ui| {
                ui.label(tr("Name"));
                let response = ui.text_edit_singleline(&mut name);
                if self.clips.rename_focus {
                    response.request_focus();
                    self.clips.rename_focus = false;
                }
                if response.gained_focus() {
                    self.checkpoint();
                }
                if response.changed() {
                    if let Some(a) = self
                        .doc
                        .extras
                        .animation_clips
                        .iter_mut()
                        .find(|a| a.id == asset.id)
                    {
                        a.name = name;
                    }
                    self.recover();
                }
            });
            ui.horizontal(|ui| {
                ui.label(tr("Resource"));
                let response = ui.text_edit_singleline(&mut resource);
                if response.gained_focus() {
                    self.checkpoint();
                }
                if response.changed() {
                    if let Some(a) = self
                        .doc
                        .extras
                        .animation_clips
                        .iter_mut()
                        .find(|a| a.id == asset.id)
                    {
                        a.resource = resource;
                    }
                    self.recover();
                }
            });
            let width = ui.available_width();
            let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 170.), Sense::hover());
            ui.painter().rect_filled(rect, 6., Color32::from_gray(245));
            if let Some(symbol) = self
                .doc
                .extras
                .symbols
                .iter()
                .find(|s| s.id == asset.symbol)
                .cloned()
            {
                let clock = self.clips.clock.get_or_insert_with(std::time::Instant::now);
                let frame = asset.start
                    + if self.clips.playing {
                        ((clock.elapsed().as_secs_f64() * self.doc.fps as f64) as usize)
                            % (asset.end - asset.start)
                    } else {
                        0
                    };
                let mut preview = self.doc.clone();
                preview.layers = symbol.layers;
                preview.extras.rig = symbol.rig;
                preview.extras.cameras = Default::default();
                preview.extras.scenes = Default::default();
                let bounds = if let Some((_, _, bounds)) = self
                    .clips
                    .preview_bounds
                    .as_ref()
                    .filter(|(id, owners, _)| {
                        *id == asset.id && owners.identity() == self.doc.extras.symbols.identity()
                    }) {
                    *bounds
                } else {
                    let mut bounds = Rect::NOTHING;
                    let samples = (asset.end - asset.start).min(8);
                    for sample in 0..samples {
                        let frame = asset.start
                            + sample * (asset.end - asset.start - 1)
                                / samples.saturating_sub(1).max(1);
                        for (_, _, mark) in crate::features::resolve_export(&preview, frame, false)
                        {
                            let radius = if mark.stroke {
                                mark.width
                                    * mark
                                        .width_profile
                                        .iter()
                                        .map(|stop| stop.width)
                                        .fold(1., f32::max)
                                    * 0.5
                            } else {
                                0.
                            };
                            bounds =
                                bounds.union(crate::vector::control_bounds(&mark).expand(radius));
                        }
                    }
                    if !bounds.is_finite() {
                        bounds = Rect::from_min_size(Pos2::ZERO, Vec2::splat(64.));
                    }
                    self.clips.preview_bounds =
                        Some((asset.id, self.doc.extras.symbols.clone(), bounds));
                    bounds
                };
                let view = crate::canvas::View {
                    center: rect.center(),
                    stage_center: bounds.center(),
                    angle: 0.,
                    scale: ((rect.width() - 20.) / bounds.width().max(1.))
                        .min((rect.height() - 20.) / bounds.height().max(1.)),
                };
                self.clips.preview_cache.begin_frame();
                for (_, _, mark) in crate::features::resolve_export(&preview, frame, false) {
                    crate::paint::draw(
                        &ui.painter().with_clip_rect(rect),
                        &mark,
                        view,
                        false,
                        false,
                        Some(&mut self.clips.preview_cache),
                    );
                }
                if self.clips.playing {
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(40));
                }
            }
            ui.horizontal_wrapped(|ui| {
                if button(
                    ui,
                    if self.clips.playing {
                        Icon::Pause
                    } else {
                        Icon::Play
                    },
                    "",
                    "Preview animation",
                    self.clips.playing,
                    true,
                )
                .clicked()
                {
                    self.clips.playing = !self.clips.playing;
                    self.clips.clock = Some(std::time::Instant::now());
                }
                if button(
                    ui,
                    Icon::Pen,
                    "Edit animation",
                    "Edit animation",
                    false,
                    true,
                )
                .clicked()
                {
                    edit = Some(asset.clone());
                }
                if button(ui, Icon::Plus, "Insert clip", "Insert clip", false, true).clicked() {
                    let track = self
                        .doc
                        .layers
                        .get(self.layer)
                        .filter(|l| l.options.animation_track)
                        .map(|_| self.layer);
                    self.place_animation_clip(asset.id, self.frame, track, None);
                }
                ui.small(format!(
                    "{} {} · {:.2} s",
                    asset.end - asset.start,
                    tr("frames"),
                    (asset.end - asset.start) as f32 / self.doc.fps as f32
                ));
            });
        }
        if let Some(asset) = edit {
            self.enter_symbol(asset.symbol);
            self.frame = asset.start.min(self.doc.layers[0].frames.len() - 1);
            self.feature.frame_selection = Some((0, asset.start, asset.end - 1));
            self.clips.playing = false;
            self.clips.open = false;
            self.feature.library = false;
        }
    }
    pub(crate) fn animation_clip_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut create) = self.clips.create.take() else {
            return;
        };
        let mut accept = false;
        let mut cancel = false;
        egui::Modal::new(egui::Id::unique("create-animation-clip")).show(ctx,|ui|{
            ui.set_width(420.);ui.horizontal(|ui|{button(ui,Icon::Film,"","Animation clip",false,false);ui.heading(tr("Create Animation Clip…"));});ui.separator();
            egui::Grid::new("clip-creation-fields").num_columns(2).show(ui,|ui|{
                ui.label(tr("Name"));ui.text_edit_singleline(&mut create.name);ui.end_row();
                ui.label(tr("Resource"));ui.text_edit_singleline(&mut create.resource);ui.end_row();
                ui.label(tr("Frame range"));ui.horizontal(|ui|{let count=self.doc.layers[0].frames.len();let mut start=create.start+1;let mut end=create.end+1;ui.add(egui::DragValue::new(&mut start).range(1..=count));ui.label("–");ui.add(egui::DragValue::new(&mut end).range(1..=count));create.start=start-1;create.end=end-1;});ui.end_row();
            });
            ui.separator();design::category(ui,"LAYERS");
            egui::ScrollArea::vertical().max_height(180.).show(ui,|ui|{for (index,layer) in self.doc.layers.iter().enumerate(){if !layer.options.animation_track && layer.options.mannequin.is_none(){if let Some(enabled)=create.layers.get_mut(index){ui.checkbox(enabled,&layer.name);}}}});
            ui.add_enabled(!self.doc.extras.rig.is_empty(),egui::Checkbox::new(&mut create.rig,tr("Include rig and linked symbols")));
            ui.add_enabled(create.rig,egui::Checkbox::new(&mut create.root_motion,tr("Include root motion"))).on_hover_text(tr("Disable to keep root bone translation fixed while reusing the action."));
            ui.checkbox(&mut create.looping,tr("Loop"));ui.separator();
            ui.small(tr("The original animation stays unchanged. Double-click the clip to edit its shared animation."));
            ui.horizontal(|ui|{accept=button(ui,Icon::Film,"Create clip","Create clip",false,create.start<=create.end&&create.layers.iter().any(|v|*v)).clicked();cancel=button(ui,Icon::Close,"Cancelar","Cancelar",false,true).clicked();});
        });
        if accept {
            if let Err(error) = self.capture_animation_clip(&create) {
                self.status = error;
                self.clips.create = Some(create);
            }
        } else if !cancel {
            self.clips.create = Some(create);
        }
    }
}

impl Editor {
    pub(crate) fn animation_drop_target(
        &mut self,
        ui: &mut egui::Ui,
        row: Rect,
        grid_left: f32,
        cell: f32,
        track: Option<usize>,
    ) {
        if !egui::DragAndDrop::has_payload_of_type::<ClipDrag>(ui.ctx()) {
            return;
        }
        let Some(pos) = ui.input(|i| i.pointer.interact_pos()) else {
            return;
        };
        if !row.intersect(ui.clip_rect()).contains(pos) || pos.x < grid_left {
            return;
        }
        let Some(payload) = egui::DragAndDrop::payload::<ClipDrag>(ui.ctx()) else {
            return;
        };
        let Some(asset) = self
            .doc
            .extras
            .animation_clips
            .iter()
            .find(|a| a.id == payload.0)
        else {
            return;
        };
        let at = (((pos.x - grid_left) / cell).floor().max(0.) as usize).min(9999);
        let duration = (asset.end - asset.start).min(10000 - at);
        let destination = track.filter(|&l| self.doc.layers[l].options.animation_track);
        let valid = destination.is_none_or(|l| {
            !self.doc.layers[l].options.locked
                && spans(&self.doc.layers[l])
                    .iter()
                    .all(|s| at + duration <= s.start || at >= s.end)
        });
        let rect = Rect::from_min_size(
            Pos2::new(grid_left + at as f32 * cell, row.top() + 2.),
            Vec2::new(duration as f32 * cell, row.height() - 4.),
        );
        let color = if valid {
            crate::themes::accent()
        } else {
            Color32::LIGHT_RED
        };
        ui.painter()
            .rect_filled(rect, 3., color.gamma_multiply(0.25));
        ui.painter()
            .rect_stroke(rect, 3., Stroke::new(2., color), egui::StrokeKind::Inside);
        ui.ctx().set_cursor_icon(if valid {
            egui::CursorIcon::Grabbing
        } else {
            egui::CursorIcon::NotAllowed
        });
        if ui.input(|i| i.pointer.primary_released()) {
            egui::DragAndDrop::take_payload::<ClipDrag>(ui.ctx());
            if valid {
                self.clips
                    .pending
                    .push(Operation::Place(payload.0, at, destination));
            }
        }
    }
    pub(crate) fn animation_track_ui(
        &mut self,
        ui: &mut egui::Ui,
        layer: usize,
        row: Rect,
        grid_left: f32,
        cell: f32,
    ) {
        let color = self
            .studio
            .prefs
            .layer_color(crate::studio::LayerColor::Animation);
        let frames_id = self.doc.layers[layer].frames.identity();
        let keys_id = self.doc.layers[layer].options.keys.identity();
        if self.clips.cache.get(&layer).is_none_or(|cache| {
            cache.frames.identity() != frames_id || cache.keys.identity() != keys_id
        }) {
            self.clips.cache.insert(
                layer,
                CachedTrack {
                    frames: self.doc.layers[layer].frames.clone(),
                    keys: self.doc.layers[layer].options.keys.clone(),
                    spans: spans(&self.doc.layers[layer]),
                },
            );
        }
        let list = self.clips.cache[&layer].spans.clone();
        let locked = self.doc.layers[layer].options.locked;
        let clip_rect = ui.clip_rect();
        ui.painter()
            .rect_filled(row, 0., color.gamma_multiply(0.08));
        let background = ui.interact(
            row,
            ui.scope_id().with(("animation-track", layer)),
            Sense::click(),
        );
        if background.clicked() {
            self.layer = layer;
            self.canvas.clear_selection();
            self.clips.selected_span = None;
            if let Some(p) = background.interact_pointer_pos() {
                self.frame = (((p.x - grid_left) / cell).floor().max(0.) as usize)
                    .min(self.doc.layers[0].frames.len() - 1);
                self.playing = false;
            }
        }
        background.context_menu(|ui| {
            if button(
                ui,
                Icon::Plus,
                "Insert clip",
                "Insert clip",
                false,
                !locked && self.clips.selected.is_some(),
            )
            .clicked()
            {
                if let Some(asset) = self.clips.selected {
                    let at = background.interact_pointer_pos().map_or(self.frame, |p| {
                        (((p.x - grid_left) / cell).floor().max(0.) as usize).min(9999)
                    });
                    self.clips
                        .pending
                        .push(Operation::Place(asset, at, Some(layer)));
                }
                ui.close();
            }
        });
        for span in list
            .iter()
            .filter(|span| {
                grid_left + span.end as f32 * cell >= clip_rect.left()
                    && grid_left + span.start as f32 * cell <= clip_rect.right()
            })
            .cloned()
        {
            let rect = Rect::from_min_max(
                Pos2::new(grid_left + span.start as f32 * cell, row.top() + 3.),
                Pos2::new(grid_left + span.end as f32 * cell, row.bottom() - 3.),
            );
            if !rect.intersects(clip_rect) {
                continue;
            }
            let selected = self.clips.selected_span == Some((layer, span.start));
            let asset = span
                .mark
                .style
                .animation_clip
                .as_ref()
                .and_then(|t| {
                    self.doc
                        .extras
                        .animation_clips
                        .iter()
                        .find(|a| a.id == t.asset)
                })
                .cloned();
            let name = asset.as_ref().map_or("?", |a| a.name.as_str());
            ui.painter().rect_filled(
                rect,
                4.,
                color.gamma_multiply(if selected { 0.85 } else { 0.55 }),
            );
            ui.painter().rect_stroke(
                rect,
                4.,
                Stroke::new(
                    if selected { 2. } else { 1. },
                    if selected { Color32::WHITE } else { color },
                ),
                egui::StrokeKind::Inside,
            );
            let visible = rect.intersect(clip_rect);
            let icon_rect =
                Rect::from_min_size(visible.left_top() + Vec2::new(7., 3.), Vec2::splat(14.));
            design::draw(ui.painter(), icon_rect, Icon::Film, Color32::WHITE);
            if visible.width() > 45. {
                ui.painter().with_clip_rect(visible.shrink(3.)).text(
                    visible.left_center() + Vec2::new(25., 0.),
                    egui::Align2::LEFT_CENTER,
                    name,
                    egui::FontId::proportional(11.),
                    Color32::WHITE,
                );
            }
            let response=ui.interact(rect,ui.scope_id().with(("animation-clip",layer,span.start)),Sense::click_and_drag()).on_hover_text(tr("Drag to move · drag edges to trim · Shift-drag edges to stretch · right-click for options"));
            if response.clicked() {
                self.layer = layer;
                self.frame = response.interact_pointer_pos().map_or(span.start, |p| {
                    (((p.x - grid_left) / cell).floor().max(0.) as usize)
                        .clamp(span.start, span.end - 1)
                });
                self.playing = false;
                self.clips.selected_span = Some((layer, span.start));
                self.clips.selected = asset.as_ref().map(|a| a.id);
                self.clips.selected_folder = None;
                self.canvas.selected = vec![(layer, 0)];
            }
            if response.double_clicked() {
                if let Some(asset) = &asset {
                    self.clips.pending.push(Operation::Edit(asset.id));
                }
            }
            if response.drag_started() && !locked {
                if let Some(pos) = ui.input(|i| i.pointer.press_origin()) {
                    let handle = (cell * 0.25).clamp(3., 7.).min(rect.width() * 0.25);
                    let mode = if pos.x < rect.left() + handle {
                        -1
                    } else if pos.x > rect.right() - handle {
                        1
                    } else {
                        0
                    };
                    self.clips.gesture = Some(Gesture {
                        layer,
                        destination: layer,
                        drop_area: row,
                        original: span.clone(),
                        preview: span.clone(),
                        mode,
                        stretch: ui.input(|i| i.modifiers.shift),
                        pointer: pos.x,
                        valid: true,
                    });
                    self.clips.selected_span = Some((layer, span.start));
                }
            }
            if response.hovered() && !locked {
                let pos = ui.input(|i| i.pointer.hover_pos()).unwrap_or(rect.center());
                let handle = (cell * 0.25).clamp(3., 7.).min(rect.width() * 0.25);
                ui.ctx().set_cursor_icon(
                    if pos.x < rect.left() + handle || pos.x > rect.right() - handle {
                        egui::CursorIcon::ResizeHorizontal
                    } else {
                        egui::CursorIcon::Grab
                    },
                );
                for x in [rect.left() + 3., rect.right() - 3.] {
                    ui.painter().line_segment(
                        [
                            Pos2::new(x, rect.top() + 5.),
                            Pos2::new(x, rect.bottom() - 5.),
                        ],
                        Stroke::new(1.5, Color32::WHITE),
                    );
                }
            }
            response.context_menu(|ui| {
                ui.strong(name);
                ui.separator();
                if let Some(asset) = &asset {
                    if button(
                        ui,
                        Icon::Pen,
                        "Edit animation",
                        "Edit animation",
                        false,
                        true,
                    )
                    .clicked()
                    {
                        self.clips.pending.push(Operation::Edit(asset.id));
                        ui.close();
                    }
                }
                let mut changed = span.clone();
                let timing = changed.mark.style.animation_clip.as_mut().unwrap();
                let mut update = false;
                ui.add_enabled_ui(!locked, |ui| {
                    ui.horizontal(|ui| {
                        button(ui, Icon::Tween, "", "Playback speed", false, false);
                        ui.label(tr("Playback speed"));
                        update |= ui
                            .add(
                                egui::DragValue::new(&mut timing.rate)
                                    .range(0.01..=100.)
                                    .speed(0.05)
                                    .suffix("×")
                                    .update_while_editing(false),
                            )
                            .changed();
                    });
                    ui.horizontal(|ui| {
                        button(ui, Icon::Loop, "", "Loop", false, false);
                        let toggled = ui.checkbox(&mut timing.looping, tr("Loop")).changed();
                        if toggled && timing.looping {
                            timing.ping_pong = false;
                        }
                        update |= toggled;
                    });
                    ui.horizontal(|ui| {
                        button(ui, Icon::PingPong, "", "Ping-pong", false, false);
                        let toggled = ui
                            .checkbox(&mut timing.ping_pong, tr("Ping-pong"))
                            .changed();
                        if toggled && timing.ping_pong {
                            timing.looping = false;
                        }
                        update |= toggled;
                    });
                    ui.horizontal(|ui| {
                        button(ui, Icon::Previous, "", "Reverse", false, false);
                        update |= ui.checkbox(&mut timing.reversed, tr("Reverse")).changed();
                    });
                });
                if update {
                    let rate = changed.mark.style.animation_clip.as_ref().unwrap().rate;
                    if rate != span.mark.style.animation_clip.as_ref().unwrap().rate {
                        let retimed = retime_span(&span, rate);
                        changed.end = retimed.end;
                    }
                    if self.clip_drop_valid(layer, span.start, &changed) {
                        self.clips.pending.push(Operation::Change(
                            layer,
                            span.start,
                            Some(changed),
                        ));
                    } else {
                        self.status = tr(
                            "Not enough space to change playback speed. Move the next clip first.",
                        )
                        .into();
                    }
                }
                if button(
                    ui,
                    Icon::Copy,
                    "Duplicate clip",
                    "Duplicate clip",
                    false,
                    !locked,
                )
                .clicked()
                {
                    let mut next = span.clone();
                    next.start = span.end;
                    next.end = next.start + (span.end - span.start);
                    if next.end <= 10000 && self.clip_drop_valid(layer, usize::MAX, &next) {
                        self.clips
                            .pending
                            .push(Operation::Change(layer, usize::MAX, Some(next)));
                    }
                    ui.close();
                }
                if button(
                    ui,
                    Icon::Trash,
                    "Delete clip",
                    "Delete clip",
                    false,
                    !locked,
                )
                .clicked()
                {
                    self.clips
                        .pending
                        .push(Operation::Change(layer, span.start, None));
                    ui.close();
                }
            });
        }
        if let Some(mut gesture) = self.clips.gesture.clone().filter(|g| {
            (g.mode != 0 && g.layer == layer)
                || (g.mode == 0
                    && ui
                        .input(|i| i.pointer.interact_pos())
                        .is_some_and(|p| row.intersect(ui.clip_rect()).contains(p)))
        }) {
            if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                if gesture.mode == 0 {
                    gesture.destination = layer;
                    gesture.drop_area = row.intersect(ui.clip_rect());
                }
                let delta = ((pos.x - gesture.pointer) / cell).round() as isize;
                let timing = gesture.original.mark.style.animation_clip.as_ref().unwrap();
                let count = self
                    .doc
                    .extras
                    .animation_clips
                    .iter()
                    .find(|a| a.id == timing.asset)
                    .map_or(1, |a| a.end - a.start);
                let preview = edit_edge(
                    &gesture.original,
                    gesture.mode,
                    delta,
                    gesture.stretch,
                    count,
                );
                gesture.valid = !locked
                    && preview
                        .mark
                        .style
                        .animation_clip
                        .as_ref()
                        .is_some_and(|t| t.rate.is_finite() && (0.01..=100.).contains(&t.rate))
                    && preview.start < preview.end
                    && preview.end <= 10000
                    && clear_of_other_clips(
                        &list,
                        if gesture.layer == layer {
                            gesture.original.start
                        } else {
                            usize::MAX
                        },
                        &preview,
                    );
                gesture.preview = preview;
                let rect = Rect::from_min_max(
                    Pos2::new(
                        grid_left + gesture.preview.start as f32 * cell,
                        row.top() + 1.,
                    ),
                    Pos2::new(
                        grid_left + gesture.preview.end as f32 * cell,
                        row.bottom() - 1.,
                    ),
                );
                ui.painter().rect_stroke(
                    rect,
                    3.,
                    Stroke::new(
                        2.,
                        if gesture.valid {
                            Color32::WHITE
                        } else {
                            Color32::LIGHT_RED
                        },
                    ),
                    egui::StrokeKind::Inside,
                );
                ui.ctx().set_cursor_icon(if gesture.mode == 0 {
                    egui::CursorIcon::Grabbing
                } else {
                    egui::CursorIcon::ResizeHorizontal
                });
                self.clips.gesture = Some(gesture);
            }
        }
        if self.clips.selected_span.is_some_and(|s| s.0 == layer)
            && !locked
            && !ui.ctx().egui_wants_keyboard_input()
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete))
        {
            let start = self.clips.selected_span.unwrap().1;
            self.clips
                .pending
                .push(Operation::Change(layer, start, None));
        }
    }
    pub(crate) fn finish_animation_clip_ui(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.clips.gesture = None;
        }
        if ctx.input(|i| i.pointer.primary_released()) {
            if let Some(gesture) = self.clips.gesture.take() {
                if gesture.valid
                    && (gesture.mode != 0
                        || ctx
                            .input(|i| i.pointer.interact_pos())
                            .is_some_and(|p| gesture.drop_area.contains(p)))
                {
                    if gesture.destination == gesture.layer {
                        self.clips.pending.push(Operation::Change(
                            gesture.layer,
                            gesture.original.start,
                            Some(gesture.preview),
                        ));
                    } else {
                        self.clips.pending.push(Operation::Transfer(
                            gesture.layer,
                            gesture.original.start,
                            gesture.destination,
                            gesture.preview,
                        ));
                    }
                }
            }
        }
        for operation in std::mem::take(&mut self.clips.pending) {
            match operation {
                Operation::Change(layer, at, span) => {
                    self.change_animation_span(layer, at, span);
                }
                Operation::Transfer(source, start, target, span) => {
                    self.transfer_animation_clip(source, start, target, span);
                }
                Operation::Edit(id) => {
                    if let Some(asset) = self
                        .doc
                        .extras
                        .animation_clips
                        .iter()
                        .find(|a| a.id == id)
                        .cloned()
                    {
                        self.enter_symbol(asset.symbol);
                        self.frame = asset.start.min(self.doc.layers[0].frames.len() - 1);
                    }
                }
                Operation::Place(id, at, layer) => {
                    self.place_animation_clip(id, at, layer, None);
                }
            }
        }
    }
    pub(crate) fn animation_canvas_drop(
        &mut self,
        ui: &mut egui::Ui,
        area: Rect,
        view: crate::canvas::View,
    ) {
        let Some(payload) = egui::DragAndDrop::payload::<ClipDrag>(ui.ctx()) else {
            return;
        };
        let Some(pos) = ui
            .input(|i| i.pointer.interact_pos())
            .filter(|p| area.contains(*p))
        else {
            return;
        };
        let Some(asset) = self
            .doc
            .extras
            .animation_clips
            .iter()
            .find(|a| a.id == payload.0)
        else {
            return;
        };
        let bounds = self
            .animation_source_bounds(asset.symbol, asset.start)
            .unwrap_or(Rect::from_center_size(Pos2::ZERO, Vec2::splat(32.)));
        let rect = Rect::from_center_size(pos, bounds.size() * view.scale);
        let painter = ui.painter().with_clip_rect(area);
        painter.rect_filled(rect, 4., crate::themes::accent().gamma_multiply(0.12));
        painter.rect_stroke(
            rect,
            4.,
            Stroke::new(2., crate::themes::accent()),
            egui::StrokeKind::Inside,
        );
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        if ui.input(|i| i.pointer.primary_released()) {
            egui::DragAndDrop::take_payload::<ClipDrag>(ui.ctx());
            self.place_animation_clip(payload.0, self.frame, None, Some(view.world(pos)));
        }
    }
}

impl Editor {
    pub(crate) fn queue_animation_clip_delete(&mut self, layer: usize) -> bool {
        if !self
            .doc
            .layers
            .get(layer)
            .is_some_and(|l| l.options.animation_track)
        {
            return false;
        }
        if let Some(span) = spans(&self.doc.layers[layer])
            .into_iter()
            .find(|s| (s.start..s.end).contains(&self.frame))
        {
            self.clips
                .pending
                .push(Operation::Change(layer, span.start, None));
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn fixture() -> Editor {
        let mut editor = Editor::new(&eframe::CreationContext::_new_kittest(
            egui::Context::default(),
        ));
        editor.recovery = std::env::temp_dir().join(format!(
            "clips-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ));
        editor.doc = crate::fresh_document();
        for f in 0..24 {
            editor.doc.layers[0].frames[f] = vec![Mark {
                points: vec![[f as f32, 10.], [f as f32 + 10., 20.]].into(),
                rectangle: true,
                fill: Some([80, 100, 220, 255]),
                ..Default::default()
            }]
            .into();
        }
        editor.doc.layers[0].options.keys = Default::default();
        editor
    }
    pub(super) fn create(editor: &mut Editor) -> u64 {
        editor
            .capture_animation_clip(&Create {
                name: "Jump".into(),
                resource: "Actor".into(),
                start: 2,
                end: 7,
                layers: vec![true],
                rig: true,
                root_motion: true,
                looping: false,
            })
            .unwrap()
    }
    #[test]
    fn capture_preserves_shared_editable_art_and_exact_cut_timing() {
        let mut e = fixture();
        let original = e.doc.layers[0].frames[3][0].points.clone();
        let asset = create(&mut e);
        let source = e.doc.extras.animation_clips[0].symbol;
        assert_eq!(
            original.identity(),
            e.doc.extras.symbols[0].layers[0].frames[3][0]
                .points
                .identity()
        );
        assert!(e.place_animation_clip(asset, 10, None, None));
        let layer = e.layer;
        let timing = e.doc.layers[layer].frames[10][0]
            .style
            .animation_clip
            .clone()
            .unwrap();
        assert_eq!(source_frame(&e.doc, &timing, 10, 10), Some(2));
        assert_eq!(source_frame(&e.doc, &timing, 10, 15), Some(7));
        let rendered = crate::features::resolve_export(&e.doc, 11, false);
        assert!(
            rendered
                .iter()
                .any(|(l, _, mark)| *l == layer && mark.points[0][0] == 3.)
        );
        assert!(
            crate::features::resolve_export(&e.doc, 16, false)
                .iter()
                .all(|(l, _, _)| *l != layer)
        );
        e.enter_symbol(source);
        e.doc.layers[0].frames[3][0].points[0][0] = 55.;
        let snapshot = e.project_document();
        assert_eq!(snapshot.extras.animation_clips.len(), 1);
        assert_eq!(snapshot.layers[0].frames[3][0].points[0][0], 3.);
        e.exit_symbol();
        assert_eq!(
            e.doc.extras.symbols[0].layers[0].frames[3][0].points[0][0],
            55.
        );
        assert_eq!(original[0][0], 3.);
        assert!(e.doc.validate().is_ok());
        e.enter_symbol(source);
        e.doc.layers[0].frames.truncate(4);
        e.doc.layers[0].options.keys.truncate(4);
        let saved = e.project_document();
        assert_eq!(saved.extras.animation_clips[0].end, 4);
        assert!(saved.validate().is_ok());
        e.exit_symbol();
        assert!(e.doc.validate().is_ok());
    }
    #[test]
    fn clip_move_trim_stretch_loop_delete_and_undo_preserve_other_tracks() {
        let mut e = fixture();
        let asset = create(&mut e);
        assert!(e.place_animation_clip(asset, 10, None, None));
        let layer = e.layer;
        let original = spans(&e.doc.layers[layer])[0].clone();
        let mut moved = original.clone();
        moved.start = 30;
        moved.end = 36;
        assert!(e.change_animation_span(layer, 10, Some(moved.clone())));
        assert!(e.doc.layers[layer].frames[10].is_empty());
        assert_eq!(e.doc.layers[0].frames.len(), 36);
        assert!(!e.place_animation_clip(asset, 32, Some(layer), None));
        moved.start = 31;
        moved.mark.style.animation_clip.as_mut().unwrap().offset = 1.;
        assert!(e.change_animation_span(layer, 30, Some(moved.clone())));
        let mut stretched = moved.clone();
        stretched.end = 41;
        stretched.mark.style.animation_clip.as_mut().unwrap().rate = 0.5;
        assert!(e.change_animation_span(layer, 31, Some(stretched.clone())));
        let timing = stretched.mark.style.animation_clip.as_mut().unwrap();
        assert_eq!(source_frame(&e.doc, timing, 31, 35), Some(5));
        timing.looping = true;
        timing.reversed = true;
        assert_eq!(source_frame(&e.doc, timing, 31, 43), Some(6));
        let count = e.doc.layers.len();
        assert!(e.change_animation_span(layer, 31, None));
        assert_eq!(e.doc.layers.len(), count - 1);
        assert_eq!(e.doc.extras.animation_clips.len(), 1);
        e.history(false);
        assert_eq!(spans(&e.doc.layers[layer])[0].end, 41);
        assert!(e.doc.validate().is_ok());
        e.delete_animation_asset(asset);
        assert_eq!(e.doc.extras.animation_clips.len(), 1);
        e.change_animation_span(layer, 31, None);
        e.delete_animation_asset(asset);
        assert!(e.doc.extras.animation_clips.is_empty());
        assert!(e.doc.validate().is_ok());
    }
    #[test]
    fn trim_stretch_and_speed_share_the_same_source_window() {
        let mut e = fixture();
        let asset = create(&mut e);
        e.place_animation_clip(asset, 10, None, None);
        let original = spans(&e.doc.layers[e.layer])[0].clone();
        let left = edit_edge(&original, -1, 2, false, 6);
        let trimmed = edit_edge(&left, 1, -2, false, 6);
        let stretched = edit_edge(&trimmed, 1, 4, true, 6);
        let timing = stretched.mark.style.animation_clip.as_ref().unwrap();
        assert_eq!((stretched.start, stretched.end), (12, 18));
        assert!((timing.rate - 1. / 3.).abs() < 1e-12);
        assert_eq!(timing.offset, 2.);
        let frames: Vec<_> = (12..18)
            .map(|f| source_frame(&e.doc, timing, 12, f).unwrap())
            .collect();
        assert_eq!(frames, vec![4, 4, 4, 5, 5, 5]);
        let faster = retime_span(&stretched, 2. / 3.);
        assert_eq!((faster.start, faster.end), (12, 15));
        assert_eq!(
            faster.mark.style.animation_clip.as_ref().unwrap().offset,
            2.
        );
        let restored = retime_span(&faster, 1. / 3.);
        assert_eq!(restored.end, 18);
        assert!(e.change_animation_span(e.layer, 10, Some(restored)));
        assert!(e.doc.validate().is_ok());
        e.history(false);
        assert_eq!(spans(&e.doc.layers[e.layer])[0].end, 16);
    }
    #[test]
    fn extending_loop_left_preserves_fractional_phase_in_both_directions() {
        let mut e = fixture();
        let asset = create(&mut e);
        e.place_animation_clip(asset, 10, None, None);
        for ping_pong in [false, true] {
            for reverse in [false, true] {
                let mut original = spans(&e.doc.layers[e.layer])[0].clone();
                let timing = original.mark.style.animation_clip.as_mut().unwrap();
                timing.looping = !ping_pong;
                timing.ping_pong = ping_pong;
                timing.reversed = reverse;
                timing.rate = 0.5;
                let extended = edit_edge(&original, -1, -3, false, 6);
                assert_eq!(extended.start, 7);
                for f in 10..16 {
                    assert_eq!(
                        source_frame(
                            &e.doc,
                            original.mark.style.animation_clip.as_ref().unwrap(),
                            10,
                            f
                        ),
                        source_frame(
                            &e.doc,
                            extended.mark.style.animation_clip.as_ref().unwrap(),
                            7,
                            f
                        )
                    );
                }
            }
        }
    }
    #[test]
    fn ping_pong_has_no_duplicated_endpoints_and_survives_serialization() {
        let mut e = fixture();
        let asset = create(&mut e);
        e.place_animation_clip(asset, 0, None, None);
        let mut timing = spans(&e.doc.layers[e.layer])[0]
            .mark
            .style
            .animation_clip
            .clone()
            .unwrap();
        timing.ping_pong = true;
        let frames: Vec<_> = (0..11)
            .map(|f| source_frame(&e.doc, &timing, 0, f).unwrap())
            .collect();
        assert_eq!(frames, vec![2, 3, 4, 5, 6, 7, 6, 5, 4, 3, 2]);
        let bytes = serde_json::to_vec(&timing).unwrap();
        let restored: Timing = serde_json::from_slice(&bytes).unwrap();
        assert!(timing == restored);
        let old: Timing = serde_json::from_str(
            r#"{"asset":1,"offset":0,"rate":1,"looping":true,"reversed":false}"#,
        )
        .unwrap();
        assert!(!old.ping_pong);
        e.doc.extras.animation_clips[0].end = 3;
        for f in 0..20 {
            assert_eq!(source_frame(&e.doc, &timing, 0, f), Some(2));
        }
    }
    #[test]
    fn speed_changes_do_not_silently_drop_source_frames_at_scene_limit() {
        let mut e = fixture();
        let asset = create(&mut e);
        e.place_animation_clip(asset, 10, None, None);
        let original = spans(&e.doc.layers[e.layer])[0].clone();
        let mut huge = original.clone();
        huge.end = 10000;
        let too_long = retime_span(&huge, 0.01);
        assert!(too_long.end > 10000);
        assert!(!e.change_animation_span(e.layer, 10, Some(too_long)));
        assert_eq!(spans(&e.doc.layers[e.layer])[0].end, 16);
    }
    #[test]
    fn deleting_a_used_resource_confirms_then_removes_instances_and_undo_restores() {
        let mut e = fixture();
        let asset = create(&mut e);
        e.place_animation_clip(asset, 10, None, None);
        let track = e.layer;
        let original = e.doc.layers[0].frames.clone();
        e.delete_animation_asset(asset);
        assert_eq!(e.clips.delete_asset, Some(asset));
        assert!(e.clip_in_use(asset));
        e.remove_animation_asset(asset);
        assert!(!e.doc.extras.animation_clips.iter().any(|a| a.id == asset));
        assert!(!e.clip_in_use(asset));
        assert_eq!(e.doc.layers.len(), 1);
        assert_eq!(e.doc.layers[0].frames.identity(), original.identity());
        assert!(e.doc.validate().is_ok());
        e.history(false);
        assert!(e.doc.extras.animation_clips.iter().any(|a| a.id == asset));
        assert_eq!(spans(&e.doc.layers[track]).len(), 1);
        assert!(e.doc.validate().is_ok());
    }

    #[test]
    fn animated_instance_selection_follows_the_current_pose() {
        let mut e = fixture();
        let asset = create(&mut e);
        e.place_animation_clip(asset, 10, None, None);
        let layer = e.layer;
        e.doc.extras.symbols[0].layers[0].frames[7][0].points =
            vec![[300., 10.], [310., 20.]].into();
        e.frame = 15;
        let mark = &e.doc.layers[layer].frames[15][0];
        assert!(e.selection_hit(mark, Pos2::new(305., 15.), 0.));
        e.frame = 10;
        let mark = &e.doc.layers[layer].frames[10][0];
        assert!(!e.selection_hit(mark, Pos2::new(305., 15.), 0.));
    }

    #[test]
    fn trimming_slow_motion_preserves_the_fractional_playback_phase() {
        let mut e = fixture();
        let asset = create(&mut e);
        let timing = Timing {
            asset,
            offset: 0.,
            rate: 0.5,
            looping: false,
            reversed: false,
            ping_pong: false,
        };
        let mut trimmed = timing.clone();
        trimmed.offset = 0.5;
        for frame in 1..10 {
            assert_eq!(
                source_frame(&e.doc, &timing, 0, frame),
                source_frame(&e.doc, &trimmed, 1, frame)
            );
        }
    }

    #[test]
    fn extending_other_layers_never_lengthens_an_existing_clip() {
        let mut e = fixture();
        let asset = create(&mut e);
        e.place_animation_clip(asset, 18, None, None);
        let track = e.layer;
        assert_eq!(spans(&e.doc.layers[track])[0].end, 24);
        assert!(e.place_animation_clip(asset, 40, None, None));
        assert_eq!(spans(&e.doc.layers[track])[0].end, 24);
        assert!(e.doc.layers[track].frames[30].is_empty());
        e.queue_animation_clip_delete(track); // Current frame belongs to a different track.
        e.clips.pending.clear();
        e.frame = 20;
        e.queue_animation_clip_duplicate(track);
        e.finish_animation_clip_ui(&egui::Context::default());
        assert_eq!(spans(&e.doc.layers[track]).len(), 2);
    }

    #[test]
    fn rigs_are_kept_editable_and_optional_root_motion_is_isolated() {
        let mut e = fixture();
        e.doc.extras.rig.bones.push(crate::rig::Bone {
            id: id(),
            name: "Root".into(),
            parent: None,
            start: [0., 0.],
            end: [0., 50.],
            radius: 20.,
            ik: false,
            targets: Default::default(),
            keys: vec![
                crate::rig::BoneKey::default(),
                crate::rig::BoneKey {
                    frame: 10,
                    angle: 0.5,
                    translation: [50., 0.],
                    ..Default::default()
                },
            ]
            .into(),
        });
        e.capture_animation_clip(&Create {
            name: "Walk".into(),
            resource: "Actor".into(),
            start: 2,
            end: 7,
            layers: vec![true],
            rig: true,
            root_motion: false,
            looping: true,
        })
        .unwrap();
        let rig = &e.doc.extras.symbols[0].rig;
        assert_eq!(rig.bones[0].keys.len(), 2);
        assert_eq!(
            rig.bones[0].keys[0].translation,
            rig.bones[0].keys[1].translation
        );
        assert_ne!(
            e.doc.extras.rig.bones[0].keys[0].translation,
            e.doc.extras.rig.bones[0].keys[1].translation
        );
        assert_eq!(rig.bones[0].keys[1].angle, 0.5);
    }
    #[test]
    fn moving_between_tracks_deletes_the_empty_source_and_undo_restores_both() {
        let mut e = fixture();
        let asset = create(&mut e);
        e.place_animation_clip(asset, 2, None, None);
        let source = e.layer;
        e.place_animation_clip(asset, 14, None, None);
        let target = e.layer;
        let mut span = spans(&e.doc.layers[source])[0].clone();
        span.start = 3;
        span.end = 9;
        assert!(e.transfer_animation_clip(source, 2, target, span));
        assert_eq!(e.doc.layers.len(), 2);
        assert_eq!(spans(&e.doc.layers[1]).len(), 2);
        e.history(false);
        assert_eq!(e.doc.layers.len(), 3);
        assert_eq!(spans(&e.doc.layers[source]).len(), 1);
        e.change_animation_span(source, 2, None);
        e.change_animation_span(1, 14, None);
        assert!(e.doc.layers.iter().all(|l| !l.options.animation_track));
        assert_eq!(e.doc.extras.animation_clips.len(), 1);
    }

    #[test]
    fn native_fla_and_json_keep_assets_tracks_and_source_frames() {
        let mut e = fixture();
        let id = create(&mut e);
        e.place_animation_clip(id, 3, None, Some(Pos2::new(150., 100.)));
        let mark = &e.doc.layers[e.layer].frames[3][0];
        let source = e.animation_instance_bounds(mark).unwrap();
        let center = crate::rig::point(
            mark.style.instance.as_ref().unwrap().matrix,
            source.center(),
        );
        assert!((center - Pos2::new(150., 100.)).length() < 0.01);
        let before = crate::features::resolve_export(&e.doc, 5, false);
        let json: Document = serde_json::from_slice(&serde_json::to_vec(&e.doc).unwrap()).unwrap();
        assert!(json.validate().is_ok());
        let restored = crate::fla::decode(&crate::fla::encode(&e.doc).unwrap()).unwrap();
        assert_eq!(restored.extras.animation_clips.len(), 1);
        assert!(restored.layers[e.layer].options.animation_track);
        let after = crate::features::resolve_export(&restored, 5, false);
        assert_eq!(before.len(), after.len());
        for (a, b) in before.iter().zip(after.iter()) {
            assert_eq!(a.2.points, b.2.points);
        }
        let mut bad = json;
        bad.layers[e.layer].frames[3][0]
            .style
            .animation_clip
            .as_mut()
            .unwrap()
            .rate = f64::NAN;
        assert!(bad.validate().is_err());
    }
    #[test]
    fn complete_short_round_trips_native_scenes_clips_audio_references_and_png_export() {
        let mut editor = fixture();
        editor.doc.width = 64;
        editor.doc.height = 64;
        let clip = create(&mut editor);
        assert!(editor.place_animation_clip(clip, 0, None, Some(Pos2::new(24., 24.))));
        let sound = crate::media::imported_audio(crate::media::test_wav(), "Voice".into()).unwrap();
        let track =
            crate::media::imported_sound_track("Dialogue".into(), vec![(&sound, 0, 0, 12)], 24)
                .unwrap();
        editor.doc.extras.media.assets.push(sound);
        editor.doc.extras.media.tracks.push(track);
        editor.doc.extras.markers.push(crate::dialogue::Marker {
            frame: 0,
            name: "Opening".into(),
            color: [91, 158, 255],
        });
        let mut reference = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 255, 255]))
            .write_to(&mut reference, image::ImageFormat::Png)
            .unwrap();
        editor
            .doc
            .extras
            .references
            .push(crate::reftool::Reference {
                id: crate::features::id(),
                name: "Reference".into(),
                dimensions: [2, 2],
                data: reference.into_inner().into(),
                position: [140., 120.],
                size: [350., 350.],
                zoom: 0.,
            });
        let first =
            crate::animation::scene_from(&editor.doc, crate::features::id(), "Opening".into());
        let second = crate::animation::scene_from(
            &crate::fresh_document(),
            crate::features::id(),
            "End".into(),
        );
        editor.doc.extras.scenes = vec![first, second].into();
        let dir = std::env::temp_dir().join(format!("libretoon-short-{}", crate::features::id()));
        std::fs::create_dir(&dir).unwrap();
        let project = dir.join("short.ltoon");
        crate::write_document(&project, &editor.project_document()).unwrap();
        let restored = crate::documents::read_project(&project).unwrap();
        assert_eq!(restored.extras.scenes.len(), 2);
        assert_eq!(restored.extras.animation_clips.len(), 1);
        assert_eq!(restored.extras.media.tracks.len(), 1);
        assert_eq!(restored.extras.markers[0].name, "Opening");
        assert_eq!(restored.extras.references[0].dimensions, [2, 2]);
        let settings = crate::exporter::ExportSettings {
            name: "short".into(),
            folder: dir.to_string_lossy().into_owned(),
            format: 6,
            first: 1,
            last: 25,
            width: 64,
            height: 64,
            fps: 24,
            quality: 95,
            loops: 0,
            colors: 256,
            dither: false,
            svg_sequence: false,
            sheet: Default::default(),
        };
        crate::exporter::run_export(
            &restored,
            &settings,
            0,
            &dir.join("short.png"),
            dir.join("no-ffmpeg"),
            Default::default(),
            Default::default(),
        )
        .unwrap();
        let art = image::open(dir.join("short-00001.png"))
            .unwrap()
            .into_rgba8();
        let blank = image::open(dir.join("short-00025.png"))
            .unwrap()
            .into_rgba8();
        assert_eq!(art.dimensions(), (64, 64));
        assert!(art.pixels().any(|pixel| pixel[3] == 255));
        assert_eq!(art.get_pixel(63, 63)[3], 0);
        assert!(blank.pixels().all(|pixel| pixel[3] == 0));
        std::fs::remove_dir_all(dir).unwrap();
    }
    fn button_at(pos: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }
    fn tick(ctx: &egui::Context, e: &mut Editor, events: Vec<egui::Event>) {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000., 250.))),
                events,
                ..Default::default()
            },
            |ui| {
                let row = Rect::from_min_max(Pos2::new(100., 60.), Pos2::new(900., 88.));
                if e.doc.layers.len() > 1 {
                    e.animation_track_ui(ui, 1, row, 100., 20.);
                }
            },
        );
        output.textures_delta.clear();
        e.finish_animation_clip_ui(ctx);
    }
    #[test]
    fn timeline_pointer_moves_and_trims_clips_without_inserting_keyframes() {
        let mut e = fixture();
        let asset = create(&mut e);
        e.place_animation_clip(asset, 2, None, None);
        let ctx = egui::Context::default();
        tick(&ctx, &mut e, vec![]);
        let from = Pos2::new(190., 74.);
        let to = from + Vec2::new(40., 0.);
        tick(&ctx, &mut e, vec![egui::Event::PointerMoved(from)]);
        tick(&ctx, &mut e, vec![button_at(from, true)]);
        tick(&ctx, &mut e, vec![egui::Event::PointerMoved(to)]);
        tick(&ctx, &mut e, vec![button_at(to, false)]);
        let list = spans(&e.doc.layers[1]);
        assert_eq!((list[0].start, list[0].end), (4, 10));
        assert_eq!(list.len(), 1);
        let from = Pos2::new(298., 74.);
        let to = from - Vec2::new(40., 0.);
        tick(&ctx, &mut e, vec![egui::Event::PointerMoved(from)]);
        tick(&ctx, &mut e, vec![button_at(from, true)]);
        tick(&ctx, &mut e, vec![egui::Event::PointerMoved(to)]);
        tick(&ctx, &mut e, vec![button_at(to, false)]);
        let list = spans(&e.doc.layers[1]);
        assert_eq!((list[0].start, list[0].end), (4, 8));
        assert_eq!(list.len(), 1);
    }
    #[test]
    fn releasing_shift_before_mouse_keeps_stretch_mode() {
        let mut e = fixture();
        let asset = create(&mut e);
        e.place_animation_clip(asset, 2, None, None);
        let ctx = egui::Context::default();
        let run = |e: &mut Editor, mut events: Vec<egui::Event>, shift: bool| {
            events.insert(
                0,
                egui::Event::ModifiersChanged(egui::Modifiers {
                    shift,
                    ..Default::default()
                }),
            );
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000., 250.))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    e.animation_track_ui(
                        ui,
                        1,
                        Rect::from_min_max(Pos2::new(100., 60.), Pos2::new(900., 88.)),
                        100.,
                        20.,
                    )
                },
            );
            output.textures_delta.clear();
            e.finish_animation_clip_ui(&ctx);
        };
        run(&mut e, vec![], false);
        let from = Pos2::new(258., 74.);
        let to = Pos2::new(378., 74.);
        run(&mut e, vec![egui::Event::PointerMoved(from)], true);
        let press = |pos, pressed, shift| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers {
                shift,
                ..Default::default()
            },
        };
        run(&mut e, vec![press(from, true, true)], true);
        run(&mut e, vec![egui::Event::PointerMoved(to)], true);
        assert!(e.clips.gesture.as_ref().unwrap().stretch);
        run(&mut e, vec![press(to, false, false)], false);
        let list = spans(&e.doc.layers[1]);
        assert_eq!((list[0].start, list[0].end), (2, 14));
        assert_eq!(
            list[0].mark.style.animation_clip.as_ref().unwrap().rate,
            0.5
        );
    }
    #[test]
    fn resource_deletion_preserves_other_resources_and_cleans_other_scenes() {
        let mut e = fixture();
        let asset = create(&mut e);
        e.place_animation_clip(asset, 2, None, None);
        let scene = crate::animation::scene_from(&e.doc, id(), "Other".into());
        e.doc.extras.scenes.push(scene.clone());
        let mut other_scene = scene;
        other_scene.id = id();
        e.doc.extras.scenes.push(other_scene);
        e.doc.extras.active_scene = 0;
        let mut other = e.doc.extras.animation_clips[0].clone();
        other.id = id();
        let other_id = other.id;
        let shared_symbol = other.symbol;
        e.doc.extras.animation_clips.push(other);
        e.remove_animation_asset(asset);
        assert!(
            e.doc
                .extras
                .animation_clips
                .iter()
                .any(|a| a.id == other_id)
        );
        assert!(e.doc.extras.symbols.iter().any(|s| s.id == shared_symbol));
        assert!(!e.clip_in_use(asset));
        assert!(e.doc.extras.scenes.iter().all(|s| s.layers.len() == 1));
        assert!(e.doc.validate().is_ok());
    }
    #[test]
    fn manager_and_creation_dialog_render_without_touching_document() {
        let mut e = fixture();
        create(&mut e);
        let snapshot = serde_json::to_vec(&e.doc).unwrap();
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(370., 800.))),
                ..Default::default()
            },
            |ui| {
                e.animation_manager_ui(ui);
            },
        );
        if let Some(path) = std::env::var_os("LIBRETOON_MANAGER_PREVIEW") {
            let pixels = crate::exporter::raster_ui_test(&ctx, &mut output, 370, 800);
            image::save_buffer(path, &pixels, 370, 800, image::ColorType::Rgb8).unwrap();
        }
        output.textures_delta.clear();
        e.begin_animation_clip();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200., 800.))),
                ..Default::default()
            },
            |ui| {
                e.animation_clip_dialog(ui.ctx());
            },
        );
        output.textures_delta.clear();
        assert_eq!(snapshot, serde_json::to_vec(&e.doc).unwrap());
    }
}

fn remove_asset_instances(doc: &mut Document, id: u64) {
    fn layers(layers: &mut SharedVec<Layer>, rig: &mut crate::rig::Rig, id: u64) -> Vec<usize> {
        let count = layers.first().map_or(1, |l| l.frames.len());
        let mut converted =
            std::collections::HashMap::<usize, (SharedVec<Mark>, SharedVec<Mark>)>::new();
        let mut affected = std::collections::HashSet::new();
        let mut targets = std::collections::HashSet::new();
        for (index, layer) in layers.iter_mut().enumerate() {
            if !layer.frames.iter().any(|f| {
                f.iter().any(|m| {
                    m.style
                        .animation_clip
                        .as_ref()
                        .is_some_and(|t| t.asset == id)
                })
            }) {
                continue;
            }
            for frame in layer.frames.iter_mut() {
                if let Some((_, replacement)) = converted.get(&frame.identity()) {
                    affected.insert(index);
                    *frame = replacement.clone();
                    continue;
                }
                if frame.iter().any(|m| {
                    m.style
                        .animation_clip
                        .as_ref()
                        .is_some_and(|t| t.asset == id)
                }) {
                    affected.insert(index);
                    let original = frame.clone();
                    let replacement: SharedVec<Mark> = frame
                        .iter()
                        .filter(|m| {
                            let remove = m
                                .style
                                .animation_clip
                                .as_ref()
                                .is_some_and(|t| t.asset == id);
                            if remove {
                                targets.insert(m.style.track);
                            }
                            !remove
                        })
                        .cloned()
                        .collect::<Vec<_>>()
                        .into();
                    converted.insert(original.identity(), (original, replacement.clone()));
                    *frame = replacement;
                }
            }
        }
        let removed: Vec<_> = layers
            .iter()
            .enumerate()
            .filter(|(i, l)| {
                affected.contains(i)
                    && l.options.animation_track
                    && l.frames.iter().all(|f| f.is_empty())
            })
            .map(|(i, _)| i)
            .collect();
        let mut index = 0;
        layers.retain(|_| {
            let keep = !removed.contains(&index);
            index += 1;
            keep
        });
        if layers.is_empty() {
            let mut layer = crate::fresh_document().layers[0].clone();
            layer.frames.resize(count, Default::default());
            layer.init_keys();
            layers.push(layer);
        }
        rig.bindings.retain(|b| !targets.contains(&b.target));
        for bone in &mut rig.bones {
            bone.targets.retain(|target| !targets.contains(target));
        }
        removed
    }
    fn slots(tracks: &mut SharedVec<crate::media::Track>, removed: &[usize]) {
        for track in tracks {
            track.slot -= removed.iter().filter(|&&i| i < track.slot).count();
        }
    }
    let removed = layers(&mut doc.layers, &mut doc.extras.rig, id);
    slots(&mut doc.extras.media.tracks, &removed);
    for scene in &mut doc.extras.scenes {
        let removed = layers(&mut scene.layers, &mut scene.rig, id);
        slots(&mut scene.media_tracks, &removed);
    }
    for symbol in &mut doc.extras.symbols {
        layers(&mut symbol.layers, &mut symbol.rig, id);
    }
}

impl Manager {
    pub(crate) fn reset_document(&mut self) {
        self.cache.clear();
        self.delete_asset = None;
        self.preview_bounds = None;
        self.gesture = None;
        self.selected_span = None;
        self.pending.clear();
        self.selected = None;
        self.selected_folder = None;
        self.folder_color_transaction = false;
        self.rename_focus = false;
        self.clock = None;
        self.playing = false;
        self.create = None;
    }
}

impl Editor {
    pub(crate) fn queue_animation_clip_duplicate(&mut self, layer: usize) -> bool {
        if !self
            .doc
            .layers
            .get(layer)
            .is_some_and(|l| l.options.animation_track)
        {
            return false;
        }
        if let Some(mut span) = spans(&self.doc.layers[layer])
            .into_iter()
            .find(|s| (s.start..s.end).contains(&self.frame))
        {
            let duration = span.end - span.start;
            span.start = span.end;
            span.end = span.start + duration;
            span.mark.style.track = id();
            if self.clip_drop_valid(layer, usize::MAX, &span) {
                self.clips
                    .pending
                    .push(Operation::Change(layer, usize::MAX, Some(span)));
            }
        }
        true
    }
}

impl Editor {
    fn transfer_animation_clip(
        &mut self,
        source: usize,
        start: usize,
        target: usize,
        span: Span,
    ) -> bool {
        if source == target {
            return self.change_animation_span(source, start, Some(span));
        }
        if source >= self.doc.layers.len()
            || target >= self.doc.layers.len()
            || !self.doc.layers[source].options.animation_track
            || !self.doc.layers[target].options.animation_track
            || self.doc.layers[source].options.locked
            || !self.clip_drop_valid(target, usize::MAX, &span)
        {
            return false;
        }
        let mut from = spans(&self.doc.layers[source]);
        let Some(index) = from.iter().position(|s| s.start == start) else {
            return false;
        };
        let mut to = spans(&self.doc.layers[target]);
        let at = span.start;
        let end = span.end;
        to.push(span);
        from.remove(index);
        self.checkpoint();
        let count = self.doc.layers[0].frames.len().max(end);
        for layer in &mut self.doc.layers {
            if resize_track(layer, count) {
                continue;
            }
            layer.init_keys();
            let held = layer.frames.last().cloned().unwrap_or_default();
            layer.frames.resize(count, held);
            layer.options.keys.resize(count, false);
        }
        self.write_clip_spans(target, to);
        let target = if from.is_empty() {
            self.doc.layers.remove(source);
            if source < target { target - 1 } else { target }
        } else {
            self.write_clip_spans(source, from);
            target
        };
        self.layer = target;
        self.frame = at;
        self.canvas.clear_selection();
        self.clips.selected_span = Some((target, at));
        self.clips.cache.clear();
        self.recover();
        true
    }
}

impl Editor {
    pub(crate) fn animation_instance_bounds(&self, mark: &Mark) -> Option<Rect> {
        let instance = mark.style.instance.as_ref()?;
        let Some(timing) = mark.style.animation_clip.as_ref() else {
            return self.symbol_bounds(instance.symbol);
        };
        let frame = source_frame(&self.doc, timing, instance.born, self.frame)?;
        self.animation_source_bounds(instance.symbol, frame)
    }
    fn animation_source_bounds(&self, symbol_id: u64, frame: usize) -> Option<Rect> {
        let symbol = self.doc.extras.symbols.iter().find(|s| s.id == symbol_id)?;
        let mut source = self.doc.clone();
        source.layers = symbol.layers.clone();
        source.extras.rig = symbol.rig.clone();
        source.extras.cameras = Default::default();
        let mut bounds = Rect::NOTHING;
        for (_, _, mark) in crate::features::resolve_export(&source, frame, false) {
            let radius = if mark.stroke { mark.width * 0.5 } else { 0. };
            bounds = bounds.union(crate::vector::control_bounds(&mark).expand(radius));
        }
        bounds.is_finite().then_some(bounds)
    }
}

fn validate_collection(doc: &Document) -> Result<(), String> {
    let collection = &doc.extras.animation_library;
    crate::library::Library {
        folders: collection.folders.clone(),
        order: Default::default(),
    }
    .validate(&[])?;
    let folders: std::collections::HashSet<_> = collection.folders.iter().map(|f| f.id).collect();
    if doc
        .extras
        .animation_clips
        .iter()
        .any(|a| a.folder.is_some_and(|f| !folders.contains(&f)))
    {
        return Err("Missing animation folder".into());
    }
    let clips: std::collections::HashSet<_> =
        doc.extras.animation_clips.iter().map(|a| a.id).collect();
    let mut seen = std::collections::HashSet::new();
    if collection.order.len() > 1200
        || collection.order.iter().any(|entry| {
            !seen.insert(*entry)
                || match entry {
                    Entry::Folder(id) => !folders.contains(id),
                    Entry::Clip(id) => !clips.contains(id),
                }
        })
    {
        return Err("Invalid animation library order".into());
    }
    Ok(())
}
fn collection_rows(
    doc: &Document,
    parent: Option<u64>,
    depth: usize,
    query: &str,
    out: &mut Vec<(Entry, usize)>,
) {
    if depth > 32 {
        return;
    }
    let collection = &doc.extras.animation_library;
    let mut children: Vec<_> = collection
        .folders
        .iter()
        .filter(|f| f.parent == parent)
        .map(|f| Entry::Folder(f.id))
        .chain(
            doc.extras
                .animation_clips
                .iter()
                .filter(|a| {
                    a.folder == parent
                        && (query.is_empty()
                            || format!("{} {}", a.name, a.resource)
                                .to_lowercase()
                                .contains(query))
                })
                .map(|a| Entry::Clip(a.id)),
        )
        .collect();
    let positions: std::collections::HashMap<_, _> = collection
        .order
        .iter()
        .enumerate()
        .map(|(i, e)| (*e, i))
        .collect();
    children.sort_by_key(|entry| positions.get(entry).copied().unwrap_or(usize::MAX));
    for entry in children {
        out.push((entry, depth));
        if let Entry::Folder(id) = entry {
            if let Some(folder) = collection.folders.iter().find(|f| f.id == id) {
                if !folder.collapsed || !query.is_empty() {
                    collection_rows(doc, Some(id), depth + 1, query, out);
                }
            }
        }
    }
}
impl Editor {
    fn create_animation_folder(&mut self) -> u64 {
        let parent = self.clips.selected_folder;
        let folder = id();
        let number = self.doc.extras.animation_library.folders.len() + 1;
        self.checkpoint();
        self.doc
            .extras
            .animation_library
            .folders
            .push(crate::library::Folder {
                id: folder,
                name: format!("Folder {number}"),
                parent,
                color: [153, 119, 231, 255],
                collapsed: false,
            });
        if let Some(parent) = parent {
            if let Some(f) = self
                .doc
                .extras
                .animation_library
                .folders
                .iter_mut()
                .find(|f| f.id == parent)
            {
                f.collapsed = false;
            }
        }
        self.clips.selected_folder = Some(folder);
        self.clips.selected = None;
        self.clips.rename_focus = true;
        self.recover();
        folder
    }
    fn delete_animation_folder(&mut self, id: u64) {
        let Some(folder) = self
            .doc
            .extras
            .animation_library
            .folders
            .iter()
            .find(|f| f.id == id)
            .cloned()
        else {
            return;
        };
        self.checkpoint();
        for child in &mut self.doc.extras.animation_library.folders {
            if child.parent == Some(id) {
                child.parent = folder.parent;
            }
        }
        for clip in &mut self.doc.extras.animation_clips {
            if clip.folder == Some(id) {
                clip.folder = folder.parent;
            }
        }
        self.doc
            .extras
            .animation_library
            .folders
            .retain(|f| f.id != id);
        self.doc
            .extras
            .animation_library
            .order
            .retain(|e| *e != Entry::Folder(id));
        self.clips.selected_folder = folder.parent;
        self.recover();
    }
    fn animation_entry_parent(&self, entry: Entry) -> Option<u64> {
        match entry {
            Entry::Folder(id) => self
                .doc
                .extras
                .animation_library
                .folders
                .iter()
                .find(|f| f.id == id)
                .and_then(|f| f.parent),
            Entry::Clip(id) => self
                .doc
                .extras
                .animation_clips
                .iter()
                .find(|a| a.id == id)
                .and_then(|a| a.folder),
        }
    }
    fn animation_entry_drop_valid(&self, entry: Entry, parent: Option<u64>) -> bool {
        if parent.is_some_and(|id| {
            !self
                .doc
                .extras
                .animation_library
                .folders
                .iter()
                .any(|f| f.id == id)
        }) {
            return false;
        }
        match entry {
            Entry::Folder(id) => {
                self.doc
                    .extras
                    .animation_library
                    .folders
                    .iter()
                    .any(|f| f.id == id)
                    && self.doc.extras.animation_library.can_parent(id, parent)
            }
            Entry::Clip(id) => self.doc.extras.animation_clips.iter().any(|a| a.id == id),
        }
    }
    fn move_animation_entry(
        &mut self,
        entry: Entry,
        parent: Option<u64>,
        sibling: Option<Entry>,
        before: bool,
    ) -> bool {
        if sibling == Some(entry) || !self.animation_entry_drop_valid(entry, parent) {
            return false;
        }
        self.checkpoint();
        match entry {
            Entry::Folder(id) => {
                if let Some(f) = self
                    .doc
                    .extras
                    .animation_library
                    .folders
                    .iter_mut()
                    .find(|f| f.id == id)
                {
                    f.parent = parent;
                }
            }
            Entry::Clip(id) => {
                if let Some(a) = self
                    .doc
                    .extras
                    .animation_clips
                    .iter_mut()
                    .find(|a| a.id == id)
                {
                    a.folder = parent;
                }
            }
        }
        let mut order: Vec<_> = self
            .doc
            .extras
            .animation_library
            .order
            .iter()
            .copied()
            .collect();
        for e in self
            .doc
            .extras
            .animation_library
            .folders
            .iter()
            .map(|f| Entry::Folder(f.id))
            .chain(
                self.doc
                    .extras
                    .animation_clips
                    .iter()
                    .map(|a| Entry::Clip(a.id)),
            )
        {
            if !order.contains(&e) {
                order.push(e);
            }
        }
        order.retain(|e| *e != entry);
        let at = sibling
            .and_then(|s| order.iter().position(|e| *e == s))
            .map_or(order.len(), |i| i + usize::from(!before));
        order.insert(at, entry);
        self.doc.extras.animation_library.order = order.into();
        if let Some(parent) = parent {
            if let Some(f) = self
                .doc
                .extras
                .animation_library
                .folders
                .iter_mut()
                .find(|f| f.id == parent)
            {
                f.collapsed = false;
            }
        }
        self.recover();
        true
    }
    fn set_animation_folder_color(&mut self, id: u64, color: Color32) {
        if !self.clips.folder_color_transaction {
            self.checkpoint();
            self.clips.folder_color_transaction = true;
        }
        if let Some(folder) = self
            .doc
            .extras
            .animation_library
            .folders
            .iter_mut()
            .find(|f| f.id == id)
        {
            let mut rgba = color.to_srgba_unmultiplied();
            rgba[3] = 255;
            folder.color = rgba;
        }
        self.recover();
    }
    fn animation_folder_editor(&mut self, ui: &mut egui::Ui, folder: &crate::library::Folder) {
        let mut name = folder.name.clone();
        ui.horizontal(|ui| {
            button(ui, Icon::Rename, "", "Renombrar", false, false);
            let response = ui.text_edit_singleline(&mut name);
            if self.clips.rename_focus {
                response.request_focus();
                self.clips.rename_focus = false;
            }
            if response.gained_focus() {
                self.checkpoint();
            }
            if response.changed() {
                while name.len() > 1024 {
                    name.pop();
                }
                if let Some(f) = self
                    .doc
                    .extras
                    .animation_library
                    .folders
                    .iter_mut()
                    .find(|f| f.id == folder.id)
                {
                    f.name = name;
                }
                self.recover();
            }
        });
        design::category(ui, "COLOR DE CARPETA");
        let mut color =
            Color32::from_rgba_unmultiplied(folder.color[0], folder.color[1], folder.color[2], 255);
        ui.spacing_mut().slider_width = 240.;
        if egui::color_picker::color_picker_color32(
            ui,
            &mut color,
            egui::color_picker::Alpha::Opaque,
        ) {
            self.set_animation_folder_color(folder.id, color);
        }
    }
    fn animation_collection_ui(&mut self, ui: &mut egui::Ui) -> Option<Asset> {
        let query = self.clips.search.to_lowercase();
        let mut rows = vec![];
        collection_rows(&self.doc, None, 0, &query, &mut rows);
        let source = egui::DragAndDrop::payload::<ClipDrag>(ui.ctx())
            .map(|p| Entry::Clip(p.0))
            .or_else(|| {
                egui::DragAndDrop::payload::<FolderDrag>(ui.ctx()).map(|p| Entry::Folder(p.0))
            });
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let mut drop = None;
        let mut edit = None;
        egui::ScrollArea::vertical()
            .id_salt("animation-collection")
            .max_height((ui.available_height() * 0.45).max(100.))
            .show(ui, |ui| {
                let root = button(
                    ui,
                    Icon::Home,
                    "Animations",
                    "Animation Manager",
                    self.clips.selected_folder.is_none() && self.clips.selected.is_none(),
                    true,
                );
                if root.clicked() {
                    self.clips.selected_folder = None;
                    self.clips.selected = None;
                }
                if source.is_some() && root.contains_pointer() {
                    drop = Some((None, None, false));
                    ui.painter().rect_stroke(
                        root.rect,
                        3.,
                        Stroke::new(2., crate::themes::accent()),
                        egui::StrokeKind::Inside,
                    );
                }
                for (entry, depth) in rows {
                    let (name, color, folder) = match entry {
                        Entry::Folder(id) => {
                            let Some(f) = self
                                .doc
                                .extras
                                .animation_library
                                .folders
                                .iter()
                                .find(|f| f.id == id)
                                .cloned()
                            else {
                                continue;
                            };
                            (
                                f.name.clone(),
                                Color32::from_rgba_unmultiplied(
                                    f.color[0], f.color[1], f.color[2], 255,
                                ),
                                Some(f),
                            )
                        }
                        Entry::Clip(id) => {
                            let Some(a) =
                                self.doc.extras.animation_clips.iter().find(|a| a.id == id)
                            else {
                                continue;
                            };
                            (
                                format!("{}  ·  {} {}", a.name, a.end - a.start, tr("frames")),
                                crate::themes::accent(),
                                None,
                            )
                        }
                    };
                    let (rect, response) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), 26.),
                        Sense::click_and_drag(),
                    );
                    #[cfg(test)]
                    self.clips.test_rows.insert(entry, rect);
                    let selected = match entry {
                        Entry::Folder(id) => self.clips.selected_folder == Some(id),
                        Entry::Clip(id) => self.clips.selected == Some(id),
                    };
                    if selected || response.hovered() {
                        ui.painter().rect_filled(
                            rect,
                            4.,
                            crate::themes::accent().gamma_multiply(if selected {
                                0.25
                            } else {
                                0.1
                            }),
                        );
                    }
                    let left = rect.left() + 6. + depth as f32 * 14.;
                    let icon = Rect::from_min_size(
                        Pos2::new(left + 12., rect.top() + 4.),
                        Vec2::splat(18.),
                    );
                    design::draw(
                        ui.painter(),
                        icon,
                        if folder.is_some() {
                            Icon::Folder
                        } else {
                            Icon::Film
                        },
                        color,
                    );
                    ui.painter().with_clip_rect(rect.shrink(2.)).text(
                        Pos2::new(left + 35., rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        &name,
                        egui::FontId::proportional(11.),
                        crate::themes::palette().text,
                    );
                    if response.clicked() {
                        self.clips.selected_span = None;
                        self.canvas.clear_selection();
                        match entry {
                            Entry::Folder(id) => {
                                self.clips.selected_folder = Some(id);
                                self.clips.selected = None;
                            }
                            Entry::Clip(id) => {
                                self.clips.selected = Some(id);
                                self.clips.selected_folder = None;
                                self.clips.clock = Some(std::time::Instant::now());
                            }
                        }
                    }
                    if let Some(folder) = &folder {
                        let arrow =
                            Rect::from_min_size(Pos2::new(left, rect.top() + 5.), Vec2::splat(14.));
                        let c = arrow.center();
                        let points = if folder.collapsed {
                            vec![
                                c + Vec2::new(-2., -4.),
                                c + Vec2::new(-2., 4.),
                                c + Vec2::new(3., 0.),
                            ]
                        } else {
                            vec![
                                c + Vec2::new(-4., -2.),
                                c + Vec2::new(4., -2.),
                                c + Vec2::new(0., 3.),
                            ]
                        };
                        ui.painter()
                            .add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
                        if ui
                            .interact(
                                arrow,
                                ui.scope_id().with(("animation-folder-arrow", folder.id)),
                                Sense::click(),
                            )
                            .clicked()
                            || response.double_clicked()
                        {
                            if let Some(f) = self
                                .doc
                                .extras
                                .animation_library
                                .folders
                                .iter_mut()
                                .find(|f| f.id == folder.id)
                            {
                                f.collapsed = !folder.collapsed;
                            }
                            self.recover();
                        }
                    } else if response.double_clicked() {
                        if let Entry::Clip(id) = entry {
                            edit = self
                                .doc
                                .extras
                                .animation_clips
                                .iter()
                                .find(|a| a.id == id)
                                .cloned();
                        }
                    }
                    if response.drag_started() {
                        match entry {
                            Entry::Folder(id) => {
                                egui::DragAndDrop::set_payload(ui.ctx(), FolderDrag(id))
                            }
                            Entry::Clip(id) => {
                                egui::DragAndDrop::set_payload(ui.ctx(), ClipDrag(id));
                                self.clips.selected = Some(id);
                                self.clips.selected_folder = None;
                            }
                        }
                    }
                    egui::Popup::context_menu(&response)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .show(|ui| match &folder {
                            Some(folder) => {
                                self.animation_folder_editor(ui, folder);
                                ui.separator();
                                if button(
                                    ui,
                                    Icon::Trash,
                                    "Eliminar carpeta",
                                    "Eliminar carpeta",
                                    false,
                                    true,
                                )
                                .clicked()
                                {
                                    self.delete_animation_folder(folder.id);
                                    ui.close();
                                }
                            }
                            None => {
                                if let Entry::Clip(id) = entry {
                                    if button(
                                        ui,
                                        Icon::Rename,
                                        "Renombrar",
                                        "Renombrar",
                                        false,
                                        true,
                                    )
                                    .clicked()
                                    {
                                        self.clips.selected = Some(id);
                                        self.clips.selected_folder = None;
                                        self.clips.rename_focus = true;
                                        ui.close();
                                    }
                                    if button(
                                        ui,
                                        Icon::Pen,
                                        "Edit animation",
                                        "Edit animation",
                                        false,
                                        true,
                                    )
                                    .clicked()
                                    {
                                        edit = self
                                            .doc
                                            .extras
                                            .animation_clips
                                            .iter()
                                            .find(|a| a.id == id)
                                            .cloned();
                                        ui.close();
                                    }
                                    if button(
                                        ui,
                                        Icon::Trash,
                                        "Delete animation",
                                        "Delete animation",
                                        false,
                                        true,
                                    )
                                    .clicked()
                                    {
                                        self.delete_animation_asset(id);
                                        ui.close();
                                    }
                                }
                            }
                        });
                    if let (Some(source), Some(pos)) = (source, pointer) {
                        if response.contains_pointer() && entry != source {
                            let inside = folder.is_some()
                                && pos.y > rect.top() + 5.
                                && pos.y < rect.bottom() - 5.;
                            let parent = if inside {
                                if let Entry::Folder(id) = entry {
                                    Some(id)
                                } else {
                                    None
                                }
                            } else {
                                self.animation_entry_parent(entry)
                            };
                            let valid = self.animation_entry_drop_valid(source, parent);
                            let color = if valid {
                                crate::themes::accent()
                            } else {
                                Color32::LIGHT_RED
                            };
                            if inside {
                                ui.painter().rect_stroke(
                                    rect.shrink(1.),
                                    3.,
                                    Stroke::new(2., color),
                                    egui::StrokeKind::Inside,
                                );
                            } else {
                                let y = if pos.y < rect.center().y {
                                    rect.top()
                                } else {
                                    rect.bottom()
                                };
                                ui.painter().line_segment(
                                    [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
                                    Stroke::new(2., color),
                                );
                            }
                            if valid {
                                drop = Some((
                                    parent,
                                    if inside { None } else { Some(entry) },
                                    pos.y < rect.center().y,
                                ));
                            }
                        }
                    }
                }
                let (blank, response) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 36.), Sense::hover());
                if source.is_some() && response.contains_pointer() {
                    drop = Some((None, None, false));
                    ui.painter().line_segment(
                        [blank.left_top(), blank.right_top()],
                        Stroke::new(2., crate::themes::accent()),
                    );
                }
            });
        if let Some(source) = source {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            if ui.input(|i| i.pointer.primary_released()) {
                if let Some((parent, sibling, before)) = drop {
                    egui::DragAndDrop::clear_payload(ui.ctx());
                    self.move_animation_entry(source, parent, sibling, before);
                }
            }
        }
        if let Some(folder) = self
            .clips
            .selected_folder
            .and_then(|id| {
                self.doc
                    .extras
                    .animation_library
                    .folders
                    .iter()
                    .find(|f| f.id == id)
            })
            .cloned()
        {
            ui.separator();
            self.animation_folder_editor(ui, &folder);
        }
        edit
    }
}
impl Manager {
    pub(crate) fn clear_track_selection(&mut self) {
        self.cache.clear();
        self.gesture = None;
        self.selected_span = None;
        self.pending.clear();
    }
}

#[cfg(test)]
mod collection_regressions {
    use super::tests::{create, fixture};
    use super::*;
    fn press(pos: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }
    fn tick(ctx: &egui::Context, e: &mut Editor, events: Vec<egui::Event>) {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1440., 900.))),
                events,
                ..Default::default()
            },
            |ui| {
                e.workspace(ui);
                if !e.hub.home {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::new().inner_margin(0.))
                        .show(ui, |ui| {
                            e.canvas_ui(ui);
                        });
                }
            },
        );
        output.textures_delta.clear();
    }
    #[test]
    fn actual_manager_drag_to_canvas_creates_a_clip_track() {
        let mut e = fixture();
        let asset = create(&mut e);
        let doc = e.doc.clone();
        e.add_document(doc, None);
        e.clips.open = true;
        e.clips.selected = Some(asset);
        e.tool = crate::canvas::Tool::Object;
        let ctx = egui::Context::default();
        tick(&ctx, &mut e, vec![]);
        let from = e.clips.test_rows[&Entry::Clip(asset)].center();
        let to = Pos2::new(600., 300.);
        let count = e.doc.layers.len();
        tick(&ctx, &mut e, vec![egui::Event::PointerMoved(from)]);
        tick(&ctx, &mut e, vec![press(from, true)]);
        tick(
            &ctx,
            &mut e,
            vec![egui::Event::PointerMoved(from - Vec2::new(14., 0.))],
        );
        assert!(egui::DragAndDrop::has_payload_of_type::<ClipDrag>(&ctx));
        tick(&ctx, &mut e, vec![egui::Event::PointerMoved(to)]);
        tick(&ctx, &mut e, vec![press(to, false)]);
        assert_eq!(e.doc.layers.len(), count + 1);
        assert!(e.doc.layers[e.layer].options.animation_track);
        assert_eq!(
            spans(&e.doc.layers[e.layer])[0]
                .mark
                .style
                .animation_clip
                .as_ref()
                .unwrap()
                .asset,
            asset
        );
        assert!(e.doc.validate().is_ok());
    }
    #[test]
    fn folders_reparent_reorder_color_and_roundtrip_without_losing_clips() {
        let mut e = fixture();
        let asset = create(&mut e);
        let root = e.create_animation_folder();
        let child = e.create_animation_folder();
        assert!(e.move_animation_entry(Entry::Clip(asset), Some(child), None, false));
        assert!(!e.move_animation_entry(Entry::Folder(root), Some(child), None, false));
        assert!(e.move_animation_entry(
            Entry::Folder(child),
            None,
            Some(Entry::Folder(root)),
            true
        ));
        e.set_animation_folder_color(child, Color32::from_rgb(20, 120, 230));
        assert_eq!(
            e.doc
                .extras
                .animation_library
                .folders
                .iter()
                .find(|f| f.id == child)
                .unwrap()
                .color,
            [20, 120, 230, 255]
        );
        assert!(e.doc.validate().is_ok());
        let restored = crate::fla::decode(&crate::fla::encode(&e.doc).unwrap()).unwrap();
        assert_eq!(restored.extras.animation_clips[0].folder, Some(child));
        assert_eq!(restored.extras.animation_library.folders.len(), 2);
        assert!(restored.validate().is_ok());
        e.delete_animation_folder(child);
        assert_eq!(e.doc.extras.animation_clips[0].folder, None);
        assert_eq!(e.doc.extras.animation_clips[0].id, asset);
        e.history(false);
        assert_eq!(e.doc.extras.animation_clips[0].folder, Some(child));
        e.move_animation_entry(Entry::Clip(asset), None, None, false);
        assert!(e.doc.validate().is_ok());
    }
    #[test]
    fn nested_folder_deletion_moves_children_to_parent_and_keeps_assets() {
        let mut e = fixture();
        let asset = create(&mut e);
        let root = e.create_animation_folder();
        let child = e.create_animation_folder();
        let grandchild = e.create_animation_folder();
        e.move_animation_entry(Entry::Clip(asset), Some(child), None, false);
        e.delete_animation_folder(child);
        assert_eq!(e.doc.extras.animation_clips[0].folder, Some(root));
        assert_eq!(
            e.doc
                .extras
                .animation_library
                .folders
                .iter()
                .find(|f| f.id == grandchild)
                .unwrap()
                .parent,
            Some(root)
        );
        assert!(e.doc.validate().is_ok());
    }
    #[test]
    fn last_animation_track_can_be_deleted_and_undo_restores_it() {
        let mut e = fixture();
        let asset = create(&mut e);
        e.place_animation_clip(asset, 2, None, None);
        let track = e.doc.layers[e.layer].clone();
        e.doc.layers = vec![track].into();
        e.layer = 0;
        e.timeline_layer_action(0, 1);
        assert_eq!(e.doc.layers.len(), 1);
        assert!(!e.doc.layers[0].options.animation_track);
        assert_eq!(e.doc.extras.animation_clips.len(), 1);
        assert!(e.doc.validate().is_ok());
        e.history(false);
        assert!(e.doc.layers[0].options.animation_track);
        assert_eq!(spans(&e.doc.layers[0]).len(), 1);
    }
    #[test]
    fn deleting_rigging_keeps_artwork_and_other_contexts_and_is_undoable() {
        let mut e = fixture();
        let asset = create(&mut e);
        let art = e.doc.layers.clone();
        let symbols = e.doc.extras.symbols.clone();
        e.doc.extras.rig.bones.push(crate::rig::Bone {
            id: id(),
            name: "Root".into(),
            parent: None,
            start: [0., 0.],
            end: [0., 50.],
            radius: 20.,
            ik: false,
            targets: Default::default(),
            keys: vec![crate::rig::BoneKey::default()].into(),
        });
        assert!(e.delete_rigging_track());
        assert!(e.doc.extras.rig.is_empty());
        assert_eq!(
            serde_json::to_vec(&e.doc.layers).unwrap(),
            serde_json::to_vec(&art).unwrap()
        );
        assert_eq!(
            e.doc.layers[0].frames[0][0].points.identity(),
            art[0].frames[0][0].points.identity()
        );
        assert_eq!(e.doc.extras.symbols.identity(), symbols.identity());
        assert_eq!(e.doc.extras.animation_clips[0].id, asset);
        e.history(false);
        assert_eq!(e.doc.extras.rig.bones.len(), 1);
        assert!(e.doc.validate().is_ok());
    }
}

#[cfg(test)]
mod folder_compatibility {
    use super::*;
    #[test]
    fn an_empty_animation_folder_survives_native_fla() {
        let mut e = super::tests::fixture();
        e.create_animation_folder();
        assert!(e.doc.extras.animation_clips.is_empty());
        let copy = crate::fla::decode(&crate::fla::encode(&e.doc).unwrap()).unwrap();
        assert_eq!(copy.extras.animation_library.folders.len(), 1);
    }
    #[test]
    fn cross_project_frame_paste_detaches_imported_clips_from_source_folders() {
        let mut e = super::tests::fixture();
        let asset = super::tests::create(&mut e);
        let folder = e.create_animation_folder();
        e.move_animation_entry(Entry::Clip(asset), Some(folder), None, false);
        e.place_animation_clip(asset, 2, None, None);
        e.feature.frame_selection = Some((e.layer, 2, 4));
        assert!(e.copy_timeline_frames());
        e.doc = crate::fresh_document();
        e.layer = 0;
        e.frame = 5;
        assert!(e.paste_timeline_frames());
        assert_eq!(e.doc.extras.animation_clips.len(), 1);
        assert!(e.doc.extras.animation_clips[0].folder.is_none());
        assert!(e.doc.extras.animation_library.is_empty());
        assert!(e.doc.validate().is_ok());
    }
}
