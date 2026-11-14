//! Detached, verified in-place updates. The editor never overwrites a running binary.
use crate::{ffmpeg_setup::verify_hash, i18n::tr};
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{atomic::AtomicBool, mpsc},
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Kind {
    WindowsInstalled,
    WindowsPortable,
    MacApp,
    AppImage,
    Deb,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Host {
    kind: Kind,
    target: PathBuf,
    launch: PathBuf,
}

impl Host {
    pub(crate) fn detect() -> Result<Self, String> {
        let exe = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from);
        Self::from_paths(std::env::consts::OS, &exe, appimage.as_deref())
    }
    fn from_paths(os: &str, exe: &Path, appimage: Option<&Path>) -> Result<Self, String> {
        let root = exe.parent().ok_or("Application folder missing")?;
        let (kind,target,launch)=match os {
            "windows" if root.join("unins000.exe").is_file() => (Kind::WindowsInstalled,root.to_owned(),exe.to_owned()),
            "windows" if root.join("assets").is_dir() && root.join("LICENSE").is_file() => (Kind::WindowsPortable,root.to_owned(),exe.to_owned()),
            "macos" => {
                let app=exe.ancestors().find(|p|p.extension().is_some_and(|x|x=="app")).ok_or("LibreToon must be inside its .app bundle")?;
                if app.starts_with("/Volumes") {return Err(tr("Move LibreToon to Applications before updating.").into());}
                (Kind::MacApp,app.to_owned(),app.to_owned())
            },
            "linux" if appimage.is_some() => {
                let app=appimage.unwrap().canonicalize().map_err(|e|e.to_string())?;
                (Kind::AppImage,app.clone(),app)
            },
            "linux" if exe==Path::new("/opt/libretoon/libretoon") => (Kind::Deb,PathBuf::from("/opt/libretoon"),exe.to_owned()),
            _ => return Err(tr("This development or unmanaged copy cannot be updated automatically. Use an installed or portable package.").into()),
        };
        Ok(Self {
            kind,
            target,
            launch,
        })
    }
    pub(crate) fn accepts(&self, name: &str) -> bool {
        name.ends_with(match self.kind {
            Kind::WindowsInstalled => "-Setup.exe",
            Kind::WindowsPortable | Kind::MacApp => "-Portable.zip",
            Kind::AppImage => ".AppImage",
            Kind::Deb => ".deb",
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Plan {
    host: Host,
    package: PathBuf,
    hash: String,
    parent: u32,
    locale: crate::i18n::Language,
}

fn checked(mut command: Command) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let out = command
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{}\n{}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}

pub(crate) fn prepare(
    host: Host,
    package: PathBuf,
    hash: String,
    locale: crate::i18n::Language,
) -> Result<Plan, String> {
    let folder = package.parent().ok_or("Update folder missing")?;
    verify_hash(&package, &hash, folder, &AtomicBool::new(false))?;
    Ok(Plan {
        host,
        package,
        hash,
        parent: std::process::id(),
        locale,
    })
}

pub(crate) fn start(plan: &Plan) -> Result<(), String> {
    let folder = plan.package.parent().ok_or("Update folder missing")?;
    let helper = folder.join(if cfg!(windows) {
        "libretoon-update-helper.exe"
    } else {
        "libretoon-update-helper"
    });
    fs::copy(std::env::current_exe().map_err(|e| e.to_string())?, &helper)
        .map_err(|e| e.to_string())?;
    let manifest = folder.join("install.json");
    fs::write(
        &manifest,
        serde_json::to_vec(plan).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let mut command = Command::new(helper);
    command
        .arg("--libretoon-update-helper")
        .arg(manifest)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x00000008);
    }
    command.spawn().map_err(|e| e.to_string())?;
    Ok(())
}

fn wait_for_editor(pid: u32) -> Result<(), String> {
    if pid == 0 || pid == std::process::id() {
        return Err("Invalid editor process".into());
    }
    #[cfg(windows)]
    {
        use std::ffi::c_void;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
            fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
            fn CloseHandle(handle: *mut c_void) -> i32;
        }
        // Synchronize only: no termination rights. A process handle prevents
        // accidentally waiting on a recycled PID.
        let handle = unsafe { OpenProcess(0x00100000, 0, pid) };
        if handle.is_null() {
            let error = std::io::Error::last_os_error();
            return if error.raw_os_error() == Some(87) {
                Ok(())
            } else {
                Err(error.to_string())
            };
        }
        let result = unsafe { WaitForSingleObject(handle, 120000) };
        let error = std::io::Error::last_os_error();
        unsafe { CloseHandle(handle) };
        match result {
            0 => Ok(()),
            258 => Err("LibreToon did not close; update cancelled".into()),
            _ => Err(error.to_string()),
        }
    }
    #[cfg(not(windows))]
    {
        let start = std::time::Instant::now();
        loop {
            if !Command::new("/bin/kill")
                .args(["-0", &pid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_err(|e| e.to_string())?
                .success()
            {
                return Ok(());
            }
            if start.elapsed() > Duration::from_secs(120) {
                return Err("LibreToon did not close; update cancelled".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

fn regular_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
        for item in fs::read_dir(dir).map_err(|e| e.to_string())? {
            let path = item.map_err(|e| e.to_string())?.path();
            let ty = fs::symlink_metadata(&path)
                .map_err(|e| e.to_string())?
                .file_type();
            if ty.is_symlink() {
                return Err("Update payload contains a symbolic link".into());
            }
            if ty.is_dir() {
                walk(root, &path, out)?;
            } else if ty.is_file() {
                out.push(
                    path.strip_prefix(root)
                        .map_err(|e| e.to_string())?
                        .to_owned(),
                );
            } else {
                return Err("Unsupported update payload entry".into());
            }
        }
        Ok(())
    }
    let mut files = vec![];
    walk(root, root, &mut files)?;
    Ok(files)
}

// Only packaged files are touched. Unrelated user files stay in place. Every
// overwritten file is moved into a sibling backup before committing its replacement.
fn overlay(payload: &Path, target: &Path, backup: &Path) -> Result<(), String> {
    let mut files = regular_files(payload)?;
    files.sort();
    if backup.exists() {
        return Err("Preserving an existing update backup".into());
    }
    fs::create_dir(backup).map_err(|e| e.to_string())?;
    let mut replaced = Vec::new();
    let result = (|| {
        for rel in files {
            let destination = target.join(&rel);
            // Reject symlinks in the destination path too.
            for ancestor in destination.ancestors().take_while(|p| *p != target) {
                if fs::symlink_metadata(ancestor).is_ok_and(|m| m.file_type().is_symlink()) {
                    return Err("Update target contains a symbolic link".to_owned());
                }
            }
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let saved = backup.join(&rel);
            let existed = destination.exists();
            if existed && !destination.is_file() {
                return Err("Packaged file conflicts with an existing directory".into());
            }
            if existed {
                if let Some(parent) = saved.parent() {
                    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                fs::rename(&destination, &saved).map_err(|e| e.to_string())?;
            }
            replaced.push((destination.clone(), saved, existed));
            fs::copy(payload.join(rel), &destination).map_err(|e| e.to_string())?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        let mut rollback_errors = vec![];
        for (destination, saved, existed) in replaced.into_iter().rev() {
            if destination.exists() {
                if let Err(e) = fs::remove_file(&destination) {
                    rollback_errors.push(e.to_string());
                    continue;
                }
            }
            if existed {
                if let Err(e) = fs::rename(&saved, &destination) {
                    rollback_errors.push(e.to_string());
                }
            }
        }
        return Err(format!(
            "{error}\nRollback: {}. Backup: {}",
            if rollback_errors.is_empty() {
                "completed".into()
            } else {
                rollback_errors.join("; ")
            },
            backup.display()
        ));
    }
    Ok(())
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

fn replace_path(source: &Path, target: &Path, backup: &Path) -> Result<(), String> {
    if backup.exists() {
        return Err("Preserving an existing update backup".into());
    }
    fs::rename(target, backup).map_err(|e| e.to_string())?;
    if let Err(e) = fs::rename(source, target) {
        fs::rename(backup, target).map_err(|rollback| {
            format!(
                "{e}; restore failed: {rollback}; backup: {}",
                backup.display()
            )
        })?;
        return Err(e.to_string());
    }
    Ok(())
}

fn unpack_windows(package: &Path, unpacked: &Path) -> Result<(), String> {
    // Use our existing ZIP/CRC reader instead of depending on PowerShell's
    // module environment. Only regular files below LibreToon/ are materialized.
    if fs::metadata(package).map_err(|e| e.to_string())?.len() > 512 * 1024 * 1024 {
        return Err("Update ZIP exceeds size limit".into());
    }
    let bytes = fs::read(package).map_err(|e| e.to_string())?;
    let archive = crate::fla_zip::Archive::new(&bytes)?;
    let mut names = vec![];
    let mut unique = std::collections::HashSet::new();
    for name in archive.names() {
        let decoded = std::str::from_utf8(name)
            .map_err(|e| e.to_string())?
            .replace('\\', "/");
        let clean = decoded.trim_end_matches('/');
        let parts: Vec<_> = clean.split('/').collect();
        if parts.first() != Some(&"LibreToon")
            || parts.iter().any(|s| {
                s.is_empty()
                    || *s == "."
                    || *s == ".."
                    || s.contains(':')
                    || s.contains(char::is_control)
                    || s.trim() != *s
                    || s.ends_with('.')
            })
        {
            return Err("Unsafe update ZIP path".into());
        }
        if !unique.insert(clean.to_lowercase()) {
            return Err("Conflicting update ZIP paths".into());
        }
        names.push((name.to_vec(), PathBuf::from(clean), decoded.ends_with('/')));
        if names.len() > 8192 {
            return Err("Too many update ZIP entries".into());
        }
    }
    fs::create_dir(unpacked).map_err(|e| e.to_string())?;
    let mut total = 0usize;
    for (name, relative, is_dir) in names {
        let target = unpacked.join(relative);
        if is_dir {
            fs::create_dir_all(target).map_err(|e| e.to_string())?;
            continue;
        }
        let data = archive
            .entry_limited(&name, 512 * 1024 * 1024)?
            .ok_or("Missing ZIP entry")?;
        total = total
            .checked_add(data.len())
            .filter(|n| *n <= 2 * 1024 * 1024 * 1024)
            .ok_or("Update ZIP exceeds expanded size limit")?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)
            .map_err(|e| e.to_string())?;
        file.write_all(&data).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn installer_path_arg(flag: &str, path: &Path) -> Result<std::ffi::OsString, String> {
    let mut arg = std::ffi::OsString::from(flag);
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let mut components = path.components();
        if let Some(Component::Prefix(prefix)) = components.next() {
            match prefix.kind() {
                Prefix::VerbatimDisk(drive) => {
                    arg.push(format!("{}:", char::from(drive)));
                    arg.push(components.as_path());
                    return Ok(arg);
                }
                Prefix::VerbatimUNC(server, share) => {
                    arg.push(r"\\");
                    arg.push(server);
                    arg.push(r"\");
                    arg.push(share);
                    arg.push(components.as_path());
                    return Ok(arg);
                }
                Prefix::Verbatim(_) | Prefix::DeviceNS(_) => {
                    return Err("Unsupported Windows path for the LibreToon installer".into());
                }
                _ => {}
            }
        }
    }
    arg.push(path);
    Ok(arg)
}

fn install(plan: &Plan, tx: &mpsc::Sender<(f32, String)>) -> Result<(), String> {
    let progress = |p, label: &str| {
        let _ = tx.send((p, tr(label).into()));
    };
    let folder = plan.package.parent().ok_or("Update folder missing")?;
    progress(0.05, "Waiting for LibreToon to close…");
    wait_for_editor(plan.parent)?;
    progress(0.15, "Verifying update…");
    verify_hash(&plan.package, &plan.hash, folder, &AtomicBool::new(false))?;
    progress(0.35, "Installing update…");
    let host = &plan.host;
    match host.kind {
        Kind::WindowsInstalled => {
            let mut c = Command::new(&plan.package);
            c.args([
                "/VERYSILENT",
                "/SUPPRESSMSGBOXES",
                "/NORESTART",
                "/SP-",
                "/NOCANCEL",
                "/NOCLOSEAPPLICATIONS",
                "/NOFORCECLOSEAPPLICATIONS",
                "/NORESTARTAPPLICATIONS",
            ])
            .arg(installer_path_arg("/DIR=", &host.target)?);
            let log = folder.join("installer.log");
            c.arg(installer_path_arg("/LOG=", &log)?);
            checked(c).map_err(|error| {
                format!(
                    "{}\n{}\n{}",
                    error.trim(),
                    tr("Installer log:"),
                    log.display()
                )
            })?;
        }
        Kind::WindowsPortable => {
            let unpacked = folder.join("unpacked");
            unpack_windows(&plan.package, &unpacked)?;
            let payload = unpacked.join("LibreToon");
            if !payload.join("libretoon.exe").is_file() || !payload.join("assets").is_dir() {
                return Err("Invalid Windows portable payload".into());
            }
            let backup = host
                .target
                .join(format!(".libretoon-backup-{}", plan.parent));
            overlay(&payload, &host.target, &backup)?;
        }
        Kind::MacApp => {
            let unpacked = folder.join("unpacked");
            let mut c = Command::new("/usr/bin/ditto");
            c.args(["-x", "-k"]).arg(&plan.package).arg(&unpacked);
            checked(c)?;
            let app = unpacked.join("LibreToon.app");
            if !app.join("Contents/MacOS/libretoon").is_file() {
                return Err("Invalid macOS application payload".into());
            }
            let mut c = Command::new("/usr/bin/codesign");
            c.args(["--verify", "--strict"]).arg(&app);
            checked(c)?;
            let parent = host.target.parent().ok_or("Application folder missing")?;
            let candidate = parent.join(format!(".LibreToon-update-{}.app", plan.parent));
            let backup = parent.join(format!(".LibreToon-backup-{}", plan.parent));
            let script = folder.join("replace-macos.sh");
            // Atomic bundle rename with rollback, also when /Applications needs
            // authorization. No sudo and no silent permission bypass.
            let content = format!(
                "set -eu\ntest ! -e {new}\ntest ! -e {old}\n/usr/bin/ditto {src} {new}\n/bin/mv {dst} {old}\nif ! /bin/mv {new} {dst}; then /bin/mv {old} {dst}; exit 1; fi\n",
                new = shell_quote(&candidate),
                old = shell_quote(&backup),
                src = shell_quote(&app),
                dst = shell_quote(&host.target)
            );
            fs::write(&script, content).map_err(|e| e.to_string())?;
            let probe = parent.join(format!(".libretoon-write-test-{}", plan.parent));
            if fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&probe)
                .is_ok()
            {
                fs::remove_file(probe).map_err(|e| e.to_string())?;
                let mut c = Command::new("/bin/sh");
                c.arg(&script);
                checked(c)?;
            } else {
                let shell = format!("/bin/sh {}", shell_quote(&script));
                let apple = format!(
                    "do shell script {} with administrator privileges",
                    serde_json::to_string(&shell).map_err(|e| e.to_string())?
                );
                let mut c = Command::new("/usr/bin/osascript");
                c.arg("-e").arg(apple);
                checked(c)?;
            }
        }
        Kind::AppImage => {
            let parent = host.target.parent().ok_or("AppImage folder missing")?;
            let candidate = parent.join(format!(".libretoon-update-{}", plan.parent));
            if candidate.exists() {
                return Err("Preserving staged AppImage".into());
            }
            fs::copy(&plan.package, &candidate).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&candidate, fs::Permissions::from_mode(0o755))
                    .map_err(|e| e.to_string())?;
            }
            replace_path(
                &candidate,
                &host.target,
                &parent.join(format!(".libretoon-backup-{}", plan.parent)),
            )?;
        }
        Kind::Deb => {
            // apt upgrades the existing package record instead of creating a
            // separate portable copy. PolicyKit owns the authorization prompt.
            let mut c = Command::new("/usr/bin/pkexec");
            c.args(["/usr/bin/apt-get", "-y", "install"])
                .arg(&plan.package);
            checked(c)?;
        }
    }
    progress(0.95, "Restarting LibreToon…");
    reopen(host)?;
    progress(1., "Update installed.");
    Ok(())
}

fn reopen(host: &Host) -> Result<(), String> {
    let mut c = if host.kind == Kind::MacApp {
        let mut c = Command::new("/usr/bin/open");
        c.arg(&host.launch);
        c
    } else {
        Command::new(&host.launch)
    };
    c.current_dir(host.launch.parent().ok_or("Application folder missing")?)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    c.env_remove("APPIMAGE").env_remove("APPDIR");
    c.spawn().map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn helper_entry() -> Option<eframe::Result> {
    let mut args = std::env::args_os();
    args.next();
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--libretoon-update-helper")) {
        return None;
    }
    let result = (|| {
        let path = PathBuf::from(args.next().ok_or("Missing update manifest")?);
        let bytes = fs::read(path).map_err(|e| e.to_string())?;
        serde_json::from_slice::<Plan>(&bytes).map_err(|e| e.to_string())
    })();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([460., 180.])
            .with_resizable(false)
            .with_icon(crate::design::window_icon()),
        ..Default::default()
    };
    Some(eframe::run_native(
        "LibreToon Update",
        options,
        Box::new(move |cc| {
            crate::design::theme(&cc.egui_ctx);
            let (tx, rx) = mpsc::channel();
            let (done_tx, done) = mpsc::channel();
            let host = result.as_ref().ok().map(|p| p.host.clone());
            let locale = result.as_ref().map(|p| p.locale).unwrap_or_default();
            std::thread::spawn(move || {
                crate::i18n::set_locale(locale);
                let log = result
                    .as_ref()
                    .ok()
                    .and_then(|p| p.package.parent())
                    .map(|p| p.join("install-result.json"));
                let outcome = result.and_then(|plan| install(&plan, &tx));
                if let Some(log) = log {
                    let _ = fs::write(log, serde_json::to_vec_pretty(&serde_json::json!({"success":outcome.is_ok(),"error":outcome.as_ref().err()})).unwrap_or_default());
                }
                let _ = done_tx.send(outcome);
            });
            Ok(Box::new(Helper {
                progress: 0.,
                label: tr("Preparing update…").into(),
                rx,
                done,
                error: None,
                finished: false,
                host,
                locale,
            }))
        }),
    ))
}

struct Helper {
    progress: f32,
    label: String,
    rx: mpsc::Receiver<(f32, String)>,
    done: mpsc::Receiver<Result<(), String>>,
    error: Option<String>,
    finished: bool,
    host: Option<Host>,
    locale: crate::i18n::Language,
}
impl eframe::App for Helper {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        crate::i18n::set_locale(self.locale);
        while let Ok((p, label)) = self.rx.try_recv() {
            self.progress = p;
            self.label = label;
        }
        match self.done.try_recv() {
            Ok(Ok(())) => {
                self.finished = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Ok(Err(e)) => {
                self.error = Some(e);
                self.finished = true;
            }
            Err(mpsc::TryRecvError::Disconnected) if !self.finished => {
                self.error = Some("Update worker stopped".into());
                self.finished = true;
            }
            _ => {}
        }
        if !self.finished && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading(tr("Updating LibreToon"));
            ui.add_space(12.);
            if let Some(error) = &self.error {
                ui.label(tr(
                    "Update failed. The diagnostic and downloaded package have been kept.",
                ));
                ui.label(error);
                if let Some(host) = &self.host {
                    if crate::design::button(
                        ui,
                        crate::design::Icon::Play,
                        "Reopen LibreToon",
                        "Reopen LibreToon",
                        false,
                        true,
                    )
                    .clicked()
                    {
                        match reopen(host) {
                            Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                            Err(e) => self.label = e,
                        }
                    }
                }
                ui.label(&self.label);
            } else {
                ui.label(&self.label);
                ui.add(egui::ProgressBar::new(self.progress).animate(true));
            }
        });
        ctx.request_repaint_after(Duration::from_millis(100));
    }
}

impl crate::Editor {
    pub(crate) fn update_installation(&mut self, ctx: &egui::Context) {
        if !self.updater.install_pending {
            return;
        }
        if self.studio.export_job.is_some() || self.loader.busy() || self.references.importing() {
            self.updater.install_error(
                tr("Wait for the current import or export to finish before updating.").into(),
            );
            return;
        }
        if self.project_save.busy() || self.project_save.conflicted() {
            return;
        }
        self.finish_pending_pen();
        let dirty = self.hub.tabs.iter().any(|t| t.record.dirty);
        if dirty {
            let mut save = false;
            let mut cancel = false;
            egui::Modal::new(egui::Id::unique("update-save-documents")).show(ctx, |ui| {
                ui.heading(tr("Save before updating"));
                ui.label(tr(
                    "Save all documents before LibreToon closes to install the update.",
                ));
                ui.horizontal(|ui| {
                    save = crate::design::button(
                        ui,
                        crate::design::Icon::Save,
                        "Save all and update",
                        "Save all and update",
                        false,
                        true,
                    )
                    .clicked();
                    cancel = crate::design::button(
                        ui,
                        crate::design::Icon::Close,
                        "Cancelar",
                        "Cancelar",
                        false,
                        true,
                    )
                    .clicked();
                });
            });
            if cancel {
                self.project_save.update_save = None;
                self.updater.install_pending = false;
                return;
            }
            if save && self.project_save.update_save.is_none() {
                self.project_save.update_save = Some(self.hub.active);
            }
            if self.project_save.update_save.is_none() {
                return;
            }
            if let Some(index) = self.hub.tabs.iter().position(|tab| tab.record.dirty) {
                self.activate_document(index);
                if self.hub.active == Some(index) {
                    self.save_project(false);
                }
                if !self.project_save.busy() {
                    self.project_save.update_save = None;
                }
                return;
            }
        }
        if let Some(Some(active)) = self.project_save.update_save.take() {
            self.activate_document(active);
        }
        let Some(plan) = self.updater.prepared.as_ref() else {
            return;
        };
        match start(plan) {
            Ok(()) => {
                self.persist_documents();
                self.hub.allow_quit = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Err(e) => {
                self.updater.install_error(e);
            }
        }
        self.updater.install_pending = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_dir(name: &str) -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("work/update-tests")
            .join(format!(
                "{name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        fs::create_dir_all(&root).unwrap();
        root
    }
    #[cfg(windows)]
    #[test]
    fn inno_setup_receives_dos_or_unc_paths_instead_of_verbatim_paths() {
        for (input, expected) in [
            (
                r"\\?\C:\Users\Álvaro\LibreToon test",
                r"/DIR=C:\Users\Álvaro\LibreToon test",
            ),
            (
                r"\\?\UNC\server\share\LibreToon test",
                r"/DIR=\\server\share\LibreToon test",
            ),
            (r"D:\LibreToon", r"/DIR=D:\LibreToon"),
        ] {
            assert_eq!(
                installer_path_arg("/DIR=", Path::new(input)).unwrap(),
                expected
            );
        }
        let root = test_dir("instalación con espacios");
        let canonical = root.canonicalize().unwrap();
        let log = installer_path_arg("/LOG=", &canonical.join("installer.log")).unwrap();
        let log = log.to_string_lossy();
        let path = Path::new(log.strip_prefix("/LOG=").unwrap());
        assert!(!path.starts_with(r"\\?\"));
        assert_eq!(path.parent().unwrap().canonicalize().unwrap(), canonical);
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn real_windows_archive_extraction_handles_spaces_quotes_and_unicode() {
        let root = test_dir("literal ' $ space á");
        let archive = root.join("package.zip");
        fs::write(
            &archive,
            crate::fla_zip::pack(&[
                ("LibreToon/libretoon.exe", b"new executable".to_vec()),
                ("LibreToon/assets/test.txt", b"resource".to_vec()),
            ]),
        )
        .unwrap();
        unpack_windows(&archive, &root.join("unpacked")).unwrap();
        assert_eq!(
            fs::read(root.join("unpacked/LibreToon/libretoon.exe")).unwrap(),
            b"new executable"
        );
        let target = root.join("target");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("libretoon.exe"), "old executable").unwrap();
        overlay(
            &root.join("unpacked/LibreToon"),
            &target,
            &root.join("backup"),
        )
        .unwrap();
        assert_eq!(
            fs::read(target.join("libretoon.exe")).unwrap(),
            b"new executable"
        );
        assert_eq!(
            fs::read(root.join("backup/libretoon.exe")).unwrap(),
            b"old executable"
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn traversal_and_duplicate_zip_paths_are_rejected_before_writing() {
        let root = test_dir("unsafe-zip");
        for (index, names) in [
            vec!["LibreToon/../../outside"],
            vec!["LibreToon/FILE", "LibreToon/file"],
        ]
        .into_iter()
        .enumerate()
        {
            let zip = root.join(format!("{index}.zip"));
            let entries: Vec<_> = names.into_iter().map(|n| (n, b"bad".to_vec())).collect();
            fs::write(&zip, crate::fla_zip::pack(&entries)).unwrap();
            let out = root.join(format!("out{index}"));
            assert!(unpack_windows(&zip, &out).is_err());
            assert!(!out.exists());
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn failed_atomic_replacement_restores_the_original_path() {
        let root = test_dir("atomic");
        let target = root.join("app");
        fs::write(&target, "old").unwrap();
        assert!(replace_path(&root.join("missing"), &target, &root.join("backup")).is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "old");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn rollback_restores_files_if_a_later_replacement_fails() {
        let root = test_dir("rollback");
        let payload = root.join("payload");
        let target = root.join("target");
        fs::create_dir(&payload).unwrap();
        fs::create_dir_all(target.join("z.txt")).unwrap();
        fs::write(payload.join("a.txt"), "new").unwrap();
        fs::write(payload.join("z.txt"), "new").unwrap();
        fs::write(target.join("a.txt"), "old").unwrap();
        fs::write(target.join("z.txt/user.fla"), "keep").unwrap();
        assert!(overlay(&payload, &target, &root.join("backup")).is_err());
        assert_eq!(fs::read_to_string(target.join("a.txt")).unwrap(), "old");
        assert_eq!(
            fs::read_to_string(target.join("z.txt/user.fla")).unwrap(),
            "keep"
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn installed_and_portable_windows_copies_are_detected_in_place() {
        let root = test_dir("host");
        fs::create_dir(root.join("assets")).unwrap();
        fs::write(root.join("LICENSE"), "MIT").unwrap();
        let exe = root.join("libretoon.exe");
        assert_eq!(
            Host::from_paths("windows", &exe, None).unwrap().kind,
            Kind::WindowsPortable
        );
        fs::write(root.join("unins000.exe"), "uninstaller").unwrap();
        let host = Host::from_paths("windows", &exe, None).unwrap();
        assert_eq!(host.kind, Kind::WindowsInstalled);
        assert_eq!(host.target, root);
        assert!(host.accepts("LibreToon-1.0.0-Windows-x64-Setup.exe"));
        assert!(!host.accepts("LibreToon-1.0.0-Windows-x64-Portable.zip"));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn bundle_and_deb_updates_target_the_existing_location() {
        let host = Host::from_paths(
            "macos",
            Path::new("/Applications/LibreToon.app/Contents/MacOS/libretoon"),
            None,
        )
        .unwrap();
        assert_eq!(host.target, Path::new("/Applications/LibreToon.app"));
        assert!(host.accepts("LibreToon-1.0.0-macOS-arm64-Portable.zip"));
        assert!(!host.accepts("LibreToon-1.0.0-macOS-arm64.dmg"));
        assert!(
            Host::from_paths(
                "macos",
                Path::new("/Volumes/LibreToon/LibreToon.app/Contents/MacOS/libretoon"),
                None
            )
            .is_err()
        );
        assert_eq!(
            Host::from_paths("linux", Path::new("/opt/libretoon/libretoon"), None)
                .unwrap()
                .kind,
            Kind::Deb
        );
    }
    #[test]
    fn dirty_documents_prevent_closing_or_starting_a_helper() {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut editor = crate::Editor::new(&cc);
        editor.add_document(crate::fresh_document(), None);
        assert!(!editor.hub.tabs.is_empty());
        editor.hub.tabs[0].record.dirty = true;
        editor.updater.install_pending = true;
        let mut output = ctx.run_ui(egui::RawInput::default(), |_| {
            editor.update_installation(&ctx)
        });
        output.textures_delta.clear();
        assert!(editor.updater.install_pending);
        assert!(editor.hub.tabs[0].record.dirty);
        assert!(!editor.hub.allow_quit);
        assert!(output.viewport_output.values().all(|v| {
            !v.commands
                .iter()
                .any(|c| matches!(c, egui::ViewportCommand::Close))
        }));
    }
    #[test]
    fn portable_overlay_preserves_unrelated_files_and_old_payload() {
        let root = test_dir("overlay");
        fs::create_dir_all(root.join("payload/assets")).unwrap();
        fs::create_dir_all(root.join("target/assets")).unwrap();
        fs::write(root.join("payload/libretoon.exe"), "new").unwrap();
        fs::write(root.join("target/libretoon.exe"), "old").unwrap();
        fs::write(root.join("target/project.fla"), "user").unwrap();
        overlay(
            &root.join("payload"),
            &root.join("target"),
            &root.join("backup"),
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("target/libretoon.exe")).unwrap(),
            "new"
        );
        assert_eq!(
            fs::read_to_string(root.join("backup/libretoon.exe")).unwrap(),
            "old"
        );
        assert_eq!(
            fs::read_to_string(root.join("target/project.fla")).unwrap(),
            "user"
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn development_copy_is_not_a_portable_install() {
        assert!(
            Host::from_paths(
                "windows",
                Path::new("C:/repo/target/release/libretoon.exe"),
                None
            )
            .is_err()
        );
    }
    #[test]
    fn keeps_package_format_and_does_not_mix_installer_and_portable() {
        let host = Host {
            kind: Kind::WindowsPortable,
            target: "x".into(),
            launch: "x".into(),
        };
        assert!(host.accepts("LibreToon-0.3.0-Windows-x64-Portable.zip"));
        assert!(!host.accepts("LibreToon-0.3.0-Windows-x64-Setup.exe"));
    }
}
