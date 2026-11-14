#[cfg(test)]
use crate::Document;
use crate::Editor;
use crate::design::{Icon, button, draw};
#[cfg(test)]
use eframe::egui::Pos2;
use eframe::egui::{self, Color32, Rect, Sense, Vec2};
fn top_menu(ui: &mut egui::Ui, label: &str, content: impl FnOnce(&mut egui::Ui)) {
    let label = egui::RichText::new(crate::i18n::tr(label)).size(13.);
    let trigger = egui::Button::new(label)
        .frame_when_inactive(false)
        .corner_radius(6.)
        .min_size(egui::vec2(46., 28.));
    egui::containers::menu::MenuButton::from_button(trigger).ui(ui, content);
}

fn file_submenu(ui: &mut egui::Ui, icon: Icon, label: &str, content: impl FnOnce(&mut egui::Ui)) {
    let response = ui.menu_button(format!("        {}", crate::i18n::tr(label)), content);
    draw(
        ui.painter(),
        Rect::from_center_size(
            egui::pos2(
                response.response.rect.left() + 12.,
                response.response.rect.center().y,
            ),
            Vec2::splat(15.),
        ),
        icon,
        crate::themes::palette().text,
    );
}

impl Editor {
    fn layer_objects_ui(&mut self, ui: &mut egui::Ui) {
        use crate::features::ObjectFolderMode;
        let layer = &self.doc.layers[self.layer];
        let marks = &layer.frames[self.frame];
        let editable = layer.visible && !layer.options.locked && !layer.options.raster;
        let selection: Vec<_> = self
            .canvas
            .selected
            .iter()
            .filter_map(|&(index, mark)| {
                (index == self.layer && marks.get(mark).is_some_and(|mark| !mark.style.raster_tile))
                    .then_some(mark)
            })
            .collect();
        crate::design::category(ui, "Layer objects");
        ui.label(&layer.name);
        let mut create = None;
        ui.add_enabled_ui(
            editable && !selection.is_empty() && layer.options.object_folders.len() < 200,
            |ui| {
                ui.menu_button(crate::i18n::tr("Group selected objects"), |ui| {
                    for (mode, label) in [
                        (ObjectFolderMode::Normal, "Folder"),
                        (ObjectFolderMode::Switch, "Switch folder"),
                        (ObjectFolderMode::Solo, "Solo folder"),
                    ] {
                        if button(ui, Icon::FolderPlus, label, "", false, true).clicked() {
                            create = Some((mode, crate::i18n::tr(label).to_owned()));
                            ui.close();
                        }
                    }
                });
            },
        );
        let mut selected = None;
        let mut choose = None;
        let mut remove = None;
        let label = |index: usize, mark: &crate::Mark| {
            let (icon, name) = if let Some(instance) = &mark.style.instance {
                (
                    Icon::Symbol,
                    self.doc
                        .extras
                        .symbols
                        .iter()
                        .find(|symbol| symbol.id == instance.symbol)
                        .map_or(crate::i18n::tr("Símbolo"), |symbol| symbol.name.as_str()),
                )
            } else {
                (Icon::Object, crate::i18n::tr("Artwork"))
            };
            (icon, format!("{name} · {}", index + 1))
        };
        let folders = &layer.options.object_folders;
        let mut rows_by_folder = std::collections::HashMap::<u64, Vec<usize>>::new();
        let mut ungrouped = Vec::new();
        let folder_ids: std::collections::HashSet<_> =
            folders.iter().map(|folder| folder.id).collect();
        for (index, mark) in marks
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, mark)| !mark.style.raster_tile)
        {
            if let Some([id, _]) = mark.style.object_folder
                && folder_ids.contains(&id)
            {
                rows_by_folder.entry(id).or_default().push(index);
            } else {
                ungrouped.push(index);
            }
        }
        let mut rows = Vec::with_capacity(folders.len() + marks.len());
        for folder in folders {
            rows.push((Some(folder), None));
            if let Some(children) = rows_by_folder.get(&folder.id) {
                rows.extend(children.iter().map(|&index| (Some(folder), Some(index))));
            }
        }
        rows.extend(ungrouped.into_iter().map(|index| (None, Some(index))));
        egui::ScrollArea::vertical().show_rows(ui, 30., rows.len(), |ui, visible| {
            for row in visible {
                let (folder, index) = rows[row];
                ui.push_id(row, |ui| {
                    ui.horizontal(|ui| {
                        if let Some(index) = index {
                            let mark = &marks[index];
                            if let Some(folder) = folder {
                                ui.add_space(12.);
                                if folder.mode != ObjectFolderMode::Normal
                                    && let Some([id, object]) = mark.style.object_folder
                                    && button(
                                        ui,
                                        Icon::Eye,
                                        "",
                                        "Show this object",
                                        folder.choice(self.frame) == Some(object),
                                        editable,
                                    )
                                    .clicked()
                                {
                                    choose = Some((id, object));
                                }
                            }
                            let (icon, name) = label(index, mark);
                            if button(ui, icon, &name, "", selection.contains(&index), editable)
                                .clicked()
                            {
                                selected = Some(index);
                            }
                        } else if let Some(folder) = folder {
                            if button(ui, Icon::Trash, "", "Delete object folder", false, editable)
                                .clicked()
                            {
                                remove = Some(folder.id);
                            }
                            ui.label(&folder.name);
                        }
                    });
                });
            }
        });
        if let Some(index) = selected {
            if ui.input(|input| input.modifiers.command) {
                let id = (self.layer, index);
                if self.canvas.selected.contains(&id) {
                    self.canvas.selected.retain(|&item| item != id);
                } else {
                    self.canvas.selected.push(id);
                }
                self.canvas.pivot = None;
            } else {
                self.canvas.clear_selection();
                self.canvas.selected.push((self.layer, index));
            }
            self.frame_clipboard.focus = false;
        }
        if create.is_some() || choose.is_some() || remove.is_some() {
            let selected = self.canvas.selected.clone();
            self.checkpoint();
            let layer = &mut self.doc.layers[self.layer];
            if let Some((mode, name)) = create {
                let id = crate::features::id();
                let mut first = None;
                for index in selection {
                    let object = crate::features::id();
                    first.get_or_insert(object);
                    layer.frames[self.frame][index].style.object_folder = Some([id, object]);
                }
                let choices = first
                    .into_iter()
                    .map(|object| (self.frame, object))
                    .collect();
                let number = layer.options.object_folders.len() + 1;
                layer
                    .options
                    .object_folders
                    .push(crate::features::ObjectFolder {
                        id,
                        name: format!("{name} {number}"),
                        mode,
                        choices,
                    });
            } else if let Some((id, object)) = choose
                && let Some(folder) = layer
                    .options
                    .object_folders
                    .iter_mut()
                    .find(|folder| folder.id == id)
            {
                if folder.mode == ObjectFolderMode::Solo {
                    folder.choices = vec![(0, object)].into();
                } else {
                    match folder
                        .choices
                        .binary_search_by_key(&self.frame, |key| key.0)
                    {
                        Ok(index) => folder.choices[index].1 = object,
                        Err(index) => folder.choices.insert(index, (self.frame, object)),
                    }
                }
            } else if let Some(id) = remove {
                layer
                    .options
                    .object_folders
                    .retain(|folder| folder.id != id);
            }
            self.canvas.selected = selected;
            self.recover();
        }
    }

    pub fn workspace(&mut self, ui: &mut egui::Ui) {
        self.animation.thumbnail_cache.begin_frame();
        let ctx = ui.ctx().clone();
        egui::Panel::top("application")
            .exact_size(42.)
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    let (logo_rect, _) = ui.allocate_exact_size(Vec2::splat(28.), Sense::hover());
                    crate::design::logo(ui.painter(), logo_rect);
                    ui.label(egui::RichText::new("LibreToon").strong().size(14.));
                    ui.add_space(8.);
                    top_menu(ui, "Archivo", |ui| {
                        if button(ui, Icon::New, "Nuevo", "Nuevo documento", false, true).clicked()
                        {
                            self.new_document();
                            ui.close();
                        }
                        if button(
                            ui,
                            Icon::Open,
                            "Abrir proyecto…",
                            "Abrir proyecto",
                            false,
                            true,
                        )
                        .clicked()
                        {
                            self.open();
                            ui.close();
                        }
                        file_submenu(ui, Icon::Open, "Importar", |ui| {
                            if button(
                                ui,
                                Icon::Open,
                                "Adobe Animate (.fla)…",
                                "Import Adobe Animate project",
                                false,
                                true,
                            )
                            .clicked()
                            {
                                self.import_fla();
                                ui.close();
                            }
                        });
                        if button(
                            ui,
                            Icon::Open,
                            "Abrir copia guardada…",
                            "Abrir el archivo del disco en otra pestaña sin descartar cambios",
                            false,
                            self.path.is_some() && !self.hub.home,
                        )
                        .clicked()
                        {
                            self.open_saved_document_copy();
                            ui.close();
                        }
                        if button(ui, Icon::Save, "Guardar", "Ctrl+S", false, true).clicked() {
                            self.save_project(false);
                            ui.close();
                        }
                        if button(
                            ui,
                            Icon::SaveAs,
                            "Guardar como…",
                            "Guardar una copia",
                            false,
                            true,
                        )
                        .clicked()
                        {
                            self.save_project(true);
                            ui.close();
                        }
                        if let Some(path) = &self.path {
                            let backups: Vec<_> = (1..=3)
                                .filter_map(|version| {
                                    let file = crate::project_save::backup_path(path, version);
                                    file.is_file().then_some((version, file))
                                })
                                .collect();
                            if !backups.is_empty() {
                                file_submenu(ui, Icon::Undo, "Open previous version", |ui| {
                                    for (version, file) in backups {
                                        let label =
                                            format!("{} {version}", crate::i18n::tr("Backup"));
                                        if button(
                                            ui,
                                            Icon::SaveAs,
                                            &label,
                                            "Open previous version",
                                            false,
                                            true,
                                        )
                                        .clicked()
                                        {
                                            self.loader.request_saved_copy(file);
                                            ui.close();
                                        }
                                    }
                                });
                            }
                        }
                        if !self.hub.home
                            && (self.doc.extras.imported
                                || !self.doc.extras.import_warnings.is_empty())
                            && button(
                                ui,
                                Icon::DocumentSettings,
                                "Informe de importación FLA…",
                                "Informe de importación FLA…",
                                false,
                                true,
                            )
                            .clicked()
                        {
                            self.import_report_open = true;
                            ui.close();
                        }
                        ui.separator();
                        if button(
                            ui,
                            Icon::DocumentSettings,
                            "Ajustes de documento…",
                            "Ajustes de documento…",
                            false,
                            true,
                        )
                        .clicked()
                        {
                            self.open_document_settings();
                            ui.close();
                        }
                        if button(
                            ui,
                            Icon::Preferences,
                            "Preferencias del programa…",
                            "Preferencias del programa…",
                            false,
                            true,
                        )
                        .clicked()
                        {
                            self.studio.preferences_open = true;
                            ui.close();
                        }
                        ui.separator();
                        if button(
                            ui,
                            Icon::Export,
                            "Export Adobe Animate (.fla)…",
                            "Export Adobe Animate (.fla)…",
                            false,
                            !self.hub.home,
                        )
                        .clicked()
                        {
                            self.export_fla_copy();
                            ui.close();
                        }
                        if button(
                            ui,
                            Icon::Film,
                            "Export for Toon Boom Studio (PNG sequence)…",
                            "Export rendered frames for Toon Boom Studio",
                            false,
                            !self.hub.home,
                        )
                        .clicked()
                        {
                            self.open_export(false);
                            if let Some(settings) = &mut self.studio.export {
                                settings.use_png_sequence();
                            }
                            ui.close();
                        }
                        if button(
                            ui,
                            Icon::Grid,
                            "Export to Sprite Sheet…",
                            "Export to Sprite Sheet…",
                            false,
                            !self.hub.home,
                        )
                        .clicked()
                        {
                            self.open_sprite_sheet_export();
                            ui.close();
                        }
                        if button(
                            ui,
                            Icon::Export,
                            "Exportar vídeo…",
                            "Exportar vídeo…",
                            false,
                            true,
                        )
                        .clicked()
                        {
                            self.open_export(false);
                            ui.close();
                        }
                        if button(
                            ui,
                            Icon::Film,
                            "Exportar GIF…",
                            "Exportar GIF…",
                            false,
                            true,
                        )
                        .clicked()
                        {
                            self.open_export(true);
                            ui.close();
                        }
                    });
                    top_menu(ui, "Edición", |ui| {
                        if button(
                            ui,
                            Icon::Copy,
                            "Copy frames",
                            "Copy frames",
                            false,
                            !self.hub.home,
                        )
                        .clicked()
                        {
                            self.copy_frames_to_clipboard(ui.ctx());
                            ui.close();
                        }
                        if button(
                            ui,
                            Icon::Paste,
                            "Paste frames",
                            "Paste frames",
                            false,
                            !self.hub.home && self.can_paste_timeline_frames(),
                        )
                        .clicked()
                        {
                            self.paste_timeline_frames();
                            ui.close();
                        }
                        ui.separator();

                        if button(
                            ui,
                            Icon::Copy,
                            "Copiar",
                            "Copiar",
                            false,
                            !self.canvas.selected.is_empty(),
                        )
                        .clicked()
                        {
                            self.copy_canvas_to_clipboard(ui.ctx(), false);
                            ui.close();
                        }
                        if button(
                            ui,
                            Icon::Cut,
                            "Cortar",
                            "Cortar",
                            false,
                            !self.canvas.selected.is_empty(),
                        )
                        .clicked()
                        {
                            self.copy_canvas_to_clipboard(ui.ctx(), true);
                            ui.close();
                        }
                        if button(
                            ui,
                            Icon::Paste,
                            "Pegar",
                            "Pegar",
                            false,
                            !self.hub.home && !self.doc.layers[self.layer].options.locked,
                        )
                        .clicked()
                        {
                            if !self.paste_media_image() {
                                self.paste_selection();
                            }
                            ui.close();
                        }
                        if button(
                            ui,
                            Icon::Symbol,
                            "Convertir a símbolo · F8",
                            "Convertir a símbolo",
                            false,
                            !self.canvas.selected.is_empty(),
                        )
                        .clicked()
                        {
                            self.convert_symbol();
                            ui.close();
                        }
                        ui.separator();

                        if button(
                            ui,
                            Icon::Undo,
                            "Deshacer",
                            &self.action_hint(crate::shortcuts::Action::Undo),
                            false,
                            self.can_undo(),
                        )
                        .clicked()
                        {
                            self.perform_action(crate::shortcuts::Action::Undo, ui.ctx());
                            ui.close();
                        }
                        if button(
                            ui,
                            Icon::Redo,
                            "Rehacer",
                            &self.action_hint(crate::shortcuts::Action::Redo),
                            false,
                            self.can_redo(),
                        )
                        .clicked()
                        {
                            self.perform_action(crate::shortcuts::Action::Redo, ui.ctx());
                            ui.close();
                        }
                    });
                    top_menu(ui, "Vista", |ui| self.view_menu(ui));
                    top_menu(ui, "Herramientas", |ui| {
                        if button(ui, Icon::Reference, "RefTool", "RefTool", false, true).clicked()
                        {
                            self.new_reference_window();
                            ui.close();
                        }
                    });
                    top_menu(ui, "Ayuda", |ui| {
                        if button(ui, Icon::Info, "User Manual", "", false, true).clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(
                                "https://alvillafm.github.io/LibreToon/",
                            ));
                            ui.close();
                        }
                        if button(ui, Icon::Rotate, "Buscar actualizaciones", "", false, true)
                            .clicked()
                        {
                            self.updater.check(ui.ctx(), true);
                            ui.close();
                        }
                        if button(ui, Icon::Info, "Credits", "Credits", false, true).clicked() {
                            self.studio.credits_open = true;
                            ui.close();
                        }
                        crate::design::category(ui, "VERSIÓN ACTUAL:");
                        ui.label(env!("CARGO_PKG_VERSION"));
                    });
                    ui.add_space(12.);
                    ui.separator();
                    if button(ui, Icon::Open, "", "Abrir proyecto", false, true).clicked() {
                        self.open();
                    }
                    if button(ui, Icon::Save, "", "Guardar · Ctrl+S", false, true).clicked() {
                        self.save_project(false);
                    }
                    ui.separator();
                    if button(
                        ui,
                        Icon::Undo,
                        "",
                        &self.action_tip(crate::shortcuts::Action::Undo),
                        false,
                        self.can_undo(),
                    )
                    .clicked()
                    {
                        self.perform_action(crate::shortcuts::Action::Undo, ui.ctx());
                    }
                    if button(
                        ui,
                        Icon::Redo,
                        "",
                        &self.action_tip(crate::shortcuts::Action::Redo),
                        false,
                        self.can_redo(),
                    )
                    .clicked()
                    {
                        self.perform_action(crate::shortcuts::Action::Redo, ui.ctx());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.window_controls(ui);
                        let (rect, response) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 32.),
                            Sense::click_and_drag(),
                        );
                        let _ = rect;
                        if response.drag_started() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                        }
                        if response.double_clicked() {
                            let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                        }
                    });
                });
            });
        self.document_tabs(ui);
        if self.hub.home {
            self.home_ui(ui);
            self.document_settings_ui(&ctx);
            self.preferences_ui(&ctx);
            self.notice_ui(&ctx);
            return;
        }
        egui::Panel::left("tools")
            .exact_size(76.)
            .resizable(false)
            .show(ui, |ui| {
                let tool_height = ((ui.available_height() - 300.) / 16.).clamp(18., 24.);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 1.;
                    ui.spacing_mut().interact_size.y = tool_height;
                    use crate::canvas::Tool::*;
                    for (label, rows) in [
                        (
                            "SELECCIÓN",
                            vec![(Select, Object), (Lasso, PolygonSelect), (Wand, Wand)],
                        ),
                        ("EDICIÓN", vec![(Transform, Mirror)]),
                        ("PINTURA", vec![(Brush, Eraser), (Bucket, Eyedropper)]),
                        ("VECTORIAL", vec![(Pen, Node), (Width, Trim)]),
                        ("FORMAS", vec![(Line, Rectangle), (Ellipse, Polygon)]),
                        ("TEXTO", vec![(Text, Text)]),
                        (
                            "LIENZO",
                            vec![
                                (Pan, Zoom),
                                (Ruler, Symmetry),
                                (Perspective, Camera),
                                (Mannequin, Mannequin),
                            ],
                        ),
                        ("RIGGING", vec![(Bones, Puppet)]),
                    ] {
                        ui.add_space(3.);
                        ui.label(
                            egui::RichText::new(crate::i18n::tr(label))
                                .size(8.)
                                .color(crate::themes::muted()),
                        );
                        ui.separator();
                        for (left, right) in rows {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 3.;
                                for (index, tool) in [left, right].into_iter().enumerate() {
                                    if index == 1 && left == right {
                                        continue;
                                    }
                                    self.tool_button(ui, tool);
                                }
                            });
                        }
                    }
                    ui.add_space(7.);
                    ui.label(
                        egui::RichText::new(crate::i18n::tr("COLOR"))
                            .size(8.)
                            .color(crate::themes::muted()),
                    );
                    ui.separator();
                    self.color_swatches(ui);
                });
            });
        if self.studio.layer_objects_open {
            egui::Panel::left("layer-objects")
                .default_size(200.)
                .min_size(140.)
                .show(ui, |ui| self.layer_objects_ui(ui));
        }
        egui::Panel::right("properties")
            .default_size(370.)
            .min_size(245.)
            .max_size(650.)
            .resizable(true)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if button(
                        ui,
                        Icon::Settings,
                        "Propiedades",
                        "Propiedades",
                        !self.feature.library && !self.clips.open && !self.studio.palette.open,
                        true,
                    )
                    .clicked()
                    {
                        self.feature.library = false;
                        self.clips.open = false;
                        self.studio.palette.open = false;
                    }
                    if button(
                        ui,
                        Icon::Library,
                        "Biblioteca",
                        "Biblioteca",
                        self.feature.library && !self.clips.open && !self.studio.palette.open,
                        true,
                    )
                    .clicked()
                    {
                        self.feature.library = true;
                        self.clips.open = false;
                        self.studio.palette.open = false;
                    }
                    if button(
                        ui,
                        Icon::Film,
                        "Animation",
                        "Animation Manager",
                        self.clips.open && !self.studio.palette.open,
                        true,
                    )
                    .clicked()
                    {
                        self.clips.open = true;
                        self.studio.palette.open = false;
                    }
                    if button(
                        ui,
                        Icon::Eyedropper,
                        "Color",
                        "Color palettes",
                        self.studio.palette.open,
                        true,
                    )
                    .clicked()
                    {
                        self.studio.palette.open = true;
                    }
                });
                ui.separator();
                if self.studio.palette.open {
                    self.palette_ui(ui);
                } else if self.clips.open {
                    self.animation_manager_ui(ui);
                } else if self.feature.library {
                    self.library_ui(ui);
                } else {
                    self.properties_ui(ui);
                }
            });
        self.sequencer(ui);
        self.finish_animation_clip_ui(&ctx);
        self.animation_clip_dialog(&ctx);
        self.grid_settings_ui(&ctx);
        egui::Panel::top("document_tab")
            .exact_size(34.)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    draw(
                        ui.painter(),
                        Rect::from_min_size(ui.cursor().min, Vec2::splat(20.)),
                        Icon::Film,
                        crate::themes::accent(),
                    );
                    ui.add_space(26.);
                    ui.label(
                        self.path
                            .as_ref()
                            .and_then(|p| p.file_name())
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or("Sin título.ltoon".into()),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if button(
                            ui,
                            Icon::Stage,
                            "",
                            "Solo escenario",
                            self.studio.stage_only,
                            true,
                        )
                        .clicked()
                        {
                            self.studio.stage_only = !self.studio.stage_only;
                            ui.ctx().request_repaint();
                        }
                        if self
                            .doc
                            .layers
                            .iter()
                            .any(|layer| layer.options.light.is_some())
                            && button(
                                ui,
                                Icon::Bulb,
                                "",
                                "Show / Hide Lights",
                                !self.post.hide_lights,
                                true,
                            )
                            .clicked()
                        {
                            self.post.hide_lights = !self.post.hide_lights;
                            ui.ctx().request_repaint();
                        }
                        if crate::post::present(&self.doc)
                            && button(
                                ui,
                                Icon::Fx,
                                "",
                                "Show / Hide Post-Processing",
                                !self.post.hidden,
                                true,
                            )
                            .clicked()
                        {
                            self.post.hidden = !self.post.hidden;
                            ui.ctx().request_repaint();
                        }
                        if crate::depth::camera(&self.doc, self.frame).is_some_and(|c| {
                            crate::animation::camera_optics(c, self.frame).strength > 0.
                        }) {
                            if button(
                                ui,
                                Icon::Blur,
                                "",
                                "Show / Hide Camera Blur",
                                !self.depth.hide_blur,
                                true,
                            )
                            .clicked()
                            {
                                self.depth.hide_blur = !self.depth.hide_blur;
                                ui.ctx().request_repaint();
                            }
                        }
                        if button(
                            ui,
                            Icon::Fit,
                            "",
                            "Encajar escenario y centrar vista",
                            false,
                            true,
                        )
                        .clicked()
                        {
                            self.canvas.fit();
                        }
                        if button(
                            ui,
                            Icon::Rotate,
                            "",
                            "Rotar lienzo: arrastra sobre el escenario",
                            self.tool == crate::canvas::Tool::RotateView,
                            true,
                        )
                        .clicked()
                        {
                            self.tool = crate::canvas::Tool::RotateView;
                        }
                        let mut degrees = self.canvas.angle.to_degrees();
                        if ui
                            .add(
                                egui::DragValue::new(&mut degrees)
                                    .speed(1.)
                                    .suffix(crate::i18n::tr("°")),
                            )
                            .on_hover_text(crate::i18n::tr(
                                "Ángulo de la vista. Doble clic para escribir 0 y restablecer.",
                            ))
                            .changed()
                        {
                            self.canvas.angle = degrees.rem_euclid(360.).to_radians();
                        }
                        ui.label(
                            egui::RichText::new(format!("{:.0}%", self.canvas.actual_scale * 100.))
                                .size(11.)
                                .color(crate::themes::muted()),
                        );
                    });
                });
            });
        egui::Panel::top("scene_bar")
            .exact_size(28.)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if !self.feature.symbol_stack.is_empty() {
                        if button(
                            ui,
                            Icon::Previous,
                            "Volver a escena",
                            "Volver a escena",
                            false,
                            true,
                        )
                        .clicked()
                        {
                            self.exit_symbol();
                        }
                        if let Some((_, _, _, id)) = self.feature.symbol_stack.last() {
                            if let Some(symbol) =
                                self.doc.extras.symbols.iter().find(|s| s.id == *id)
                            {
                                ui.label(
                                    egui::RichText::new(format!("/ {}", symbol.name))
                                        .color(crate::themes::accent()),
                                );
                            }
                        }
                    } else {
                        self.scene_dropdown(ui);
                        ui.label(
                            egui::RichText::new(crate::i18n::tr(" /  Escenario"))
                                .size(11.)
                                .color(crate::themes::muted()),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let quality = &mut self.studio.prefs.viewport_quality;
                        let before = *quality;
                        egui::ComboBox::from_id_salt("viewport-quality")
                            .selected_text(crate::i18n::tr(quality.label()))
                            .show_ui(ui, |ui| {
                                for option in crate::depth::ViewportQuality::ALL {
                                    ui.selectable_value(
                                        quality,
                                        option,
                                        crate::i18n::tr(option.label()),
                                    );
                                }
                            })
                            .response
                            .on_hover_text(crate::i18n::tr("Viewport quality"));
                        if *quality != before {
                            if let Err(error) = self.studio.save() {
                                self.status = error;
                            }
                            ui.ctx().request_repaint();
                        }
                    });
                });
            });
        if self.in_character_symbol() {
            egui::Panel::top("character-symbol-tools")
                .exact_size(32.)
                .show(ui, |ui| {
                    self.character_toolbar(ui);
                });
        }
        if !ctx.egui_wants_keyboard_input()
            && !self.animation.scenes_open
            && self.animation.picker.is_none()
            && !self.studio.preferences_open
            && self.document_settings.is_none()
            && self.studio.export.is_none()
        {
            if self.studio.prefs.bindings[17].key == "W"
                && !self.studio.prefs.bindings[17].ctrl
                && !self.studio.prefs.bindings[17].shift
                && !self.studio.prefs.bindings[17].alt
                && !self.animation.scenes_open
                && self.animation.picker.is_none()
                && !self.studio.preferences_open
                && self.document_settings.is_none()
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::W))
            {
                self.activate_tool(crate::canvas::Tool::Width);
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::X)) {
                self.swap_colors();
                self.change_selected_style(true, true, false, false);
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::D)) {
                self.color = Color32::BLACK;
                self.fill_color = Color32::WHITE;
                self.stroke_enabled = true;
                self.fill_enabled = true;
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F8)) {
                self.convert_symbol();
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Comma)) {
                self.frame = self.frame.saturating_sub(1);
                self.playing = false;
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Period)) {
                self.frame = (self.frame + 1).min(self.doc.layers[0].frames.len() - 1);
                self.playing = false;
            }
            self.tool_shortcuts(&ctx);
        }
        if !ctx.input(|i| i.pointer.primary_down()) {
            self.style_transaction = false;
        }
        self.document_settings_ui(&ctx);
        self.preferences_ui(&ctx);
        self.scenes_ui(&ctx);
        self.frame_picker_ui(&ctx);
        self.export_ui(&ctx);
        self.notice_ui(&ctx);
    }
    pub(crate) fn new_document(&mut self) {
        self.playing = false;
        self.document_settings = Some(crate::inspector::DocumentSettings {
            creating: true,
            width: 1920,
            height: 1080,
            fps: 24,
        });
    }
}

/// Include partially visible cells, clamp to content and skip offscreen work.
pub(crate) fn visible_range(
    origin: f32,
    size: f32,
    count: usize,
    min: f32,
    max: f32,
) -> std::ops::Range<usize> {
    let first = ((min - origin) / size).floor().max(0.) as usize;
    let end = ((max - origin) / size).ceil().max(0.) as usize;
    first.min(count)..end.min(count).max(first.min(count))
}

#[cfg(test)]
mod performance_tests {
    use super::*;
    #[test]
    fn timeline_only_visits_visible_cells_at_any_scroll_offset() {
        assert_eq!(visible_range(180., 22., 10_000, 0., 1000.), 0..38);
        assert_eq!(visible_range(-10000., 22., 10_000, 0., 1000.), 454..500);
        assert_eq!(visible_range(180., 22., 10_000, 0., 100.), 0..0);
        assert_eq!(
            visible_range(-300000., 22., 10_000, 0., 1000.),
            10000..10000
        );
        assert_eq!(visible_range(24., 27., 100, 25., 100.), 0..3);
    }
}

#[cfg(test)]
mod timeline_ui_tests {
    use super::*;
    fn draw_workspace(editor: &mut Editor, ctx: &egui::Context) -> usize {
        editor.hub.home = false;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1360., 860.))),
                ..Default::default()
            },
            |ui| editor.workspace(ui),
        );
        output.textures_delta.clear();
        output.shapes.len()
    }
    #[test]
    fn large_timeline_keeps_paint_work_bounded_by_viewport() {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut editor = Editor::new(&cc);
        editor.doc = Document::default();
        editor.doc.layers[0].frames.resize(100, Default::default());
        let layer = editor.doc.layers[0].clone();
        editor.doc.layers.resize(10, layer);
        draw_workspace(&mut editor, &ctx);
        let small = draw_workspace(&mut editor, &ctx);
        editor.doc.layers[0]
            .frames
            .resize(10_000, Default::default());
        let layer = editor.doc.layers[0].clone();
        editor.doc.layers.resize(100, layer);
        let large = draw_workspace(&mut editor, &ctx);
        eprintln!("timeline shapes: 10x100={small}, 100x10000={large}");
        assert!(large <= small + 32, "small={small}, large={large}");
    }
}

#[cfg(test)]
mod shortcut_regressions {
    use super::*;
    #[test]
    fn shift_w_uses_key_event_modifiers_even_after_shift_was_released() {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut editor = Editor::new(&cc);
        editor.doc = Document::default();
        editor.hub.home = false;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1360., 860.))),
                events: vec![
                    egui::Event::Key {
                        key: egui::Key::W,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::SHIFT,
                    },
                    egui::Event::Key {
                        key: egui::Key::W,
                        physical_key: None,
                        pressed: false,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..Default::default()
            },
            |ui| editor.workspace(ui),
        );
        output.textures_delta.clear();
        assert!(editor.tool == crate::canvas::Tool::Width);
    }
}
