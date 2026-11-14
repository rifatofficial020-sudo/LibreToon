use crate::{
    Editor, Mark,
    design::{Icon, button},
    document::Layer,
    features::{LayerOptions, Symbol},
    i18n::tr,
    rig::{self, Matrix},
    shared::SharedVec,
};
use eframe::egui;
use serde::{Deserialize, Serialize};

const VARIANTS: usize = 16;
pub(crate) const FACE_FRAMES: usize = 8 * (1 + VARIANTS);
const EYES: &[&str] = &[
    "Open",
    "Half closed",
    "Closed",
    "Look left",
    "Look right",
    "Look up",
    "Look down",
];
const BROWS: &[&str] = &["Neutral", "Raised", "Lowered", "Tilt in", "Tilt out"];

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
enum FacePart {
    #[default]
    None,
    Mouth,
    Eye,
    Brow,
}

impl FacePart {
    fn label(self) -> &'static str {
        match self {
            Self::None => "Artwork",
            Self::Mouth => "Mouth",
            Self::Eye => "Eye",
            Self::Brow => "Brow",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct FaceKey {
    frame: usize,
    variant: usize,
}

pub(crate) const VIEWS: [&str; 8] = [
    "Front",
    "Front ¾ R",
    "Right",
    "Back ¾ R",
    "Back",
    "Back ¾ L",
    "Left",
    "Front ¾ L",
];

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Piece {
    pub track: u64,
    pub matrix: Matrix,
    #[serde(default)]
    hidden: [bool; 8],
    #[serde(default)]
    order: [i16; 8],
    #[serde(default)]
    joints: [Vec<(u64, [[f32; 2]; 2])>; 8],
    #[serde(default)]
    face: FacePart,
    #[serde(default)]
    face_keys: SharedVec<FaceKey>,
}

impl Piece {
    fn variant(&self, frame: usize) -> usize {
        let index = self.face_keys.partition_point(|key| key.frame <= frame);
        index
            .checked_sub(1)
            .map_or(0, |index| self.face_keys[index].variant)
    }

    fn set_variant(&mut self, frame: usize, variant: usize) {
        match self.face_keys.binary_search_by_key(&frame, |key| key.frame) {
            Ok(index) => self.face_keys[index].variant = variant,
            Err(index) => self.face_keys.insert(index, FaceKey { frame, variant }),
        }
    }

    pub(crate) fn valid(&self) -> bool {
        self.track != 0
            && self.face_keys.len() <= 10000
            && self
                .face_keys
                .windows(2)
                .all(|keys| keys[0].frame < keys[1].frame)
            && self
                .face_keys
                .iter()
                .all(|key| key.frame < 10000 && key.variant < VARIANTS)
            && rig::inverse(self.matrix).is_some()
            && self.matrix.iter().all(|value| value.is_finite())
            && self.joints.iter().all(|view| {
                view.len() <= 128
                    && view.iter().all(|(id, points)| {
                        *id != 0
                            && points
                                .iter()
                                .flatten()
                                .all(|value| value.is_finite() && value.abs() <= 1e7)
                    })
            })
    }

    pub(crate) fn remap(
        &mut self,
        track: impl FnOnce(u64) -> u64,
        mut bone: impl FnMut(u64) -> u64,
    ) {
        self.track = track(self.track);
        for (id, _) in self.joints.iter_mut().flatten() {
            *id = bone(*id);
        }
    }
}

pub(crate) fn view_joints(
    doc: &crate::Document,
    rig: &rig::Rig,
    frame: usize,
) -> Vec<(u64, [[f32; 2]; 2])> {
    let Some(symbol) = doc
        .extras
        .symbols
        .iter()
        .find(|s| Some(s.id) == rig.character_symbol)
    else {
        return Vec::new();
    };
    let Some(settings) = symbol
        .layers
        .first()
        .and_then(|l| l.options.character_piece.as_ref())
    else {
        return Vec::new();
    };
    let body = rig.character_turns.body.value(frame);
    let head = rig.character_turns.head.value(frame);
    let head_branch = rig
        .character_turns
        .head
        .root
        .map_or_else(Vec::new, |root| rig.pose_branch(root, None));
    rig.bones
        .iter()
        .filter_map(|bone| {
            let angle = if head_branch.contains(&bone.id) {
                body + head
            } else {
                body
            };
            let a = angle.floor();
            let value = angle - a;
            let at = |slot: i32| {
                settings.joints[slot.rem_euclid(8) as usize]
                    .iter()
                    .find(|(id, _)| *id == bone.id)
                    .map_or([bone.start, bone.end], |(_, joints)| *joints)
            };
            let left = at(a as i32);
            let right = at(a as i32 + 1);
            let joints = std::array::from_fn(|end| {
                std::array::from_fn(|axis| {
                    left[end][axis] * (1. - value) + right[end][axis] * value
                })
            });
            (joints != [bone.start, bone.end]).then_some((bone.id, joints))
        })
        .collect()
}

pub(crate) fn artwork(
    doc: &crate::Document,
    track: u64,
    frame: usize,
    mouth_pose: Option<usize>,
) -> Option<(SharedVec<Layer>, usize)> {
    let rig = &doc.extras.rig;
    if track == 0 {
        return None;
    }
    let view = rig.character_angle(track, frame)?;
    let symbol = doc
        .extras
        .symbols
        .iter()
        .find(|symbol| Some(symbol.id) == rig.character_symbol)?;
    let layer = symbol.layers.iter().find(|layer| {
        layer
            .options
            .character_piece
            .as_ref()
            .is_some_and(|p| p.track == track)
    })?;
    let piece = layer.options.character_piece.as_ref()?;
    if piece.hidden[view] {
        return Some((Default::default(), 0));
    }
    let inverse = rig::inverse(piece.matrix)?;
    let variant = match piece.face {
        FacePart::Mouth => Some((mouth_pose.filter(|&pose| pose < VARIANTS)? + 1) * 8),
        FacePart::Eye | FacePart::Brow => Some((piece.variant(frame) + 1) * 8),
        FacePart::None => None,
    };
    let nearest = |offset: usize| {
        (0..VIEWS.len())
            .filter(|&index| {
                layer
                    .frames
                    .get(index + offset)
                    .is_some_and(|marks| !marks.is_empty())
                    && !piece.hidden[index]
            })
            .min_by_key(|&index| {
                let distance = index.abs_diff(view);
                distance.min(8 - distance)
            })
            .map(|index| index + offset)
    };
    let slot = match variant {
        Some(offset) if piece.face == FacePart::Mouth => nearest(offset)?,
        Some(offset) => nearest(offset).or_else(|| nearest(0))?,
        None => nearest(0)?,
    };
    let marks = layer.frames[slot]
        .iter()
        .map(|mark| crate::features::affine(mark, inverse))
        .collect();
    Some((
        vec![Layer {
            frames: vec![marks].into(),
            options: LayerOptions {
                keys: vec![true].into(),
                ..Default::default()
            },
            ..layer.clone()
        }]
        .into(),
        0,
    ))
}

pub(crate) fn order(doc: &crate::Document, mark: &Mark, frame: usize) -> i16 {
    let rig = &doc.extras.rig;
    let Some(angle) = rig.character_angle(mark.style.track, frame) else {
        return 0;
    };
    doc.extras
        .symbols
        .iter()
        .find(|symbol| Some(symbol.id) == rig.character_symbol)
        .and_then(|symbol| {
            symbol.layers.iter().find_map(|layer| {
                layer
                    .options
                    .character_piece
                    .as_ref()
                    .filter(|piece| piece.track == mark.style.track)
            })
        })
        .map_or(0, |piece| piece.order[angle])
}

impl Editor {
    pub(crate) fn onion_frames(&self) -> std::ops::Range<usize> {
        let start = if self.in_character_symbol() {
            self.frame / 8 * 8
        } else {
            0
        };
        let end = if self.in_character_symbol() {
            start + 8
        } else {
            self.doc.layers[0].frames.len()
        };
        self.frame
            .saturating_sub(self.studio.onion_before)
            .max(start)
            ..self
                .frame
                .saturating_add(self.studio.onion_after)
                .saturating_add(1)
                .min(end)
    }

    pub(crate) fn face_controls(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.doc.extras.rig.character_symbol else {
            return;
        };
        let Some(index) = self
            .doc
            .extras
            .symbols
            .iter()
            .position(|symbol| symbol.id == id)
        else {
            return;
        };
        let frame = self.group_rig_frame();
        let mut edits = Vec::new();
        egui::CollapsingHeader::new(tr("Face Controls")).show(ui, |ui| {
            for (row, layer) in self.doc.extras.symbols[index].layers.iter().enumerate() {
                let Some(piece) = &layer.options.character_piece else {
                    continue;
                };
                let variants = match piece.face {
                    FacePart::Eye => EYES,
                    FacePart::Brow => BROWS,
                    _ => continue,
                };
                ui.push_id(row, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(&layer.name);
                        let value = piece.variant(frame).min(variants.len() - 1);
                        egui::ComboBox::from_id_salt("expression")
                            .selected_text(tr(variants[value]))
                            .show_ui(ui, |ui| {
                                for (variant, label) in variants.iter().enumerate() {
                                    if ui.selectable_label(variant == value, tr(label)).clicked() {
                                        edits.push((row, Some(variant)));
                                    }
                                }
                            });
                        if button(
                            ui,
                            Icon::KeyRemove,
                            "",
                            "Delete expression key",
                            false,
                            piece.face_keys.iter().any(|key| key.frame == frame),
                        )
                        .clicked()
                        {
                            edits.push((row, None));
                        }
                    });
                });
            }
        });
        if !edits.is_empty() {
            self.checkpoint();
            for (row, variant) in edits {
                let piece = self.doc.extras.symbols[index].layers[row]
                    .options
                    .character_piece
                    .as_mut()
                    .unwrap();
                if let Some(variant) = variant {
                    piece.set_variant(frame, variant);
                } else {
                    piece.face_keys.retain(|key| key.frame != frame);
                }
            }
            self.recover();
        }
    }

    pub(crate) fn character_timeline(&mut self, ui: &mut egui::Ui) {
        let mut selection = None;
        egui::ScrollArea::both()
            .id_salt("character-view-grid")
            .show(ui, |ui| {
                egui::Grid::new("character-views")
                    .striped(true)
                    .min_col_width(82.)
                    .show(ui, |ui| {
                        ui.strong(tr("Character Symbol"));
                        for label in VIEWS {
                            ui.strong(tr(label));
                        }
                        ui.end_row();
                        for (index, layer) in self.doc.layers.iter().enumerate() {
                            ui.label(&layer.name);
                            for slot in 0..8 {
                                let hidden = layer
                                    .options
                                    .character_piece
                                    .as_ref()
                                    .is_some_and(|piece| piece.hidden[slot]);
                                let offset = if self.layer == index {
                                    self.frame / 8 * 8
                                } else {
                                    0
                                };
                                let drawn = !layer.frames[offset + slot].is_empty();
                                let icon = if hidden {
                                    Icon::EyeOff
                                } else if drawn {
                                    Icon::Keyframe
                                } else {
                                    Icon::BlankKeyAdd
                                };
                                if button(
                                    ui,
                                    icon,
                                    "",
                                    VIEWS[slot],
                                    self.layer == index && self.frame % 8 == slot,
                                    true,
                                )
                                .clicked()
                                {
                                    selection = Some((index, slot));
                                }
                            }
                            ui.end_row();
                        }
                    });
            });
        if let Some((layer, slot)) = selection {
            self.finish_pending_pen();
            self.layer = layer;
            let variant = if self.layer == layer {
                self.frame / 8 * 8
            } else {
                0
            };
            self.frame = slot + variant;
            self.canvas.clear_selection();
            self.playing = false;
        }
    }

    pub(crate) fn in_character_symbol(&self) -> bool {
        self.feature
            .symbol_stack
            .last()
            .is_some_and(|(parent, _, _, id)| parent.extras.rig.character_symbol == Some(*id))
    }

    pub(crate) fn open_character_symbol(&mut self) {
        if let Some(id) = self.doc.extras.rig.character_symbol
            && self.doc.extras.symbols.iter().any(|symbol| symbol.id == id)
        {
            self.enter_symbol(id);
            return;
        }
        let mut layers = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for layer in &self.doc.layers {
            for mark in crate::animation::marks(layer, self.frame).iter() {
                let Some(instance) = &mark.style.instance else {
                    continue;
                };
                if !self
                    .doc
                    .extras
                    .rig
                    .bindings
                    .iter()
                    .any(|binding| binding.target == mark.style.track)
                    || !seen.insert(mark.style.track)
                {
                    continue;
                }
                let Some(symbol) = self
                    .doc
                    .extras
                    .symbols
                    .iter()
                    .find(|s| s.id == instance.symbol)
                else {
                    continue;
                };
                let mut local = self.doc.clone();
                local.layers = symbol.layers.clone();
                local.extras.rig = symbol.rig.clone();
                local.extras.folders = symbol.folders.clone();
                local.extras.cameras = Default::default();
                let frame = crate::features::symbol_frame(
                    instance,
                    local.layers[0].frames.len(),
                    self.frame,
                    self.frame,
                );
                let front = crate::features::resolve_export(&local, frame, false)
                    .into_iter()
                    .map(|(_, _, mark)| crate::features::affine(&mark, instance.matrix))
                    .collect();
                let mut frames = vec![SharedVec::default(); VIEWS.len()];
                frames[0] = front;
                layers.push(Layer {
                    name: symbol.name.clone(),
                    visible: true,
                    frames: frames.into(),
                    options: LayerOptions {
                        keys: vec![true; VIEWS.len()].into(),
                        character_piece: Some(Piece {
                            track: mark.style.track,
                            matrix: instance.matrix,
                            hidden: [false; 8],
                            order: [0; 8],
                            joints: Default::default(),
                            face: if symbol.mouth_poses.is_empty() {
                                FacePart::None
                            } else {
                                FacePart::Mouth
                            },
                            face_keys: Default::default(),
                        }),
                        ..Default::default()
                    },
                });
            }
        }
        if layers.is_empty() {
            self.status =
                tr("Bind at least one symbol to the rig before setting up a character.").into();
            return;
        }
        if layers.len() > 100 || self.doc.extras.symbols.len() >= 500 {
            self.status = tr("Character Symbol exceeds the project limits.").into();
            return;
        }
        let id = crate::features::id();
        self.checkpoint();
        let rig = &mut self.doc.extras.rig;
        if rig.character_turns.body.root.is_none() {
            rig.character_turns.body.root = rig
                .bones
                .iter()
                .find(|bone| bone.parent.is_none())
                .map(|bone| bone.id);
        }
        self.doc.extras.rig.character_symbol = Some(id);
        self.doc.extras.symbols.push(Symbol {
            id,
            name: tr("Character Symbol").into(),
            layers: layers.into(),
            rig: Default::default(),
            library_folder: None,
            mouth_poses: Default::default(),
            folders: Default::default(),
            perspective: Default::default(),
        });
        self.recover();
        self.enter_symbol(id);
    }

    pub(crate) fn character_toolbar(&mut self, ui: &mut egui::Ui) {
        let frame = self.frame % 8;
        let layer = self.layer;
        let art_frame = self.frame;
        ui.horizontal(|ui| {
            if button(ui, Icon::Brush, "Artwork", "", false, true).clicked() {
                self.rigging.edit_character_bones = false;
                self.activate_tool(crate::canvas::Tool::Brush);
            }
            if button(
                ui,
                Icon::Bones,
                "Edit Bones",
                "",
                self.rigging.edit_character_bones,
                true,
            )
            .clicked()
            {
                self.rigging.edit_character_bones ^= true;
            }
            let copy = button(ui, Icon::Copy, "Copy from View", "", false, true);
            egui::Popup::menu(&copy).show(|ui| {
                for (source, name) in VIEWS.into_iter().enumerate().filter(|(i, _)| *i != frame) {
                    if ui.button(tr(name)).clicked() {
                        self.finish_pending_pen();
                        self.checkpoint();
                        let frames = &mut self.doc.layers[layer].frames;
                        frames[art_frame] = frames[art_frame / 8 * 8 + source].clone();
                        if let Some(settings) = self.doc.layers[0].options.character_piece.as_mut()
                        {
                            settings.joints[frame] = settings.joints[source].clone();
                        }
                        self.recover();
                        ui.close();
                    }
                }
            });
            let arrange = button(
                ui,
                Icon::Layers,
                "Arrange",
                "",
                false,
                self.doc.layers[layer].options.character_piece.is_some(),
            );
            egui::Popup::menu(&arrange).show(|ui| {
                let Some(mut piece) = self.doc.layers[layer].options.character_piece.clone() else {
                    return;
                };
                let hidden = piece.hidden[frame];
                let mut changed = false;
                if button(
                    ui,
                    if hidden { Icon::EyeOff } else { Icon::Eye },
                    if hidden { "Show piece" } else { "Hide piece" },
                    "",
                    hidden,
                    true,
                )
                .clicked()
                {
                    piece.hidden[frame] = !hidden;
                    changed = true;
                }
                if button(ui, Icon::StepFront, "Traer adelante", "", false, true).clicked() {
                    piece.order[frame] = piece.order[frame].saturating_add(1);
                    changed = true;
                }
                if button(ui, Icon::StepBack, "Enviar atrás", "", false, true).clicked() {
                    piece.order[frame] = piece.order[frame].saturating_sub(1);
                    changed = true;
                }
                if changed {
                    self.checkpoint();
                    self.doc.layers[layer].options.character_piece = Some(piece);
                    self.recover();
                }
            });
            ui.label(tr(VIEWS[frame]));
            let face = button(ui, Icon::Mouth, "Face artwork", "", false, true);
            egui::Popup::menu(&face).show(|ui| self.face_artwork_menu(ui, layer, frame));
            if button(ui, Icon::Onion, "", "Onion skin", self.onion, true).clicked() {
                self.onion ^= true;
            }
            ui.add(
                egui::DragValue::new(&mut self.studio.onion_before)
                    .range(0..=7)
                    .prefix("‹ "),
            );
            ui.add(
                egui::DragValue::new(&mut self.studio.onion_after)
                    .range(0..=7)
                    .prefix("› "),
            );
            if button(
                ui,
                Icon::Play,
                "Preview Pose",
                "",
                self.rigging.preview_character_pose,
                true,
            )
            .clicked()
            {
                self.rigging.preview_character_pose ^= true;
            }
        });
        if self.rigging.preview_character_pose {
            self.preview_character_pose(ui.ctx(), frame);
        }
    }

    fn face_artwork_menu(&mut self, ui: &mut egui::Ui, layer: usize, angle: usize) {
        let Some(piece) = self.doc.layers[layer].options.character_piece.as_ref() else {
            return;
        };
        let track = piece.track;
        let part = piece.face;
        let parent = &self.feature.symbol_stack.last().unwrap().0;
        let source = parent
            .extras
            .rig
            .bindings
            .iter()
            .find(|binding| binding.target == track)
            .and_then(|binding| {
                self.doc
                    .extras
                    .symbols
                    .iter()
                    .find(|symbol| symbol.id == binding.symbol)
            });
        let labels: Vec<_> = match part {
            FacePart::Mouth => source
                .into_iter()
                .flat_map(|symbol| symbol.mouth_poses.iter().take(VARIANTS))
                .map(|pose| pose.name().to_owned())
                .collect(),
            FacePart::Eye => EYES.iter().map(|name| name.to_string()).collect(),
            FacePart::Brow => BROWS.iter().map(|name| name.to_string()).collect(),
            FacePart::None => Vec::new(),
        };
        let mouth_enabled = source.is_some_and(|symbol| !symbol.mouth_poses.is_empty());
        for kind in [
            FacePart::None,
            FacePart::Mouth,
            FacePart::Eye,
            FacePart::Brow,
        ] {
            if ui
                .add_enabled(
                    kind != FacePart::Mouth || mouth_enabled,
                    egui::Button::selectable(part == kind, tr(kind.label())),
                )
                .clicked()
                && kind != part
            {
                self.finish_pending_pen();
                self.checkpoint();
                let piece = self.doc.layers[layer]
                    .options
                    .character_piece
                    .as_mut()
                    .unwrap();
                piece.face = kind;
                piece.face_keys.clear();
                self.frame = angle;
                self.recover();
                return;
            }
        }
        ui.separator();
        let mut choose = None;
        if ui
            .selectable_label(self.frame == angle, tr("Base artwork"))
            .clicked()
        {
            choose = Some(angle);
        }
        for (index, label) in labels.iter().enumerate() {
            let slot = (index + 1) * 8 + angle;
            if ui.selectable_label(self.frame == slot, tr(label)).clicked() {
                choose = Some(slot);
            }
        }
        if let Some(slot) = choose {
            self.finish_pending_pen();
            if slot >= self.doc.layers[0].frames.len() {
                self.checkpoint();
                for layer in &mut self.doc.layers {
                    layer.frames.resize(FACE_FRAMES, Default::default());
                    layer.options.keys.resize(FACE_FRAMES, true);
                }
                self.recover();
            }
            self.frame = slot;
            self.canvas.clear_selection();
            ui.close();
        }
    }

    fn preview_character_pose(&mut self, ctx: &egui::Context, slot: usize) {
        let (mut scene, frame, _, id) = self.feature.symbol_stack.last().unwrap().clone();
        let symbol = scene
            .extras
            .symbols
            .iter_mut()
            .find(|symbol| symbol.id == id)
            .unwrap();
        symbol.layers = self.doc.layers.clone();
        let turns = &mut scene.extras.rig.character_turns;
        turns.body.keys.clear();
        turns.head.keys.clear();
        let turn = if slot > 4 {
            slot as f32 - 8.
        } else {
            slot as f32
        };
        if turns.body.root.is_some() {
            turns.body.set(0, turn);
        } else {
            turns.head.set(0, turn);
        }
        let mut open = true;
        egui::Window::new(tr("Preview Pose"))
            .open(&mut open)
            .show(ctx, |ui| {
                let (rect, painter) =
                    ui.allocate_painter(egui::vec2(360., 230.), egui::Sense::hover());
                let mut marks = crate::features::resolve_export(&scene, frame, false);
                marks.retain(|(layer, index, _)| {
                    crate::animation::marks(&scene.layers[*layer], frame)
                        .get(*index)
                        .is_some_and(|mark| {
                            scene
                                .extras
                                .rig
                                .bindings
                                .iter()
                                .any(|binding| binding.target == mark.style.track)
                        })
                });
                let bounds = marks
                    .iter()
                    .fold(egui::Rect::NOTHING, |bounds, (_, _, mark)| {
                        bounds.union(crate::vector::control_bounds(mark))
                    });
                if bounds.is_finite() {
                    let view = crate::canvas::View {
                        center: rect.rect.center(),
                        stage_center: bounds.center(),
                        angle: 0.,
                        scale: (340. / bounds.width().max(1.)).min(210. / bounds.height().max(1.)),
                    };
                    for (_, _, mark) in &marks {
                        crate::canvas::paint_mark(&painter, mark, view, false, false, None);
                    }
                }
            });
        self.rigging.preview_character_pose = open;
    }

    pub(crate) fn character_bone_input(
        &mut self,
        ui: &egui::Ui,
        painter: &egui::Painter,
        area: egui::Rect,
        view: crate::canvas::View,
    ) -> bool {
        if !self.in_character_symbol() || !self.rigging.edit_character_bones {
            return false;
        }
        let frame = self.frame % 8;
        let Some(settings) = self.doc.layers[0].options.character_piece.as_ref() else {
            return true;
        };
        let parent = &self.feature.symbol_stack.last().unwrap().0.extras.rig;
        let bones: Vec<_> = parent
            .bones
            .iter()
            .map(|bone| {
                let points = settings.joints[frame]
                    .iter()
                    .find(|(id, _)| *id == bone.id)
                    .map_or([bone.start, bone.end], |(_, points)| *points);
                (bone.id, points)
            })
            .collect();
        let overlay = painter
            .ctx()
            .layer_painter(egui::LayerId::new(
                egui::Order::Middle,
                ui.scope_id().with("character-bones"),
            ))
            .with_clip_rect(area);
        let color = crate::themes::accent();
        for &(id, points) in &bones {
            let screen = points.map(|p| view.screen(crate::vector::pos(p)));
            overlay.line_segment(screen, egui::Stroke::new(2., color));
            for (end, center) in screen.into_iter().enumerate() {
                overlay.circle_filled(center, 5., color);
                let response = ui.interact(
                    egui::Rect::from_center_size(center, egui::Vec2::splat(16.)).intersect(area),
                    ui.scope_id().with(("character-joint", id, end)),
                    egui::Sense::drag(),
                );
                if response.drag_started() {
                    self.checkpoint();
                }
                if (response.dragged() || response.drag_stopped())
                    && let Some(pointer) = response.interact_pointer_pos()
                {
                    let mut points = points;
                    points[end] = crate::vector::xy(view.world(pointer));
                    if crate::vector::pos(points[0]).distance(crate::vector::pos(points[1])) > 0.01
                    {
                        let joints = &mut self.doc.layers[0]
                            .options
                            .character_piece
                            .as_mut()
                            .unwrap()
                            .joints[frame];
                        if let Some((_, stored)) = joints.iter_mut().find(|(bone, _)| *bone == id) {
                            *stored = points;
                        } else {
                            joints.push((id, points));
                        }
                    }
                }
                if response.drag_stopped() {
                    self.recover();
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn character_view_selects_artwork_and_keeps_missing_views_editable() {
        let mut doc = crate::fresh_document();
        let rig = &mut doc.extras.rig;
        rig.character_symbol = Some(5);
        rig.bones.push(rig::Bone {
            id: 7,
            name: "Torso".into(),
            parent: None,
            start: [0., 0.],
            end: [10., 0.],
            radius: 10.,
            ik: false,
            targets: vec![9].into(),
            keys: Default::default(),
        });
        rig.character_turns.body.root = Some(7);
        rig.character_turns.body.set(5, 2.);
        let mut frames = vec![SharedVec::default(); 8];
        frames[0] = vec![Mark {
            color: [255, 0, 0, 255],
            ..Default::default()
        }]
        .into();
        frames[2] = vec![Mark {
            color: [0, 0, 255, 255],
            ..Default::default()
        }]
        .into();
        let mut piece = Piece {
            track: 9,
            matrix: rig::IDENTITY,
            hidden: [false; 8],
            order: [0; 8],
            joints: Default::default(),
            face: FacePart::None,
            face_keys: Default::default(),
        };
        piece.hidden[4] = true;
        piece.joints[2].push((7, [[0., 0.], [5., 0.]]));
        doc.extras.symbols.push(Symbol {
            id: 5,
            name: "Character Symbol".into(),
            rig: Default::default(),
            library_folder: None,
            mouth_poses: Default::default(),
            folders: Default::default(),
            perspective: Default::default(),
            layers: vec![Layer {
                name: "Torso".into(),
                visible: true,
                frames: frames.into(),
                options: LayerOptions {
                    character_piece: Some(piece),
                    ..Default::default()
                },
            }]
            .into(),
        });
        let saved = serde_json::to_vec(&doc).unwrap();
        let mut doc: crate::Document = serde_json::from_slice(&saved).unwrap();
        assert_eq!(
            artwork(&doc, 9, 0, None).unwrap().0[0].frames[0][0].color,
            [255, 0, 0, 255]
        );
        assert_eq!(
            artwork(&doc, 9, 5, None).unwrap().0[0].frames[0][0].color,
            [0, 0, 255, 255]
        );
        assert_eq!(view_joints(&doc, &doc.extras.rig, 5)[0].1[1], [5., 0.]);
        doc.extras.rig.character_turns.body.set(7, 4.);
        assert!(artwork(&doc, 9, 7, None).unwrap().0.is_empty());
        assert!(artwork(&doc, 10, 5, None).is_none());
        let layer = &mut doc.extras.symbols[0].layers[0];
        layer.options.character_piece.as_mut().unwrap().face = FacePart::Mouth;
        layer.frames.resize(FACE_FRAMES, Default::default());
        let o = 6 * 8;
        layer.frames[o + 2] = vec![Mark {
            color: [33, 44, 55, 255],
            ..Default::default()
        }]
        .into();
        assert_eq!(
            artwork(&doc, 9, 5, Some(5)).unwrap().0[0].frames[0][0].color,
            [33, 44, 55, 255]
        );
        assert!(artwork(&doc, 9, 5, Some(2)).is_none());
        assert!(artwork(&doc, 9, 7, Some(5)).unwrap().0.is_empty());
    }
}
