use crate::{Document, Editor, i18n::tr};
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant, SystemTime},
};

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    pub enabled: bool,
    pub incremental: bool,
    pub minutes: u32,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            enabled: true,
            incremental: false,
            minutes: 30,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamp {
    modified: SystemTime,
    length: u64,
}
pub fn stamp(path: &Path) -> Option<Stamp> {
    let meta = fs::metadata(path).ok()?;
    Some(Stamp {
        modified: meta.modified().ok()?,
        length: meta.len(),
    })
}

struct Job {
    target: PathBuf,
    result: mpsc::Receiver<Result<(PathBuf, Stamp), SaveError>>,
    recovery: PathBuf,
    revision: u64,
    close: bool,
    interchange: bool,
}
#[derive(Debug)]
enum SaveError {
    Conflict,
    Failed(String),
}
impl From<io::Error> for SaveError {
    fn from(error: io::Error) -> Self {
        Self::Failed(error.to_string())
    }
}
#[derive(Default)]
pub struct State {
    job: Option<Job>,
    conflict: Option<(PathBuf, bool)>,
    error: Option<String>,
    next: Option<Instant>,
    schedule: Option<(bool, bool, u32)>,
    pending: std::collections::VecDeque<PathBuf>,
    pub(crate) update_save: Option<Option<usize>>,
    pub(crate) quit_after_save: bool,
}
impl State {
    pub(crate) fn conflicted(&self) -> bool {
        self.conflict.is_some()
    }
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
}

pub(crate) fn backup_path(path: &Path, number: usize) -> PathBuf {
    let file = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!(".{file}.backup{number}"))
}

fn increment_base(stem: &str) -> &str {
    stem.rsplit_once("_v")
        .filter(|(_, n)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        .map_or(stem, |(base, _)| base)
}

// A no-replace move publishes a complete snapshot even on Windows filesystems
// where hard links are unsupported. Unix uses an atomic hard link.
#[cfg(windows)]
fn publish_new(source: &Path, target: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(source: *const u16, target: *const u16, flags: u32) -> i32;
    }
    let source: Vec<_> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<_> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    // Both paths are NUL-terminated and remain alive for the duration of the call.
    if unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), 0) } != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(windows))]
fn publish_new(source: &Path, target: &Path) -> io::Result<()> {
    fs::hard_link(source, target)
}

fn write_snapshot(
    doc: &Document,
    target: &Path,
    expected: Option<Stamp>,
    incremental: bool,
) -> Result<(PathBuf, Stamp), SaveError> {
    let exists = target.try_exists()?;
    let unchanged = || -> Result<bool, SaveError> {
        Ok(if exists {
            expected.is_some_and(|expected| stamp(target) == Some(expected))
        } else {
            !target.try_exists()?
        })
    };
    if !incremental && !unchanged()? {
        return Err(SaveError::Conflict);
    }
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = target.with_file_name(format!(
        ".libretoon-save-{}-{nonce}.tmp",
        std::process::id()
    ));
    let result = (|| {
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        let mut writer = io::BufWriter::new(file);
        if crate::native_project::is_native_path(target) {
            crate::native_project::write(doc, &mut writer).map_err(SaveError::Failed)?;
        } else if target
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("fla"))
        {
            let bytes = crate::fla::encode(doc).map_err(SaveError::Failed)?;
            if bytes.len() > 256 * 1024 * 1024 {
                return Err(SaveError::Failed(
                    "Project exceeds the 256 MiB opening limit".into(),
                ));
            }
            writer.write_all(&bytes)?;
        } else {
            serde_json::to_writer(&mut writer, doc)
                .map_err(|error| SaveError::Failed(error.to_string()))?;
        }
        writer.flush()?;
        writer.get_ref().sync_all()?;
        drop(writer);
        let size = fs::metadata(&temporary)?.len();
        let published = |path: PathBuf| {
            let stamp = stamp(&path)
                .filter(|stamp| stamp.length == size)
                .ok_or_else(|| {
                    SaveError::Failed(format!(
                        "Could not verify the saved project at {}",
                        path.display()
                    ))
                })?;
            Ok((path, stamp))
        };
        if incremental {
            let stem = target.file_stem().unwrap_or_default().to_string_lossy();
            let base = increment_base(&stem);
            let ext = target.extension().unwrap_or_default().to_string_lossy();
            let mut latest = 0u64;
            for entry in fs::read_dir(target.parent().unwrap_or(Path::new(".")))? {
                let entry = entry?;
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if let Some(number) = name
                    .strip_prefix(base)
                    .and_then(|n| n.strip_prefix("_v"))
                    .and_then(|n| n.strip_suffix(&format!(".{ext}")))
                    .and_then(|n| n.parse::<u64>().ok())
                {
                    latest = latest.max(number);
                }
            }
            loop {
                latest = latest
                    .checked_add(1)
                    .ok_or_else(|| SaveError::Failed("No available version number".into()))?;
                let version = target.with_file_name(format!("{base}_v{latest}.{ext}"));
                match publish_new(&temporary, &version) {
                    Ok(()) => return published(version),
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(e) => return Err(e.into()),
                }
            }
        }
        if !unchanged()? {
            return Err(SaveError::Conflict);
        }
        if !exists {
            match publish_new(&temporary, target) {
                Ok(()) => (),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    return Err(SaveError::Conflict);
                }
                Err(e) => return Err(e.into()),
            }
        } else {
            let backup = |n| backup_path(target, n);
            fs::copy(target, backup(0))?;
            if !unchanged()? {
                return Err(SaveError::Conflict);
            }
            for n in (1..=3).rev() {
                let previous = backup(n - 1);
                if previous.try_exists()? {
                    fs::rename(previous, backup(n))?;
                }
            }
            if !unchanged()? {
                return Err(SaveError::Conflict);
            }
            fs::rename(&temporary, target)?;
        }
        published(target.to_path_buf())
    })();
    let _ = fs::remove_file(temporary);
    result
}

impl Editor {
    pub(crate) fn save_project(&mut self, save_as: bool) {
        self.start_project_save(save_as, false);
    }
    pub(crate) fn start_project_save(&mut self, save_as: bool, close: bool) {
        if self.references.importing() {
            self.status =
                tr("Wait for the reference image to finish loading before saving or closing.")
                    .into();
            return;
        }
        if self.project_save.busy() || self.hub.active.is_none() {
            return;
        }
        self.finish_raster_stroke();
        self.finish_pending_pen();
        let choose = save_as
            || !self
                .path
                .as_deref()
                .is_some_and(crate::native_project::is_native_path)
            || self.doc.extras.imported;
        let target = if choose {
            let name = self
                .path
                .as_ref()
                .and_then(|p| p.file_stem())
                .map_or("animacion".into(), |s| s.to_string_lossy());
            let mut dialog = rfd::FileDialog::new()
                .add_filter("LibreToon", &["ltoon"])
                .set_file_name(format!(
                    "{}{name}.ltoon",
                    if self.project_save.conflict.is_some() {
                        "copia_"
                    } else {
                        ""
                    }
                ));
            if let Some(folder) = self.path.as_deref().and_then(Path::parent) {
                dialog = dialog.set_directory(folder);
            }
            dialog.save_file()
        } else {
            self.path.clone()
        };
        if let Some(mut path) = target {
            if path.extension().is_none() {
                path.set_extension("ltoon");
            }
            if !crate::native_project::is_native_path(&path) {
                self.status = tr("Choose a .ltoon file for the project.").into();
                return;
            }
            let path = if choose {
                path.canonicalize().unwrap_or(path)
            } else {
                path
            };
            if choose && self.doc.extras.imported && self.path.as_ref() == Some(&path) {
                self.status =
                    tr("Choose a different name to preserve the original Animate file.").into();
                return;
            }
            if self.hub.tabs.iter().enumerate().any(|(index, tab)| {
                Some(index) != self.hub.active && tab.record.path.as_ref() == Some(&path)
            }) {
                self.status =
                    tr("This file is open in another tab. Choose a different name.").into();
                return;
            }
            let record = &self.hub.tabs[self.hub.active.unwrap()].record;
            let expected = if record.path.as_ref() == Some(&path) {
                record.stamp
            } else if choose {
                stamp(&path)
            } else {
                None
            };
            self.queue_project_save(path, expected, false, close);
        }
    }
    fn queue_project_save(
        &mut self,
        path: PathBuf,
        expected: Option<Stamp>,
        incremental: bool,
        close: bool,
    ) {
        self.queue_project_save_for(self.hub.active.unwrap(), path, expected, incremental, close);
    }
    fn queue_project_save_for(
        &mut self,
        index: usize,
        path: PathBuf,
        expected: Option<Stamp>,
        incremental: bool,
        close: bool,
    ) {
        let record = &self.hub.tabs[index].record;
        let Some(mut document) = self.save_snapshot_for(index) else {
            return;
        };
        if incremental && document.extras.imported {
            return;
        }
        document.extras.imported = false;
        let (send, result) = mpsc::channel();
        self.project_save.job = Some(Job {
            target: path.clone(),
            result,
            recovery: record.recovery.clone(),
            revision: record.revision,
            close,
            interchange: false,
        });
        self.status = tr("Guardando proyecto…").into();
        std::thread::spawn(move || {
            let _ = send.send(write_snapshot(&document, &path, expected, incremental));
        });
    }
    pub(crate) fn export_fla_copy(&mut self) {
        if self.hub.home || self.project_save.busy() || self.references.importing() {
            return;
        }
        self.finish_raster_stroke();
        self.finish_pending_pen();
        let Some(index) = self.hub.active else {
            return;
        };
        let document = self.project_document();
        let layers = document
            .layers
            .iter()
            .chain(
                document
                    .extras
                    .scenes
                    .iter()
                    .flat_map(|scene| scene.layers.iter()),
            )
            .chain(
                document
                    .extras
                    .symbols
                    .iter()
                    .flat_map(|symbol| symbol.layers.iter()),
            );
        let (mut raster, mut effects) = (false, false);
        for layer in layers {
            raster |= layer.options.raster;
            effects |= layer.options.light.is_some() || crate::post::is_adjustment(layer);
        }
        let mut description = tr("The FLA preview preserves compatible vector artwork and timing. Media, rigs and other LibreToon features may be missing or flattened in Animate.").to_owned();
        let has_rig = !document.extras.rig.is_empty()
            || document
                .extras
                .scenes
                .iter()
                .any(|scene| !scene.rig.is_empty())
            || document
                .extras
                .symbols
                .iter()
                .any(|symbol| !symbol.rig.is_empty());
        let affected = [
            (!document.extras.media.assets.is_empty(), "Imported media"),
            (has_rig, "Rigging"),
            (raster, "Raster layers"),
            (effects, "Lights and adjustment layers"),
        ];
        for (present, label) in affected {
            if present {
                description.push_str(&format!(
                    "
• {}",
                    tr(label)
                ));
            }
        }
        if rfd::MessageDialog::new()
            .set_title(tr("Export Adobe Animate (.fla)…"))
            .set_description(&description)
            .set_buttons(rfd::MessageButtons::OkCancel)
            .show()
            != rfd::MessageDialogResult::Ok
        {
            return;
        }
        let source = self.path.as_deref();
        let stem = source
            .and_then(Path::file_stem)
            .map_or("animation".into(), |name| name.to_string_lossy());
        let mut dialog = rfd::FileDialog::new()
            .add_filter("Adobe Animate", &["fla"])
            .set_file_name(format!("{stem}-export.fla"));
        if let Some(folder) = source.and_then(Path::parent) {
            dialog = dialog.set_directory(folder);
        }
        let Some(mut path) = dialog.save_file() else {
            return;
        };
        if path.extension().is_none() {
            path.set_extension("fla");
        }
        if !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("fla"))
        {
            self.status = tr("Choose a .fla file for the export.").into();
            return;
        }
        let path = path.canonicalize().unwrap_or(path);
        if self
            .hub
            .tabs
            .iter()
            .any(|tab| tab.record.path.as_ref() == Some(&path))
        {
            self.status = tr("This file is open in another tab. Choose a different name.").into();
            return;
        }
        let record = &self.hub.tabs[index].record;
        let mut doc = document;
        doc.extras.imported = false;
        crate::raster::prune(&mut doc);
        let expected = stamp(&path);
        let (send, result) = mpsc::channel();
        self.project_save.job = Some(Job {
            target: path.clone(),
            result,
            recovery: record.recovery.clone(),
            revision: record.revision,
            close: false,
            interchange: true,
        });
        self.status = tr("Guardando proyecto…").into();
        std::thread::spawn(move || {
            let _ = send.send(write_snapshot(&doc, &path, expected, false));
        });
    }
    pub(crate) fn project_save_ui(&mut self, ctx: &egui::Context) {
        if self.project_save.busy() && ctx.input(|input| input.pointer.any_down()) {
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        if let Some(job) = &self.project_save.job {
            let result = match job.result.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err(SaveError::Failed("Save worker stopped".into())))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            };
            if let Some(result) = result {
                let job = self.project_save.job.take().unwrap();
                let quit = std::mem::take(&mut self.project_save.quit_after_save);
                match result {
                    Ok((path, stamp)) => {
                        let message = if job.interchange {
                            tr("FLA exportado")
                        } else {
                            tr("Proyecto guardado")
                        };
                        self.status = format!("{message} {}", path.display());
                        if !job.interchange {
                            self.complete_project_save(
                                &job.recovery,
                                job.revision,
                                path,
                                stamp,
                                job.close,
                            );
                        }
                        if quit {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                    Err(SaveError::Conflict) if job.interchange => {
                        self.status =
                            tr("The export target changed on disk. Choose a different file.")
                                .into();
                    }
                    Err(SaveError::Conflict) => {
                        self.project_save.conflict = Some((job.recovery, job.close))
                    }
                    Err(SaveError::Failed(error)) => {
                        self.project_save.update_save = None;
                        self.status = error.clone();
                        let mut details = format!(
                            "{error}

{} {}",
                            tr("Destination"),
                            job.target.display()
                        );
                        if !job.interchange && job.recovery.exists() {
                            details.push_str(&format!(
                                "
{} {}",
                                tr("Recovery file"),
                                job.recovery.display()
                            ));
                        }
                        self.project_save.error = Some(details);
                    }
                }
            } else {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }
        if let Some((recovery, close)) = self.project_save.conflict.clone() {
            let mut copy = false;
            let mut cancel = false;
            egui::Modal::new(egui::Id::unique("project-save-conflict")).show(ctx, |ui| {
                ui.heading(tr("El archivo cambió en disco"));
                if let Some(path) = self
                    .hub
                    .tabs
                    .iter()
                    .find(|tab| tab.record.recovery == recovery)
                    .and_then(|tab| tab.record.path.as_ref())
                {
                    ui.label(path.display().to_string());
                }
                ui.label(tr(
                    "Guarda tu trabajo en una copia para conservar la versión del disco.",
                ));
                ui.horizontal(|ui| {
                    copy = ui.button(tr("Guardar una copia")).clicked();
                    cancel = ui.button(tr("Cancelar")).clicked();
                });
            });
            if copy {
                if let Some(index) = self
                    .hub
                    .tabs
                    .iter()
                    .position(|t| t.record.recovery == recovery)
                {
                    self.activate_document(index);
                    if self.hub.active == Some(index) {
                        self.start_project_save(true, close);
                    }
                }
            }
            if cancel || (copy && !self.project_save.busy()) {
                self.project_save.update_save = None;
            }
            if copy || cancel {
                self.project_save.conflict = None;
            }
            return;
        }
        if let Some(error) = &self.project_save.error {
            let mut dismiss = false;
            egui::Modal::new(egui::Id::unique("project-save-error")).show(ctx, |ui| {
                ui.heading(tr("Could not save the project"));
                ui.label(error);
                dismiss = ui.button(tr("Aceptar")).clicked();
            });
            if dismiss {
                self.project_save.error = None;
            }
            return;
        }
        let options = &self.studio.prefs.saving;
        let schedule = (
            options.enabled,
            options.incremental,
            options.minutes.clamp(1, 1440),
        );
        let interval = Duration::from_secs(60 * u64::from(schedule.2));
        let now = Instant::now();
        if self.project_save.schedule != Some(schedule) {
            self.project_save.pending.clear();
            self.project_save.schedule = Some(schedule);
            self.project_save.next = Some(now + interval);
        }
        if !schedule.0 && !schedule.1 {
            return;
        }
        let deadline = self.project_save.next.unwrap_or(now + interval);
        if now >= deadline {
            self.project_save.next = Some(now + interval);
            self.project_save.pending = self
                .hub
                .tabs
                .iter()
                .filter(|tab| {
                    tab.record.dirty
                        && tab
                            .record
                            .path
                            .as_deref()
                            .is_some_and(crate::native_project::is_native_path)
                })
                .map(|tab| tab.record.recovery.clone())
                .collect();
        }
        ctx.request_repaint_after(
            self.project_save
                .next
                .unwrap_or(now + interval)
                .saturating_duration_since(now),
        );
        if self.project_save.pending.is_empty() {
            return;
        }
        if self.project_save.busy()
            || self.playing
            || self.hub.closing.is_some()
            || self.updater.install_pending
            || self.references.importing()
            || ctx.input(|i| i.pointer.any_down())
        {
            ctx.request_repaint_after(Duration::from_secs(1));
            return;
        }
        while let Some(recovery) = self.project_save.pending.pop_front() {
            let Some(index) = self
                .hub
                .tabs
                .iter()
                .position(|tab| tab.record.recovery == recovery && tab.record.dirty)
            else {
                continue;
            };
            let record = &self.hub.tabs[index].record;
            if let Some(path) = record.path.clone()
                && self
                    .save_snapshot_for(index)
                    .is_some_and(|doc| !doc.extras.imported)
            {
                self.queue_project_save_for(index, path, record.stamp, schedule.1, false);
                ctx.request_repaint_after(Duration::from_millis(100));
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consecutive_saves_use_published_metadata_and_recreate_a_missing_target() {
        let dir =
            std::env::temp_dir().join(format!("libretoon-repeat-save-{}", crate::features::id()));
        fs::create_dir(&dir).unwrap();
        let target = dir.join("animation.ltoon");
        let mut doc = crate::fresh_document();
        let mut expected = None;
        for fps in [24, 25, 30, 48] {
            doc.fps = fps;
            let (_, saved) = write_snapshot(&doc, &target, expected, false).unwrap();
            assert!(stamp(&target) == Some(saved));
            expected = Some(saved);
        }
        fs::remove_file(&target).unwrap();
        doc.fps = 60;
        let (_, recreated) = write_snapshot(&doc, &target, expected, false).unwrap();
        assert!(stamp(&target) == Some(recreated));
        assert_eq!(crate::documents::read_project(&target).unwrap().fps, 60);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn saving_preserves_external_edits_and_creates_numbered_versions() {
        let dir = std::env::temp_dir().join(format!("libretoon-save-{}", crate::features::id()));
        fs::create_dir(&dir).unwrap();
        let target = dir.join("animation.ltoon");
        let mut doc = crate::fresh_document();
        let (_, original) = write_snapshot(&doc, &target, None, false).unwrap();
        doc.fps = 30;
        let (_, updated) = write_snapshot(&doc, &target, Some(original), false).unwrap();
        assert_eq!(
            crate::documents::read_project(&backup_path(&target, 1))
                .unwrap()
                .fps,
            24
        );
        fs::write(&target, b"external edit").unwrap();
        assert!(matches!(
            write_snapshot(&doc, &target, Some(updated), false),
            Err(SaveError::Conflict)
        ));
        assert_eq!(fs::read(&target).unwrap(), b"external edit");
        let (one, _) = write_snapshot(&doc, &target, Some(updated), true).unwrap();
        let (two, _) = write_snapshot(&doc, &one, None, true).unwrap();
        assert_eq!(one.file_name().unwrap(), "animation_v1.ltoon");
        assert_eq!(two.file_name().unwrap(), "animation_v2.ltoon");
        assert_eq!(crate::documents::read_project(&one).unwrap().fps, 30);
        assert_eq!(crate::documents::read_project(&two).unwrap().fps, 30);
        assert_eq!(fs::read(&target).unwrap(), b"external edit");
        fs::remove_dir_all(&dir).unwrap();
    }
}
