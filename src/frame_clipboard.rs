//! In-app, cross-document frame clipboard. Resources are imported with fresh IDs;
//! a validated candidate is committed once, without replacing existing artwork.
use crate::{
    Document, Editor, Mark,
    document::Layer,
    features::{Instance, Playback, Symbol, SymbolClock, id},
    i18n::tr,
    shared::SharedVec,
};
use std::collections::{HashMap, HashSet, VecDeque};
#[derive(Default)]
pub(crate) struct State {
    pub focus: bool,
    copied: Option<Package>,
}
#[derive(Clone)]
struct Package {
    source: Document,
    frames: SharedVec<SharedVec<Mark>>,
    keys: Vec<bool>,
    first: usize,
    source_matrix: crate::rig::Matrix,
}
impl State {
    pub fn has_frames(&self) -> bool {
        self.copied.is_some()
    }
    pub fn clear(&mut self) {
        self.copied = None;
        self.focus = false;
    }
}
#[derive(Default)]
struct Remap {
    symbols: HashMap<u64, u64>,
    media: HashMap<u64, u64>,
    clips: HashMap<u64, u64>,
    tracks: HashMap<u64, u64>,
    bones: HashMap<u64, u64>,
    pins: HashMap<u64, u64>,
    folders: HashMap<u64, u64>,
    frames: HashMap<usize, SharedVec<Mark>>,
}
fn mapped(table: &mut HashMap<u64, u64>, old: u64) -> u64 {
    if old == 0 {
        0
    } else {
        *table.entry(old).or_insert_with(id)
    }
}
fn references(
    mark: &Mark,
    symbols: &mut VecDeque<u64>,
    media: &mut HashSet<u64>,
    clips: &mut VecDeque<u64>,
) {
    if let Some(instance) = &mark.style.instance {
        symbols.push_back(instance.symbol);
    }
    if let Some(image) = mark.style.media_image {
        media.insert(image);
    }
    if let Some(clip) = &mark.style.animation_clip {
        clips.push_back(clip.asset);
    }
}
impl Remap {
    fn dependencies(package: &Package) -> Result<Self, String> {
        let mut result = Self::default();
        let mut symbols = VecDeque::new();
        let mut clips = VecDeque::new();
        let mut media = HashSet::new();
        for frame in &package.frames {
            for mark in frame {
                references(mark, &mut symbols, &mut media, &mut clips);
            }
        }
        while !symbols.is_empty() || !clips.is_empty() {
            while let Some(old) = symbols.pop_front() {
                if result.symbols.contains_key(&old) {
                    continue;
                }
                let symbol = package
                    .source
                    .extras
                    .symbols
                    .iter()
                    .find(|s| s.id == old)
                    .ok_or("Missing copied symbol")?;
                result.symbols.insert(old, id());
                let mut seen = HashSet::new();
                for layer in &symbol.layers {
                    for frame in layer.frames.iter().filter(|f| seen.insert(f.identity())) {
                        for mark in frame {
                            references(mark, &mut symbols, &mut media, &mut clips);
                        }
                    }
                }
                symbols.extend(symbol.rig.bindings.iter().map(|b| b.symbol));
            }
            while let Some(old) = clips.pop_front() {
                if result.clips.contains_key(&old) {
                    continue;
                }
                let clip = package
                    .source
                    .extras
                    .animation_clips
                    .iter()
                    .find(|a| a.id == old)
                    .ok_or("Missing copied animation")?;
                result.clips.insert(old, id());
                symbols.push_back(clip.symbol);
            }
        }
        for image in media {
            if !package
                .source
                .extras
                .media
                .assets
                .iter()
                .any(|a| a.id == image)
            {
                return Err("Missing copied image".into());
            }
            result.media.insert(image, id());
        }
        Ok(result)
    }
    fn mark(&mut self, mark: &Mark) -> Mark {
        let mut out = mark.clone();
        out.style.track = mapped(&mut self.tracks, out.style.track);
        if let Some(i) = &mut out.style.instance {
            i.symbol = self.symbols[&i.symbol];
        }
        out.style.mouth_routes.retain(|route| {
            route
                .path
                .iter()
                .all(|slot| self.symbols.contains_key(&slot.symbol))
        });
        for route in &mut out.style.mouth_routes {
            for slot in &mut route.path {
                slot.symbol = self.symbols[&slot.symbol];
                slot.track = mapped(&mut self.tracks, slot.track);
            }
        }
        if let Some(i) = &mut out.style.media_image {
            *i = self.media[i];
        }
        if let Some(c) = &mut out.style.animation_clip {
            c.asset = self.clips[&c.asset];
        }
        out
    }
    fn frame(&mut self, frame: &SharedVec<Mark>) -> SharedVec<Mark> {
        if let Some(cached) = self.frames.get(&frame.identity()) {
            return cached.clone();
        }
        let out: SharedVec<Mark> = frame.iter().map(|mark| self.mark(mark)).collect();
        self.frames.insert(frame.identity(), out.clone());
        out
    }
    fn rig(&mut self, rig: &crate::rig::Rig) -> crate::rig::Rig {
        let mut out = rig.clone();
        out.character_symbol = out
            .character_symbol
            .and_then(|id| self.symbols.get(&id).copied());
        for bone in &mut out.bones {
            bone.id = mapped(&mut self.bones, bone.id);
            bone.parent = bone.parent.map(|id| mapped(&mut self.bones, id));
            for target in &mut bone.targets {
                *target = mapped(&mut self.tracks, *target);
            }
        }
        for binding in &mut out.bindings {
            binding.target = mapped(&mut self.tracks, binding.target);
            binding.symbol = self.symbols[&binding.symbol];
            for cell in &mut binding.cells {
                for influence in &mut cell.influences {
                    influence.bone = mapped(&mut self.bones, influence.bone);
                }
            }
            for pin in &mut binding.pins {
                pin.id = mapped(&mut self.pins, pin.id);
            }
        }
        for channel in [&mut out.character_turns.head, &mut out.character_turns.body] {
            channel.root = channel.root.map(|id| mapped(&mut self.bones, id));
            for pose in channel.views.iter_mut().flatten() {
                for (id, _) in &mut std::sync::Arc::make_mut(pose).bones {
                    *id = mapped(&mut self.bones, *id);
                }
            }
        }
        out
    }
    fn symbol(&mut self, symbol: &Symbol) -> Symbol {
        let mut out = symbol.clone();
        out.id = self.symbols[&symbol.id];
        out.library_folder = None;
        for folder in &mut out.folders {
            folder.id = mapped(&mut self.folders, folder.id);
            folder.parent = folder.parent.map(|p| mapped(&mut self.folders, p));
        }
        for layer in &mut out.layers {
            if let Some(piece) = &mut layer.options.character_piece {
                piece.remap(
                    |id| mapped(&mut self.tracks, id),
                    |id| mapped(&mut self.bones, id),
                );
            }
            layer.options.folder = layer.options.folder.map(|f| mapped(&mut self.folders, f));
            layer.frames = layer.frames.iter().map(|frame| self.frame(frame)).collect();
        }
        out.rig = self.rig(&symbol.rig);
        out
    }
    fn install(&mut self, package: &Package, target: &mut Document) {
        for source in package
            .source
            .extras
            .symbols
            .iter()
            .filter(|s| self.symbols.contains_key(&s.id))
            .cloned()
            .collect::<Vec<_>>()
        {
            target.extras.symbols.push(self.symbol(&source));
        }
        for source in package
            .source
            .extras
            .media
            .assets
            .iter()
            .filter(|s| self.media.contains_key(&s.id))
        {
            let mut asset = source.clone();
            asset.id = self.media[&asset.id];
            asset.folder = None;
            target.extras.media.assets.push(asset);
        }
        for source in package
            .source
            .extras
            .animation_clips
            .iter()
            .filter(|s| self.clips.contains_key(&s.id))
        {
            let mut asset = source.clone();
            asset.id = self.clips[&asset.id];
            asset.symbol = self.symbols[&asset.symbol];
            asset.folder = None;
            target.extras.animation_clips.push(asset);
        }
    }
}
pub(crate) fn retime(mark: &mut Mark, source_start: usize, destination: usize, doc: &Document) {
    if let Some(instance) = &mut mark.style.instance {
        let elapsed = source_start.saturating_sub(instance.born);
        if instance.born < source_start {
            if let Some(timing) = &mut mark.style.animation_clip {
                timing.offset += elapsed as f64 * timing.rate as f64;
                if let Some(asset) = doc
                    .extras
                    .animation_clips
                    .iter()
                    .find(|a| a.id == timing.asset)
                {
                    let count = (asset.end - asset.start) as f64;
                    timing.offset = if timing.ping_pong && count > 1. {
                        timing.offset.rem_euclid(2. * (count - 1.))
                    } else if timing.looping {
                        timing.offset.rem_euclid(count)
                    } else {
                        timing.offset.min(count - 1.)
                    };
                }
            } else if instance.mode != Playback::Single {
                instance.first = instance.first.saturating_add(elapsed);
            }
        }
        instance.born = destination.saturating_add(instance.born.saturating_sub(source_start));
    }
}
impl Editor {
    pub(crate) fn frame_clipboard_context(&mut self, layer: usize, frame: usize) {
        self.layer = layer;
        self.frame = frame;
        self.frame_clipboard.focus = true;
        if !self
            .feature
            .frame_selection
            .is_some_and(|(l, a, b)| l == layer && (a..=b).contains(&frame))
        {
            self.feature.frame_selection = Some((layer, frame, frame));
        }
    }
    pub(crate) fn copy_timeline_frames(&mut self) -> bool {
        self.finish_pending_pen();
        let (layer, first, last) = self
            .feature
            .frame_selection
            .filter(|(l, _, _)| *l == self.layer)
            .unwrap_or((self.layer, self.frame, self.frame));
        let Some(selected) = self.doc.layers.get(layer) else {
            return false;
        };
        if first > last || last >= selected.frames.len() || selected.options.mannequin.is_some() {
            return false;
        }
        let mut source = self.doc.clone();
        source.extras.scenes = Default::default();
        source.extras.cameras = Default::default();
        let tracks: HashSet<_> = selected
            .frames
            .iter()
            .flatten()
            .map(|m| m.style.track)
            .filter(|id| *id != 0)
            .collect();
        let bound = source
            .extras
            .rig
            .bindings
            .iter()
            .any(|b| tracks.contains(&b.target));
        let mut keys: Vec<bool> = (first..=last)
            .map(|f| f == first || selected.key(f) || crate::animation::span(selected, f).is_some())
            .collect();
        let frames: SharedVec<SharedVec<Mark>> = if selected.options.clip {
            keys = vec![true; last - first + 1];
            (first..=last)
                .map(|frame| {
                    crate::features::resolve_export(&self.doc, frame, false)
                        .into_iter()
                        .filter(|(l, _, _)| *l == layer)
                        .map(|(_, _, mark)| mark)
                        .collect()
                })
                .collect()
        } else if bound {
            let mut start = layer;
            while start > 0 && self.doc.layers[start].options.clip {
                start -= 1;
            }
            let layers: SharedVec<Layer> = self.doc.layers[start..=layer]
                .iter()
                .map(|l| crate::folder_groups::inherited_layer(&self.doc, l))
                .collect();
            let allowed: HashSet<_> = layers
                .iter()
                .flat_map(|l| l.frames.iter().flatten())
                .map(|m| m.style.track)
                .collect();
            let mut rig = source.extras.rig.clone();
            rig.bindings.retain(|b| allowed.contains(&b.target));
            let symbol = id();
            source.extras.symbols.push(Symbol {
                id: symbol,
                name: format!("{} · {}", selected.name, tr("Copied frames")),
                layers,
                rig,
                library_folder: None,
                mouth_poses: Default::default(),
                folders: Default::default(),
                perspective: source.extras.perspective.clone(),
            });
            let mut mark = Mark {
                points: vec![[0., 0.], [source.width as f32, source.height as f32]].into(),
                rectangle: true,
                stroke: false,
                ..Default::default()
            };
            mark.style.instance = Some(Instance {
                symbol,
                matrix: crate::rig::IDENTITY,
                mode: Playback::Once,
                clock: SymbolClock::Scene,
                first: 0,
                born: 0,
            });
            let frame: SharedVec<Mark> = vec![mark].into();
            (first..=last).map(|_| frame.clone()).collect()
        } else {
            (first..=last)
                .map(|f| {
                    if crate::folder_groups::layer_active(&self.doc, selected, f) {
                        crate::animation::marks(selected, f)
                    } else {
                        Default::default()
                    }
                })
                .collect()
        };
        source.layers = vec![selected.clone()].into();
        let mut package = Package {
            source,
            frames,
            keys,
            first,
            source_matrix: if bound || selected.options.clip {
                crate::rig::IDENTITY
            } else {
                crate::folder_groups::layer_matrix(&self.doc, selected)
            },
        };
        // Release unrelated documents/media when the source tab is closed.
        let needs = match Remap::dependencies(&package) {
            Ok(needs) => needs,
            Err(error) => {
                self.status = error;
                return false;
            }
        };
        package
            .source
            .extras
            .symbols
            .retain(|s| needs.symbols.contains_key(&s.id));
        package
            .source
            .extras
            .media
            .assets
            .retain(|a| needs.media.contains_key(&a.id));
        package
            .source
            .extras
            .animation_clips
            .retain(|a| needs.clips.contains_key(&a.id));
        package.source.extras.media.tracks = Default::default();
        package.source.extras.rig = Default::default();
        package.source.extras.references = Default::default();
        package.source.extras.active_scene = 0;
        package.source.layers = vec![Layer {
            name: tr("Copied frames").into(),
            visible: true,
            options: Default::default(),
            frames: vec![SharedVec::default()].into(),
        }]
        .into();
        self.frame_clipboard.copied = Some(package);
        self.frame_clipboard.focus = true;
        self.studio.clipboard.clear();
        self.status =
            tr("Frames copied. Select a frame in the destination project and paste.").into();
        true
    }
    pub(crate) fn can_paste_timeline_frames(&self) -> bool {
        self.frame_clipboard.has_frames()
            && self.doc.layers.get(self.layer).is_some_and(|l| {
                l.visible
                    && !l.options.locked
                    && !l.options.animation_track
                    && !l.options.raster
                    && l.options.adjustment.is_none()
                    && l.options.mannequin.is_none()
            })
    }
    pub(crate) fn paste_timeline_frames(&mut self) -> bool {
        match self.paste_frame_package() {
            Ok(()) => true,
            Err(error) => {
                self.status = error;
                false
            }
        }
    }
    fn paste_frame_package(&mut self) -> Result<(), String> {
        if !self.can_paste_timeline_frames() {
            return Err(tr("Choose an unlocked drawing layer to paste frames.").into());
        }
        let package = self
            .frame_clipboard
            .copied
            .clone()
            .ok_or("No copied frames")?;
        let start = self.frame;
        let end = start
            .checked_add(package.frames.len())
            .filter(|end| *end <= 10000)
            .ok_or_else(|| tr("The pasted range exceeds the 10,000-frame limit.").to_owned())?;
        let mut remap = Remap::dependencies(&package)?;
        let mut candidate = self.doc.clone();
        remap.install(&package, &mut candidate);
        let count = candidate.layers[0].frames.len().max(end);
        for layer in &mut candidate.layers {
            if crate::animation_clips::resize_track(layer, count) {
                continue;
            }
            layer.init_keys();
            let held = layer.frames.last().cloned().unwrap_or_default();
            layer.frames.resize(count, held);
            layer.options.keys.resize(count, false);
        }
        let destination =
            crate::folder_groups::layer_matrix(&candidate, &candidate.layers[self.layer]);
        let relative = crate::rig::multiply(
            crate::folder_groups::inverse(destination)
                .ok_or("Invalid destination folder transform")?,
            package.source_matrix,
        );
        let target = &mut candidate.layers[self.layer];
        let intersecting: Vec<_> = target
            .options
            .tweens
            .iter()
            .filter(|t| t.start < end && t.end >= start)
            .map(|t| (t.start, t.end))
            .collect();
        // One layer cannot carry independent overlapping tweens. Sample only the
        // affected spans so existing art, including outside the pasted range, stays put.
        for (a, b) in &intersecting {
            for frame in *a..=*b {
                target.frames[frame] = crate::animation::marks(&self.doc.layers[self.layer], frame);
                target.options.keys[frame] = true;
            }
        }
        target
            .options
            .tweens
            .retain(|t| !intersecting.contains(&(t.start, t.end)));
        let mut retimed = HashMap::<usize, SharedVec<Mark>>::new();
        for (offset, frame) in package.frames.iter().enumerate() {
            let pasted = if let Some(cached) = retimed.get(&frame.identity()) {
                cached.clone()
            } else {
                let marks: SharedVec<Mark> = frame
                    .iter()
                    .map(|m| {
                        let mut m = m.clone();
                        retime(&mut m, package.first, start, &package.source);
                        crate::folder_groups::mark(&remap.mark(&m), relative)
                    })
                    .collect();
                retimed.insert(frame.identity(), marks.clone());
                marks
            };
            let at = start + offset;
            let mut combined = target.frames[at].to_vec();
            combined.extend(pasted.iter().cloned());
            target.frames[at] = combined.into();
            target.options.keys[at] |= package.keys[offset];
        }
        target.options.keys[start] = true;
        if end < count {
            target.options.keys[end] = true;
        }
        candidate
            .validate()
            .map_err(|e| format!("{}: {e}", tr("Frames could not be pasted")))?;
        self.finish_pending_pen();
        self.checkpoint();
        self.doc = candidate;
        self.playing = false;
        self.canvas.clear_selection();
        self.feature.frame_selection = Some((self.layer, start, end - 1));
        self.frame_clipboard.focus = true;
        self.clips.reset_document();
        self.fit_camera_duration();
        self.recover();
        self.status =
            tr("Frames pasted into the active layer. Existing artwork was preserved.").into();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{self, Pos2, Rect, Vec2};
    #[test]
    fn copying_ping_pong_frames_preserves_the_return_leg() {
        let mut doc = crate::fresh_document();
        doc.extras
            .animation_clips
            .push(crate::animation_clips::Asset {
                folder: None,
                id: 77,
                name: "clip".into(),
                resource: "resource".into(),
                symbol: 88,
                start: 2,
                end: 8,
                looping: false,
            });
        let mut mark = Mark::default();
        mark.style.instance = Some(Instance {
            symbol: 88,
            born: 0,
            matrix: crate::rig::IDENTITY,
            mode: Playback::Once,
            first: 2,
            clock: crate::features::SymbolClock::Scene,
        });
        mark.style.animation_clip = Some(crate::animation_clips::Timing {
            asset: 77,
            offset: 0.,
            rate: 0.5,
            looping: false,
            reversed: false,
            ping_pong: true,
        });
        let original = mark.style.animation_clip.clone().unwrap();
        retime(&mut mark, 15, 30, &doc);
        let copied = mark.style.animation_clip.as_ref().unwrap();
        for frame in 0..8 {
            assert_eq!(
                crate::animation_clips::source_frame(&doc, &original, 0, 15 + frame),
                crate::animation_clips::source_frame(&doc, copied, 30, 30 + frame)
            );
        }
    }
    #[test]
    fn copying_between_transformed_folders_preserves_stage_position() {
        let mut e = editor();
        let f = crate::features::Folder {
            id: 990,
            name: "Source".into(),
            parent: None,
            collapsed: false,
            transform: crate::folder_groups::Transform {
                position: [40., 20.],
                scale: 2.,
                ..Default::default()
            },
            timeline_slot: None,
            range: None,
            source_shift: 0,
        };
        e.doc.extras.folders.push(f.clone());
        e.doc.layers[0].options.folder = Some(990);
        e.doc.layers[0].frames[0] = vec![ink([120, 90, 220, 255], 10.)].into();
        e.feature.frame_selection = Some((0, 0, 0));
        let original = crate::features::resolve_export(&e.doc, 0, false)[0]
            .2
            .clone();
        assert!(e.copy_timeline_frames());
        e.doc = crate::fresh_document();
        let mut dest = f;
        dest.id = 991;
        dest.transform.position = [-80., 5.];
        dest.transform.angle = 0.4;
        dest.transform.scale = 0.5;
        e.doc.extras.folders.push(dest);
        e.doc.layers[0].options.folder = Some(991);
        e.frame = 4;
        assert!(e.paste_timeline_frames());
        let output = crate::features::resolve_export(&e.doc, 4, false);
        assert_eq!(output.len(), 1);
        let a = crate::vector::control_bounds(&original);
        let b = crate::vector::control_bounds(&output[0].2);
        assert!(a.min.distance(b.min) < 0.01);
        assert!(a.max.distance(b.max) < 0.01);
        assert!(e.doc.validate().is_ok());
    }
    fn ink(color: [u8; 4], x: f32) -> Mark {
        Mark {
            points: vec![[x, 10.], [x + 10., 20.]].into(),
            rectangle: true,
            fill: Some(color),
            stroke: false,
            ..Default::default()
        }
    }
    fn editor() -> Editor {
        let mut e = Editor::new(&eframe::CreationContext::_new_kittest(
            egui::Context::default(),
        ));
        e.doc = crate::fresh_document();
        e.recovery = std::env::temp_dir().join(format!(
            "frames-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ));
        e
    }
    fn symbol(id: u64, color: [u8; 4]) -> Symbol {
        let mut layer = crate::fresh_document().layers[0].clone();
        for f in 0..24 {
            layer.frames[f] = vec![ink(color, f as f32)].into();
        }
        Symbol {
            id,
            name: "FX".into(),
            layers: vec![layer].into(),
            rig: Default::default(),
            folders: Default::default(),
            library_folder: None,
            mouth_poses: Default::default(),
            perspective: Default::default(),
        }
    }
    fn instance(symbol: u64, track: u64) -> Mark {
        let mut m = Mark {
            points: vec![[0., 0.], [20., 20.]].into(),
            rectangle: true,
            stroke: false,
            ..Default::default()
        };
        m.style.track = track;
        m.style.instance = Some(Instance {
            symbol,
            matrix: crate::rig::IDENTITY,
            mode: Playback::Loop,
            clock: SymbolClock::Scene,
            first: 0,
            born: 0,
        });
        m
    }
    #[test]
    fn frames_cross_document_tabs_keep_exposures_blank_keys_and_existing_art() {
        let mut e = editor();
        let mut source = crate::fresh_document();
        source.layers[0].options.keys = vec![false; 24].into();
        source.layers[0].options.keys[0] = true;
        source.layers[0].options.keys[2] = true;
        source.layers[0].frames[0] = vec![ink([200, 0, 0, 255], 30.)].into();
        source.layers[0].frames[1] = source.layers[0].frames[0].clone();
        let original = source.layers[0].frames[0][0].points.clone();
        e.add_document(source, None);
        e.feature.frame_selection = Some((0, 0, 3));
        assert!(e.copy_timeline_frames());
        let mut target = crate::fresh_document();
        for frame in &mut target.layers[0].frames {
            *frame = vec![ink([0, 200, 0, 255], 80.)].into();
        }
        e.add_document(target, None);
        e.frame = 5;
        assert!(e.paste_timeline_frames());
        assert_eq!(e.doc.layers.len(), 1);
        assert_eq!(e.doc.layers[0].frames[5].len(), 2);
        assert_eq!(e.doc.layers[0].frames[6].len(), 2);
        assert_eq!(e.doc.layers[0].frames[7].len(), 1);
        assert_eq!(e.doc.layers[0].frames[9].len(), 1);
        assert!(e.doc.layers[0].key(5));
        assert!(!e.doc.layers[0].key(6));
        assert!(e.doc.layers[0].key(7));
        assert!(e.doc.layers[0].key(9));
        e.doc.layers[0].frames[5][1].points[0][0] = 999.;
        assert_eq!(original[0][0], 30.);
        e.activate_document(0);
        assert_eq!(e.doc.layers[0].frames[0][0].points[0][0], 30.);
        e.activate_document(1);
        e.history(false);
        assert_eq!(e.doc.layers[0].frames[5].len(), 1);
    }
    #[test]
    fn symbols_images_and_same_id_conflicts_are_imported_without_overwriting() {
        let mut e = editor();
        e.doc.extras.symbols.push(symbol(7, [0, 0, 200, 255]));
        let mut image = crate::media::bitmap(vec![120; 16], 2, 2, "Source image".into());
        image.id = 9;
        e.doc.extras.media.assets.push(image);
        e.doc.extras.symbols[0].layers[0].frames[4][0]
            .style
            .media_image = Some(9);
        for frame in &mut e.doc.layers[0].frames {
            *frame = vec![instance(7, 55)].into();
        }
        e.feature.frame_selection = Some((0, 4, 5));
        assert!(e.copy_timeline_frames());
        e.doc = crate::fresh_document();
        e.layer = 0;
        e.frame = 10;
        e.doc.extras.symbols.push(symbol(7, [0, 200, 0, 255]));
        let mut other = crate::media::bitmap(vec![240; 16], 2, 2, "Target image".into());
        other.id = 9;
        e.doc.extras.media.assets.push(other);
        assert!(e.paste_timeline_frames());
        assert_eq!(e.doc.layers.len(), 1);
        assert_eq!(e.doc.extras.symbols.len(), 2);
        assert_eq!(e.doc.extras.media.assets.len(), 2);
        assert_eq!(e.doc.extras.symbols[0].id, 7);
        assert_eq!(e.doc.extras.media.assets[0].data[0], 240);
        let pasted = &e.doc.layers[0].frames[10][0];
        let i = pasted.style.instance.as_ref().unwrap();
        assert_ne!(i.symbol, 7);
        assert_eq!((i.first, i.born), (4, 10));
        let imported = e
            .doc
            .extras
            .symbols
            .iter()
            .find(|s| s.id == i.symbol)
            .unwrap();
        let image = imported.layers[0].frames[4][0].style.media_image.unwrap();
        assert_ne!(image, 9);
        assert_eq!(
            e.doc
                .extras
                .media
                .assets
                .iter()
                .find(|a| a.id == image)
                .unwrap()
                .data[0],
            120
        );
        assert!(e.doc.validate().is_ok());
        e.history(false);
        assert_eq!(e.doc.extras.symbols.len(), 1);
        assert_eq!(e.doc.extras.media.assets.len(), 1);
    }
    #[test]
    fn project_rigs_remain_self_contained_editable_and_visually_identical() {
        let mut e = editor();
        e.doc.extras.symbols.push(symbol(7, [30, 60, 200, 255]));
        for frame in &mut e.doc.layers[0].frames {
            *frame = vec![instance(7, 55)].into();
        }
        e.doc.extras.rig.bones.push(crate::rig::Bone {
            id: 11,
            name: "Root".into(),
            parent: None,
            start: [0., 0.],
            end: [0., 40.],
            radius: 20.,
            ik: false,
            targets: vec![55].into(),
            keys: vec![
                crate::rig::BoneKey::default(),
                crate::rig::BoneKey {
                    frame: 5,
                    angle: 0.4,
                    ..Default::default()
                },
            ]
            .into(),
        });
        let mut binding = crate::rig::Binding::new(
            55,
            7,
            crate::rig::IDENTITY,
            Rect::from_min_size(Pos2::ZERO, Vec2::splat(30.)),
        );
        for cell in &mut binding.cells {
            cell.influences[0] = crate::rig::Influence {
                bone: 11,
                weight: 1.,
            };
        }
        e.doc.extras.rig.bindings.push(binding);
        assert!(e.doc.validate().is_ok());
        let before = crate::features::resolve_export(&e.doc, 2, false);
        e.feature.frame_selection = Some((0, 1, 3));
        assert!(e.copy_timeline_frames());
        e.doc = crate::fresh_document();
        e.frame = 10;
        assert!(e.paste_timeline_frames());
        assert_eq!(e.doc.layers.len(), 1);
        assert!(e.doc.extras.rig.is_empty());
        let root = e.doc.layers[0].frames[10][0]
            .style
            .instance
            .as_ref()
            .unwrap()
            .symbol;
        let rig = &e
            .doc
            .extras
            .symbols
            .iter()
            .find(|s| s.id == root)
            .unwrap()
            .rig;
        assert_eq!(rig.bones.len(), 1);
        assert_ne!(rig.bones[0].id, 11);
        assert_eq!(rig.bindings[0].cells[0].influences[0].bone, rig.bones[0].id);
        let after = crate::features::resolve_export(&e.doc, 11, false);
        assert_eq!(before.len(), after.len());
        for (a, b) in before.iter().zip(after.iter()) {
            assert_eq!(a.2.points.len(), b.2.points.len());
            for (p, q) in a.2.points.iter().zip(b.2.points.iter()) {
                assert!((p[0] - q[0]).abs() < 0.001 && (p[1] - q[1]).abs() < 0.001);
            }
        }
        assert!(e.doc.validate().is_ok());
    }
    #[test]
    fn pasting_into_a_tween_preserves_its_appearance_outside_the_range() {
        let mut e = editor();
        e.doc.layers[0].frames[0] = vec![ink([200, 40, 20, 255], 20.)].into();
        e.feature.frame_selection = Some((0, 0, 1));
        e.copy_timeline_frames();
        e.doc = crate::fresh_document();
        let mut a = ink([20, 40, 200, 255], 0.);
        a.style.track = 77;
        let mut b = a.clone();
        b.points = vec![[100., 10.], [110., 20.]].into();
        for f in 0..24 {
            e.doc.layers[0].frames[f] = vec![if f < 10 { a.clone() } else { b.clone() }].into();
        }
        e.doc.layers[0].options.keys = vec![false; 24].into();
        e.doc.layers[0].options.keys[0] = true;
        e.doc.layers[0].options.keys[10] = true;
        e.doc.layers[0].options.tweens = vec![crate::animation::Tween {
            start: 0,
            end: 10,
            curve: Default::default(),
        }]
        .into();
        let before: Vec<_> = [0, 3, 9, 11]
            .into_iter()
            .map(|f| {
                crate::animation::marks(&e.doc.layers[0], f)[0]
                    .points
                    .clone()
            })
            .collect();
        e.frame = 5;
        assert!(e.paste_timeline_frames());
        for (f, expected) in [0, 3, 9, 11].into_iter().zip(before) {
            assert_eq!(
                crate::animation::marks(&e.doc.layers[0], f)[0].points,
                expected
            );
        }
        e.history(false);
        assert_eq!(e.doc.layers[0].options.tweens.len(), 1);
    }
    #[test]
    fn rejected_paste_is_atomic_and_extending_does_not_extend_animation_clips() {
        let mut e = editor();
        e.doc.layers[0].frames[0] = vec![ink([100, 50, 0, 255], 0.)].into();
        e.feature.frame_selection = Some((0, 0, 3));
        e.copy_timeline_frames();
        let before = serde_json::to_vec(&e.doc).unwrap();
        e.doc.layers[0].options.locked = true;
        assert!(!e.paste_timeline_frames());
        e.doc.layers[0].options.locked = false;
        e.frame = 9999;
        assert!(!e.paste_timeline_frames());
        assert_eq!(before, serde_json::to_vec(&e.doc).unwrap());
        e.frame = 23;
        assert!(e.paste_timeline_frames());
        assert_eq!(e.doc.layers[0].frames.len(), 27);
        assert_eq!(e.doc.layers.len(), 1);
        assert!(e.doc.validate().is_ok());
    }
    #[test]
    fn native_timeline_clipboard_events_keep_the_canvas_clipboard_behavior() {
        let mut e = editor();
        e.doc.layers[0].frames[0] = vec![ink([200, 0, 0, 255], 0.)].into();
        e.frame_clipboard.focus = true;
        e.feature.frame_selection = Some((0, 0, 0));
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Copy],
                ..Default::default()
            },
            |ui| {
                e.clipboard_shortcuts(ui.ctx());
            },
        );
        output.textures_delta.clear();
        assert!(e.frame_clipboard.has_frames());
        e.doc = crate::fresh_document();
        e.frame = 4;
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Paste("LibreToon frames".into())],
                ..Default::default()
            },
            |ui| {
                e.clipboard_shortcuts(ui.ctx());
            },
        );
        output.textures_delta.clear();
        assert_eq!(e.doc.layers[0].frames[4].len(), 1);
        assert_eq!(e.doc.layers.len(), 1);
        e.frame_clipboard.focus = false;
        e.canvas.selected = vec![(0, 0)];
        e.frame = 4;
        e.copy_selection(false);
        assert!(!e.frame_clipboard.has_frames());
        assert_eq!(e.studio.clipboard.len(), 1);
    }
}
