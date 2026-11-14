//! Ordered library resources and colored folders, shared by all scene/symbol
//! editing contexts. Dragging changes references and ordering, never file paths.
use crate::{
    Editor,
    design::{self, Icon},
    features::id,
    i18n::tr,
    shared::SharedVec,
};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Vec2};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Library {
    pub folders: SharedVec<Folder>,
    pub order: SharedVec<Asset>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Folder {
    pub id: u64,
    pub name: String,
    pub parent: Option<u64>,
    pub color: [u8; 4],
    pub collapsed: bool,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Asset {
    Folder(u64),
    Symbol(u64),
    Media(u64),
    Root,
}
#[derive(Default)]
pub struct LibraryState {
    pub selected: Option<Asset>,
    pub(crate) drag: Option<Asset>,
    rename: Option<(Asset, String)>,
    focus: bool,
    usage: Option<(Asset, Vec<(String, usize)>)>,
}
impl Library {
    pub fn is_empty(&self) -> bool {
        self.folders.is_empty()
    }
    pub fn can_parent(&self, id: u64, mut parent: Option<u64>) -> bool {
        let mut seen = HashSet::new();
        while let Some(p) = parent {
            if p == id || !seen.insert(p) || seen.len() > 32 {
                return false;
            }
            let Some(f) = self.folders.iter().find(|f| f.id == p) else {
                return false;
            };
            parent = f.parent;
        }
        true
    }
    pub fn validate(&self, symbols: &[crate::features::Symbol]) -> Result<(), String> {
        let mut ids = HashSet::new();
        if self.folders.len() > 200 {
            return Err("Too many library folders".into());
        }
        for f in &self.folders {
            if f.id == 0
                || !ids.insert(f.id)
                || f.name.len() > 1024
                || !self.can_parent(f.id, f.parent)
            {
                return Err("Invalid library folder hierarchy".into());
            }
        }
        if symbols
            .iter()
            .any(|s| s.library_folder.is_some_and(|f| !ids.contains(&f)))
        {
            return Err("Missing symbol library folder".into());
        }
        Ok(())
    }
}
fn rows(e: &Editor, parent: Option<u64>, depth: usize, out: &mut Vec<(Asset, usize)>) {
    if depth > 32 {
        return;
    }
    for f in e
        .doc
        .extras
        .library
        .folders
        .iter()
        .filter(|f| f.parent == parent)
    {
        out.push((Asset::Folder(f.id), depth));
        if !f.collapsed {
            rows(e, Some(f.id), depth + 1, out);
        }
    }
    let first = out.len();
    for s in e.doc.extras.symbols.iter().filter(|s| {
        s.library_folder == parent
            && !e
                .doc
                .extras
                .animation_clips
                .iter()
                .any(|clip| clip.symbol == s.id)
    }) {
        out.push((Asset::Symbol(s.id), depth));
    }
    out.extend(
        e.doc
            .extras
            .media
            .assets
            .iter()
            .filter(|asset| asset.folder == parent && !asset.raster_tile)
            .map(|asset| (Asset::Media(asset.id), depth)),
    );
    out[first..].sort_by_key(|(asset, _)| {
        e.doc
            .extras
            .library
            .order
            .iter()
            .position(|item| item == asset)
            .unwrap_or(usize::MAX)
    });
}
impl Editor {
    pub fn reorder_library_folder(&mut self, id: u64, target: u64, before: bool) {
        if id == target {
            return;
        }
        let Some(parent) = self
            .doc
            .extras
            .library
            .folders
            .iter()
            .find(|f| f.id == target)
            .map(|f| f.parent)
        else {
            return;
        };
        if !self.doc.extras.library.can_parent(id, parent) {
            return;
        }
        let Some(index) = self
            .doc
            .extras
            .library
            .folders
            .iter()
            .position(|f| f.id == id)
        else {
            return;
        };
        self.checkpoint();
        let mut f = self.doc.extras.library.folders.remove(index);
        f.parent = parent;
        let at = self
            .doc
            .extras
            .library
            .folders
            .iter()
            .position(|f| f.id == target)
            .unwrap()
            + usize::from(!before);
        self.doc.extras.library.folders.insert(at, f);
        self.recover();
    }
    pub fn move_library_asset(&mut self, source: Asset, target: Asset) {
        if source == target || source == Asset::Root {
            return;
        }
        let parent = match target {
            Asset::Root => None,
            Asset::Folder(id) => Some(id),
            Asset::Media(id) => self
                .doc
                .extras
                .media
                .assets
                .iter()
                .find(|asset| asset.id == id)
                .and_then(|asset| asset.folder),
            Asset::Symbol(id) => self
                .doc
                .extras
                .symbols
                .iter()
                .find(|s| s.id == id)
                .and_then(|s| s.library_folder),
        };
        if let Asset::Folder(id) = source {
            if !self.doc.extras.library.can_parent(id, parent) {
                self.status = tr("No se pueden crear ciclos de carpetas").into();
                return;
            }
        }
        self.checkpoint();
        if matches!(source, Asset::Media(_) | Asset::Symbol(_)) {
            let order = &mut self.doc.extras.library.order;
            for item in self
                .doc
                .extras
                .symbols
                .iter()
                .map(|symbol| Asset::Symbol(symbol.id))
                .chain(
                    self.doc
                        .extras
                        .media
                        .assets
                        .iter()
                        .map(|asset| Asset::Media(asset.id)),
                )
            {
                if !order.contains(&item) {
                    order.push(item);
                }
            }
            order.retain(|item| *item != source);
            if matches!(target, Asset::Media(_) | Asset::Symbol(_)) && !order.contains(&target) {
                order.push(target);
            }
            let index = order
                .iter()
                .position(|item| *item == target)
                .unwrap_or(order.len());
            order.insert(index, source);
        }
        match source {
            Asset::Folder(id) => {
                if let Some(i) = self
                    .doc
                    .extras
                    .library
                    .folders
                    .iter()
                    .position(|f| f.id == id)
                {
                    self.doc.extras.library.folders[i].parent = parent;
                    if let Asset::Symbol(_) = target {
                    } else if target == Asset::Root {
                        let f = self.doc.extras.library.folders.remove(i);
                        self.doc.extras.library.folders.push(f);
                    }
                }
            }
            Asset::Symbol(id) => {
                if let Some(i) = self.doc.extras.symbols.iter().position(|s| s.id == id) {
                    let mut s = self.doc.extras.symbols.remove(i);
                    s.library_folder = parent;
                    let at = match target {
                        Asset::Symbol(other) => self
                            .doc
                            .extras
                            .symbols
                            .iter()
                            .position(|s| s.id == other)
                            .unwrap_or(self.doc.extras.symbols.len()),
                        _ => self.doc.extras.symbols.len(),
                    };
                    self.doc.extras.symbols.insert(at, s);
                }
            }
            Asset::Media(id) => {
                if let Some(asset) = self
                    .doc
                    .extras
                    .media
                    .assets
                    .iter_mut()
                    .find(|asset| asset.id == id)
                {
                    asset.folder = parent;
                }
            }
            Asset::Root => {}
        }
        self.recover();
    }
    fn rename_asset(&mut self, asset: Asset, name: String) {
        let name = name.trim();
        if name.is_empty() || name.len() > 1024 {
            return;
        }
        self.checkpoint();
        match asset {
            Asset::Folder(id) => {
                if let Some(f) = self
                    .doc
                    .extras
                    .library
                    .folders
                    .iter_mut()
                    .find(|f| f.id == id)
                {
                    f.name = name.into();
                }
            }
            Asset::Symbol(id) => {
                if let Some(s) = self.doc.extras.symbols.iter_mut().find(|s| s.id == id) {
                    s.name = name.into();
                }
            }
            Asset::Media(id) => {
                if let Some(asset) = self
                    .doc
                    .extras
                    .media
                    .assets
                    .iter_mut()
                    .find(|asset| asset.id == id)
                {
                    asset.name = name.into();
                }
            }
            Asset::Root => {}
        }
        self.recover();
    }
    fn library_usage(&self, asset: Asset) -> Vec<(String, usize)> {
        let mut usage = Vec::new();
        let mut scan = |name: &str, layers: &[crate::document::Layer], media: usize| {
            let mut cels = HashSet::new();
            let marks = layers
                .iter()
                .flat_map(|layer| layer.frames.iter())
                .filter(|cel| cels.insert(cel.identity()))
                .flat_map(|cel| cel.iter())
                .filter(|mark| match asset {
                    Asset::Symbol(id) => {
                        mark.style.instance.as_ref().is_some_and(|i| i.symbol == id)
                    }
                    Asset::Media(id) => mark.style.media_image == Some(id),
                    _ => false,
                })
                .count();
            if marks + media > 0 {
                usage.push((name.to_owned(), marks + media));
            }
        };
        let clip_count = |tracks: &[crate::media::Track]| match asset {
            Asset::Media(id) => tracks.iter().map(|track| track.asset_use_count(id)).sum(),
            _ => 0,
        };
        scan(
            tr("Current timeline"),
            &self.doc.layers,
            clip_count(&self.doc.extras.media.tracks),
        );
        for scene in self
            .doc
            .extras
            .scenes
            .iter()
            .enumerate()
            .filter(|(index, _)| {
                *index != self.doc.extras.active_scene || !self.feature.symbol_stack.is_empty()
            })
            .map(|(_, scene)| scene)
        {
            scan(&scene.name, &scene.layers, clip_count(&scene.media_tracks));
        }
        for symbol in self.doc.extras.symbols.iter() {
            if symbol.layers.identity() != self.doc.layers.identity() {
                scan(&symbol.name, &symbol.layers, 0);
            }
        }
        usage
    }

    pub fn asset_library_ui(&mut self, ui: &mut egui::Ui) {
        let library_rect = ui.available_rect_before_wrap();
        ui.heading(tr("Biblioteca"));
        self.media_library_ui(ui);
        ui.add_space(6.);
        let mut new_folder = false;
        let mut inspect_usage = None;
        let mut replace_media = None;
        let mut edit = None;
        let mut insert = None;
        let mut remove = None;
        let mut rename = None;
        let mut color = None;
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let mut drop_target = pointer
            .filter(|p| library_rect.contains(*p))
            .map(|_| Asset::Root);
        let mut reorder = None;
        ui.horizontal(|ui| {
            if design::button(
                ui,
                Icon::FolderPlus,
                "",
                "Nueva carpeta",
                false,
                self.doc.extras.library.folders.len() < 200,
            )
            .clicked()
            {
                new_folder = true;
            }
            if design::button(
                ui,
                Icon::Rename,
                "",
                "Renombrar",
                false,
                self.assets.selected.is_some(),
            )
            .clicked()
            {
                rename = self.assets.selected;
            }
            if design::button(
                ui,
                Icon::Trash,
                "",
                "Eliminar recurso o carpeta",
                false,
                self.assets.selected.is_some(),
            )
            .clicked()
            {
                remove = self.assets.selected;
            }
        });
        let mut list = vec![];
        rows(self, None, 0, &mut list);
        let mut commit_rename = None;
        egui::ScrollArea::vertical()
            .id_salt("library-resources")
            .max_height((ui.available_height() - 115.).max(120.))
            .show_rows(ui, 44., list.len(), |ui, visible| {
                for row in visible {
                    let (asset, depth) = list[row];
                    let (name, tag, collapsed) = match asset {
                        Asset::Folder(id) => {
                            let f = self
                                .doc
                                .extras
                                .library
                                .folders
                                .iter()
                                .find(|f| f.id == id)
                                .unwrap();
                            (
                                f.name.clone(),
                                Color32::from_rgba_unmultiplied(
                                    f.color[0], f.color[1], f.color[2], f.color[3],
                                ),
                                f.collapsed,
                            )
                        }
                        Asset::Symbol(id) => {
                            let s = self.doc.extras.symbols.iter().find(|s| s.id == id).unwrap();
                            let tag = s
                                .library_folder
                                .and_then(|id| {
                                    self.doc.extras.library.folders.iter().find(|f| f.id == id)
                                })
                                .map_or(crate::themes::accent(), |f| {
                                    Color32::from_rgb(f.color[0], f.color[1], f.color[2])
                                });
                            (s.name.clone(), tag, false)
                        }
                        Asset::Media(id) => {
                            let asset = self
                                .doc
                                .extras
                                .media
                                .assets
                                .iter()
                                .find(|asset| asset.id == id)
                                .unwrap();
                            (asset.name.clone(), crate::themes::accent(), false)
                        }
                        Asset::Root => continue,
                    };
                    let (r, response) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), 42.),
                        Sense::click_and_drag(),
                    );
                    if self.assets.selected == Some(asset) || response.hovered() {
                        ui.painter().rect_filled(
                            r,
                            4.,
                            if self.assets.selected == Some(asset) {
                                crate::themes::color(Color32::from_rgb(45, 64, 87))
                            } else {
                                crate::themes::color(Color32::from_gray(51))
                            },
                        );
                    }
                    ui.painter().rect_filled(
                        Rect::from_min_size(r.min + Vec2::new(2., 5.), Vec2::new(3., 32.)),
                        1.,
                        tag,
                    );
                    let icon = Rect::from_min_size(
                        r.min + Vec2::new(10. + depth as f32 * 12., 9.),
                        Vec2::splat(24.),
                    );
                    match asset {
                        Asset::Folder(_) => {
                            design::draw(ui.painter(), icon, Icon::Folder, tag);
                            if collapsed {
                                ui.painter().text(
                                    icon.center() + Vec2::new(12., 0.),
                                    egui::Align2::CENTER_CENTER,
                                    "+",
                                    egui::FontId::proportional(10.),
                                    tag,
                                );
                            }
                        }
                        Asset::Symbol(id) => {
                            ui.scope_builder(
                                egui::UiBuilder::new().max_rect(Rect::from_min_size(
                                    icon.min - Vec2::new(0., 3.),
                                    Vec2::new(32., 30.),
                                )),
                                |ui| {
                                    self.symbol_thumbnail(ui, id, 0, Vec2::new(32., 30.));
                                },
                            );
                        }
                        Asset::Media(id) => {
                            if let Some(asset) = self
                                .doc
                                .extras
                                .media
                                .assets
                                .iter()
                                .find(|asset| asset.id == id)
                            {
                                ui.painter().text(
                                    icon.center(),
                                    egui::Align2::CENTER_CENTER,
                                    asset.icon(),
                                    egui::FontId::proportional(20.),
                                    tag,
                                );
                            }
                        }
                        _ => {}
                    }
                    let text_rect = Rect::from_min_max(
                        Pos2::new(icon.right() + 14., r.top() + 7.),
                        r.max - Vec2::new(6., 7.),
                    );
                    if self
                        .assets
                        .rename
                        .as_ref()
                        .is_some_and(|(id, _)| *id == asset)
                    {
                        let (_, buffer) = self.assets.rename.as_mut().unwrap();
                        let edit = ui.put(text_rect, egui::TextEdit::singleline(buffer));
                        if self.assets.focus {
                            edit.request_focus();
                            self.assets.focus = false;
                        } else if edit.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter))
                        {
                            commit_rename = Some((asset, buffer.clone()));
                        }
                    } else {
                        let mut job = egui::text::LayoutJob::simple(
                            name.clone(),
                            egui::FontId::proportional(12.),
                            crate::themes::color(Color32::from_gray(225)),
                            text_rect.width(),
                        );
                        job.wrap.max_rows = 1;
                        let galley = ui.painter().layout_job(job);
                        ui.painter().galley(
                            text_rect.min + Vec2::new(0., 4.),
                            galley,
                            crate::themes::color(Color32::WHITE),
                        );
                    }
                    // Register the row above the thumbnail's click widget, so
                    // dragging its preview behaves like dragging its name.
                    // Keep the inline rename field interactive while editing.
                    let response = if self
                        .assets
                        .rename
                        .as_ref()
                        .is_some_and(|(id, _)| *id == asset)
                    {
                        response
                    } else {
                        ui.interact(
                            r,
                            ui.scope_id().with(("library-row", row)),
                            Sense::click_and_drag(),
                        )
                    };
                    if response.clicked() {
                        self.assets.selected = Some(asset);
                        if let Asset::Symbol(id) = asset {
                            self.feature.selected_symbol = Some(id);
                        }
                    }
                    if response.double_clicked() {
                        match asset {
                            Asset::Folder(id) => {
                                if let Some(f) = self
                                    .doc
                                    .extras
                                    .library
                                    .folders
                                    .iter_mut()
                                    .find(|f| f.id == id)
                                {
                                    f.collapsed = !f.collapsed;
                                }
                            }
                            Asset::Symbol(id) => edit = Some(id),
                            _ => {}
                        }
                    }
                    if response.drag_started() {
                        self.assets.drag = Some(asset);
                        if let Asset::Media(id) = asset {
                            response.dnd_set_drag_payload(crate::media::MediaDrag(id));
                        }
                    }
                    if response
                        .rect
                        .contains(ui.input(|i| i.pointer.interact_pos().unwrap_or(Pos2::ZERO)))
                    {
                        // The left gutter is outside the folder hierarchy.
                        let outdent = depth > 0
                            && pointer.is_some_and(|p| p.x < r.left() + 10. + depth as f32 * 12.);
                        drop_target = Some(if outdent { Asset::Root } else { asset });
                        if self.assets.drag.is_some_and(|source| source != asset) {
                            ui.painter().rect_stroke(
                                r.shrink(1.),
                                4.,
                                egui::Stroke::new(2., crate::themes::accent()),
                                egui::StrokeKind::Inside,
                            );
                        }
                        if let (Some(Asset::Folder(source)), Asset::Folder(target), Some(pos)) = (
                            self.assets.drag,
                            asset,
                            ui.input(|i| i.pointer.interact_pos()),
                        ) {
                            if pos.y < r.top() + 10. || pos.y > r.bottom() - 10. {
                                let before = pos.y < r.center().y;
                                reorder = Some((source, target, before));
                                let y = if before { r.top() } else { r.bottom() };
                                ui.painter().line_segment(
                                    [Pos2::new(r.left(), y), Pos2::new(r.right(), y)],
                                    egui::Stroke::new(2., crate::themes::accent()),
                                );
                            }
                        }
                    }
                    egui::Popup::context_menu(&response)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .show(|ui| {
                            if ui.button(tr("Renombrar")).clicked() {
                                rename = Some(asset);
                                ui.close();
                            }
                            if matches!(asset, Asset::Symbol(_) | Asset::Media(_))
                                && ui.button(tr("Where used")).clicked()
                            {
                                inspect_usage = Some(asset);
                                ui.close();
                            }
                            if let Asset::Media(id) = asset {
                                let choices = self.media_replacement_choices(id);
                                if !choices.is_empty() {
                                    ui.menu_button(tr("Replace media placements"), |ui| {
                                        for (target, name) in choices {
                                            if ui.button(name).clicked() {
                                                replace_media = Some((id, target));
                                                ui.close();
                                            }
                                        }
                                    });
                                }
                            }
                            if let Asset::Folder(id) = asset {
                                design::category(ui, "COLOR DE CARPETA");
                                let mut c = tag;
                                ui.spacing_mut().slider_width = 240.;
                                // Keep the picker in the same popup: opening a second
                                // popup replaces the context menu that owns it.
                                if egui::color_picker::color_picker_color32(
                                    ui,
                                    &mut c,
                                    egui::color_picker::Alpha::OnlyBlend,
                                ) {
                                    color = Some((id, c.to_srgba_unmultiplied()));
                                }
                            }
                        });
                }
            });
        if let Some((old, new)) = replace_media {
            self.replace_media_placements(old, new);
        }
        if let Some(asset) = inspect_usage {
            self.assets.usage = Some((asset, self.library_usage(asset)));
        }
        if let Some((_, usage)) = &self.assets.usage {
            let mut open = true;
            egui::Window::new(tr("Where used"))
                .open(&mut open)
                .show(ui.ctx(), |ui| {
                    if usage.is_empty() {
                        ui.label(tr("No placements found"));
                    }
                    for (name, count) in usage {
                        ui.label(format!("{name} · {count}"));
                    }
                });
            if !open {
                self.assets.usage = None;
            }
        }
        if let Some((asset, name)) = commit_rename {
            self.assets.rename = None;
            self.rename_asset(asset, name);
        }
        if let Some(Asset::Media(id)) = self.assets.selected {
            self.media_asset_preview(ui, id);
        } else {
            self.clear_media_asset_preview();
        }
        if let Some(Asset::Symbol(id)) = self.assets.selected {
            ui.separator();
            ui.horizontal(|ui| {
                if design::button(
                    ui,
                    Icon::Plus,
                    "",
                    "Insertar símbolo en el contexto actual",
                    false,
                    self.symbol_insert_allowed(id),
                )
                .clicked()
                {
                    insert = Some(id);
                }
                if design::button(ui, Icon::Node, "", "Editar símbolo", false, true).clicked() {
                    edit = Some(id);
                }
                if design::button(
                    ui,
                    Icon::Trash,
                    "",
                    "Eliminar símbolo",
                    false,
                    !self.symbol_in_use(id),
                )
                .clicked()
                {
                    remove = Some(Asset::Symbol(id));
                }
            });
        }
        ui.small(tr(
            "Arrastra recursos entre carpetas. Doble clic abre; Renombrar cambia el nombre.",
        ));
        if new_folder {
            let parent = match self.assets.selected {
                Some(Asset::Folder(id)) => Some(id),
                _ => None,
            };
            self.checkpoint();
            let folder = Folder {
                id: id(),
                name: tr("Nueva carpeta").into(),
                parent,
                color: [91, 158, 255, 255],
                collapsed: false,
            };
            let id = folder.id;
            let name = folder.name.clone();
            self.doc.extras.library.folders.push(folder);
            self.assets.selected = Some(Asset::Folder(id));
            self.assets.rename = Some((Asset::Folder(id), name));
            self.assets.focus = true;
            self.recover();
        }
        if let Some(asset) = rename {
            let name = match asset {
                Asset::Folder(id) => self
                    .doc
                    .extras
                    .library
                    .folders
                    .iter()
                    .find(|f| f.id == id)
                    .map(|f| f.name.clone()),
                Asset::Symbol(id) => self
                    .doc
                    .extras
                    .symbols
                    .iter()
                    .find(|s| s.id == id)
                    .map(|s| s.name.clone()),
                Asset::Media(id) => self
                    .doc
                    .extras
                    .media
                    .assets
                    .iter()
                    .find(|asset| asset.id == id)
                    .map(|asset| asset.name.clone()),
                _ => None,
            };
            if let Some(name) = name {
                self.assets.rename = Some((asset, name));
                self.assets.focus = true;
            }
        }
        if let Some((id, c)) = color {
            self.checkpoint();
            if let Some(f) = self
                .doc
                .extras
                .library
                .folders
                .iter_mut()
                .find(|f| f.id == id)
            {
                f.color = c;
            }
            self.recover();
        }
        if let Some(asset) = remove {
            match asset {
                Asset::Symbol(id) => {
                    if self.symbol_in_use(id) {
                        self.status = tr("El símbolo está en uso").into();
                    } else {
                        self.checkpoint();
                        self.doc.extras.symbols.retain(|s| s.id != id);
                        self.recover();
                        self.assets.selected = None;
                    }
                }
                Asset::Media(id) => self.delete_library_media(id),
                Asset::Folder(id) => {
                    let parent = self
                        .doc
                        .extras
                        .library
                        .folders
                        .iter()
                        .find(|f| f.id == id)
                        .and_then(|f| f.parent);
                    self.checkpoint();
                    for f in &mut self.doc.extras.library.folders {
                        if f.parent == Some(id) {
                            f.parent = parent;
                        }
                    }
                    for s in &mut self.doc.extras.symbols {
                        if s.library_folder == Some(id) {
                            s.library_folder = parent;
                        }
                    }
                    for asset in &mut self.doc.extras.media.assets {
                        if asset.folder == Some(id) {
                            asset.folder = parent;
                        }
                    }
                    self.doc.extras.library.folders.retain(|f| f.id != id);
                    self.assets.selected = None;
                    self.recover();
                }
                _ => {}
            }
        }
        if let (Some(source), Some(pos)) = (self.assets.drag, pointer) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            let name = match source {
                Asset::Symbol(id) => self
                    .doc
                    .extras
                    .symbols
                    .iter()
                    .find(|s| s.id == id)
                    .map(|s| s.name.as_str()),
                Asset::Folder(id) => self
                    .doc
                    .extras
                    .library
                    .folders
                    .iter()
                    .find(|f| f.id == id)
                    .map(|f| f.name.as_str()),
                Asset::Media(id) => self
                    .doc
                    .extras
                    .media
                    .assets
                    .iter()
                    .find(|asset| asset.id == id)
                    .map(|asset| asset.name.as_str()),
                Asset::Root => None,
            }
            .unwrap_or("");
            let destination = match drop_target {
                Some(Asset::Root) => tr("Biblioteca"),
                Some(Asset::Folder(id)) => self
                    .doc
                    .extras
                    .library
                    .folders
                    .iter()
                    .find(|f| f.id == id)
                    .map_or("", |f| f.name.as_str()),
                Some(Asset::Symbol(id)) => self
                    .doc
                    .extras
                    .symbols
                    .iter()
                    .find(|s| s.id == id)
                    .and_then(|s| s.library_folder)
                    .and_then(|id| self.doc.extras.library.folders.iter().find(|f| f.id == id))
                    .map_or(tr("Biblioteca"), |f| f.name.as_str()),
                Some(Asset::Media(id)) => self
                    .doc
                    .extras
                    .media
                    .assets
                    .iter()
                    .find(|asset| asset.id == id)
                    .and_then(|asset| asset.folder)
                    .and_then(|id| {
                        self.doc
                            .extras
                            .library
                            .folders
                            .iter()
                            .find(|folder| folder.id == id)
                    })
                    .map_or(tr("Biblioteca"), |folder| folder.name.as_str()),
                None => "",
            };
            let text = if destination.is_empty() {
                name.to_owned()
            } else {
                format!("{name} → {destination}")
            };
            let painter = ui.ctx().layer_painter(egui::LayerId::new(
                egui::Order::Tooltip,
                ui.scope_id().with("asset-drag-preview"),
            ));
            let galley = painter.layout_no_wrap(
                text,
                egui::FontId::proportional(12.),
                crate::themes::palette().text,
            );
            let ghost =
                Rect::from_min_size(pos + Vec2::new(16., 18.), galley.size() + Vec2::splat(14.));
            painter.rect_filled(ghost, 5., crate::themes::panel());
            painter.rect_stroke(
                ghost,
                5.,
                egui::Stroke::new(1., crate::themes::accent()),
                egui::StrokeKind::Inside,
            );
            painter.galley(
                ghost.min + Vec2::splat(7.),
                galley,
                crate::themes::palette().text,
            );
            if drop_target == Some(Asset::Root) {
                painter.line_segment(
                    [library_rect.left_bottom(), library_rect.right_bottom()],
                    egui::Stroke::new(2., crate::themes::accent()),
                );
            }
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.assets.drag = None;
        }
        if ui.input(|i| i.pointer.primary_released()) {
            if let Some((source, target, before)) = reorder {
                self.assets.drag = None;
                self.reorder_library_folder(source, target, before);
            } else if let Some(target) = drop_target {
                if let Some(source) = self.assets.drag.take() {
                    self.move_library_asset(source, target);
                }
            }
        }
        if let Some(id) = insert {
            self.insert_symbol(id);
        }
        if let Some(id) = edit {
            self.enter_symbol(id);
        }
    }
}
