use crate::{
    Editor, Mark,
    design::{Icon, button},
    document::Layer,
    i18n::tr,
    shared::SharedVec,
};
use eframe::egui;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Swatch {
    pub(crate) id: u64,
    pub(crate) name: String,
    color: [u8; 4],
}

#[derive(Default)]
pub(crate) struct Panel {
    pub(crate) open: bool,
    linked: Option<u64>,
    main: Option<(usize, usize)>,
    editing_color: bool,
}

impl Panel {
    pub(crate) fn clear_linked_selection(&mut self) {
        self.linked = None;
        self.main = None;
        self.editing_color = false;
    }
}

fn recolor(
    layers: &mut SharedVec<Layer>,
    id: u64,
    old: [u8; 4],
    new: Option<[u8; 4]>,
    cels: &mut std::collections::HashMap<usize, Option<SharedVec<Mark>>>,
) {
    for index in 0..layers.len() {
        for frame in 0..layers[index].frames.len() {
            let cel = &layers[index].frames[frame];
            let replacement = cels.entry(cel.identity()).or_insert_with(|| {
                if !cel.iter().any(|mark| {
                    mark.style.palette_fill == Some(id) || mark.style.palette_stroke == Some(id)
                }) {
                    return None;
                }
                let mut edited = cel.clone();
                for mark in &mut edited {
                    if mark.style.palette_fill == Some(id) {
                        if let Some(new) = new
                            && mark.fill == Some(old)
                            && mark.style.gradient.is_none()
                        {
                            mark.fill = Some(new);
                        } else {
                            mark.style.palette_fill = None;
                        }
                    }
                    if mark.style.palette_stroke == Some(id) {
                        if let Some(new) = new
                            && mark.stroke
                            && mark.color == old
                        {
                            mark.color = new;
                        } else {
                            mark.style.palette_stroke = None;
                        }
                    }
                }
                Some(edited)
            });
            if let Some(edited) = replacement {
                layers[index].frames[frame] = edited.clone();
            }
        }
    }
}

impl Editor {
    fn link_palette_selection(&mut self, swatch: Option<Swatch>) {
        let ids: Vec<_> = self
            .canvas
            .selected
            .iter()
            .copied()
            .filter(|&(layer, mark)| {
                self.doc.layers.get(layer).is_some_and(|layer| {
                    !layer.options.locked
                        && layer.frames[self.frame].get(mark).is_some_and(|mark| {
                            mark.fill.is_some()
                                && mark.style.instance.is_none()
                                && !mark.style.raster_tile
                                && mark.style.media_image.is_none()
                                && mark.style.gradient.is_none()
                        })
                })
            })
            .collect();
        if ids.is_empty() {
            return;
        }
        self.checkpoint();
        for &(layer, mark) in &ids {
            let mark = &mut self.doc.layers[layer].frames[self.frame][mark];
            mark.style.palette_fill = swatch.as_ref().map(|swatch| swatch.id);
            if let Some(swatch) = &swatch {
                mark.fill = Some(swatch.color);
            }
        }
        self.canvas.selected = ids;
        self.recover();
    }

    fn change_linked_art(&mut self, id: u64, old: [u8; 4], new: Option<[u8; 4]>) {
        let mut cels = std::collections::HashMap::new();
        recolor(&mut self.doc.layers, id, old, new, &mut cels);
        for scene in &mut self.doc.extras.scenes {
            recolor(&mut scene.layers, id, old, new, &mut cels);
        }
        for symbol in &mut self.doc.extras.symbols {
            recolor(&mut symbol.layers, id, old, new, &mut cels);
        }
    }

    pub(crate) fn track_vector_color(&mut self, color: [u8; 4]) -> Option<u64> {
        if color[3] == 0 {
            return None;
        }
        let palette = &mut self.doc.extras.palette;
        if let Some(entry) = palette.iter().find(|entry| entry.color == color) {
            return Some(entry.id);
        }
        if palette.len() >= 256 {
            return None;
        }
        let id = crate::features::id();
        palette.push(Swatch {
            id,
            name: hex(color),
            color,
        });
        Some(id)
    }

    pub(crate) fn link_drawn_colors(&mut self, mark: &mut Mark) {
        if mark.style.instance.is_some()
            || mark.style.media_image.is_some()
            || mark.style.raster_tile
            || mark.style.guide
        {
            return;
        }
        mark.style.palette_fill = mark
            .fill
            .filter(|_| mark.style.gradient.is_none())
            .and_then(|color| self.track_vector_color(color));
        mark.style.palette_stroke = mark
            .stroke
            .then(|| self.track_vector_color(mark.color))
            .flatten();
    }

    fn palette_checkpoint(&mut self) {
        let selection = self.canvas.selected.clone();
        self.checkpoint();
        self.canvas.selected = selection;
    }

    fn palette_channel_color(&self) -> egui::Color32 {
        if self.active_fill {
            self.fill_color
        } else {
            self.color
        }
    }

    pub(crate) fn palette_ui(&mut self, ui: &mut egui::Ui) {
        let mut panel = std::mem::take(&mut self.studio.palette);
        let mut prefs_changed = false;
        let mut add_linked = false;
        let mut add_main = false;
        let mut remove_main = false;
        let mut preset_index = self.studio.prefs.color_preset.min(PRESETS.len() - 1);
        let mut picked = None;
        let mut remove = None;
        let mut link = None;
        let mut unlink = false;
        let in_scene = self.feature.symbol_stack.is_empty();
        let mut actions = GridActions::default();
        let mut current = self.palette_channel_color().to_array();
        if panel.main.is_some_and(|(preset, index)| {
            preset != preset_index || main_colors(&self.doc, preset).get(index) != Some(&current)
        }) {
            panel.clear_linked_selection();
        }
        if panel.linked.is_some_and(|id| {
            !self
                .doc
                .extras
                .palette
                .iter()
                .any(|swatch| swatch.id == id && swatch.color == current)
        }) {
            panel.clear_linked_selection();
        }
        egui::ScrollArea::vertical()
            .id_salt("color-panel")
            .show(ui, |ui| {
                let selected = panel.linked.and_then(|id| {
                    self.doc
                        .extras
                        .palette
                        .iter()
                        .find(|swatch| swatch.id == id)
                });
                let mut color =
                    selected.map_or_else(|| color32(current), |swatch| color32(swatch.color));
                let can_edit = in_scene || selected.is_none();
                let mut clear_selection = false;
                ui.horizontal_top(|ui| {
                    let size = (ui.available_width() - 106.).clamp(84., 330.);
                    let mut hsva = egui::ecolor::Hsva::from(color);
                    let hue_id = ui.scope_id().with("compact-picker-hue");
                    if hsva.s == 0. || hsva.v == 0. {
                        hsva.h = ui.data(|data| data.get_temp(hue_id)).unwrap_or(hsva.h);
                    }
                    let mut changed = false;
                    ui.add_enabled_ui(can_edit, |ui| {
                        changed = compact_picker(ui, &mut hsva, size);
                    });
                    if changed {
                        ui.data_mut(|data| data.insert_temp(hue_id, hsva.h));
                        color = hsva.into();
                    }
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.;
                        let (preview, _) =
                            ui.allocate_exact_size(egui::vec2(53., 25.), egui::Sense::hover());
                        for y in 0..4 {
                            for x in 0..8 {
                                let cell = egui::Rect::from_min_size(
                                    preview.min + egui::vec2(x as f32 * 7., y as f32 * 7.),
                                    egui::vec2(7., 7.),
                                )
                                .intersect(preview);
                                ui.painter().rect_filled(
                                    cell,
                                    0.,
                                    egui::Color32::from_gray(if (x + y) % 2 == 0 {
                                        205
                                    } else {
                                        145
                                    }),
                                );
                            }
                        }
                        ui.painter().rect_filled(preview, 0., color);
                        let mut rgba = color.to_srgba_unmultiplied();
                        let mut edited = false;
                        ui.add_enabled_ui(can_edit, |ui| {
                            for (value, label) in rgba.iter_mut().zip(["R ", "G ", "B ", "A "]) {
                                edited |= ui
                                    .add_sized(
                                        [53., 19.],
                                        egui::DragValue::new(value).prefix(label),
                                    )
                                    .changed();
                            }
                        });
                        if edited {
                            color = egui::Color32::from_rgba_unmultiplied(
                                rgba[0], rgba[1], rgba[2], rgba[3],
                            );
                            changed = true;
                        }
                    });
                    ui.vertical(|ui| {
                        for (fill, icon, label) in
                            [(true, Icon::Bucket, "Fill"), (false, Icon::Line, "Stroke")]
                        {
                            if button(ui, icon, "", label, self.active_fill == fill, true).clicked()
                                && self.active_fill != fill
                            {
                                self.active_fill = fill;
                                clear_selection = true;
                            }
                        }
                        if button(
                            ui,
                            Icon::Eyedropper,
                            "",
                            "Cuentagotas",
                            self.studio.color_picker.is_some(),
                            true,
                        )
                        .clicked()
                        {
                            clear_selection = true;
                            self.studio.color_picker = Some(self.active_fill);
                        }
                    });
                    if changed {
                        current = color.to_array();
                        picked = Some(color);
                    }
                });
                if clear_selection {
                    panel.clear_linked_selection();
                    current = self.palette_channel_color().to_array();
                    picked = None;
                } else if let Some(swatch) = selected {
                    ui.small(format!("{} · {}", tr("Linked color"), swatch.name));
                }
                if !in_scene && selected.is_some() {
                    ui.small(tr("Return to the scene to edit linked colors."));
                }
                crate::design::category(ui, "MAIN PALETTE");
                let preset = &mut preset_index;
                egui::ComboBox::from_id_salt("color-preset")
                    .selected_text(tr(PRESETS[*preset].0))
                    .show_ui(ui, |ui| {
                        for (index, (name, _)) in PRESETS.iter().enumerate() {
                            prefs_changed |= ui.selectable_value(preset, index, tr(name)).changed();
                        }
                    });
                let colors = main_colors(&self.doc, *preset);
                ui.horizontal(|ui| {
                    add_main = button(
                        ui,
                        Icon::Plus,
                        "",
                        "Add main palette color",
                        false,
                        colors.len() < 256,
                    )
                    .clicked();
                    remove_main = button(
                        ui,
                        Icon::Trash,
                        "",
                        "Remove main palette color",
                        false,
                        panel.main.is_some_and(|(p, _)| p == *preset),
                    )
                    .clicked();
                });
                let main: Vec<_> = colors
                    .iter()
                    .copied()
                    .enumerate()
                    .map(|(index, color)| (Some(index as u64), hex(color), color))
                    .collect();
                grid(ui, "main", &main, current, &mut actions, false);

                crate::design::category(ui, "LINKED COLORS");
                if button(
                    ui,
                    Icon::Plus,
                    "",
                    "Add linked color",
                    false,
                    self.doc.extras.palette.len() < 256 && current[3] != 0,
                )
                .clicked()
                {
                    add_linked = true;
                }
                let linked: Vec<_> = self
                    .doc
                    .extras
                    .palette
                    .iter()
                    .map(|entry| (Some(entry.id), entry.name.clone(), entry.color))
                    .collect();
                if linked.is_empty() {
                    ui.small(tr("Solid vector colors appear here as you draw."));
                }
                grid(ui, "linked", &linked, current, &mut actions, false);
                if let Some(entry) = panel
                    .linked
                    .and_then(|id| self.doc.extras.palette.iter().find(|entry| entry.id == id))
                {
                    ui.horizontal_wrapped(|ui| {
                        if button(
                            ui,
                            Icon::Eyedropper,
                            "",
                            "Link selected fills",
                            false,
                            !self.canvas.selected.is_empty(),
                        )
                        .clicked()
                        {
                            link = Some(entry.clone());
                        }
                        if button(
                            ui,
                            Icon::Minus,
                            "",
                            "Unlink selected fills",
                            false,
                            !self.canvas.selected.is_empty(),
                        )
                        .clicked()
                        {
                            unlink = true;
                        }
                        if button(ui, Icon::Trash, "", "Remove linked color", false, in_scene)
                            .clicked()
                        {
                            remove = Some(entry.clone());
                        }
                    });
                }

                crate::design::category(ui, "FAVOURITE COLORS");
                if button(
                    ui,
                    Icon::Plus,
                    "",
                    "Add current color to favourites",
                    false,
                    !self.studio.prefs.favorite_colors.contains(&current)
                        && self.studio.prefs.favorite_colors.len() < 256,
                )
                .clicked()
                {
                    actions.favourite = Some(current);
                }
                let favourites: Vec<_> = self
                    .studio
                    .prefs
                    .favorite_colors
                    .iter()
                    .map(|&color| (None, hex(color), color))
                    .collect();
                if favourites.is_empty() {
                    ui.small(tr("Right-click a swatch to add it to favourites."));
                }
                grid(ui, "favourites", &favourites, current, &mut actions, true);
            });
        if preset_index != self.studio.prefs.color_preset {
            self.studio.prefs.color_preset = preset_index;
            panel.clear_linked_selection();
            prefs_changed = true;
        }
        if let Some(color) = picked {
            if let Some((preset, index)) = panel.main {
                let mut colors = main_colors(&self.doc, preset);
                if colors[index] != color.to_array() {
                    if !panel.editing_color {
                        self.palette_checkpoint();
                    }
                    panel.editing_color = true;
                    colors[index] = color.to_array();
                    self.doc.extras.main_palettes.insert(preset, colors);
                    self.recover();
                }
            }
            if in_scene
                && let Some(id) = panel.linked
                && let Some(index) = self
                    .doc
                    .extras
                    .palette
                    .iter()
                    .position(|entry| entry.id == id)
            {
                let old = self.doc.extras.palette[index].color;
                if old != color.to_array() {
                    if !panel.editing_color {
                        let selection = self.canvas.selected.clone();
                        self.checkpoint();
                        self.canvas.selected = selection;
                    }
                    panel.editing_color = true;
                    self.change_linked_art(id, old, Some(color.to_array()));
                    let entry = &mut self.doc.extras.palette[index];
                    if entry.name == hex(old) {
                        entry.name = hex(color.to_array());
                    }
                    entry.color = color.to_array();
                    self.recover();
                }
            }
            self.apply_picker_color(color, self.active_fill);
        }
        if !ui.input(|input| input.pointer.primary_down()) {
            panel.editing_color = false;
        }
        if let Some((color, stroke)) = actions.color {
            panel.clear_linked_selection();
            if actions.from_main {
                panel.main = actions.select.map(|index| (preset_index, index as usize));
            } else {
                panel.linked = actions.select;
            }
            self.apply_picker_color(
                color32(color),
                stroke.map_or(self.active_fill, |stroke| !stroke),
            );
        }
        if add_main || remove_main {
            let mut colors = main_colors(&self.doc, preset_index);
            self.palette_checkpoint();
            if add_main {
                panel.clear_linked_selection();
                panel.main = Some((preset_index, colors.len()));
                colors.push(current);
            } else if let Some((_, index)) = panel.main {
                colors.remove(index);
                panel.clear_linked_selection();
            }
            self.doc.extras.main_palettes.insert(preset_index, colors);
            self.recover();
        }
        if add_linked {
            panel.clear_linked_selection();
            let existing = self
                .doc
                .extras
                .palette
                .iter()
                .find(|entry| entry.color == current)
                .map(|entry| entry.id);
            panel.linked = existing.or_else(|| {
                if current[3] == 0 {
                    return None;
                }
                let selection = self.canvas.selected.clone();
                self.checkpoint();
                self.canvas.selected = selection;
                let id = self.track_vector_color(current);
                self.recover();
                id
            });
        }
        if let Some(color) = actions.favourite
            && !self.studio.prefs.favorite_colors.contains(&color)
            && self.studio.prefs.favorite_colors.len() < 256
        {
            self.studio.prefs.favorite_colors.push(color);
            prefs_changed = true;
        }
        if let Some(color) = actions.remove_favourite {
            self.studio
                .prefs
                .favorite_colors
                .retain(|entry| *entry != color);
            prefs_changed = true;
        }
        if prefs_changed && let Err(error) = self.studio.save() {
            self.status = error;
        }
        if let Some(swatch) = remove {
            self.checkpoint();
            self.change_linked_art(swatch.id, swatch.color, None);
            self.doc
                .extras
                .palette
                .retain(|entry| entry.id != swatch.id);
            self.recover();
            panel.clear_linked_selection();
        }
        if let Some(swatch) = link {
            self.link_palette_selection(Some(swatch));
        } else if unlink {
            self.link_palette_selection(None);
        }
        self.studio.palette = panel;
    }
}

fn compact_picker(ui: &mut egui::Ui, hsva: &mut egui::ecolor::Hsva, width: f32) -> bool {
    let mut changed = false;
    ui.vertical(|ui| {
        let fill = |h| egui::Color32::from(egui::ecolor::Hsva::new(h, 1., 1., 1.));
        let (square, response) =
            ui.allocate_exact_size(egui::vec2(width, 105.), egui::Sense::click_and_drag());
        if (response.clicked() || response.dragged())
            && let Some(pos) = response.interact_pointer_pos()
        {
            hsva.s = ((pos.x - square.left()) / square.width()).clamp(0., 1.);
            hsva.v = ((square.bottom() - pos.y) / square.height()).clamp(0., 1.);
            changed = true;
        }
        let white = egui::Color32::WHITE;
        let black = egui::Color32::BLACK;
        let clear = egui::Color32::TRANSPARENT;
        let hue = fill(hsva.h);
        paint_gradient(ui, square, [white, hue, hue, white]);
        paint_gradient(ui, square, [clear, clear, black, black]);
        let dot = egui::pos2(
            square.left() + hsva.s * square.width(),
            square.bottom() - hsva.v * square.height(),
        );
        ui.painter()
            .circle_stroke(dot, 4., egui::Stroke::new(3., black));
        ui.painter()
            .circle_stroke(dot, 4., egui::Stroke::new(1.5, white));

        let (bar, response) =
            ui.allocate_exact_size(egui::vec2(width, 16.), egui::Sense::click_and_drag());
        if (response.clicked() || response.dragged())
            && let Some(pos) = response.interact_pointer_pos()
        {
            hsva.h = ((pos.x - bar.left()) / bar.width()).clamp(0., 1.);
            changed = true;
        }
        for i in 0..6 {
            let a = i as f32 / 6.;
            let b = (i + 1) as f32 / 6.;
            let segment = egui::Rect::from_min_max(
                egui::pos2(egui::lerp(bar.x_range(), a), bar.top()),
                egui::pos2(egui::lerp(bar.x_range(), b), bar.bottom()),
            );
            paint_gradient(ui, segment, [fill(a), fill(b), fill(b), fill(a)]);
        }
        let x = egui::lerp(bar.x_range(), hsva.h);
        ui.painter()
            .vline(x, bar.y_range(), egui::Stroke::new(3., black));
        ui.painter()
            .vline(x, bar.y_range(), egui::Stroke::new(1., white));
    });
    changed
}

fn paint_gradient(ui: &egui::Ui, rect: egui::Rect, colors: [egui::Color32; 4]) {
    let mut mesh = egui::Mesh::default();
    for (pos, color) in [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
    ]
    .into_iter()
    .zip(colors)
    {
        mesh.colored_vertex(pos, color);
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    ui.painter().add(egui::Shape::mesh(mesh));
}

fn color32(color: [u8; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_premultiplied(color[0], color[1], color[2], color[3])
}

fn hex(color: [u8; 4]) -> String {
    let [r, g, b, a] = color32(color).to_srgba_unmultiplied();
    if a == 255 {
        format!("#{r:02X}{g:02X}{b:02X}")
    } else {
        format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
    }
}

#[derive(Default)]
struct GridActions {
    color: Option<([u8; 4], Option<bool>)>,
    select: Option<u64>,
    from_main: bool,
    favourite: Option<[u8; 4]>,
    remove_favourite: Option<[u8; 4]>,
}

fn grid(
    ui: &mut egui::Ui,
    id: &str,
    entries: &[(Option<u64>, String, [u8; 4])],
    active: [u8; 4],
    actions: &mut GridActions,
    favourites: bool,
) {
    let grid_id = id;
    if entries.is_empty() {
        return;
    }
    egui::ScrollArea::vertical()
        .id_salt(id)
        .max_height(112.)
        .show(ui, |ui| {
            let columns = (ui.available_width() / 19.).floor().max(1.) as usize;
            let tile = (ui.available_width() - 3. * (columns - 1) as f32) / columns as f32;
            ui.spacing_mut().item_spacing.y = 3.;
            for row in entries.chunks(columns) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.;
                    for (id, label, color) in row {
                        let (rect, response) =
                            ui.allocate_exact_size(egui::vec2(tile, 16.), egui::Sense::click());
                        ui.painter().rect_filled(rect, 4., crate::themes::panel());
                        ui.painter()
                            .rect_filled(rect.shrink(1.), 3., color32(*color));
                        if response.hovered() || *color == active {
                            ui.painter().rect_stroke(
                                rect,
                                4.,
                                egui::Stroke::new(2., crate::themes::accent()),
                                egui::StrokeKind::Inside,
                            );
                        }
                        let response = response.on_hover_text(format!("{label}  {}", hex(*color)));
                        if response.clicked() {
                            actions.color = Some((*color, None));
                            actions.select = *id;
                            actions.from_main = grid_id == "main";
                        }
                        response.context_menu(|ui| {
                            if ui.button(tr("Use as fill")).clicked() {
                                actions.color = Some((*color, Some(false)));
                                actions.select = *id;
                                actions.from_main = grid_id == "main";
                                ui.close();
                            }
                            if ui.button(tr("Use as stroke")).clicked() {
                                actions.color = Some((*color, Some(true)));
                                actions.select = *id;
                                actions.from_main = grid_id == "main";
                                ui.close();
                            }
                            let label = if favourites {
                                "Remove from favourites"
                            } else {
                                "Add to favourites"
                            };
                            if ui.button(tr(label)).clicked() {
                                if favourites {
                                    actions.remove_favourite = Some(*color);
                                } else {
                                    actions.favourite = Some(*color);
                                }
                                ui.close();
                            }
                        });
                    }
                });
            }
        });
}

const PRESETS: &[(&str, [u32; 8])] = &[
    (
        "Default",
        [
            0x656D79, 0xEB464D, 0xF98A34, 0xEDC43E, 0x5CA84A, 0x38B5AD, 0x4779E5, 0xA55DD8,
        ],
    ),
    (
        "90s Anime",
        [
            0x58466A, 0xAD5264, 0xCF7B66, 0xE8B86E, 0x788C62, 0x4F9A96, 0x647FAF, 0xC895B2,
        ],
    ),
    (
        "Modern Anime",
        [
            0x242C4A, 0xDD536F, 0xEB9A71, 0xF8D584, 0x7ACB9A, 0x60C5DA, 0x6E80DC, 0xB57CD5,
        ],
    ),
    (
        "Retro",
        [
            0x3B403B, 0xAE4937, 0xD57641, 0xD7AB53, 0x84955F, 0x558E83, 0x41627A, 0x966579,
        ],
    ),
    (
        "Comic",
        [
            0x263341, 0xE73743, 0xFB812D, 0xFFCC2F, 0x6CBD4E, 0x29B9C2, 0x3373CF, 0xBA5196,
        ],
    ),
    (
        "Superheroes",
        [
            0x282C45, 0xB92245, 0xE76738, 0xF5BC2F, 0x458455, 0x2F949F, 0x2D51AD, 0x793BA3,
        ],
    ),
    (
        "Noir",
        [
            0x3B3C43, 0x69464A, 0x85695B, 0xA19576, 0x596154, 0x48615F, 0x4B576C, 0x66566E,
        ],
    ),
    (
        "Pastel",
        [
            0x9190A8, 0xE592A5, 0xF5AC91, 0xE7CB7C, 0xA5C98A, 0x91CCBA, 0x91A9E0, 0xC7A2E1,
        ],
    ),
    (
        "Cyberpunk",
        [
            0x282342, 0xED397A, 0xFF7344, 0xF0D64E, 0x87E44E, 0x39E7D2, 0x536AFF, 0xB842F0,
        ],
    ),
    (
        "Watercolour",
        [
            0x7E7886, 0xBF7682, 0xCD9477, 0xC7AA68, 0x93A47B, 0x75A8A1, 0x7293BA, 0xA48DBA,
        ],
    ),
    (
        "Forest",
        [
            0x343D32, 0xA85248, 0xB68155, 0xCAB26F, 0x849E59, 0x5C906E, 0x53787D, 0x746584,
        ],
    ),
    (
        "Ocean",
        [
            0x29354C, 0xCA697E, 0xD89174, 0xE1C487, 0x7BB695, 0x4EADB4, 0x416BB1, 0x776BB1,
        ],
    ),
    (
        "Sunset",
        [
            0x48344C, 0xC95366, 0xEF8059, 0xF8BE6A, 0x9B9A65, 0x558D91, 0x666EA8, 0xA761A0,
        ],
    ),
];

pub(crate) fn valid_main_palettes(
    palettes: &std::collections::BTreeMap<usize, SharedVec<[u8; 4]>>,
) -> bool {
    palettes
        .iter()
        .all(|(preset, colors)| *preset < PRESETS.len() && colors.len() <= 256)
}

fn main_colors(doc: &crate::Document, preset: usize) -> SharedVec<[u8; 4]> {
    doc.extras
        .main_palettes
        .get(&preset)
        .cloned()
        .unwrap_or_else(|| preset_colors(preset).to_vec().into())
}

fn preset_colors(preset: usize) -> [[u8; 4]; 64] {
    let anchors = &PRESETS[preset].1;
    std::array::from_fn(|index| {
        let row = index % 8;
        let column = index / 8;
        let anchor = anchors[column];
        if column == 0 && preset == 0 {
            let gray = (row * 255 / 7) as u8;
            return [gray, gray, gray, 255];
        }
        let mix = [-0.78, -0.56, -0.34, -0.16, 0., 0.24, 0.47, 0.72][row];
        let channel = |shift: u32| {
            let value = ((anchor >> shift) & 255) as f32;
            let target = if mix < 0. { 0. } else { 255. };
            (value + (target - value) * f32::abs(mix)).round() as u8
        };
        [channel(16), channel(8), channel(0), 255]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_palette_changes_roundtrip_per_preset_and_document() {
        let mut doc = crate::Document::default();
        let mut colors = main_colors(&doc, 1);
        colors[0] = [10, 20, 30, 255];
        colors.remove(1);
        colors.push([90, 100, 110, 255]);
        doc.extras.main_palettes.insert(1, colors.clone());
        let encoded = crate::native_project::encode(&doc).unwrap();
        let reopened = crate::native_project::decode_if_native(&encoded)
            .unwrap()
            .unwrap();
        assert_eq!(&*main_colors(&reopened, 1), &*colors);
        assert_eq!(&*main_colors(&reopened, 2), &preset_colors(2));
        assert_eq!(
            &*main_colors(&crate::Document::default(), 1),
            &preset_colors(1)
        );
    }

    #[test]
    fn drawing_reuses_swatches_and_edits_both_channels() {
        let ctx = egui::Context::default();
        let mut editor = Editor::new(&eframe::CreationContext::_new_kittest(ctx));
        let color = [70, 120, 180, 255];
        let mut mark = Mark {
            color,
            fill: Some(color),
            stroke: true,
            ..Default::default()
        };
        editor.link_drawn_colors(&mut mark);
        let swatch = editor.doc.extras.palette.last().unwrap().clone();
        assert_eq!(mark.style.palette_fill, Some(swatch.id));
        assert_eq!(mark.style.palette_stroke, Some(swatch.id));
        assert_eq!(editor.track_vector_color(color), Some(swatch.id));
        editor.doc.layers[0].frames[0].push(mark);
        let new = [120, 80, 190, 255];
        editor.change_linked_art(swatch.id, color, Some(new));
        let mark = &editor.doc.layers[0].frames[0][0];
        assert_eq!((mark.fill, mark.color), (Some(new), new));
        editor.change_linked_art(swatch.id, new, None);
        let mark = &editor.doc.layers[0].frames[0][0];
        assert_eq!((mark.fill, mark.color), (Some(new), new));
        assert_eq!(
            (mark.style.palette_fill, mark.style.palette_stroke),
            (None, None)
        );
    }
}
