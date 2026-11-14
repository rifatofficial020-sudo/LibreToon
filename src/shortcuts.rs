//! Shared, exact-modifier shortcut routing for tools and document actions.
use crate::{Editor, design::Icon, studio::Binding};
use eframe::egui;
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Undo,
    Redo,
    Depth,
    Blur,
    Stage,
    Scenes,
    ImportMedia,
    Properties,
    Library,
    AddCamera,
    CameraKey,
    Front,
    Forward,
    Backward,
    Back,
    ShiftTrace,
    ExposureSheet,
    BatchTransform,
    AnimationManager,
    ColorPanel,
    PostProcessing,
    Lights,
    Onion,
    LayerObjects,
    CommandHelper,
    PreparePreview,
    CreateClip,
    KeyAdd,
    BlankKey,
    ExtendExposure,
    ShortenExposure,
    DeleteKey,
}
// Append new entries: preferences from older versions keep their action indices.
pub const ACTIONS: &[Action] = &[
    Action::Undo,
    Action::Redo,
    Action::Depth,
    Action::Blur,
    Action::Stage,
    Action::Scenes,
    Action::ImportMedia,
    Action::Properties,
    Action::Library,
    Action::AddCamera,
    Action::CameraKey,
    Action::Front,
    Action::Forward,
    Action::Backward,
    Action::Back,
    Action::ShiftTrace,
    Action::ExposureSheet,
    Action::BatchTransform,
    Action::AnimationManager,
    Action::ColorPanel,
    Action::PostProcessing,
    Action::Lights,
    Action::Onion,
    Action::LayerObjects,
    Action::CommandHelper,
    Action::PreparePreview,
    Action::CreateClip,
    Action::KeyAdd,
    Action::BlankKey,
    Action::ExtendExposure,
    Action::ShortenExposure,
    Action::DeleteKey,
];
impl Action {
    pub fn name(self) -> &'static str {
        match self {
            Self::Undo => "Deshacer",
            Self::Redo => "Rehacer",
            Self::Depth => "Profundidad de capas",
            Self::Blur => "Show / Hide Camera Blur",
            Self::Stage => "Solo escenario",
            Self::Scenes => "Scene manager",
            Self::ImportMedia => "Importar medios…",
            Self::Properties => "Propiedades",
            Self::Library => "Biblioteca",
            Self::AddCamera => "Añadir cámara",
            Self::CameraKey => "Insertar clave de cámara",
            Self::Front => "Traer al frente",
            Self::Forward => "Traer adelante",
            Self::Backward => "Enviar atrás",
            Self::Back => "Enviar al fondo",
            Self::ShiftTrace => "Shift & Trace",
            Self::ExposureSheet => "Exposure sheet",
            Self::BatchTransform => "Transform keyframes",
            Self::AnimationManager => "Animation Manager",
            Self::ColorPanel => "Color palettes",
            Self::PostProcessing => "Show / Hide Post-Processing",
            Self::Lights => "Show / Hide Lights",
            Self::Onion => "Papel cebolla",
            Self::LayerObjects => "Layer objects",
            Self::CommandHelper => "COMMAND HELPER",
            Self::PreparePreview => "Prepare preview",
            Self::CreateClip => "Create Animation Clip…",
            Self::KeyAdd => "Insert keyframe",
            Self::BlankKey => "Insert blank keyframe",
            Self::ExtendExposure => "Ampliar duración de fotograma",
            Self::ShortenExposure => "Reducir duración de fotograma",
            Self::DeleteKey => "Eliminar fotograma clave",
        }
    }
    pub fn icon(self) -> Icon {
        match self {
            Self::Undo => Icon::Undo,
            Self::Redo => Icon::Redo,
            Self::Depth => Icon::Depth,
            Self::Blur => Icon::Blur,
            Self::Stage => Icon::Stage,
            Self::Scenes => Icon::Film,
            Self::ImportMedia => Icon::Open,
            Self::Properties => Icon::Settings,
            Self::Library => Icon::Library,
            Self::AddCamera => Icon::Camera,
            Self::CameraKey => Icon::Keyframe,
            Self::Front => Icon::BringFront,
            Self::Forward => Icon::StepFront,
            Self::Backward => Icon::StepBack,
            Self::Back => Icon::SendBack,
            Self::ShiftTrace => Icon::Reference,
            Self::ExposureSheet => Icon::Layers,
            Self::BatchTransform => Icon::Transform,
            Self::AnimationManager => Icon::Film,
            Self::ColorPanel => Icon::Eyedropper,
            Self::PostProcessing => Icon::Fx,
            Self::Lights => Icon::Bulb,
            Self::Onion => Icon::Onion,
            Self::LayerObjects => Icon::Layers,
            Self::CommandHelper => Icon::CommandHelper,
            Self::PreparePreview => Icon::Play,
            Self::CreateClip => Icon::Film,
            Self::KeyAdd => Icon::KeyAdd,
            Self::BlankKey => Icon::BlankKeyAdd,
            Self::ExtendExposure => Icon::FrameExtend,
            Self::ShortenExposure => Icon::FrameShorten,
            Self::DeleteKey => Icon::KeyRemove,
        }
    }
}
fn command(key: &str, shift: bool) -> Binding {
    Binding {
        key: key.into(),
        ctrl: true,
        shift,
        alt: false,
    }
}
pub fn defaults() -> Vec<Binding> {
    let mut bindings = vec![Binding::new(""); ACTIONS.len()];
    bindings[0] = command("Z", false);
    bindings[1] = if cfg!(target_os = "macos") {
        command("Z", true)
    } else {
        command("Y", false)
    };
    bindings
}
impl Binding {
    fn matches(&self, key: egui::Key, m: egui::Modifiers) -> bool {
        egui::Key::from_name(&self.key) == Some(key)
            && self.ctrl == (m.command || m.ctrl)
            && self.shift == m.shift
            && self.alt == m.alt
    }
    pub(crate) fn take_pressed(&self, ctx: &egui::Context) -> bool {
        if self.key.is_empty() {
            return false;
        }
        ctx.input_mut(|i| {
   let index=i.events.iter().position(|e|matches!(e,egui::Event::Key {key,modifiers,pressed:true,..} if self.matches(*key,*modifiers)));
   if let Some(index)=index {i.events.remove(index);true} else {false}
  })
    }
}
fn redo_alias(prefs: &crate::studio::Preferences) -> Option<Binding> {
    if prefs.actions.get(1) != defaults().get(1) {
        return None;
    }
    let alias = if cfg!(target_os = "macos") {
        command("Y", false)
    } else {
        command("Z", true)
    };
    if prefs
        .bindings
        .iter()
        .chain(&prefs.actions)
        .any(|b| *b == alias)
    {
        None
    } else {
        Some(alias)
    }
}
impl Editor {
    pub(crate) fn action_hint(&self, action: Action) -> String {
        let index = ACTIONS.iter().position(|&a| a == action).unwrap();
        let mut text = self
            .studio
            .prefs
            .actions
            .get(index)
            .map(Binding::label)
            .unwrap_or_default();
        if action == Action::Redo {
            if let Some(alias) = redo_alias(&self.studio.prefs) {
                text.push_str(" / ");
                text.push_str(&alias.label());
            }
        }
        if text.is_empty() { "—".into() } else { text }
    }
    pub(crate) fn action_tip(&self, action: Action) -> String {
        format!(
            "{} · {}",
            crate::i18n::tr(action.name()),
            self.action_hint(action)
        )
    }
    pub(crate) fn can_undo(&self) -> bool {
        !self.undo.is_empty() || self.canvas.can_undo_pen()
    }
    pub(crate) fn can_redo(&self) -> bool {
        !self.redo.is_empty() || self.canvas.can_redo_pen()
    }
    pub(crate) fn action_shortcuts(&mut self, ctx: &egui::Context) {
        if self.hub.home
            || self.studio.preferences_open
            || self.studio.export.is_some()
            || self.document_settings.is_some()
            || self.view_options.draft.is_some()
            || self.folder_groups.dialog.is_some()
            || self.animation.picker.is_some()
            || ctx.egui_wants_keyboard_input()
        {
            return;
        }
        let action = ACTIONS
            .iter()
            .copied()
            .zip(&self.studio.prefs.actions)
            .find_map(|(a, b)| b.take_pressed(ctx).then_some(a))
            .or_else(|| {
                redo_alias(&self.studio.prefs)
                    .filter(|b| b.take_pressed(ctx))
                    .map(|_| Action::Redo)
            });
        if let Some(action) = action {
            self.perform_action(action, ctx);
            ctx.request_repaint();
        }
    }
    pub(crate) fn perform_action(&mut self, action: Action, ctx: &egui::Context) {
        if matches!(action, Action::Undo | Action::Redo) {
            self.playing = false;
            self.restart_media_audio();
        }
        match action {
            Action::Undo => {
                if !self.canvas.undo_pen() {
                    self.history(false);
                }
            }
            Action::Redo => {
                if !self.canvas.redo_pen() {
                    self.history(true);
                }
            }
            Action::Depth => self.depth.open = !self.depth.open,
            Action::Blur => {
                if crate::depth::camera(&self.doc, self.frame)
                    .is_some_and(|c| crate::animation::camera_optics(c, self.frame).strength > 0.)
                {
                    self.depth.hide_blur = !self.depth.hide_blur;
                }
            }
            Action::Stage => self.studio.stage_only = !self.studio.stage_only,
            Action::Scenes => self.animation.scenes_open = !self.animation.scenes_open,
            Action::ImportMedia => self.import_media(ctx),
            Action::Properties | Action::Library => {
                self.feature.library = action == Action::Library;
                self.clips.open = false;
                self.studio.palette.open = false;
            }
            Action::ShiftTrace => self.toggle_shift_trace(),
            Action::ExposureSheet => self.animation.exposure_sheet ^= true,
            Action::BatchTransform => self.open_batch_transform(),
            Action::AnimationManager => {
                self.clips.open = true;
                self.studio.palette.open = false;
            }
            Action::ColorPanel => self.studio.palette.open = true,
            Action::PostProcessing => self.post.hidden ^= true,
            Action::Lights => self.post.hide_lights ^= true,
            Action::Onion => self.onion ^= true,
            Action::LayerObjects => self.studio.layer_objects_open ^= true,
            Action::CommandHelper => self.studio.command_helper ^= true,
            Action::PreparePreview => self.prepare_preview(None),
            Action::CreateClip => self.begin_animation_clip(),
            Action::KeyAdd
            | Action::BlankKey
            | Action::ExtendExposure
            | Action::ShortenExposure
            | Action::DeleteKey => {
                if crate::frame_workflow::editable(&self.doc.layers[self.layer]) {
                    let operation = match action {
                        Action::KeyAdd => 0,
                        Action::BlankKey => 1,
                        Action::ExtendExposure => 2,
                        Action::ShortenExposure => 3,
                        _ => 4,
                    };
                    self.timeline_edit(operation);
                }
            }
            Action::AddCamera => self.add_camera(),
            Action::CameraKey => {
                if let Some(id) = crate::depth::camera(&self.doc, self.frame)
                    .map(|c| c.id)
                    .or(self.feature.selected_camera)
                {
                    self.camera_timeline_action(id, self.frame, 0);
                }
            }
            Action::Front | Action::Forward | Action::Backward | Action::Back => {
                let op = match action {
                    Action::Front => crate::ordering::Order::Front,
                    Action::Forward => crate::ordering::Order::Forward,
                    Action::Backward => crate::ordering::Order::Backward,
                    _ => crate::ordering::Order::Back,
                };
                let mut state = std::mem::take(&mut self.canvas);
                self.arrange_selection(&mut state, op);
                self.canvas = state;
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Document, Mark, canvas::Tool, studio::Preferences};
    fn editor(ctx: &egui::Context) -> Editor {
        let mut e = Editor::new(&eframe::CreationContext::_new_kittest(ctx.clone()));
        e.doc = Document::default();
        e.undo.clear();
        e.redo.clear();
        e.hub.home = false;
        e.studio.prefs = Preferences::default();
        e.recovery = std::env::temp_dir().join(format!(
            "shortcuts-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ));
        e
    }
    fn press(ctx: &egui::Context, e: &mut Editor, key: egui::Key, modifiers: egui::Modifiers) {
        let mut out = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                }],
                ..Default::default()
            },
            |_| {
                e.action_shortcuts(ctx);
                e.tool_shortcuts(ctx);
            },
        );
        out.textures_delta.clear();
    }
    fn add_mark(e: &mut Editor, x: f32) {
        e.checkpoint();
        e.doc.layers[0].frames[0].push(Mark {
            points: vec![[x, 20.], [x + 10., 40.]].into(),
            fill: Some([0, 0, 0, 255]),
            ..Default::default()
        });
    }
    #[test]
    fn undo_and_both_standard_redo_shortcuts_move_in_opposite_directions() {
        let ctx = egui::Context::default();
        let mut e = editor(&ctx);
        add_mark(&mut e, 10.);
        add_mark(&mut e, 30.);
        let command = egui::Modifiers {
            command: true,
            ..Default::default()
        };
        press(&ctx, &mut e, egui::Key::Z, command);
        assert_eq!(e.doc.layers[0].frames[0].len(), 1);
        press(
            &ctx,
            &mut e,
            egui::Key::Z,
            egui::Modifiers {
                shift: true,
                ..command
            },
        );
        assert_eq!(e.doc.layers[0].frames[0].len(), 2);
        e.perform_action(Action::Undo, &ctx);
        assert_eq!(e.doc.layers[0].frames[0].len(), 1);
        press(
            &ctx,
            &mut e,
            egui::Key::Y,
            egui::Modifiers {
                ctrl: true,
                command: true,
                ..Default::default()
            },
        );
        assert_eq!(e.doc.layers[0].frames[0].len(), 2);
        e.perform_action(Action::Undo, &ctx);
        e.perform_action(Action::Redo, &ctx);
        assert_eq!(e.doc.layers[0].frames[0].len(), 2);
    }
    #[test]
    fn custom_actions_use_exact_event_modifiers_and_do_not_steal_claimed_redo_aliases() {
        let ctx = egui::Context::default();
        let mut e = editor(&ctx);
        add_mark(&mut e, 10.);
        press(
            &ctx,
            &mut e,
            egui::Key::Z,
            egui::Modifiers {
                ctrl: true,
                command: true,
                alt: true,
                ..Default::default()
            },
        );
        assert_eq!(e.doc.layers[0].frames[0].len(), 1);
        e.studio.prefs.actions[0] = Binding {
            key: "G".into(),
            ctrl: false,
            shift: false,
            alt: true,
        };
        press(
            &ctx,
            &mut e,
            egui::Key::Z,
            egui::Modifiers {
                command: true,
                ..Default::default()
            },
        );
        assert_eq!(e.doc.layers[0].frames[0].len(), 1);
        press(
            &ctx,
            &mut e,
            egui::Key::G,
            egui::Modifiers {
                alt: true,
                ..Default::default()
            },
        );
        assert!(e.doc.layers[0].frames[0].is_empty());
        let alias = redo_alias(&e.studio.prefs).unwrap();
        let index = crate::studio::TOOLS
            .iter()
            .position(|&t| t == Tool::Bones)
            .unwrap();
        e.studio.prefs.bindings[index] = alias.clone();
        press(
            &ctx,
            &mut e,
            egui::Key::from_name(&alias.key).unwrap(),
            egui::Modifiers {
                command: true,
                ctrl: true,
                shift: alias.shift,
                ..Default::default()
            },
        );
        assert!(e.doc.layers[0].frames[0].is_empty());
        assert!(e.tool == Tool::Bones);
    }
    #[test]
    fn old_preferences_keep_tool_bindings_and_gain_actions_without_reassigning_tools() {
        let mut prefs = Preferences::default();
        prefs.bindings[33] = Binding {
            key: "J".into(),
            ctrl: false,
            shift: true,
            alt: true,
        };
        let mut value = serde_json::to_value(&prefs).unwrap();
        value.as_object_mut().unwrap().remove("actions");
        let restored: Preferences = serde_json::from_value(value).unwrap();
        assert!(restored.bindings == prefs.bindings);
        assert!(restored.actions == defaults());
        for tool in [
            Tool::Bones,
            Tool::Puppet,
            Tool::Mannequin,
            Tool::Text,
            Tool::SketchBrush,
            Tool::Camera,
        ] {
            assert!(crate::studio::TOOLS.contains(&tool));
        }
        for action in [
            Action::Depth,
            Action::Blur,
            Action::ImportMedia,
            Action::Scenes,
            Action::CameraKey,
            Action::Front,
        ] {
            assert!(ACTIONS.contains(&action));
        }
    }
    #[test]
    fn history_restoration_ends_transactions_and_new_edits_clear_redo() {
        let ctx = egui::Context::default();
        let mut e = editor(&ctx);
        add_mark(&mut e, 10.);
        add_mark(&mut e, 30.);
        e.style_transaction = true;
        e.profile_transaction = true;
        e.drawing.text.transaction = true;
        e.animation.timing_transaction = true;
        e.perform_action(Action::Undo, &ctx);
        assert!(
            !e.style_transaction
                && !e.profile_transaction
                && !e.drawing.text.transaction
                && !e.animation.timing_transaction
        );
        assert!(e.can_redo());
        e.canvas.selected = vec![(0, 0)];
        e.fill_color = egui::Color32::RED;
        e.change_selected_style(true, false, false, false);
        assert!(!e.can_redo());
        e.perform_action(Action::Undo, &ctx);
        assert_eq!(e.doc.layers[0].frames[0][0].fill, Some([0, 0, 0, 255]));
    }
    #[test]
    fn newer_actions_can_be_bound_and_persisted_independently() {
        let ctx = egui::Context::default();
        let mut e = editor(&ctx);
        let index = ACTIONS.iter().position(|&a| a == Action::Depth).unwrap();
        e.studio.prefs.actions[index] = Binding {
            key: "G".into(),
            ctrl: false,
            shift: true,
            alt: true,
        };
        e.studio.prefs =
            serde_json::from_slice(&serde_json::to_vec(&e.studio.prefs).unwrap()).unwrap();
        let before = serde_json::to_vec(&e.doc).unwrap();
        press(
            &ctx,
            &mut e,
            egui::Key::G,
            egui::Modifiers {
                shift: true,
                alt: true,
                ..Default::default()
            },
        );
        assert!(e.depth.open);
        assert_eq!(before, serde_json::to_vec(&e.doc).unwrap());
    }
}
