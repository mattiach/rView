//! winit ApplicationHandler: windows, loader, folder navigation, action routing.

use crate::Config;
use crate::actions::{self, Action};
use crate::driver::{Driver, Observation, Step};
use crate::i18n::{self, t, tf};
use crate::menus::Menus;
use crate::render::Gpu;
use crate::shortcuts::Shortcuts;
use crate::ui::tool::{Content, ToolOut, ToolWindow};
use crate::ui::{self, Dialog, Outcome as DialogOutcome};
use crate::update;
use crate::window::{Request, ViewerWindow};
use rv_core::color::ViewSpace;
use rv_core::folder::{self, Folder, Nav};
use rv_core::formats::dialog_extensions;
use rv_core::loader::{self, Loader, Outcome, Preload};
use rv_core::settings::Settings;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::window::{Window, WindowId};

/// Show the loading indicator only for loads slower than this.
const INDICATOR_AFTER: Duration = Duration::from_millis(150);

pub enum UserEvent {
    Loaded(Outcome),
    Scanned {
        window: WindowId,
        generation: u64,
        folder: Folder,
        path: PathBuf,
        index: Option<usize>,
    },
    Menu(muda::MenuId),
    AccessKit(egui_winit::accesskit_winit::Event),
    /// An animation decoder produced a frame (the loop then checks what is due).
    AnimWake,
    /// Save Frame As finished (Err = message for the user).
    Saved(Result<(), String>),
    /// Recycle Bin / permanent delete finished (Ok(Some(id)) = undoable).
    Deleted {
        window: WindowId,
        path: PathBuf,
        permanent: bool,
        result: Result<Option<OsString>, String>,
    },
    /// Undo Delete finished: the restored path.
    Restored {
        window: WindowId,
        name: String,
        result: Result<PathBuf, String>,
    },
    DownloadProgress {
        window: WindowId,
        received: u64,
        total: Option<u64>,
    },
    Downloaded {
        window: WindowId,
        url: String,
        result: Result<PathBuf, String>,
    },
    /// Open With apps for `path` (enumerated on a worker 250 ms after the file changed).
    OpenWithList {
        window: WindowId,
        path: PathBuf,
        apps: Vec<crate::platform::Handler>,
    },
    /// A background operation (Copy, Show in Explorer, Open With) finished. `counted`: it was
    /// in the window's `pending_ops`.
    OpDone {
        window: WindowId,
        title: &'static str,
        counted: bool,
        result: Result<(), String>,
    },
    /// Recent files found missing at start (checked on a worker).
    RecentsMissing(Vec<PathBuf>),
    /// The update check finished.
    UpdateChecked(Result<update::Release, String>),
    /// A frame of this window produced a dialog outcome or a Markup request (run outside the
    /// paint).
    FrameOutcome(WindowId),
    /// Markup: the document's source decoded (and, for a `.rvm`, its document and the original
    /// for the view).
    MarkupSource {
        window: WindowId,
        result: Result<MarkupLoad, String>,
    },
    /// A sharper raster of the SVG of `window`, for its load `generation`.
    SvgRaster {
        window: WindowId,
        generation: u64,
        result: Result<(u32, u32, Vec<u8>), String>,
    },
    /// Save Project / Export / Overwrite Original finished. Ok(Some(id)): the
    /// Recycle Bin item of the overwritten original's backup.
    MarkupWritten {
        window: WindowId,
        kind: WriteKind,
        path: PathBuf,
        revision: rv_core::edit::Revision,
        result: Result<Option<OsString>, String>,
    },
}

pub struct MarkupLoad {
    src: rv_core::export::DocSource,
    /// From a project: its document and the original's name.
    project: Option<(rv_core::edit::DocState, String)>,
    /// From a project: the original, for the view (the file on screen is the flattened render).
    view: Option<rv_core::loader::ViewImage>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WriteKind {
    Project,
    Export,
    Overwrite,
}

impl From<egui_winit::accesskit_winit::Event> for UserEvent {
    fn from(e: egui_winit::accesskit_winit::Event) -> Self {
        UserEvent::AccessKit(e)
    }
}

pub struct App {
    cfg: Config,
    proxy: EventLoopProxy<UserEvent>,
    started: Instant,
    gpu: Option<Gpu>,
    /// GPU init started at process start, overlapping event loop/window creation and decode.
    gpu_init: Option<std::thread::JoinHandle<anyhow::Result<Gpu>>>,
    loader: Loader,
    /// View colour space from `colorspaceconversion` (provisional: Auto = sRGB).
    view_space: ViewSpace,
    anim_wake: Arc<dyn Fn() + Send + Sync>,
    shortcuts: Shortcuts,
    settings: Settings,
    windows: HashMap<WindowId, ViewerWindow>,
    /// Creation order (observation / tests).
    order: Vec<WindowId>,
    /// Last active windows, most recent first (MRU of 5): target of menu and shortcut actions.
    mru: VecDeque<WindowId>,
    state: rv_core::state::State,
    state_path: Option<PathBuf>,
    /// settings.toml (None in tests that pass their settings) and its time when last read/written.
    settings_path: Option<PathBuf>,
    settings_mtime: Option<std::time::SystemTime>,
    geometry_restored: bool,
    driver: Option<Driver>,
    pub failures: Vec<String>,
    /// Last message for the user (tests read it instead of a message box).
    last_message: Option<String>,
    /// A driver runs the app: messages are recorded, not shown, and windows are cloaked (the
    /// driver itself is taken out of `App` while its steps run).
    record_messages: bool,
    /// Tests: the answer of the next Save dialog (`Step::SavePath`).
    next_save_path: Option<PathBuf>,
    /// Tests: the button of the next unsaved-changes prompt (`Step::Answer`).
    driver_answer: Option<Option<usize>>,
    /// Files downloaded by Open URL, removed at exit.
    downloads: Vec<PathBuf>,
    /// Number formats and dates of the Info window.
    locale: ui::info::Locale,
    /// Decode options for Copy (SVG raster size).
    decode_opts: rv_core::decode::DecodeOptions,
    /// Settings, Welcome, update notice: at most one window of each kind.
    tools: HashMap<WindowId, ToolWindow>,
    update_status: update::Status,
    update_url: String,
    pending_update: bool,
    /// Last link the app opened in the browser (tests read it instead).
    opened_url: Option<String>,
}

impl Drop for App {
    fn drop(&mut self) {
        for p in &self.downloads {
            let _ = std::fs::remove_file(p);
        }
        // Windows (surfaces, egui renderers) before the GPU device: in field order the device
        // would go first and the process can hang at exit.
        self.windows.clear();
        self.tools.clear();
    }
}

fn file_mtime(p: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

fn client(id: WindowId) -> loader::ClientId {
    u64::from(id)
}

fn same_dir(a: &Path, b: &Path) -> bool {
    folder::same_path_ordinal(a, b)
}

impl App {
    pub fn new(mut cfg: Config, proxy: EventLoopProxy<UserEvent>) -> App {
        let driver = cfg.driver.take().map(Driver::new);
        let record_messages = driver.is_some();
        let software = cfg.software;
        let gpu_init = Some(std::thread::spawn(move || Gpu::new(software)));
        let settings_path = cfg.settings_file.clone();
        let update_url = cfg
            .update_url
            .clone()
            .unwrap_or_else(|| update::RELEASES_API.to_string());
        let settings = cfg
            .settings
            .take()
            .or_else(|| settings_path.as_deref().map(rv_core::settings::load))
            .unwrap_or_default();
        let settings_mtime = settings_path.as_deref().and_then(file_mtime);
        i18n::set_language(&settings.language);
        let state_path = cfg.state_file.clone();
        let mut state: rv_core::state::State = state_path.as_deref().map(rv_core::state::load).unwrap_or_default();
        if !settings.save_recents {
            state.recents.clear();
        }
        // recents whose file is gone are dropped once checked off the UI thread
        let paths: Vec<PathBuf> = state.recents.iter().map(|r| r.path.clone()).collect();
        let p = proxy.clone();
        std::thread::spawn(move || {
            let missing: Vec<PathBuf> = paths.into_iter().filter(|p| !p.exists()).collect();
            if !missing.is_empty() {
                let _ = p.send_event(UserEvent::RecentsMissing(missing));
            }
        });
        let budget = rv_core::cache::view_budget(
            settings.preloading_mode.cache_budget(),
            rv_core::cache::physical_memory(),
        );
        let p = proxy.clone();
        let loader = Loader::new(loader::default_workers(), budget, Default::default(), move |o| {
            let _ = p.send_event(UserEvent::Loaded(o));
        });
        let view_space = rv_core::color::view_space(settings.color_space_conversion);
        let p = proxy.clone();
        let anim_wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            let _ = p.send_event(UserEvent::AnimWake);
        });
        App {
            gpu_init,
            loader,
            view_space,
            anim_wake,
            cfg,
            proxy,
            started: Instant::now(),
            gpu: None,
            shortcuts: Shortcuts::new(&state.shortcuts.clone().into_iter().collect()),
            settings,
            windows: HashMap::new(),
            order: Vec::new(),
            mru: VecDeque::new(),
            state,
            state_path,
            settings_path,
            settings_mtime,
            geometry_restored: false,
            driver,
            failures: Vec::new(),
            last_message: None,
            record_messages,
            next_save_path: None,
            driver_answer: None,
            downloads: Vec::new(),
            locale: ui::info::Locale::user(),
            decode_opts: Default::default(),
            tools: HashMap::new(),
            update_status: update::Status::Off,
            update_url,
            pending_update: false,
            opened_url: None,
        }
    }

    /// Error or notice for the user: a message box, or recorded when a test drives the app.
    /// `title_id`: i18n message id of the box title.
    fn message(&mut self, id: Option<WindowId>, title_id: &str, text: String) {
        let title = t(title_id);
        log::warn!("{title}: {text}");
        if self.record_messages {
            self.last_message = Some(text);
            return;
        }
        let mut d = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title(title)
            .set_description(text);
        if let Some(w) = id.and_then(|id| self.windows.get(&id)) {
            d = d.set_parent(w.window.as_ref());
            crate::platform::before_modal(crate::platform::hwnd(&w.window));
        }
        d.show();
    }

    /// Most recent window without a file, for Open Recent / Open URL; None = a new one.
    fn empty_window(&self) -> Option<WindowId> {
        self.mru
            .iter()
            .copied()
            .find(|id| self.windows.get(id).is_some_and(|w| w.path.is_none()))
    }

    fn name_of(path: &Path) -> String {
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string())
    }

    /// Open Recent changed: every window's menu, and state.toml.
    fn recents_changed(&mut self) {
        let names: Vec<String> = self.state.recents.iter().map(|r| r.name.clone()).collect();
        for w in self.windows.values_mut() {
            if let Some(m) = &mut w.menus {
                m.set_recents(&names);
            }
        }
        self.save_state();
    }

    /// state.toml, when persistence is on (not in tests without a state file).
    fn save_state(&self) {
        if let Some(path) = &self.state_path
            && let Err(e) = rv_core::state::save(path, &self.state)
        {
            log::warn!("saving {}: {e}", path.display());
        }
    }

    /// settings.toml, when persistence is on; remembers its time so our own write is not taken
    /// for an outside change.
    fn save_settings(&mut self) {
        if let Some(path) = &self.settings_path {
            if let Err(e) = rv_core::settings::save(path, &self.settings) {
                log::warn!("saving {}: {e}", path.display());
            }
            self.settings_mtime = file_mtime(path);
        }
    }

    fn add_recent(&mut self, path: &Path) {
        if self.settings.save_recents && self.state.add_recent(path) {
            self.recents_changed();
        }
    }

    fn open_recent(&mut self, el: &ActiveEventLoop, i: usize) {
        let Some(r) = self.state.recents.get(i).cloned() else {
            return;
        };
        if !r.path.exists() {
            self.state.recents.remove(i);
            self.recents_changed();
            let text = tf("recent-missing", &[("path", &r.path.display().to_string())]);
            self.message(self.current(), "title-recent", text);
            return;
        }
        let target = self.empty_window();
        self.open_paths(el, target, vec![r.path], false);
    }

    /// Run in the background; the result comes back as a user event.
    fn spawn(&self, f: impl FnOnce() -> UserEvent + Send + 'static) {
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let _ = proxy.send_event(f());
        });
    }

    /// The file on screen (decoded without error), for actions with NeedsImage.
    fn image_path(&self, id: WindowId) -> Option<PathBuf> {
        let w = self.windows.get(&id)?;
        w.path.clone().filter(|_| w.image.is_some() && w.error.is_none())
    }

    /// Copy: the image as sRGB PNG + CF_DIBV5 and the file as CF_HDROP; the frame on
    /// screen for animations. Decoded again from the file on a worker thread.
    fn copy(&mut self, id: WindowId) {
        let Some(path) = self.image_path(id) else { return };
        let w = self.windows.get_mut(&id).unwrap();
        let frame = w.player.as_ref().map(|p| p.index()).zip(w.anim_bytes.clone());
        let format = w.info.as_ref().map(|i| i.format);
        let opts = self.decode_opts;
        w.pending_ops += 1;
        self.spawn(move || {
            let result = (|| -> Result<(), String> {
                use rv_core::color::{ViewSpace, ViewTransform};
                let (fw, fh, rgba) = match (frame, format) {
                    (Some((n, bytes)), Some(format)) => {
                        let (f, icc) = rv_core::anim::source_frame(&bytes, format, n)?;
                        let mut px = f.rgba;
                        ViewTransform::new(icc.as_deref(), ViewSpace::Srgb).apply(&mut px);
                        (f.w, f.h, px)
                    }
                    _ => {
                        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                        let ext = rv_core::formats::Format::from_path(&path);
                        let img = rv_core::decode::decode(&bytes, ext, &opts).map_err(|e| e.to_string())?;
                        let (w, h) = (img.size.w, img.size.h);
                        (w, h, rv_core::color::into_srgb_straight(img))
                    }
                };
                let mut png = Vec::new();
                image::ImageEncoder::write_image(
                    image::codecs::png::PngEncoder::new_with_quality(
                        &mut png,
                        image::codecs::png::CompressionType::Fast,
                        image::codecs::png::FilterType::Adaptive,
                    ),
                    &rgba,
                    fw,
                    fh,
                    image::ExtendedColorType::Rgba8,
                )
                .map_err(|e| e.to_string())?;
                let dib = crate::platform::dibv5_bytes(fw, fh, &rgba);
                crate::platform::clipboard_set(None, &png, &dib, &[path])
            })();
            UserEvent::OpDone {
                window: id,
                title: "title-copy",
                counted: true,
                result,
            }
        });
    }

    /// Paste: files from the clipboard open (first here, the others in new windows); an
    /// http(s) URL is downloaded like Open URL.
    fn paste(&mut self, el: &ActiveEventLoop, id: Option<WindowId>) {
        let files = crate::platform::clipboard_files();
        if !files.is_empty() {
            self.open_paths(el, id, files, false);
        } else if let Some(t) = crate::platform::clipboard_text().filter(|t| ui::openurl::is_url(t)) {
            self.start_download(id, t.trim().to_string());
        }
    }

    fn start_download(&mut self, id: Option<WindowId>, url: String) {
        let Some(id) = id.or_else(|| self.current()) else {
            return;
        };
        let Some(w) = self.windows.get_mut(&id) else { return };
        let cancel = Arc::new(AtomicBool::new(false));
        w.download_cancel = Some(cancel.clone());
        w.dialog = Some(Dialog::Download(ui::openurl::Progress {
            url: url.clone(),
            received: 0,
            total: None,
        }));
        w.pending_ops += 1;
        w.window.request_redraw();
        let proxy = self.proxy.clone();
        self.spawn(move || {
            let mut last = Instant::now();
            let result = ui::openurl::download(&url, &cancel, |received, total| {
                if last.elapsed() >= Duration::from_millis(100) {
                    last = Instant::now();
                    let _ = proxy.send_event(UserEvent::DownloadProgress {
                        window: id,
                        received,
                        total,
                    });
                }
            });
            UserEvent::Downloaded {
                window: id,
                url,
                result,
            }
        });
    }

    fn on_downloaded(&mut self, el: &ActiveEventLoop, id: WindowId, url: String, result: Result<PathBuf, String>) {
        if let Some(w) = self.windows.get_mut(&id) {
            w.pending_ops = w.pending_ops.saturating_sub(1);
            w.download_cancel = None;
            if matches!(w.dialog, Some(Dialog::Download(_))) {
                w.dialog = None;
                w.window.request_redraw();
            }
        }
        match result {
            Ok(path) => {
                self.downloads.push(path.clone());
                let target = self.empty_window();
                self.open_paths(el, target, vec![path], false);
            }
            Err(e) if e == "cancelled" => {}
            Err(e) => {
                let text = tf("download-failed", &[("url", &url), ("reason", &e)]);
                self.message(Some(id), "title-openurl", text)
            }
        }
    }

    /// Rename: never over an existing file; the renamed file is opened again.
    fn rename(&mut self, id: WindowId, path: PathBuf, name: String) {
        let new = match ui::rename::validate(&path, &name) {
            Err(e) => return self.message(Some(id), "title-rename", e),
            Ok(None) => return,
            Ok(Some(new)) => new,
        };
        let old_name = Self::name_of(&path);
        if std::fs::metadata(&path).is_ok_and(|m| m.permissions().readonly()) {
            return self.message(Some(id), "title-rename", tf("rename-read-only", &[("name", &old_name)]));
        }
        if let Err(e) = crate::platform::rename_no_replace(&path, &new) {
            let why = if e.kind() == std::io::ErrorKind::AlreadyExists {
                tf("rename-exists", &[("name", &name)])
            } else {
                e.to_string()
            };
            let text = tf("rename-failed", &[("old", &old_name), ("new", &name), ("reason", &why)]);
            return self.message(Some(id), "title-rename", text);
        }
        if let Some(r) = self
            .state
            .recents
            .iter_mut()
            .find(|r| folder::same_path_ordinal(&r.path, &path))
        {
            r.path = new.clone();
            r.name = Self::name_of(&new);
            self.recents_changed();
        }
        if let Some(w) = self.windows.get_mut(&id) {
            w.last_load = None; // the folder list must be read again
        }
        self.load(id, new, false, None);
    }

    /// Delete (Recycle Bin, undoable) or Delete Permanently.
    fn delete(&mut self, id: WindowId, permanent: bool) {
        let Some(path) = self.image_path(id) else { return };
        let name = Self::name_of(&path);
        let title = if permanent {
            "title-delete-permanent"
        } else {
            "title-delete"
        };
        if std::fs::metadata(&path).is_ok_and(|m| m.permissions().readonly()) {
            return self.message(Some(id), title, tf("delete-read-only", &[("name", &name)]));
        }
        if !permanent && !crate::platform::has_recycle_bin(&path) {
            return self.message(Some(id), title, tf("delete-no-bin", &[("name", &name)]));
        }
        if permanent || self.settings.ask_delete {
            let hwnd = self.windows.get(&id).and_then(|w| crate::platform::hwnd(&w.window));
            let (yes, dont_ask) = match self.cfg.auto_confirm {
                Some(v) => (v, false),
                None if permanent => crate::platform::confirm(
                    hwnd,
                    &t(title),
                    &tf("delete-permanent-confirm", &[("name", &name)]),
                    &t("delete-permanent-detail"),
                    None,
                ),
                None => crate::platform::confirm(
                    hwnd,
                    &t(title),
                    &tf("delete-confirm", &[("name", &name)]),
                    &t("delete-confirm-detail"),
                    Some(&t("delete-dont-ask")),
                ),
            };
            if !yes {
                return;
            }
            if dont_ask {
                self.settings.ask_delete = false;
                self.save_settings();
            }
        }
        if let Some(w) = self.windows.get_mut(&id) {
            w.pending_ops += 1;
        }
        self.spawn(move || {
            let result = if permanent {
                crate::platform::delete_overwritten(&path).map(|_| None)
            } else {
                crate::platform::recycle(&path)
            };
            UserEvent::Deleted {
                window: id,
                path,
                permanent,
                result,
            }
        });
    }

    fn on_deleted(&mut self, id: WindowId, path: PathBuf, permanent: bool, result: Result<Option<OsString>, String>) {
        let name = Self::name_of(&path);
        if let Some(w) = self.windows.get_mut(&id) {
            w.pending_ops = w.pending_ops.saturating_sub(1);
        }
        let bin_id = match result {
            Err(e) => {
                let text = tf("delete-failed", &[("name", &name), ("reason", &e)]);
                return self.message(Some(id), "title-delete", text);
            }
            Ok(b) => b,
        };
        if self.state.forget_recents(std::slice::from_ref(&path)) {
            self.recents_changed();
        }
        let (mode, looping) = (self.settings.after_delete, self.settings.loop_folders_enabled);
        let Some(w) = self.windows.get_mut(&id) else { return };
        match bin_id {
            Some(b) => w.undo.push((path.clone(), b, false)),
            None if !permanent => {
                let text = tf("delete-not-in-bin", &[("name", &name)]);
                self.message(Some(id), "title-delete", text);
            }
            None => {}
        }
        let Some(w) = self.windows.get_mut(&id) else { return };
        let deleted_at = w.folder.as_ref().and_then(|f| f.index_of_fast(&path));
        if let Some(f) = &mut w.folder {
            f.remove(&path);
        }
        // Only the file still on screen moves the window (the user may have navigated meanwhile).
        if !w.path.as_ref().is_some_and(|p| folder::same_path_ordinal(p, &path)) {
            w.update_menus();
            return;
        }
        let len = w.folder.as_ref().map_or(0, |f| f.files.len());
        match deleted_at.and_then(|i| folder::after_delete(len, i, mode, looping)) {
            Some(t) => {
                let next = w.folder.as_ref().unwrap().files[t].clone();
                w.index = Some(t);
                let dir = if mode == rv_core::settings::AfterDelete::MoveBack {
                    -1
                } else {
                    1
                };
                self.load(id, next, false, Some(dir));
            }
            None => w.clear(),
        }
    }

    /// Undo Delete: restore the last deleted file of this window and open it.
    fn undo(&mut self, id: WindowId) {
        let Some(w) = self.windows.get_mut(&id) else { return };
        let Some((path, bin_id, overwritten)) = w.undo.pop() else {
            return;
        };
        w.pending_ops += 1;
        let name = Self::name_of(&path);
        self.spawn(move || {
            let mut result = crate::platform::restore(&bin_id);
            if overwritten {
                // "Restore original": the backup of an Overwrite Original replaces the file again
                result = result.and_then(|backup| {
                    rv_core::atomic::restore_backup(&backup, &path)
                        .map(|_| path.clone())
                        .map_err(|e| e.to_string())
                });
            }
            UserEvent::Restored {
                window: id,
                name,
                result,
            }
        });
    }

    fn on_restored(&mut self, id: WindowId, name: String, result: Result<PathBuf, String>) {
        if let Some(w) = self.windows.get_mut(&id) {
            w.pending_ops = w.pending_ops.saturating_sub(1);
        }
        match result {
            Ok(path) => {
                if let Some(w) = self.windows.get_mut(&id) {
                    w.last_load = None; // the folder list must be read again
                }
                self.load(id, path, false, None);
            }
            Err(e) => {
                let text = tf("undo-failed", &[("name", &name), ("reason", &e)]);
                self.message(Some(id), "title-undo", text)
            }
        }
    }

    fn on_dialog(&mut self, id: WindowId, o: DialogOutcome) {
        let Some(w) = self.windows.get_mut(&id) else { return };
        w.window.request_redraw();
        match o {
            DialogOutcome::Closed => w.dialog = None,
            DialogOutcome::CancelDownload => {
                if let Some(c) = &w.download_cancel {
                    c.store(true, Ordering::Relaxed);
                }
                w.dialog = None;
            }
            DialogOutcome::Rename(path, name) => {
                w.dialog = None;
                self.rename(id, path, name);
            }
            DialogOutcome::Export(o) => {
                w.dialog = None;
                self.export(id, o);
            }
            DialogOutcome::OpenUrl(url) => {
                w.dialog = None;
                if ui::openurl::is_url(&url) {
                    self.start_download(Some(id), url);
                } else if !url.is_empty() {
                    self.message(Some(id), "title-openurl", tf("openurl-invalid", &[("url", &url)]));
                }
            }
        }
    }

    /// OK (true) or Cancel of the active window's dialog, as the driver presses them.
    fn dialog_button(&mut self, accept: bool) {
        if let Some(id) = self.current()
            && let Some(d) = self.windows.get(&id).and_then(|w| w.dialog.as_ref())
        {
            let o = if accept { d.accept() } else { d.cancel() };
            self.on_dialog(id, o);
        }
    }

    /// Info rows of a window whose Info is open, following the file on screen.
    fn refresh_info(&mut self, id: WindowId) {
        let Some(w) = self.windows.get_mut(&id) else { return };
        if !w.info_open || w.loading {
            return;
        }
        let Some(path) = w.path.clone() else {
            w.info_rows.clear();
            return;
        };
        if w.info_file.as_ref().is_none_or(|(p, _)| *p != path) {
            let facts = std::fs::metadata(&path)
                .ok()
                .map(|m| (m.len(), m.modified().unwrap_or(std::time::UNIX_EPOCH)));
            w.info_file = Some((path.clone(), facts));
        }
        let frames = w.player.as_mut().and_then(|p| p.frame_count());
        let rows = match (&w.info, w.image_size, &w.error) {
            (Some(info), Some(size), None) => ui::info::rows(
                &ui::info::Facts {
                    path: &path,
                    size,
                    info,
                    frames,
                    file: w.info_file.as_ref().and_then(|(_, f)| *f),
                },
                &self.locale,
            ),
            _ => vec![
                ("info-name", Self::name_of(&path)),
                (
                    "info-location",
                    path.parent().map(|p| p.display().to_string()).unwrap_or_default(),
                ),
            ],
        };
        if rows != w.info_rows {
            w.info_rows = rows;
            w.window.request_redraw();
        }
    }

    /// Open With entries for the file on screen, enumerated in the background.
    fn spawn_open_with(&mut self, id: WindowId) {
        let Some(path) = self.windows.get(&id).and_then(|w| w.path.clone()) else {
            return;
        };
        self.spawn(move || {
            let _com = crate::platform::ComThread::init();
            let apps = crate::platform::open_with_handlers(&path);
            UserEvent::OpenWithList { window: id, path, apps }
        });
    }

    fn on_open_with_list(&mut self, id: WindowId, path: PathBuf, apps: Vec<crate::platform::Handler>) {
        let Some(w) = self.windows.get_mut(&id) else { return };
        if !w.path.as_ref().is_some_and(|p| folder::same_path_ordinal(p, &path)) {
            return; // the file changed meanwhile
        }
        w.open_with = apps.iter().map(|a| a.ui_name.clone()).collect();
        let entries: Vec<(String, Option<muda::Icon>)> = apps
            .into_iter()
            .map(|a| {
                let icon = a.icon.and_then(|(w, h, px)| muda::Icon::from_rgba(px, w, h).ok());
                (a.ui_name, icon)
            })
            .collect();
        if let Some(m) = &mut w.menus {
            m.set_open_with(&entries);
        }
    }

    fn open_with_app(&mut self, id: WindowId, i: usize) {
        let Some(path) = self.image_path(id) else { return };
        let Some(name) = self.windows.get(&id).and_then(|w| w.open_with.get(i).cloned()) else {
            return;
        };
        self.spawn(move || {
            let _com = crate::platform::ComThread::init();
            UserEvent::OpDone {
                window: id,
                title: "title-openwith",
                counted: false,
                result: crate::platform::open_with(&path, &name),
            }
        });
    }

    /// The window actions go to: most recently active (MRU), else the first one.
    fn current(&self) -> Option<WindowId> {
        self.mru.front().copied().or_else(|| self.order.first().copied())
    }

    fn touch(&mut self, id: WindowId) {
        self.mru.retain(|x| *x != id);
        self.mru.push_front(id);
        self.mru.truncate(5);
    }

    /// Save the window's placement when state persistence is on.
    fn remember_geometry(&mut self, id: WindowId) {
        let Some(w) = self.windows.get(&id).filter(|_| self.state_path.is_some()) else {
            return;
        };
        if let Some(g) = crate::platform::placement(&w.window) {
            self.state.geometry = Some(g);
            self.save_state();
        }
    }

    /// `launch`: window opened for a file given at start (WindowResizeMode "when launching").
    fn new_window(&mut self, el: &ActiveEventLoop, path: Option<PathBuf>, launch: bool) -> anyhow::Result<WindowId> {
        let attrs = Window::default_attributes()
            .with_title("rView")
            .with_visible(false)
            .with_inner_size(winit::dpi::LogicalSize::new(1024.0, 768.0))
            .with_min_inner_size(winit::dpi::LogicalSize::new(144.0, 100.0));
        let window = Arc::new(el.create_window(crate::platform::with_app_icon(attrs))?);
        let id = window.id();
        // Shown right away, painted by the system with the background brush; the GPU frame
        // replaces it with identical pixels.
        crate::platform::set_background_brush(&window, self.settings.bg_color);
        let ui = ViewerWindow::ui_state(el, &window, self.proxy.clone());
        crate::platform::set_dark_titlebar(&window, window.theme() == Some(winit::window::Theme::Dark));
        crate::platform::grab_cursor_on_caption(&window);
        // Geometry of the last session for the first window
        let mut placed = false;
        if !self.geometry_restored {
            self.geometry_restored = true;
            if let Some(g) = &self.state.geometry {
                crate::platform::set_placement(&window, g);
                placed = true;
            }
        }
        if self.record_messages && !self.cfg.visible {
            crate::platform::cloak(&window);
        }
        window.set_visible(true);
        if self.cfg.measure_startup && self.windows.is_empty() {
            println!(
                "window visible after {:.1} ms",
                self.started.elapsed().as_secs_f64() * 1000.0
            );
        }
        // Start decoding (and scanning the folder) before waiting for the GPU.
        if let Some(p) = &path {
            let side = window.inner_size().width.max(window.inner_size().height);
            self.loader.request(client(id), 1, p.clone(), false, side, vec![]);
            self.spawn_scan(id, 1, p.clone(), None);
        }
        if self.gpu.is_none() {
            let init = self.gpu_init.take().expect("gpu init started");
            let gpu = init.join().map_err(|_| anyhow::anyhow!("gpu init panicked"))??;
            log::debug!("gpu ready at {:?}", self.started.elapsed());
            self.gpu = Some(gpu);
        }
        let gpu = self.gpu.as_ref().unwrap();
        let surface = gpu.instance.create_surface(window.clone())?;
        let mut w = ViewerWindow::new(gpu, window.clone(), surface, ui);
        w.view_space = self.view_space;
        w.title_mode = self.settings.title_bar_mode;
        w.resize_pending = launch;
        w.placement_restored = placed;
        let has_file = path.is_some();
        if let Some(p) = path {
            w.generation = 1;
            w.scan_generation = 1;
            w.scanning = true;
            w.begin_load(p, None);
        }
        self.windows.insert(id, w);
        self.order.push(id);
        self.touch(id);
        if has_file {
            // Through the event queue: a decode that finished while the GPU was initialising
            // (12 MP JPEG at ~55 ms, GPU at ~200 ms) is processed first and lands in the first
            // frame instead of the next one (−30 ms to the first image). Until then the
            // system background brush paints the window.
            window.request_redraw();
        } else {
            self.redraw(el, id);
        }
        Ok(id)
    }

    /// Folder scan + sort + index on a worker thread (no I/O on the UI thread).
    fn spawn_scan(&self, id: WindowId, generation: u64, path: PathBuf, prev: Option<Folder>) {
        let Some(dir) = path.parent().map(Path::to_path_buf) else {
            return;
        };
        let (proxy, s) = (self.proxy.clone(), self.settings.clone());
        std::thread::spawn(move || {
            let folder = Folder::load(&dir, &s, prev.as_ref());
            let index = folder.index_of(&path);
            let _ = proxy.send_event(UserEvent::Scanned {
                window: id,
                generation,
                folder,
                path,
                index,
            });
        });
    }

    /// Files to preload around the current index: ±radius, in the direction of travel first.
    fn preloads(&self, w: &ViewerWindow) -> Vec<Preload> {
        let (Some(f), Some(i)) = (&w.folder, w.index) else {
            return vec![];
        };
        let n = f.files.len() as isize;
        let dir = w.direction as isize;
        let mut out: Vec<Preload> = Vec::new();
        for d in 1..=self.settings.preloading_mode.radius() as isize {
            for (side, behind) in [(dir, false), (-dir, true)] {
                let mut j = i as isize + side * d;
                if !(0..n).contains(&j) {
                    if !self.settings.loop_folders_enabled {
                        continue;
                    }
                    j = j.rem_euclid(n);
                }
                let path = &f.files[j as usize];
                if j as usize != i && !out.iter().any(|p| &p.path == path) {
                    out.push(Preload {
                        path: path.clone(),
                        distance: d as u8,
                        behind,
                    });
                }
            }
        }
        out
    }

    /// Show `path` in window `id`. `nav` = direction of travel when it comes from navigation.
    fn load(&mut self, id: WindowId, path: PathBuf, bypass_cache: bool, nav: Option<i8>) {
        let Some(w) = self.windows.get_mut(&id) else { return };
        w.generation += 1;
        let rescan = folder::should_rescan(w.last_load);
        w.begin_load(path.clone(), nav);
        // Another folder (or none yet): its list is unknown until the scan finishes.
        let in_folder = w
            .folder
            .as_ref()
            .is_some_and(|f| path.parent().is_some_and(|d| same_dir(d, &f.dir)));
        if !in_folder {
            w.folder = None;
            w.index = None;
        } else if nav.is_none() {
            w.index = w.folder.as_ref().and_then(|f| f.index_of_fast(&path));
        }
        let need_scan = !in_folder || rescan;
        let generation = w.generation;
        let prev = need_scan.then(|| w.folder.clone()).flatten();
        if need_scan {
            w.scan_generation += 1;
            w.scanning = true;
        }
        let scan_gen = w.scan_generation;
        let w = &self.windows[&id];
        let preloads = self.preloads(w);
        let side = w.preview_side();
        self.loader
            .request(client(id), generation, path.clone(), bypass_cache, side, preloads);
        if need_scan {
            self.spawn_scan(id, scan_gen, path, prev);
        }
    }

    /// Returns false when nothing changed (no folder yet, edge without loop).
    fn navigate(&mut self, id: WindowId, nav: Nav) -> bool {
        if !self.leave_markup(id) {
            return false;
        }
        let looping = self.settings.loop_folders_enabled;
        let Some(w) = self.windows.get_mut(&id) else {
            return false;
        };
        let Some(f) = &w.folder else { return false }; // NeedsFolder
        let Some(t) = folder::navigate(f.files.len(), w.index, nav, looping, &mut w.rng) else {
            return false;
        };
        let path = f.files[t].clone();
        w.index = Some(t);
        self.load(id, path, false, Some(nav.direction()));
        true
    }

    /// Slideshow step: next (or previous if reversed); stops at an edge without loop.
    fn slideshow_step(&mut self, id: WindowId, now: Instant) {
        let nav = if self.settings.slideshow_reversed {
            Nav::Previous
        } else {
            Nav::Next
        };
        let moved = self.navigate(id, nav);
        let period = Duration::from_secs_f64(self.settings.slideshow_timer.max(0.05));
        if let Some(w) = self.windows.get_mut(&id) {
            w.slideshow_due = moved.then(|| {
                let due = w.slideshow_due.unwrap_or(now) + period;
                if due < now { now + period } else { due }
            });
        }
    }

    /// First path in `id`, the others in new windows.
    /// Any manual open stops the slideshow of that window.
    fn open_paths(&mut self, el: &ActiveEventLoop, id: Option<WindowId>, paths: Vec<PathBuf>, launch: bool) {
        let mut it = paths.into_iter();
        if let Some(first) = it.next() {
            match id {
                Some(id) => {
                    if !self.leave_markup(id) {
                        return;
                    }
                    if let Some(w) = self.windows.get_mut(&id) {
                        w.slideshow_due = None;
                    }
                    self.load(id, first, false, None)
                }
                None => {
                    let _ = self.new_window(el, Some(first), launch);
                }
            }
        }
        for p in it {
            if let Err(e) = self.new_window(el, Some(p), launch) {
                log::error!("new window: {e}");
            }
        }
    }

    /// Open a tool window, or bring the open one of the same kind to the front.
    fn open_tool(&mut self, el: &ActiveEventLoop, content: Content) {
        if let Some(t) = self.tools.values().find(|t| t.content.kind() == content.kind()) {
            t.window.focus_window();
            return;
        }
        let Some(gpu) = &self.gpu else { return };
        let cloaked = self.record_messages && !self.cfg.visible;
        match ToolWindow::new(el, gpu, content, self.proxy.clone(), cloaked) {
            Ok(t) => {
                self.tools.insert(t.window.id(), t);
            }
            Err(e) => log::error!("tool window: {e:#}"),
        }
    }

    fn tool_of(&mut self, kind: &str) -> Option<WindowId> {
        self.tools
            .iter()
            .find(|(_, t)| t.content.kind() == kind)
            .map(|(id, _)| *id)
    }

    fn on_tool_outs(&mut self, el: &ActiveEventLoop, id: WindowId, outs: Vec<ToolOut>) {
        for o in outs {
            match o {
                ToolOut::Apply { settings, shortcuts } => self.apply_settings(settings, Some(shortcuts), true),
                ToolOut::Close => {
                    self.tools.remove(&id);
                }
                ToolOut::OpenUrl(url) => {
                    if self.record_messages {
                        self.opened_url = Some(url);
                    } else {
                        crate::platform::open_url(&url);
                    }
                }
                ToolOut::Welcome { update_notifications } => {
                    self.state.first_launch = false;
                    self.save_state();
                    if update_notifications != self.settings.update_notifications {
                        let mut s = self.settings.clone();
                        s.update_notifications = update_notifications;
                        self.apply_settings(s, None, true);
                    }
                    self.maybe_check_updates();
                }
            }
        }
        let _ = el;
    }

    fn tool_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(gpu) = &self.gpu else { return };
        let Some(t) = self.tools.get_mut(&id) else { return };
        let outs = match &event {
            WindowEvent::RedrawRequested => t.redraw(gpu),
            WindowEvent::Focused(true) => {
                self.check_settings_file();
                return;
            }
            e => t.on_event(gpu, e),
        };
        self.on_tool_outs(el, id, outs);
        if matches!(event, WindowEvent::RedrawRequested) {
            self.wake_driver();
        }
    }

    /// After a frame: the driver may be waiting for it. Its steps don't run inside the paint (a
    /// step may open a modal dialog, which must not start there): a user event brings the loop
    /// back, and `user_event` runs the driver.
    fn wake_driver(&self) {
        if self.driver.is_some() {
            let _ = self.proxy.send_event(UserEvent::AnimWake);
        }
    }

    /// Start the update check if it is on and due.
    fn maybe_check_updates(&mut self) {
        if !cfg!(feature = "update-check") || !self.settings.update_notifications {
            self.update_status = update::Status::Off;
            return;
        }
        if self.pending_update {
            return;
        }
        let now = update::now_epoch();
        if !update::due(self.state.last_update_check, now) {
            self.update_status = update::Status::Recent(self.state.last_update_check);
            return;
        }
        self.update_status = update::Status::Checking;
        self.pending_update = true;
        let url = self.update_url.clone();
        self.spawn(move || UserEvent::UpdateChecked(update::fetch(&url)));
    }

    fn on_update_checked(&mut self, el: &ActiveEventLoop, result: Result<update::Release, String>) {
        self.pending_update = false;
        self.state.last_update_check = update::now_epoch();
        self.save_state();
        self.update_status = match result {
            Ok(r) if update::newer(&r.version, env!("CARGO_PKG_VERSION")) => {
                let notice = ui::notice::UpdateNotice {
                    version: r.version.clone(),
                    notes: r.notes.clone(),
                    url: r.url.clone(),
                };
                self.open_tool(el, Content::Update(notice));
                update::Status::Available(r)
            }
            Ok(_) => update::Status::UpToDate,
            Err(e) => {
                log::warn!("update check: {e}");
                update::Status::Failed(e)
            }
        };
    }

    /// settings.toml changed outside this process (another instance, an editor): reload it when
    /// a window of the app becomes active (the WM_ACTIVATEAPP rule).
    fn check_settings_file(&mut self) {
        let Some(path) = self.settings_path.clone() else { return };
        let mtime = file_mtime(&path);
        if mtime == self.settings_mtime {
            return;
        }
        self.settings_mtime = mtime;
        let s = rv_core::settings::load(&path);
        if s != self.settings {
            log::info!("settings changed on disk, reloaded");
            self.apply_settings(s, None, false);
        }
    }

    /// New menus for every window (language or shortcuts changed).
    fn rebuild_menus(&mut self) {
        let names: Vec<String> = self.state.recents.iter().map(|r| r.name.clone()).collect();
        let now = Instant::now();
        for w in self.windows.values_mut() {
            let Some(h) = crate::platform::hwnd(&w.window) else {
                continue;
            };
            if let Some(old) = w.menus.take() {
                old.detach(h.0 as isize);
            }
            let mut m = Menus::build(&self.shortcuts);
            m.attach(h.0 as isize, self.settings.menubar_enabled && !w.is_fullscreen());
            m.set_recents(&names);
            w.menus = Some(m);
            w.open_with_due = w.path.as_ref().map(|_| now); // Open With entries again
            w.update_menus();
        }
    }

    /// Apply new settings to the whole app and every window.
    /// `shortcuts`: new custom shortcuts, if they were edited. `save`: write them.
    fn apply_settings(&mut self, new: Settings, shortcuts: Option<BTreeMap<String, Vec<String>>>, save: bool) {
        let old = std::mem::replace(&mut self.settings, new.sanitized());
        let s = self.settings.clone();
        let mut menus = false;
        if old.language != s.language {
            i18n::set_language(&s.language);
            menus = true;
            for t in self.tools.values() {
                t.refresh_texts();
            }
        }
        if let Some(custom) = shortcuts
            && custom != self.state.shortcuts
        {
            self.shortcuts = Shortcuts::new(&custom.clone().into_iter().collect());
            self.state.shortcuts = custom;
            self.save_state();
            menus = true;
        }
        if menus {
            self.rebuild_menus();
        }
        if old.color_space_conversion != s.color_space_conversion {
            self.view_space = rv_core::color::view_space(s.color_space_conversion);
            for w in self.windows.values_mut() {
                w.view_space = self.view_space;
            }
            self.loader.set_options(loader::Options {
                decode: self.decode_opts,
                view_space: self.view_space,
            });
        }
        if old.preloading_mode != s.preloading_mode {
            self.loader.set_cache_budget(rv_core::cache::view_budget(
                s.preloading_mode.cache_budget(),
                rv_core::cache::physical_memory(),
            ));
        }
        let rescan = (
            old.sort_mode,
            old.sort_descending,
            old.skip_hidden,
            old.allow_mime_content_detection,
        ) != (
            s.sort_mode,
            s.sort_descending,
            s.skip_hidden,
            s.allow_mime_content_detection,
        );
        let refit = (old.crop_mode, old.past_actual_size_enabled) != (s.crop_mode, s.past_actual_size_enabled);
        if let Some(w) = self.windows.values().next()
            && (old.bg_color, old.bg_color_enabled) != (s.bg_color, s.bg_color_enabled)
        {
            let bg = if s.bg_color_enabled {
                s.bg_color
            } else {
                crate::platform::system_window_color()
            };
            crate::platform::set_background_brush(&w.window, bg);
        }
        let ids: Vec<WindowId> = self.order.clone();
        for id in ids {
            let reload = old.color_space_conversion != s.color_space_conversion;
            let Some(w) = self.windows.get_mut(&id) else { continue };
            w.title_mode = s.title_bar_mode;
            w.refresh_title();
            if old.menubar_enabled != s.menubar_enabled
                && let (Some(m), Some(h)) = (&w.menus, crate::platform::hwnd(&w.window))
            {
                m.set_bar_visible(h.0 as isize, s.menubar_enabled && !w.is_fullscreen());
            }
            if refit {
                w.view.fit(&s);
            }
            w.window.request_redraw();
            let path = w.path.clone();
            if rescan && let Some(p) = &path {
                w.scan_generation += 1;
                w.scanning = true;
                let generation = w.scan_generation;
                self.spawn_scan(id, generation, p.clone(), None);
            }
            if reload && let Some(p) = path {
                // the cache key has the view space: the image comes back in the new one
                self.load(id, p, false, None);
            }
        }
        if save {
            self.save_settings();
        }
        if old.update_notifications != s.update_notifications {
            self.maybe_check_updates();
        }
    }

    /// SVG windows zoomed beyond their raster get a sharper one, decoded on a worker.
    fn spawn_svg_rasters(&mut self) {
        let space = self.view_space;
        let jobs: Vec<(WindowId, u64, PathBuf, u32)> = self
            .windows
            .iter_mut()
            .filter_map(|(id, w)| {
                let target = w.svg_want.take()?;
                let path = w.path.clone()?;
                w.svg_pending = true;
                Some((*id, w.generation, path, target))
            })
            .collect();
        for (id, generation, path, target) in jobs {
            let opts = rv_core::decode::DecodeOptions { svg_target: target };
            self.spawn(move || {
                let result = (|| {
                    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                    let img = rv_core::decode::decode(&bytes, Some(rv_core::formats::Format::Svg), &opts)
                        .map_err(|e| e.to_string())?;
                    let (w, h) = (img.size.w, img.size.h);
                    Ok((w, h, rv_core::color::into_view(img, space)))
                })();
                UserEvent::SvgRaster {
                    window: id,
                    generation,
                    result,
                }
            });
        }
    }

    /// Enter Markup on the image of `id`: a new document of the image's size; the source
    /// (pixels at full precision, original bytes) is decoded again on a worker. A `.rvm` brings its
    /// document and original instead.
    fn enter_markup(&mut self, id: WindowId, tool: crate::markup::Tool) {
        let styles = self.settings.markup.last_style.clone();
        let Some(w) = self.windows.get_mut(&id) else { return };
        if !w.enable_state().editable {
            return;
        }
        if let Some(m) = &mut w.markup {
            m.set_tool(tool);
            w.window.request_redraw();
            return;
        }
        let (Some((iw, ih)), Some(path)) = (w.image_size, w.path.clone()) else {
            return;
        };
        w.slideshow_due = None;
        w.markup = Some(crate::markup::Markup::new(iw, ih, styles, tool, path.clone()));
        w.sync_markup_view(&self.settings);
        w.refresh_title();
        w.update_menus();
        w.pending_ops += 1;
        let (opts, space) = (self.decode_opts, self.view_space);
        self.spawn(move || {
            let result = (|| {
                use rv_core::formats::Format;
                let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                let ext = Format::from_path(&path);
                if Format::sniff(&bytes).or(ext) == Some(Format::Project) {
                    let p = rv_core::project::read(&bytes, &opts).map_err(|e| e.to_string())?;
                    let mut info = rv_core::loader::ImageInfo::of(&p.image);
                    info.file_size = bytes.len() as u64;
                    let (w, h) = (p.image.size.w, p.image.size.h);
                    let src = rv_core::export::DocSource::new(p.image.clone(), p.original.into());
                    let view = rv_core::loader::ViewImage {
                        w,
                        h,
                        rgba: rv_core::color::into_view(p.image, space),
                        full: (w, h),
                        preview: false,
                        info,
                        animation: None,
                    };
                    return Ok(MarkupLoad {
                        src,
                        project: Some((p.doc, p.manifest.original_name)),
                        view: Some(view),
                    });
                }
                let img = rv_core::decode::decode(&bytes, ext, &opts).map_err(|e| e.to_string())?;
                Ok(MarkupLoad {
                    src: rv_core::export::DocSource::new(img, bytes.into()),
                    project: None,
                    view: None,
                })
            })();
            UserEvent::MarkupSource { window: id, result }
        });
    }

    fn on_markup_source(&mut self, id: WindowId, result: Result<MarkupLoad, String>) {
        let Some(w) = self.windows.get_mut(&id) else { return };
        w.pending_ops = w.pending_ops.saturating_sub(1);
        let load = match result {
            Ok(l) => l,
            Err(e) => {
                log::warn!("markup source: {e}");
                self.exit_markup(id);
                return self.message(Some(id), "title-markup", e);
            }
        };
        let Some(m) = &mut w.markup else { return };
        if let Some((doc, name)) = load.project {
            let mut missing: Vec<String> = doc
                .shapes
                .iter()
                .filter_map(|s| match &s.geom {
                    rv_core::edit::Geom::Text { font, .. } if !rv_core::edit::text::has_family(&font.family) => {
                        Some(font.family.clone())
                    }
                    _ => None,
                })
                .collect();
            missing.sort();
            missing.dedup();
            m.missing_fonts = missing;
            m.history = rv_core::edit::History::from_loaded(doc);
            m.sel.clear();
            m.project_path = Some(m.file.clone());
            m.original_name = name;
            m.reload_on_exit = true;
        }
        let doc_size = (m.doc().image_size.w, m.doc().image_size.h);
        if (load.src.w, load.src.h) != doc_size {
            log::warn!(
                "markup source {}x{} differs from the image on screen",
                load.src.w,
                load.src.h
            );
            return;
        }
        // a sharper SVG raster on screen: back to texels = image pixels (the patches are written
        // in image pixels)
        if let (None, Some(gpu), Some(img)) = (&load.view, &self.gpu, &w.image)
            && (img.w, img.h) != (load.src.w, load.src.h)
        {
            let vt = rv_core::color::ViewTransform::new(load.src.icc.as_deref(), self.view_space);
            let rgba = rv_core::color::finish_view(rv_core::color::rgba8((*load.src.pixels).clone()), &vt);
            let (vw, vh) = (load.src.w, load.src.h);
            w.image = Some(gpu.upload(vw, vh, &rgba, img.mips, w.image_size.unwrap_or((vw, vh))));
        }
        m.source = Some(load.src);
        if let (Some(v), Some(gpu)) = (load.view, &self.gpu) {
            // the original under the document (a new generation: the view fits the new size)
            w.generation += 1;
            let g = w.generation;
            w.show(gpu, &self.settings, g, &v, self.view_space, &self.anim_wake);
        }
        w.sync_markup_view(&self.settings);
        w.refresh_title();
        w.update_menus();
        w.window.request_redraw();
    }

    /// Leave Markup without asking (the document is dropped); the last styles are kept.
    fn exit_markup(&mut self, id: WindowId) {
        let Some(w) = self.windows.get_mut(&id) else { return };
        if let Some(gpu) = &self.gpu {
            w.clear_patches(gpu);
        }
        let Some(m) = w.markup.take() else { return };
        w.markup_out = None;
        w.window.set_cursor(winit::window::CursorIcon::Default);
        w.sync_markup_view(&self.settings);
        w.refresh_title();
        w.update_menus();
        w.window.request_redraw();
        if m.reload_on_exit {
            // a project shows its render again, an overwritten file its new pixels
            self.loader.invalidate(&m.file);
            self.load(id, m.file.clone(), true, None);
        }
        if m.styles != self.settings.markup.last_style {
            self.settings.markup.last_style = m.styles;
            self.save_settings();
        }
    }

    /// Leave Markup if `id` is in it, asking first when the document has unsaved changes:
    /// Save Project / Discard / Cancel, + Export… when the current state isn't exported.
    /// False = the user chose to stay (or to export first).
    fn leave_markup(&mut self, id: WindowId) -> bool {
        let Some(w) = self.windows.get(&id) else { return true };
        let Some(m) = &w.markup else { return true };
        if m.history.is_dirty() {
            let hwnd = crate::platform::hwnd(&w.window);
            let exported = m.history.exported_current();
            let detail = if exported {
                t("markup-unsaved-exported")
            } else {
                t("markup-unsaved-detail")
            };
            let mut buttons = vec![t("markup-save-project"), t("markup-discard")];
            if !exported {
                buttons.push(t("markup-export-first"));
            }
            let answer = self.driver_answer.take();
            if answer.is_some() {
                self.last_message = Some(format!("{}\n{detail}\n{}", t("markup-unsaved"), buttons.join(" | ")));
            }
            let choice = match (answer, self.cfg.auto_confirm) {
                (Some(a), _) => a,
                (None, Some(true)) => Some(1),
                (None, Some(false)) => None,
                (None, None) => crate::platform::ask(
                    hwnd,
                    &i18n::plain(&actions::name(Action::Markup)),
                    &t("markup-unsaved"),
                    &detail,
                    &buttons,
                ),
            };
            match choice {
                Some(0) if self.save_project(id, true) => {}
                Some(1) => {}
                Some(2) => {
                    self.open_export(id);
                    return false;
                }
                _ => return false,
            }
        }
        self.exit_markup(id);
        true
    }

    /// The system Save dialog (or the path a test queued with `Step::SavePath`).
    fn pick_save(
        &mut self,
        id: WindowId,
        title: &str,
        name: &str,
        dir: Option<&Path>,
        filter: (&str, &[&str]),
    ) -> Option<PathBuf> {
        if let Some(p) = self.next_save_path.take() {
            return Some(p);
        }
        let w = self.windows.get(&id)?;
        let mut dlg = rfd::FileDialog::new()
            .set_title(title)
            .set_file_name(name)
            .add_filter(filter.0, filter.1)
            .set_parent(w.window.as_ref());
        if let Some(d) = dir {
            dlg = dlg.set_directory(d);
        }
        crate::platform::before_modal(crate::platform::hwnd(&w.window));
        dlg.save_file()
    }

    fn markup_parts(&self, id: WindowId) -> Option<(&crate::markup::Markup, rv_core::export::DocSource)> {
        let m = self.windows.get(&id)?.markup.as_ref()?;
        Some((m, m.source.clone()?))
    }

    /// Save Project: the first time asks where (`<name>.rvm` next to the original), then
    /// writes there. `sync` = wait for it (the unsaved-changes prompt): true when saved.
    fn save_project(&mut self, id: WindowId, sync: bool) -> bool {
        let Some((m, src)) = self.markup_parts(id) else {
            return false;
        };
        let (doc, name, known, file) = (
            m.doc().clone(),
            m.original_name.clone(),
            m.project_path.clone(),
            m.file.clone(),
        );
        let dst = match known {
            Some(p) => p,
            None => {
                let stem = Path::new(&name).file_stem().map(|s| s.to_string_lossy().into_owned());
                let proposed = format!("{}.{}", stem.unwrap_or_default(), rv_core::project::EXTENSION);
                let title = t("save-project-title");
                let filter = t("filter-project");
                match self.pick_save(
                    id,
                    &title,
                    &proposed,
                    file.parent(),
                    (&filter, &[rv_core::project::EXTENSION]),
                ) {
                    Some(p) => p,
                    None => return false,
                }
            }
        };
        let opts = self.decode_opts;
        let (rev, path) = (doc.revision, dst.clone());
        let job = move || {
            let bytes = rv_core::project::write(&doc, &src.bytes, &name, src.format).map_err(|e| e.to_string())?;
            // verified with the same reader that opens projects
            rv_core::atomic::write_atomic(&dst, &bytes, |b| rv_core::project::read(b, &opts).is_ok())
                .map(|_| None)
                .map_err(|e| e.to_string())
        };
        if sync {
            let ok = std::thread::scope(|s| s.spawn(job).join()).unwrap_or_else(|_| Err("save failed".into()));
            let saved = ok.is_ok();
            self.on_markup_written(id, WriteKind::Project, path, rev, ok);
            return saved;
        }
        self.windows.get_mut(&id).unwrap().pending_ops += 1;
        self.spawn(move || UserEvent::MarkupWritten {
            window: id,
            kind: WriteKind::Project,
            path,
            revision: rev,
            result: job(),
        });
        true
    }

    /// Export…: the options dialog; the file name comes after it.
    fn open_export(&mut self, id: WindowId) {
        let Some((m, src)) = self.markup_parts(id) else { return };
        let redactions = !m.doc().redactions.is_empty();
        let d = ui::export::Export::new(&self.settings.markup, Some(src.format), redactions);
        let w = self.windows.get_mut(&id).unwrap();
        w.dialog = Some(Dialog::Export(d));
        w.window.request_redraw();
    }

    fn export(&mut self, id: WindowId, o: rv_core::export::ExportOptions) {
        let Some((m, src)) = self.markup_parts(id) else { return };
        let (doc, name, file) = (m.doc().clone(), m.original_name.clone(), m.file.clone());
        let stem = Path::new(&name).file_stem().map(|s| s.to_string_lossy().into_owned());
        let proposed = format!("{}-markup.{}", stem.unwrap_or_default(), o.format.exts()[0]);
        let title = i18n::plain(&actions::name(Action::Export));
        let Some(dst) = self.pick_save(id, &title, &proposed, file.parent(), (o.format.name(), o.format.exts())) else {
            return;
        };
        if folder::same_path_ordinal(&dst, &file)
            && rv_core::formats::Format::from_path(&file) == Some(rv_core::formats::Format::Project)
        {
            return self.message(Some(id), "title-export", t("export-over-project"));
        }
        self.windows.get_mut(&id).unwrap().pending_ops += 1;
        let rev = doc.revision;
        self.spawn(move || {
            let result = (|| {
                let flat = rv_core::export::flatten(&doc, &src);
                let bytes = rv_core::export::encode(&flat, src.exif.as_deref(), &o).map_err(|e| e.to_string())?;
                let (f, w, h) = (o.format, flat.w, flat.h);
                rv_core::atomic::write_atomic(&dst, &bytes, |b| rv_core::export::verify(b, f, w, h))
                    .map_err(|e| e.to_string())
            })();
            UserEvent::MarkupWritten {
                window: id,
                kind: WriteKind::Export,
                path: dst,
                revision: rev,
                result: result.map(|_| None),
            }
        });
    }

    /// Overwrite Original: only for exportable formats, confirmed
    /// (always for JPEG); the previous file goes to the Recycle Bin and can be restored.
    fn overwrite_original(&mut self, id: WindowId) {
        let Some((m, src)) = self.markup_parts(id) else { return };
        let Some(format) = rv_core::export::ExportFormat::of(src.format) else {
            return;
        };
        let (doc, file) = (m.doc().clone(), m.file.clone());
        let jpeg = format == rv_core::export::ExportFormat::Jpeg;
        if self.settings.markup.ask_overwrite || jpeg {
            let hwnd = self.windows.get(&id).and_then(|w| crate::platform::hwnd(&w.window));
            let name = Self::name_of(&file);
            let detail = if jpeg {
                t("overwrite-jpeg-detail")
            } else {
                t("overwrite-detail")
            };
            let (yes, dont_ask) = match self.cfg.auto_confirm {
                Some(v) => (v, false),
                None => crate::platform::confirm(
                    hwnd,
                    &i18n::plain(&actions::name(Action::OverwriteOriginal)),
                    &tf("overwrite-confirm", &[("name", &name)]),
                    &detail,
                    (!jpeg).then(|| t("delete-dont-ask")).as_deref(),
                ),
            };
            if !yes {
                return;
            }
            if dont_ask {
                self.settings.markup.ask_overwrite = false;
                self.save_settings();
            }
        }
        let m = &self.settings.markup;
        let o = rv_core::export::ExportOptions {
            format,
            jpeg_quality: m.jpeg_quality.clamp(1, 100),
            background: [255, 255, 255],
            preset: if doc.redactions.is_empty() {
                m.metadata_preset
            } else {
                rv_core::settings::MetadataPreset::StripAll
            },
        };
        self.windows.get_mut(&id).unwrap().pending_ops += 1;
        let rev = doc.revision;
        self.spawn(move || {
            let result = (|| {
                let flat = rv_core::export::flatten(&doc, &src);
                let bytes = rv_core::export::encode(&flat, src.exif.as_deref(), &o).map_err(|e| e.to_string())?;
                let (w, h) = (flat.w, flat.h);
                let backup =
                    rv_core::atomic::replace_original(&file, &bytes, |b| rv_core::export::verify(b, format, w, h))
                        .map_err(|e| e.to_string())?;
                // recoverable: the previous file goes to the Recycle Bin ("Restore original")
                match crate::platform::recycle(&backup) {
                    Ok(bin) => Ok(bin),
                    Err(e) => {
                        log::warn!("backup not recycled ({e}), left at {}", backup.display());
                        Ok(None)
                    }
                }
            })();
            UserEvent::MarkupWritten {
                window: id,
                kind: WriteKind::Overwrite,
                path: file,
                revision: rev,
                result,
            }
        });
    }

    fn on_markup_written(
        &mut self,
        id: WindowId,
        kind: WriteKind,
        path: PathBuf,
        rev: rv_core::edit::Revision,
        result: Result<Option<OsString>, String>,
    ) {
        let title = match kind {
            WriteKind::Project => "title-save-project",
            WriteKind::Export => "title-export",
            WriteKind::Overwrite => "title-overwrite",
        };
        let bin = match result {
            Ok(b) => b,
            Err(e) => return self.message(Some(id), title, e),
        };
        self.loader.invalidate(&path);
        let Some(w) = self.windows.get_mut(&id) else { return };
        if let Some(m) = &mut w.markup {
            match kind {
                // only Save Project makes the document clean
                WriteKind::Project => {
                    m.history.saved_revision = Some(rev);
                    m.project_path = Some(path.clone());
                }
                WriteKind::Export => m.history.exported_revision = Some(rev),
                WriteKind::Overwrite => {
                    m.history.exported_revision = Some(rev);
                    m.reload_on_exit = true;
                }
            }
        }
        if kind == WriteKind::Overwrite
            && let Some(b) = bin
        {
            w.undo.push((path, b, true));
        }
        w.refresh_title();
        w.update_menus();
    }

    /// Copy in Markup: the rendered document as sRGB PNG + CF_DIBV5, no metadata and no
    /// file (it doesn't exist).
    fn markup_copy(&mut self, id: WindowId) {
        let Some((m, src)) = self.markup_parts(id) else { return };
        let doc = m.doc().clone();
        self.windows.get_mut(&id).unwrap().pending_ops += 1;
        self.spawn(move || {
            let result = (|| -> Result<(), String> {
                let flat = rv_core::export::flatten(&doc, &src);
                let mut rgba = rv_core::color::rgba8(flat.pixels);
                rv_core::color::to_srgb_in_place(&mut rgba, flat.icc.as_deref());
                let mut png = Vec::new();
                image::ImageEncoder::write_image(
                    image::codecs::png::PngEncoder::new_with_quality(
                        &mut png,
                        image::codecs::png::CompressionType::Fast,
                        image::codecs::png::FilterType::Adaptive,
                    ),
                    &rgba,
                    flat.w,
                    flat.h,
                    image::ExtendedColorType::Rgba8,
                )
                .map_err(|e| e.to_string())?;
                let dib = crate::platform::dibv5_bytes(flat.w, flat.h, &rgba);
                crate::platform::clipboard_set(None, &png, &dib, &[])
            })();
            UserEvent::OpDone {
                window: id,
                title: "title-copy",
                counted: true,
                result,
            }
        });
    }

    /// Keys in Markup before the global shortcuts. True = taken.
    fn markup_key(&mut self, id: WindowId, seq: &str) -> bool {
        let undo = self.shortcuts.keys(Action::Undo).to_vec();
        let redo = self.shortcuts.keys(Action::Redo).to_vec();
        let Some(w) = self.windows.get_mut(&id) else {
            return false;
        };
        let Some(m) = &mut w.markup else { return false };
        let out = m.key(seq, &undo, &redo);
        w.sync_markup_view(&self.settings);
        w.refresh_title();
        w.window.request_redraw();
        match out {
            crate::markup::Out::Handled => true,
            crate::markup::Out::Pass => false,
            crate::markup::Out::Exit => {
                self.leave_markup(id);
                true
            }
            crate::markup::Out::Navigate(d) => {
                self.navigate(id, if d < 0 { Nav::Previous } else { Nav::Next });
                true
            }
            // keys never produce it (toolbar buttons do)
            crate::markup::Out::Do(_) => true,
        }
    }

    fn dispatch(&mut self, el: &ActiveEventLoop, id: Option<WindowId>, a: Action) {
        let id = id.or_else(|| self.current());
        // a modal dialog takes the input of its window
        if id
            .and_then(|id| self.windows.get(&id))
            .is_some_and(|w| w.dialog.is_some())
        {
            return;
        }
        // in Markup the file operations that could destroy work and the view rotations are
        // off; Undo/Redo are the document's
        if let Some(id) = id
            && self.windows.get(&id).is_some_and(|w| w.markup.is_some())
        {
            use Action::*;
            match a {
                Rename | Delete | DeletePermanent | Paste | SaveFrameAs | Slideshow | RotateRight | RotateLeft
                | Mirror | Flip => return,
                Copy => return self.markup_copy(id),
                Undo | Redo => {
                    if let Some(m) = self.windows.get_mut(&id).and_then(|w| w.markup.as_mut()) {
                        if a == Undo { m.undo() } else { m.redo() }
                    }
                    if let Some(w) = self.windows.get_mut(&id) {
                        w.sync_markup_view(&self.settings);
                        w.refresh_title();
                        w.window.request_redraw();
                    }
                    return;
                }
                Markup => {
                    self.leave_markup(id);
                    return;
                }
                _ => {}
            }
        }
        let gpu = self.gpu.as_ref().unwrap();
        if let Some(w) = id.and_then(|id| self.windows.get_mut(&id))
            && w.apply(gpu, &self.settings, a)
        {
            return;
        }
        let nav = match a {
            Action::FirstFile => Some(Nav::First),
            Action::PreviousFile => Some(Nav::Previous),
            Action::NextFile => Some(Nav::Next),
            Action::LastFile => Some(Nav::Last),
            Action::RandomFile => Some(Nav::Random),
            _ => None,
        };
        if let (Some(nav), Some(id)) = (nav, id) {
            self.navigate(id, nav);
            return;
        }
        match a {
            Action::Open => {
                let mut dlg = rfd::FileDialog::new()
                    .set_title(i18n::plain(&actions::name(Action::Open)))
                    .add_filter(t("filter-images"), &dialog_extensions())
                    .add_filter(t("filter-all"), &["*"]);
                if let Some(w) = id.and_then(|id| self.windows.get(&id)) {
                    dlg = dlg.set_parent(w.window.as_ref());
                }
                // recents.lastFileDialogDir
                if let Some(dir) = self.state.last_file_dialog_dir.as_ref().filter(|d| d.is_dir()) {
                    dlg = dlg.set_directory(dir);
                }
                if let Some(paths) = dlg.pick_files() {
                    if let Some(dir) = paths.first().and_then(|p| p.parent()) {
                        self.state.last_file_dialog_dir = Some(dir.to_path_buf());
                        self.save_state();
                    }
                    self.open_paths(el, id, paths, false);
                }
            }
            Action::Markup => {
                if let Some(id) = id {
                    self.enter_markup(id, crate::markup::Tool::Select);
                }
            }
            Action::Crop => {
                if let Some(id) = id {
                    self.enter_markup(id, crate::markup::Tool::Crop);
                }
            }
            Action::SaveProject => {
                if let Some(id) = id {
                    self.save_project(id, false);
                }
            }
            Action::Export => {
                if let Some(id) = id {
                    self.open_export(id);
                }
            }
            Action::OverwriteOriginal => {
                if let Some(id) = id {
                    self.overwrite_original(id);
                }
            }
            Action::ReloadFile => {
                if let Some(id) = id
                    && self.leave_markup(id)
                    && let Some(p) = self.windows.get(&id).and_then(|w| w.path.clone())
                {
                    self.load(id, p, true, None);
                }
            }
            Action::CloseWindow => {
                if let Some(id) = id {
                    self.close_window(el, id);
                }
            }
            Action::Quit => {
                let ids: Vec<WindowId> = self.order.clone();
                if !ids.into_iter().all(|w| self.leave_markup(w)) {
                    return;
                }
                if let Some(id) = id {
                    self.remember_geometry(id);
                }
                el.exit();
            }
            Action::Slideshow => {
                let period = Duration::from_secs_f64(self.settings.slideshow_timer.max(0.05));
                // starts in full screen; leaving it (F11, Esc) keeps the slideshow running
                if let Some(w) = id.and_then(|id| self.windows.get_mut(&id)) {
                    w.slideshow_due = match w.slideshow_due {
                        Some(_) => None,
                        None => {
                            if !w.is_fullscreen() {
                                w.set_fullscreen(true, &self.settings);
                            }
                            Some(Instant::now() + period)
                        }
                    };
                }
            }
            Action::SaveFrameAs => {
                if let Some(id) = id {
                    self.save_frame_as(id);
                }
            }
            Action::Copy => {
                if let Some(id) = id {
                    self.copy(id);
                }
            }
            Action::Paste => self.paste(el, id),
            Action::OpenUrl => {
                if let Some(w) = id.and_then(|id| self.windows.get_mut(&id)) {
                    w.dialog = Some(Dialog::OpenUrl(ui::openurl::OpenUrl::new(String::new())));
                    w.window.request_redraw();
                }
            }
            Action::Rename => {
                if let Some(id) = id
                    && let Some(path) = self.image_path(id)
                {
                    let w = self.windows.get_mut(&id).unwrap();
                    w.dialog = Some(Dialog::Rename(ui::rename::Rename::new(&path)));
                    w.window.request_redraw();
                }
            }
            Action::Delete | Action::DeletePermanent => {
                if let Some(id) = id {
                    self.delete(id, a == Action::DeletePermanent);
                }
            }
            Action::Undo => {
                if let Some(id) = id {
                    self.undo(id);
                }
            }
            Action::OpenContainingFolder => {
                if let Some(id) = id
                    && let Some(path) = self.image_path(id)
                {
                    self.spawn(move || {
                        let _com = crate::platform::ComThread::init();
                        UserEvent::OpDone {
                            window: id,
                            title: "title-explorer",
                            counted: false,
                            result: crate::platform::show_in_explorer(&path),
                        }
                    });
                }
            }
            Action::ShowFileInfo => {
                if let Some(id) = id
                    && let Some(w) = self.windows.get_mut(&id)
                {
                    w.info_open = !w.info_open;
                    w.window.request_redraw();
                    self.refresh_info(id);
                }
            }
            Action::OpenWithOther => {
                if let Some(id) = id
                    && let Some(path) = self.image_path(id)
                {
                    let hwnd = crate::platform::hwnd(&self.windows[&id].window);
                    crate::platform::open_with_dialog(hwnd, &path);
                }
            }
            Action::Options => {
                let o = ui::options::Options::new(&self.settings, &self.state.shortcuts);
                self.open_tool(el, Content::Settings(o));
            }
            Action::Welcome => {
                let w = ui::welcome::Welcome {
                    update_notifications: self.settings.update_notifications,
                };
                self.open_tool(el, Content::Welcome(w));
            }
            Action::ClearRecents => {
                self.state.recents.clear();
                self.recents_changed();
            }
            _ => {}
        }
    }

    /// Close one window (asking first if its Markup document is unsaved); the last one exits.
    fn close_window(&mut self, el: &ActiveEventLoop, id: WindowId) {
        if !self.leave_markup(id) {
            return;
        }
        let Some(w) = self.windows.get_mut(&id) else { return };
        if let Some(c) = &w.download_cancel {
            c.store(true, Ordering::Relaxed);
        }
        self.remember_geometry(id);
        self.loader.cancel_client(client(id));
        self.mru.retain(|x| *x != id);
        self.windows.remove(&id);
        self.order.retain(|x| *x != id);
        if self.windows.is_empty() {
            el.exit();
        }
    }

    /// Save Frame As: PNG of the current frame, `<base>-<n>.png`, written atomically
    /// on a worker thread.
    fn save_frame_as(&mut self, id: WindowId) {
        let Some(w) = self.windows.get(&id) else { return };
        let (Some(bytes), Some(p), Some(info), Some(path)) = (&w.anim_bytes, &w.player, &w.info, &w.path) else {
            return; // NeedsAnimation
        };
        let n = p.index();
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut dlg = rfd::FileDialog::new()
            .set_title(i18n::plain(&actions::name(Action::SaveFrameAs)))
            .set_file_name(format!("{stem}-{n}.png"))
            .add_filter("PNG", &["png"])
            .set_parent(w.window.as_ref());
        if let Some(dir) = path.parent() {
            dlg = dlg.set_directory(dir);
        }
        let Some(dst) = dlg.save_file() else { return };
        let (bytes, format, proxy) = (bytes.clone(), info.format, self.proxy.clone());
        std::thread::spawn(move || {
            let r = rv_core::anim::frame_png(&bytes, format, n).and_then(|png| {
                let verify = |b: &[u8]| rv_core::decode::probe(b, None).is_some();
                rv_core::atomic::write_atomic(&dst, &png, verify).map_err(|e| e.to_string())
            });
            let _ = proxy.send_event(UserEvent::Saved(r));
        });
    }

    fn on_loaded(&mut self, o: Outcome) {
        log::debug!("result for generation {} reached the UI thread", o.generation);
        let Some(id) = self.windows.keys().copied().find(|k| client(*k) == o.client) else {
            return;
        };
        let looping = self.settings.loop_folders_enabled;
        let (Some(w), Some(gpu)) = (self.windows.get_mut(&id), &self.gpu) else {
            return;
        };
        if w.generation != o.generation {
            return; // obsolete result
        }
        match o.result {
            Ok(img) => {
                w.show(
                    gpu,
                    &self.settings,
                    o.generation,
                    &img,
                    self.view_space,
                    &self.anim_wake,
                );
                if !img.preview {
                    self.add_recent(&o.path);
                    self.refresh_info(id);
                }
            }
            Err(e) if e.not_found && w.nav_dir.is_some() && w.folder.is_some() => {
                // Vanished during navigation: drop it from the list and keep going.
                let dir = w.nav_dir.unwrap_or(1);
                let f = w.folder.as_mut().unwrap();
                let i = w.index.unwrap_or(0);
                f.remove(&o.path);
                let virtual_cur = if dir > 0 { i.checked_sub(1) } else { Some(i) };
                let nav = if dir > 0 { Nav::Next } else { Nav::Previous };
                match folder::navigate(f.files.len(), virtual_cur, nav, looping, &mut w.rng) {
                    Some(t) => {
                        let path = f.files[t].clone();
                        w.index = Some(t);
                        self.load(id, path, false, Some(dir));
                    }
                    None => w.fail(&o.path, &e),
                }
            }
            Err(e) => w.fail(&o.path, &e),
        }
    }

    fn on_scanned(&mut self, id: WindowId, generation: u64, f: Folder, path: PathBuf, index: Option<usize>) {
        let Some(w) = self.windows.get_mut(&id) else { return };
        if w.scan_generation != generation {
            return;
        }
        w.scanning = false;
        w.index = match &w.path {
            Some(cur) if folder::same_path_ordinal(cur, &path) => index,
            Some(cur) => f.index_of_fast(cur),
            None => None,
        };
        w.folder = Some(f);
        w.refresh_title(); // i/N is known now (TitleBarMode 2/3)
        let generation = w.generation;
        let preloads = self.preloads(&self.windows[&id]);
        self.loader.set_preloads(client(id), generation, preloads);
    }

    fn redraw(&mut self, el: &ActiveEventLoop, id: WindowId) {
        let (Some(w), Some(gpu)) = (self.windows.get_mut(&id), &self.gpu) else {
            return;
        };
        let first = !w.presented;
        let had_image = w.shown_image;
        w.redraw(gpu, &self.settings);
        if w.menus.is_none() {
            // Native menus after the first frame: they are not needed for it (measured ~6 ms earlier frame).
            let menus = Menus::build(&self.shortcuts);
            if let Some(h) = crate::platform::hwnd(&w.window) {
                let before = w.window.inner_size().height;
                menus.attach(h.0 as isize, self.settings.menubar_enabled && !w.is_fullscreen());
                // the bar takes its height from the client area: the window grows by it instead
                let taken = before.saturating_sub(w.window.inner_size().height);
                if taken > 0
                    && !w.placement_restored
                    && !w.window.is_maximized()
                    && let (Ok(pos), size) = (w.window.outer_position(), w.window.outer_size())
                {
                    let r = crate::layout::Rect {
                        x: pos.x as f64,
                        y: pos.y as f64,
                        w: size.width as f64,
                        h: (size.height + taken) as f64,
                    };
                    crate::platform::set_outer_rect(&w.window, &r);
                }
            }
            w.menus = Some(menus);
            let names: Vec<String> = self.state.recents.iter().map(|r| r.name.clone()).collect();
            w.menus.as_mut().unwrap().set_recents(&names);
        }
        // Outcomes of the frame (dialog OK, Markup Done) run after it: a modal dialog opened while
        // the window is painting never shows and the app would hang.
        if w.dialog_outcome.is_some() || w.markup_out.is_some() {
            let _ = self.proxy.send_event(UserEvent::FrameOutcome(id));
        }
        // the Inspector may have changed the document
        w.refresh_title();
        if self.cfg.measure_startup {
            let ms = self.started.elapsed().as_secs_f64() * 1000.0;
            if first {
                println!("first frame presented after {ms:.1} ms");
            }
            if !had_image && w.image.is_some() {
                w.shown_image = true;
                println!("image presented after {ms:.1} ms");
            }
            // Without a file: stop at the first frame; with a file: once fully loaded (or failed).
            if w.path.is_none() || (!w.loading && (w.shown_image || w.error.is_some())) {
                if w.shown_image {
                    println!("full image presented after {ms:.1} ms");
                }
                el.exit();
            }
        }
    }

    fn observe(&self) -> Observation {
        let w = self.current().and_then(|id| self.windows.get(&id));
        Observation {
            titles: self
                .order
                .iter()
                .filter_map(|id| self.windows.get(id))
                .map(|w| w.title())
                .collect(),
            current: self.current().and_then(|c| self.order.iter().position(|x| *x == c)),
            slideshow: w.is_some_and(|w| w.slideshow_due.is_some()),
            fullscreen: w.is_some_and(|w| w.is_fullscreen()),
            inner_size: w.map(|w| (w.window.inner_size().width, w.window.inner_size().height)),
            pan: w.map(|w| w.view.pan).unwrap_or_default(),
            images_shown: w.map(|w| w.images_shown).unwrap_or(0),
            outer_rect: w.and_then(|w| {
                let p = w.window.outer_position().ok()?;
                let s = w.window.outer_size();
                Some((p.x, p.y, s.width, s.height))
            }),
            menu: w
                .and_then(|w| w.menus.as_ref())
                .map(|m| {
                    m.snapshot()
                        .into_iter()
                        .map(|(a, e, t)| (actions::info(a).key, e, t))
                        .collect()
                })
                .unwrap_or_default(),
            path: w.and_then(|w| w.path.clone()),
            image_size: w.and_then(|w| w.image_size),
            error: w.and_then(|w| w.error.clone()),
            title: w.map(|w| w.title()).unwrap_or_default(),
            scale: w.map(|w| w.view.scale).unwrap_or(0.0),
            viewport: w.map(|w| w.viewport()).unwrap_or_default(),
            windows: self.windows.len(),
            index: w.and_then(|w| w.index),
            folder_len: w.and_then(|w| w.folder.as_ref().map(|f| f.files.len())),
            generation: w.map(|w| w.generation).unwrap_or(0),
            shown_generation: w.map(|w| w.shown_generation).unwrap_or(0),
            stale_shown: w.map(|w| w.stale_shown).unwrap_or(0),
            first_latency: w.and_then(|w| w.first_latency),
            full_latency: w.and_then(|w| w.full_latency),
            showing_preview: w.is_some_and(|w| w.showing_preview),
            profile: w.and_then(|w| w.info.as_ref().map(|i| i.profile.clone())),
            anim: w
                .and_then(|w| w.player.as_ref())
                .map(|p| (p.index(), p.shown, p.paused, p.speed)),
            dialog: w.and_then(|w| w.dialog.as_ref()).map(|d| d.describe()),
            message: self.last_message.clone(),
            has_undo: w.is_some_and(|w| !w.undo.is_empty()),
            info_rows: w
                .filter(|w| w.info_open)
                .map(|w| w.info_rows.clone())
                .unwrap_or_default(),
            recents: self.state.recents.iter().map(|r| r.name.clone()).collect(),
            recent_menu: w
                .and_then(|w| w.menus.as_ref())
                .map(|m| m.dynamic_texts().0)
                .unwrap_or_default(),
            open_with_menu: w
                .and_then(|w| w.menus.as_ref())
                .map(|m| m.dynamic_texts().1)
                .unwrap_or_default(),
            tools: self.tools.values().map(|t| t.content.kind()).collect(),
            settings: self.settings.clone(),
            language: i18n::current(),
            update_status: self.update_status.text(),
            opened_url: self.opened_url.clone(),
            first_launch: self.state.first_launch,
            screen_from_image: w.map(|w| w.view.screen_from_image().m).unwrap_or_default(),
            texture: w.and_then(|w| w.image.as_ref()).map(|i| (i.w, i.h)),
            markup: w.and_then(|w| w.markup.as_ref()).map(|m| crate::driver::MarkupObs {
                tool: m.tool.name(),
                selection: m.sel.ids.iter().map(|i| i.0).collect(),
                doc: m.doc().clone(),
                dirty: m.history.is_dirty(),
                can_undo: m.history.can_undo(),
                can_redo: m.history.can_redo(),
                crop_draft: m.crop.as_ref().map(|c| c.rect),
                editing: m.editing.as_ref().map(|e| e.id.0),
                inspector: w.and_then(|w| w.egui_area_title("markup inspector")),
                source: m.source.clone(),
                project_path: m.project_path.clone(),
                exported: m.history.exported_current(),
            }),
            software_adapter: self
                .gpu
                .as_ref()
                .is_some_and(|g| g.adapter.get_info().device_type == wgpu::DeviceType::Cpu),
        }
    }

    fn busy(&self) -> bool {
        self.windows.values().any(|w| {
            w.loading
                || w.scanning
                || w.awaiting_present
                || !w.presented
                || w.pending_ops > 0
                || w.svg_pending
                || w.svg_want.is_some()
        }) || self.tools.values().any(|t| !t.presented)
            || self.pending_update
    }

    fn run_driver(&mut self, el: &ActiveEventLoop) {
        let Some(mut d) = self.driver.take() else { return };
        while let Some(step) = d.steps.pop_front() {
            log::trace!("driver: {} steps left", d.steps.len());
            match step {
                Step::WaitIdle => {
                    if self.busy() {
                        d.steps.push_front(Step::WaitIdle);
                        self.driver = Some(d);
                        return;
                    }
                }
                Step::Sleep(dur) => {
                    let until = *d.resume_at.get_or_insert(Instant::now() + dur);
                    if Instant::now() < until {
                        d.steps.push_front(Step::Sleep(dur));
                        self.driver = Some(d);
                        return;
                    }
                    d.resume_at = None;
                }
                Step::Until(done, timeout) => {
                    if !done() {
                        let started = d.until_started.get_or_insert_with(Instant::now);
                        if started.elapsed() < timeout {
                            d.resume_at = Some(Instant::now() + Duration::from_millis(20));
                            d.steps.push_front(Step::Until(done, timeout));
                            self.driver = Some(d);
                            return;
                        }
                        d.failures
                            .push(format!("timed out after {timeout:?} waiting for an external step"));
                    }
                    d.until_started = None;
                    d.resume_at = None;
                }
                Step::Do(a) => self.dispatch(el, None, a),
                Step::SavePath(p) => self.next_save_path = Some(p),
                Step::Answer(a) => self.driver_answer = Some(a),
                Step::Focus(i) => {
                    if let Some(id) = self.order.get(i).copied() {
                        self.touch(id);
                    }
                }
                Step::Key(seq) => {
                    if let Some(id) = self.current() {
                        self.on_key(el, id, &seq);
                    }
                }
                Step::Open(p) => {
                    let id = self.current();
                    self.open_paths(el, id, vec![p], false);
                }
                Step::Event(e) => {
                    if let Some(id) = self.current() {
                        self.window_event(el, id, e);
                    }
                }
                Step::MoveTo(f) => {
                    let (x, y) = f(&self.observe());
                    if let Some(id) = self.current() {
                        // SAFETY: no preconditions (an `unsafe fn` only in older winit versions);
                        // the id just tags a synthetic driver event.
                        #[allow(unused_unsafe)]
                        let device_id = unsafe { winit::event::DeviceId::dummy() };
                        let position = winit::dpi::PhysicalPosition::new(x as f64, y as f64);
                        self.window_event(el, id, WindowEvent::CursorMoved { device_id, position });
                    }
                }
                Step::Check(f) => {
                    if let Err(e) = f(&self.observe()) {
                        d.failures.push(e);
                    }
                }
                Step::DialogText(t) => {
                    if let Some(d) = self.current().and_then(|id| self.windows.get_mut(&id)?.dialog.as_mut()) {
                        d.set_text(&t);
                    }
                }
                Step::ToolButton(kind, button) => {
                    if let Some(id) = self.tool_of(kind) {
                        let outs = self.tools.get_mut(&id).unwrap().content.press(button);
                        self.on_tool_outs(el, id, outs);
                    }
                }
                Step::SettingsEdit(f) => {
                    if let Some(id) = self.tool_of("settings")
                        && let Content::Settings(o) = &mut self.tools.get_mut(&id).unwrap().content
                    {
                        f(&mut o.draft, &mut o.keys);
                    }
                }
                Step::MoveToImage(x, y) => {
                    if let Some(id) = self.current()
                        && let Some(w) = self.windows.get(&id)
                    {
                        let (sx, sy) = w.view.screen_from_image().apply(x, y);
                        // SAFETY: no preconditions (an `unsafe fn` only in older winit versions);
                        // the id just tags a synthetic driver event.
                        #[allow(unused_unsafe)]
                        let device_id = unsafe { winit::event::DeviceId::dummy() };
                        let e = WindowEvent::CursorMoved {
                            device_id,
                            position: winit::dpi::PhysicalPosition::new(sx as f64, sy as f64),
                        };
                        self.window_event(el, id, e);
                    }
                }
                Step::MarkupButton(name) => {
                    if let Some(id) = self.current() {
                        let out = self
                            .windows
                            .get_mut(&id)
                            .and_then(|w| w.markup.as_mut())
                            .map(|m| m.button(name));
                        if let Some(w) = self.windows.get_mut(&id) {
                            w.sync_markup_view(&self.settings);
                            w.refresh_title();
                            w.window.request_redraw();
                        }
                        match out {
                            Some(crate::markup::Out::Exit) => {
                                self.leave_markup(id);
                            }
                            Some(crate::markup::Out::Do(a)) => self.dispatch(el, Some(id), a),
                            _ => {}
                        }
                    }
                }
                Step::MarkupStyle(f) => {
                    if let Some(m) = self
                        .current()
                        .and_then(|id| self.windows.get_mut(&id))
                        .and_then(|w| w.markup.as_mut())
                    {
                        m.set_style(&*f);
                    }
                }
                Step::TypeText(text) => {
                    if let Some(w) = self.current().and_then(|id| self.windows.get_mut(&id))
                        && let Some(m) = w.markup.as_mut()
                    {
                        m.text_cmd(crate::markup::textedit::Cmd::Insert(text), false);
                        w.refresh_title();
                        w.window.request_redraw();
                    }
                }
                Step::MarkupRedact(f) => {
                    if let Some(w) = self.current().and_then(|id| self.windows.get_mut(&id))
                        && let Some(m) = w.markup.as_mut()
                    {
                        m.set_redaction(&*f);
                        w.window.request_redraw();
                    }
                }
                Step::DialogAccept => self.dialog_button(true),
                Step::DialogCancel => self.dialog_button(false),
                Step::Snapshot(f) => {
                    let w = self.current().and_then(|id| self.windows.get_mut(&id));
                    if let (Some(w), Some(gpu)) = (w, &self.gpu) {
                        let (sw, sh, px) = w.snapshot(gpu, &self.settings);
                        if let Err(e) = f(sw, sh, &px) {
                            d.failures.push(e);
                        }
                    }
                }
            }
        }
        self.failures = d.failures;
        el.exit();
    }

    /// Pressed key sequence → action through the shortcut table and the window's enable state.
    fn on_key(&mut self, el: &ActiveEventLoop, id: WindowId, seq: &str) {
        let Some(w) = self.windows.get(&id) else { return };
        if w.dialog.is_some() {
            return; // keys the dialog's text field didn't take
        }
        if w.markup.is_some() && self.markup_key(id, seq) {
            return;
        }
        let Some(w) = self.windows.get(&id) else { return };
        if let Some(a) = self.shortcuts.lookup(seq, &w.enable_state()) {
            self.dispatch(el, Some(id), a);
        } else if seq == "Esc" && w.is_fullscreen() && !self.shortcuts.is_bound("Esc") {
            w.set_fullscreen(false, &self.settings); // Esc leaves fullscreen unless used elsewhere
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if !self.windows.is_empty() {
            return;
        }
        crate::platform::init_com_ui();
        let proxy = self.proxy.clone();
        muda::MenuEvent::set_event_handler(Some(move |e: muda::MenuEvent| {
            let _ = proxy.send_event(UserEvent::Menu(e.id));
        }));
        let largest = el
            .available_monitors()
            .map(|m| m.size().width.max(m.size().height))
            .max()
            .unwrap_or(2048);
        let mut opts = loader::Options::default();
        opts.decode.svg_target = largest;
        self.decode_opts = opts.decode;
        opts.view_space = self.view_space;
        self.loader.set_options(opts);
        let files = std::mem::take(&mut self.cfg.files);
        let result = if files.is_empty() {
            self.new_window(el, None, false).map(|_| ())
        } else {
            self.open_paths(el, None, files, true);
            Ok(())
        };
        if let Err(e) = result {
            log::error!("cannot create window: {e:#}");
            self.failures.push(format!("{e:#}"));
            el.exit();
            return;
        }
        // Welcome at the first start (only with a real state file: not in tests)
        if self.state_path.is_some() && self.state.first_launch && !self.cfg.measure_startup {
            let w = ui::welcome::Welcome {
                update_notifications: self.settings.update_notifications,
            };
            self.open_tool(el, Content::Welcome(w));
        } else {
            self.maybe_check_updates();
        }
    }

    fn user_event(&mut self, el: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Loaded(o) => self.on_loaded(o),
            UserEvent::Scanned {
                window,
                generation,
                folder,
                path,
                index,
            } => self.on_scanned(window, generation, folder, path, index),
            UserEvent::Menu(id) => {
                // menus belong to the active window (the one clicked): MRU front
                if let Some(a) = actions::from_key(id.as_ref()) {
                    self.dispatch(el, None, a);
                } else if let Some((recent, i)) = crate::menus::parse_dynamic(id.as_ref()) {
                    match (recent, self.current()) {
                        (true, _) => self.open_recent(el, i),
                        (false, Some(w)) => self.open_with_app(w, i),
                        _ => {}
                    }
                }
            }
            UserEvent::AccessKit(e) => {
                if let Some(w) = self.windows.get_mut(&e.window_id) {
                    w.on_accesskit(e.window_event);
                } else if let Some(t) = self.tools.get_mut(&e.window_id) {
                    t.on_accesskit(e.window_event);
                }
            }
            UserEvent::UpdateChecked(r) => self.on_update_checked(el, r),
            UserEvent::RecentsMissing(paths) => {
                if self.state.forget_recents(&paths) {
                    self.recents_changed();
                }
            }
            UserEvent::MarkupSource { window, result } => self.on_markup_source(window, result),
            UserEvent::SvgRaster {
                window,
                generation,
                result,
            } => {
                if let (Some(w), Some(gpu)) = (self.windows.get_mut(&window), &self.gpu) {
                    match result {
                        Ok((rw, rh, rgba)) if w.generation == generation => {
                            w.show_svg_raster(gpu, &self.settings, rw, rh, &rgba)
                        }
                        Ok(_) => w.svg_pending = false,
                        Err(e) => {
                            w.svg_pending = false;
                            log::warn!("SVG raster: {e}");
                        }
                    }
                }
            }
            UserEvent::MarkupWritten {
                window,
                kind,
                path,
                revision,
                result,
            } => {
                if let Some(w) = self.windows.get_mut(&window) {
                    w.pending_ops = w.pending_ops.saturating_sub(1);
                }
                self.on_markup_written(window, kind, path, revision, result);
            }
            UserEvent::FrameOutcome(id) => {
                let Some(w) = self.windows.get_mut(&id) else { return };
                let (dialog, markup) = (w.dialog_outcome.take(), w.markup_out.take());
                if let Some(o) = dialog {
                    self.on_dialog(id, o);
                }
                match markup {
                    Some(crate::markup::Out::Exit) => {
                        self.leave_markup(id);
                    }
                    Some(crate::markup::Out::Do(a)) => self.dispatch(el, Some(id), a),
                    _ => {}
                }
            }
            UserEvent::AnimWake => {} // about_to_wait advances the players
            UserEvent::Saved(Err(e)) => self.message(self.current(), "title-saveframe", e),
            UserEvent::Saved(Ok(())) => {}
            UserEvent::Deleted {
                window,
                path,
                permanent,
                result,
            } => self.on_deleted(window, path, permanent, result),
            UserEvent::Restored { window, name, result } => self.on_restored(window, name, result),
            UserEvent::DownloadProgress {
                window,
                received,
                total,
            } => {
                if let Some(w) = self.windows.get_mut(&window)
                    && let Some(Dialog::Download(p)) = &mut w.dialog
                {
                    p.received = received;
                    p.total = total;
                    w.window.request_redraw();
                }
            }
            UserEvent::Downloaded { window, url, result } => self.on_downloaded(el, window, url, result),
            UserEvent::OpenWithList { window, path, apps } => self.on_open_with_list(window, path, apps),
            UserEvent::OpDone {
                window,
                title,
                counted,
                result,
            } => {
                if counted && let Some(w) = self.windows.get_mut(&window) {
                    w.pending_ops = w.pending_ops.saturating_sub(1);
                }
                if let Err(e) = result {
                    self.message(Some(window), title, e);
                }
            }
        }
        self.run_driver(el);
    }

    /// Wake-ups: animation frames, the loading indicator after 150 ms (then animated), driver sleeps.
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        self.run_driver(el);
        self.spawn_svg_rasters();
        let now = Instant::now();
        let mut next: Option<Instant> = self.driver.as_ref().and_then(|d| d.resume_at);
        if let Some(gpu) = &self.gpu {
            for w in self.windows.values_mut() {
                if let Some(due) = w.anim_tick(gpu, now) {
                    next = Some(next.map_or(due, |n| n.min(due)));
                }
            }
        }
        let due: Vec<WindowId> = self
            .windows
            .iter()
            .filter(|(_, w)| w.slideshow_due.is_some_and(|d| d <= now))
            .map(|(id, _)| *id)
            .collect();
        for id in due {
            self.slideshow_step(id, now);
        }
        for w in self.windows.values_mut() {
            if let Some(d) = w.slideshow_due {
                next = Some(next.map_or(d, |n| n.min(d)));
            }
            w.update_menus();
        }
        let due: Vec<WindowId> = self
            .windows
            .iter()
            .filter(|(_, w)| w.open_with_due.is_some_and(|d| d <= now))
            .map(|(id, _)| *id)
            .collect();
        for id in due {
            self.windows.get_mut(&id).unwrap().open_with_due = None;
            self.spawn_open_with(id);
        }
        let open_info: Vec<WindowId> = self
            .windows
            .iter()
            .filter(|(_, w)| w.info_open)
            .map(|(id, _)| *id)
            .collect();
        for id in open_info {
            self.refresh_info(id);
        }
        for w in self.windows.values_mut() {
            for at in [w.open_with_due, w.repaint_at].into_iter().flatten() {
                next = Some(next.map_or(at, |n| n.min(at)));
            }
            if w.repaint_at.is_some_and(|t| t <= now) {
                w.repaint_at = None;
                w.window.request_redraw();
            }
        }
        for t in self.tools.values_mut() {
            if let Some(at) = t.repaint_at {
                if at <= now {
                    t.repaint_at = None;
                    t.window.request_redraw();
                } else {
                    next = Some(next.map_or(at, |n| n.min(at)));
                }
            }
        }
        for w in self.windows.values() {
            if let Some(t) = w.loading_since.filter(|_| w.loading) {
                let show_at = t + INDICATOR_AFTER;
                let at = if now >= show_at {
                    w.window.request_redraw();
                    now + Duration::from_millis(100)
                } else {
                    show_at
                };
                next = Some(next.map_or(at, |n| n.min(at)));
            }
        }
        // winit leaves the loop on its next turn: once exit is asked for (the driver's end), waiting
        // for another message could block for ever (seen as an intermittent hang in e2e_tiling)
        el.set_control_flow(if el.exiting() {
            ControlFlow::Poll
        } else {
            next.map_or(ControlFlow::Wait, ControlFlow::WaitUntil)
        });
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self.tools.contains_key(&id) {
            self.tool_event(el, id, event);
            return;
        }
        if let WindowEvent::RedrawRequested = event {
            self.redraw(el, id);
            self.wake_driver();
            return;
        }
        let (Some(w), Some(gpu)) = (self.windows.get_mut(&id), &self.gpu) else {
            return;
        };
        match w.on_event(gpu, &self.settings, &event) {
            Request::None => {}
            Request::Action(a) => self.dispatch(el, Some(id), a),
            Request::Key(seq) => self.on_key(el, id, &seq),
            Request::Open(path, new) => self.open_paths(el, (!new).then_some(id), vec![path], false),
            Request::ContextMenu(x, y) => {
                w.update_menus();
                if let (Some(m), Some(h)) = (&w.menus, crate::platform::hwnd(&w.window)) {
                    m.show_context(h.0 as isize, x, y);
                }
            }
            Request::Focused => {
                self.touch(id);
                self.check_settings_file();
            }
            Request::Close => self.close_window(el, id),
        }
    }
}
