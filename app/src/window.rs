//! One viewer window: surface, egui, current image, view state and input mapping.

use crate::actions::{Action, EnableState};
use crate::layout::{self, FitInput};
use crate::menus::{Menus, Toggles};
use crate::render::{self, Globals, Gpu, GpuImage};
use crate::ui;
use crate::viewer::{View, step_factor};
use rv_core::anim::Player;
use rv_core::color::ViewSpace;
use rv_core::folder::{Folder, Rng};
use rv_core::loader::{ImageInfo, LoadError, ViewImage};
use rv_core::settings::{ScrollZoom, Settings, TitleBarMode};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::ModifiersState;
use winit::window::{CursorIcon, Fullscreen, Window};

pub struct ViewerWindow {
    pub window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    globals: Globals,
    egui: egui_winit::State,
    /// None while its shader compiles on a worker thread (`egui_pending`).
    egui_renderer: Option<egui_wgpu::Renderer>,
    egui_pending: Option<std::sync::mpsc::Receiver<egui_wgpu::Renderer>>,
    egui_format: wgpu::TextureFormat,
    pub image: Option<GpuImage>,
    pub path: Option<PathBuf>,
    pub image_size: Option<(u32, u32)>,
    pub error: Option<String>,
    pub view: View,
    pub generation: u64,
    pub loading: bool,
    pub presented: bool,
    /// An image frame has been presented (startup measurement).
    pub shown_image: bool,
    // --- folder navigation ---
    pub folder: Option<Folder>,
    pub index: Option<usize>,
    pub last_load: Option<Instant>,
    /// Direction of the navigation that started the current load (None = opened directly).
    pub nav_dir: Option<i8>,
    /// Last direction of travel, +1 / −1 (preload order).
    pub direction: i8,
    pub scan_generation: u64,
    pub scanning: bool,
    pub rng: Rng,
    // --- loading state ---
    pub loading_since: Option<Instant>,
    /// Generation of the image on screen; a full image replaces a preview of the same generation.
    pub shown_generation: u64,
    pub showing_preview: bool,
    /// Results older than the one on screen that reached `show` (must stay 0).
    pub stale_shown: usize,
    pub first_latency: Option<Duration>,
    pub full_latency: Option<Duration>,
    // --- current image facts and animation ---
    pub info: Option<ImageInfo>,
    pub player: Option<Player>,
    /// Bytes of the animated file on screen (Save Frame As decodes the frame again from them).
    pub anim_bytes: Option<Arc<[u8]>>,
    /// A new image was set and its frame is not on screen yet (latency measured at present).
    pub awaiting_present: bool,
    /// Full images shown since the window opened (tests: what navigation actually displayed).
    pub images_shown: u64,
    cursor: Option<(f32, f32)>,
    dragging: Option<(f32, f32)>,
    /// Color Picker reads this window: crosshair over the image, a click picks the pixel.
    picking: bool,
    mods: ModifiersState,
    /// First click of a possible double click: time and position (cleared by a drag).
    last_click: Option<(Instant, (f32, f32))>,
    /// Files dropped in the current drag&drop: the first opens here, the others in new windows.
    dropped: usize,
    drop_done: bool,
    // --- window behaviour ---
    pub title_mode: TitleBarMode,
    /// This window's native menus (enabled state depends on its image).
    pub menus: Option<Menus>,
    /// Resize to the next image (WindowResizeMode "when launching": first image of a launch window).
    pub resize_pending: bool,
    /// The outer rectangle comes from the saved placement (menu bar included): kept as it is.
    pub placement_restored: bool,
    /// Next slideshow step (None = no slideshow).
    pub slideshow_due: Option<Instant>,
    // --- file operations ---
    /// The modal dialog (Rename, Open URL, download progress).
    pub dialog: Option<ui::Dialog>,
    /// Set by the frame in which the dialog closed; the app runs it.
    pub dialog_outcome: Option<ui::Outcome>,
    /// File Info window open, and its rows (recomputed by the app).
    pub info_open: bool,
    pub info_rows: Vec<ui::info::Row>,
    /// (path, size and times) read for the Info rows.
    pub info_file: Option<(PathBuf, Option<ui::info::FileFacts>)>,
    /// The file's details (read on a worker) and whether they are still being read.
    pub info_details: Option<(PathBuf, Arc<rv_core::details::Details>)>,
    pub info_pending: bool,
    /// Undo Delete stack: (original path, Recycle Bin item id).
    /// Undo Delete stack: (file, Recycle Bin item, true = the backup of an Overwrite Original).
    pub undo: Vec<(PathBuf, std::ffi::OsString, bool)>,
    /// Background file operations still running (delete, restore, copy, download): WaitIdle.
    pub pending_ops: usize,
    /// Cancel flag of the running download.
    pub download_cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
    /// Open With: when to enumerate the apps (250 ms after the file changed), and their names.
    pub open_with_due: Option<Instant>,
    pub open_with: Vec<String>,
    /// egui asked for a repaint at this time (text cursor, animations of the dialogs).
    pub repaint_at: Option<Instant>,
    // --- Markup ---
    pub markup: Option<crate::markup::Markup>,
    /// Set by the frame in which the Markup UI asked to leave (Done); the app runs it.
    pub markup_out: Option<crate::markup::Out>,
    /// SVG: long side (texels) the current zoom wants, for a sharper raster; taken by the app.
    pub svg_want: Option<u32>,
    /// An SVG re-raster is running for this window.
    pub svg_pending: bool,
    /// (document orientation, shown region) the view was last set up for.
    markup_view: Option<(rv_core::orient::Orient, Option<rv_core::edit::RectU>)>,
    /// The input method is on (a Markup text box is being edited).
    ime: bool,
    /// Middle-button pan in Markup (the left button belongs to the tools).
    panning: Option<(f32, f32)>,
    /// View colour space: the preview converts the shapes' sRGB colours into it.
    pub view_space: ViewSpace,
}

/// What the window wants the app to do after an input event.
pub enum Request {
    None,
    Action(Action),
    /// A key sequence ("Ctrl+O"), resolved by the app.
    Key(String),
    /// The window became active (MRU).
    Focused,
    /// Open in this window (false) or in a new one (true).
    Open(PathBuf, bool),
    ContextMenu(f64, f64),
    /// Color picking: a click at this window point (physical px).
    PickColor(f32, f32),
    Close,
}

impl ViewerWindow {
    /// egui-winit state without GPU: AccessKit must exist before the window is first shown.
    pub fn ui_state(
        el: &winit::event_loop::ActiveEventLoop,
        window: &Window,
        proxy: winit::event_loop::EventLoopProxy<crate::app::UserEvent>,
    ) -> egui_winit::State {
        let mut st = egui_winit::State::new(
            egui::Context::default(),
            egui::ViewportId::ROOT,
            window,
            Some(window.scale_factor() as f32),
            None,
            None,
        );
        st.init_accesskit(el, window, proxy);
        ui::style::apply(st.egui_ctx(), true, true);
        st
    }

    pub fn new(
        gpu: &Gpu,
        window: Arc<Window>,
        surface: wgpu::Surface<'static>,
        mut egui: egui_winit::State,
    ) -> ViewerWindow {
        let size = window.inner_size();
        let caps = surface.get_capabilities(&gpu.adapter);
        // Non-sRGB surface: the image shader encodes sRGB itself, egui-wgpu expects this.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| *f == render::SURFACE_FORMAT)
            .or_else(|| caps.formats.iter().copied().find(|f| !f.is_srgb()))
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            // SDR sRGB swapchain.
            color_space: wgpu::SurfaceColorSpace::Srgb,
        };
        surface.configure(&gpu.device, &config);
        egui.set_max_texture_side(gpu.max_tex as usize);
        // The egui renderer costs ~45 ms of shader compilation (measured), on the critical path
        // of the first frame: built on a worker thread; frames before it is ready have no UI (the
        // image does not need it), then the thread asks for a redraw.
        let (tx, rx) = std::sync::mpsc::channel();
        let (device, win) = (gpu.device.clone(), window.clone());
        std::thread::spawn(move || {
            let _ = tx.send(egui_wgpu::Renderer::new(&device, format, Default::default()));
            win.request_redraw();
        });
        let mut view = View::new();
        view.set_viewport(config.width, config.height, &Settings::default());
        ViewerWindow {
            pipeline: gpu.image_pipeline(format),
            globals: gpu.globals(),
            window,
            surface,
            config,
            egui,
            egui_renderer: None,
            egui_pending: Some(rx),
            egui_format: format,
            image: None,
            path: None,
            image_size: None,
            error: None,
            view,
            generation: 0,
            loading: false,
            presented: false,
            shown_image: false,
            folder: None,
            index: None,
            last_load: None,
            nav_dir: None,
            direction: 1,
            scan_generation: 0,
            scanning: false,
            rng: Rng::from_entropy(),
            loading_since: None,
            shown_generation: 0,
            showing_preview: false,
            stale_shown: 0,
            first_latency: None,
            full_latency: None,
            info: None,
            player: None,
            anim_bytes: None,
            awaiting_present: false,
            images_shown: 0,
            cursor: None,
            dragging: None,
            picking: false,
            mods: ModifiersState::empty(),
            last_click: None,
            dropped: 0,
            drop_done: false,
            title_mode: TitleBarMode::Full,
            menus: None,
            resize_pending: false,
            placement_restored: false,
            slideshow_due: None,
            dialog: None,
            dialog_outcome: None,
            info_open: false,
            info_rows: Vec::new(),
            info_file: None,
            info_details: None,
            info_pending: false,
            undo: Vec::new(),
            pending_ops: 0,
            download_cancel: None,
            open_with_due: None,
            open_with: Vec::new(),
            repaint_at: None,
            markup: None,
            markup_out: None,
            svg_want: None,
            svg_pending: false,
            markup_view: None,
            ime: false,
            panning: None,
            view_space: ViewSpace::Srgb,
        }
    }

    pub fn on_accesskit(&mut self, e: egui_winit::accesskit_winit::WindowEvent) {
        use egui_winit::accesskit_winit::WindowEvent as E;
        match e {
            E::InitialTreeRequested => self.egui.egui_ctx().enable_accesskit(),
            E::ActionRequested(r) => self.egui.on_accesskit_action_request(r),
            E::AccessibilityDeactivated => self.egui.egui_ctx().disable_accesskit(),
        }
        self.window.request_redraw();
    }

    /// Reduced previews only need to cover the viewport.
    pub fn preview_side(&self) -> u32 {
        self.config.width.max(self.config.height)
    }

    pub fn viewport(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    pub fn title(&self) -> String {
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy());
        let t = layout::title(
            self.title_mode,
            name.as_deref(),
            self.info.as_ref().map(|i| i.file_size),
        );
        // "• " while the Markup document is not saved as a project
        if self.markup.as_ref().is_some_and(|m| m.history.is_dirty()) {
            format!("• {t}")
        } else {
            t
        }
    }

    pub fn refresh_title(&self) {
        self.window.set_title(&self.title());
    }

    pub fn is_fullscreen(&self) -> bool {
        self.window.fullscreen().is_some()
    }

    /// A point (physical px) in the title bar of the egui window with this id, as laid out in the
    /// last frame: on the panel, on none of its controls.
    pub fn egui_area_title(&self, id: &str) -> Option<(f32, f32)> {
        let ctx = self.egui.egui_ctx();
        let r = ctx.memory(|m| m.area_rect(egui::Id::unique(id)))?;
        let p = egui::pos2(r.center().x, r.top() + 12.0) * ctx.pixels_per_point();
        Some((p.x, p.y))
    }

    /// Facts for the enable rules.
    pub fn enable_state(&self) -> EnableState {
        let has_image = self.image.is_some() && self.error.is_none();
        EnableState {
            has_image,
            animated: self.player.is_some(),
            has_folder: self.folder.as_ref().is_some_and(|f| !f.files.is_empty()),
            has_undo: !self.undo.is_empty(),
            editable: has_image && self.player.is_none() && !self.showing_preview,
            in_markup: self.markup.is_some(),
            overwritable: self.markup.as_ref().is_some_and(|m| {
                m.source
                    .as_ref()
                    .is_some_and(|s| rv_core::export::ExportFormat::of(s.format).is_some())
                    && rv_core::formats::Format::from_path(&m.file) != Some(rv_core::formats::Format::Project)
            }),
        }
    }

    /// Re-apply enabled state and dynamic texts of this window's menus (no-op if unchanged).
    pub fn update_menus(&mut self) {
        let t = Toggles {
            fullscreen: self.is_fullscreen(),
            slideshow: self.slideshow_due.is_some(),
            paused: self.player.as_ref().is_some_and(|p| p.paused),
            restore: self.undo.last().is_some_and(|u| u.2),
        };
        let state = self.enable_state();
        if let Some(m) = &mut self.menus {
            m.update(&state, &t);
        }
    }

    /// Resize-to-image: never when maximized or fullscreen.
    fn fit_to_image(&mut self, s: &Settings, image: (u32, u32)) {
        if self.window.is_maximized() || self.is_fullscreen() {
            return;
        }
        let Some((monitor, work)) = crate::platform::monitor_rects(&self.window) else {
            return;
        };
        let (Ok(pos), outer, inner) = (
            self.window.outer_position(),
            self.window.outer_size(),
            self.window.inner_size(),
        ) else {
            return;
        };
        let (w, h) = self.view.orient.apply_size(image.0, image.1);
        let r = layout::fit_window(&FitInput {
            image: (w as f64, h as f64),
            monitor,
            work,
            outer: layout::Rect {
                x: pos.x as f64,
                y: pos.y as f64,
                w: outer.width as f64,
                h: outer.height as f64,
            },
            decorations: (
                outer.width.saturating_sub(inner.width) as f64,
                outer.height.saturating_sub(inner.height) as f64,
            ),
            min_pct: s.min_window_resized_percentage as f64,
            max_pct: s.max_window_resized_percentage as f64,
            min_width: 1.5 * 96.0 * self.window.scale_factor(),
        });
        crate::platform::set_outer_rect(&self.window, &r);
        self.placement_restored = false;
    }

    /// A new load starts: the previous image stays on screen until the result arrives.
    pub fn begin_load(&mut self, path: PathBuf, nav: Option<i8>) {
        let now = Instant::now();
        self.loading = true;
        self.path = Some(path);
        self.nav_dir = nav;
        if let Some(d) = nav {
            self.direction = d;
        }
        self.loading_since = Some(now);
        self.last_load = Some(now);
        self.open_with_due = Some(now + Duration::from_millis(250));
        self.first_latency = None;
        self.full_latency = None;
        // opening another file pauses the current animation (it stays until replaced).
        if let Some(p) = &mut self.player {
            p.paused = true;
        }
        self.refresh_title();
    }

    /// Show a loader result of the current generation. A preview is drawn at the full image's
    /// size, so the full image replaces it without refitting (no zoom jump).
    pub fn show(
        &mut self,
        gpu: &Gpu,
        s: &Settings,
        generation: u64,
        img: &ViewImage,
        space: ViewSpace,
        wake: &Arc<dyn Fn() + Send + Sync>,
    ) {
        if generation < self.shown_generation {
            self.stale_shown += 1;
            return;
        }
        let same = generation == self.shown_generation && self.image.is_some();
        if img.preview && same && !self.showing_preview {
            return; // the full image won the race
        }
        let (vw, vh) = self.viewport();
        // Mipmaps only when they can be needed for the downscale.
        let mips = s.scaling_enabled && (img.w > vw || img.h > vh);
        let t = Instant::now();
        // Same size as the image on screen (photos of one camera): rewrite its textures. A new
        // allocation while the old one is alive cost ~18 ms of GPU memory allocation per 12 MP
        // image on the UI thread (measured), against ~4 ms for the copy itself.
        match &self.image {
            Some(old) if old.fits(img.w, img.h, mips, img.full) => gpu.rewrite(old, &img.rgba),
            _ => self.image = Some(gpu.upload(img.w, img.h, &img.rgba, mips, img.full)),
        }
        log::debug!("upload {}x{} (mips {mips}) {:?}", img.w, img.h, t.elapsed());
        self.image_size = Some(img.full);
        self.error = None;
        if !same {
            let mode = s.window_resize_mode;
            if mode == rv_core::settings::WindowResizeMode::WhenOpeningImages
                || (mode == rv_core::settings::WindowResizeMode::WhenLaunching && self.resize_pending)
            {
                self.resize_pending = false;
                self.fit_to_image(s, img.full);
            }
            self.view.set_image(img.full.0, img.full.1, s);
        }
        self.shown_generation = generation;
        self.showing_preview = img.preview;
        self.awaiting_present = true;
        if !img.preview {
            self.loading = false;
            self.images_shown += 1;
            self.info = Some(img.info.clone());
            // Frame 0 is on screen: the player streams the rest (play only if > 1 frame).
            self.player = img
                .animation
                .as_ref()
                .map(|b| Player::spawn(b.clone(), img.info.format, space, Instant::now(), wake.clone()));
            self.anim_bytes = img.animation.clone();
        } else if !same {
            self.player = None;
            self.anim_bytes = None;
        }
        self.refresh_title();
        self.window.request_redraw();
    }

    pub fn fail(&mut self, path: &Path, e: &LoadError) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.loading = false;
        let code = e.code.to_string();
        self.set_error(crate::i18n::tf(
            "error-opening",
            &[("name", &name), ("message", &e.message), ("code", &code)],
        ));
    }

    /// No file any more (its file was deleted with AfterDelete = Do Nothing, or none was left):
    /// an empty window; the folder list stays for navigation.
    pub fn clear(&mut self) {
        self.path = None;
        self.index = None;
        self.loading = false;
        self.image = None;
        self.image_size = None;
        self.info = None;
        self.player = None;
        self.anim_bytes = None;
        self.error = None;
        self.open_with.clear();
        self.open_with_due = None;
        if let Some(m) = &mut self.menus {
            m.set_open_with(&[]);
        }
        self.refresh_title();
        self.window.request_redraw();
    }

    pub fn set_error(&mut self, message: String) {
        self.image = None;
        self.image_size = None;
        self.info = None;
        self.player = None;
        self.anim_bytes = None;
        self.error = Some(message);
        self.refresh_title();
        self.window.request_redraw();
    }

    /// Zoom changes may require a mip chain the texture doesn't have yet.
    fn after_view_change(&mut self, gpu: &Gpu, s: &Settings) {
        self.want_svg_raster();
        if let Some(img) = &self.image
            && !img.mips
            && s.scaling_enabled
            && self.view.scale < 1.0
        {
            self.image = Some(gpu.add_mips(img));
        }
        self.window.request_redraw();
    }

    /// SVG zoomed in beyond its raster (rasterised at the monitor size): ask for one at the
    /// size the view needs (+25 %), up to 8192 px on the long side. Not in Markup (its patches
    /// are in image pixels of the first raster).
    fn want_svg_raster(&mut self) {
        const MAX: u32 = 8192;
        if self.markup.is_some()
            || self.svg_pending
            || !self
                .info
                .as_ref()
                .is_some_and(|i| i.format == rv_core::formats::Format::Svg)
        {
            return;
        }
        let (Some(img), Some((lw, lh))) = (&self.image, self.image_size) else {
            return;
        };
        let need = (self.view.scale * lw.max(lh) as f32).ceil() as u32;
        let have = img.w.max(img.h);
        if need as f32 > have as f32 * 1.1 && have < MAX {
            self.svg_want = Some((need + need / 4).min(MAX));
        }
    }

    /// A sharper raster of the SVG on screen: same logical size, more texels.
    pub fn show_svg_raster(&mut self, gpu: &Gpu, s: &Settings, w: u32, h: u32, rgba: &[u8]) {
        self.svg_pending = false;
        let Some(logical) = self.image_size else { return };
        if self.markup.is_some() {
            return;
        }
        let (vw, vh) = self.viewport();
        let mips = s.scaling_enabled && (w > vw || h > vh);
        self.image = Some(gpu.upload(w, h, rgba, mips, logical));
        self.window.request_redraw();
    }

    /// Upload the next animation frame if due; returns when to wake up next.
    pub fn anim_tick(&mut self, gpu: &Gpu, now: Instant) -> Option<Instant> {
        let p = self.player.as_mut()?;
        if let (Some(f), Some(img)) = (p.tick(now), &self.image) {
            gpu.rewrite(img, &f.rgba);
            self.window.request_redraw();
        }
        p.next_due()
    }

    fn anim_action(&mut self, gpu: &Gpu, a: Action) {
        let now = Instant::now();
        let Some(p) = self.player.as_mut() else { return }; // NeedsAnimation
        match a {
            Action::Pause => p.toggle_pause(now),
            Action::PreviousFrame | Action::NextFrame => {
                let f = if a == Action::NextFrame {
                    p.next_frame(now)
                } else {
                    p.previous_frame(now)
                };
                if let (Some(f), Some(img)) = (f, &self.image) {
                    gpu.rewrite(img, &f.rgba);
                    self.window.request_redraw();
                }
            }
            Action::DecreaseSpeed => p.change_speed(-25),
            Action::ResetSpeed => p.reset_speed(),
            Action::IncreaseSpeed => p.change_speed(25),
            _ => {}
        }
    }

    /// View-related actions handled locally. Returns false if the app must handle it.
    pub fn apply(&mut self, gpu: &Gpu, s: &Settings, a: Action) -> bool {
        let o = self.view.orient;
        match a {
            Action::ZoomIn => self.view.zoom_by(step_factor(s), None, s),
            Action::ZoomOut => self.view.zoom_by(1.0 / step_factor(s), None, s),
            Action::OriginalSize => self.view.toggle_original_size(s),
            Action::RotateRight => self.view.set_orient(o.rotated(1), s),
            Action::RotateLeft => self.view.set_orient(o.rotated(-1), s),
            Action::Mirror => self.view.set_orient(o.mirrored(), s),
            Action::Flip => self.view.set_orient(o.flipped(), s),
            Action::FullScreen => self.set_fullscreen(!self.is_fullscreen(), s),
            Action::Pause
            | Action::PreviousFrame
            | Action::NextFrame
            | Action::DecreaseSpeed
            | Action::ResetSpeed
            | Action::IncreaseSpeed => {
                self.anim_action(gpu, a);
                return true;
            }
            _ => return false,
        }
        self.after_view_change(gpu, s);
        true
    }

    /// The menu bar is hidden while fullscreen.
    pub fn set_fullscreen(&self, on: bool, s: &Settings) {
        self.window.set_fullscreen(on.then_some(Fullscreen::Borderless(None)));
        if let (Some(m), Some(h)) = (&self.menus, crate::platform::hwnd(&self.window)) {
            m.set_bar_visible(h.0 as isize, s.menubar_enabled && !on);
        }
    }

    pub fn on_event(&mut self, gpu: &Gpu, s: &Settings, event: &WindowEvent) -> Request {
        let ui = self.egui.on_window_event(&self.window, event);
        // egui must see the pointer where it is before the next press: with UI on screen every
        // event it reacts to repaints (else a click on a panel went through to the image)
        if ui.consumed || (ui.repaint && (self.markup.is_some() || self.dialog.is_some() || self.info_open)) {
            self.window.request_redraw();
        }
        if ui.consumed {
            // a drag begun on the image and released over a panel still ends
            if let (
                Some(m),
                WindowEvent::MouseInput {
                    state: ElementState::Released,
                    button: MouseButton::Left,
                    ..
                },
            ) = (&mut self.markup, event)
                && m.dragging()
            {
                m.release(rv_core::edit::PointF::default());
            }
            return Request::None;
        }
        if self.picking
            && let Some(r) = self.picking_event(event)
        {
            return r;
        }
        if self.markup.is_some()
            && let Some(r) = self.markup_event(s, event)
        {
            return r;
        }
        match event {
            WindowEvent::CloseRequested => return Request::Close,
            WindowEvent::Focused(true) => return Request::Focused,
            WindowEvent::Resized(sz) => {
                self.config.width = sz.width.max(1);
                self.config.height = sz.height.max(1);
                self.surface.configure(&gpu.device, &self.config);
                self.view.set_viewport(self.config.width, self.config.height, s);
                self.window.request_redraw();
            }
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                // resolved by the app against the shortcut table and this window's enable state
                if let Some(seq) = crate::shortcuts::key_sequence(event, self.mods) {
                    return Request::Key(seq);
                }
            }
            WindowEvent::ThemeChanged(t) => {
                crate::platform::set_dark_titlebar(&self.window, *t == winit::window::Theme::Dark);
            }
            WindowEvent::CursorMoved { position, .. } => {
                let p = (position.x as f32, position.y as f32);
                // A drag beyond the double-click rectangle is not the first half of a double click.
                if let (Some((_, c)), Some(_)) = (self.last_click, self.dragging) {
                    let r = crate::platform::double_click_rect();
                    if (p.0 - c.0).abs() > r.0 / 2.0 || (p.1 - c.1).abs() > r.1 / 2.0 {
                        self.last_click = None;
                    }
                }
                if let Some(last) = self.dragging {
                    self.view.pan_by(p.0 - last.0, p.1 - last.1);
                    self.dragging = Some(p);
                    self.window.request_redraw();
                }
                self.cursor = Some(p);
            }
            WindowEvent::CursorLeft { .. } => self.cursor = None,
            WindowEvent::MouseInput { state, button, .. } => match (button, state) {
                (MouseButton::Left, ElementState::Pressed) => {
                    let now = Instant::now();
                    let pos = self.cursor.unwrap_or_default();
                    let double = layout::is_double_click(
                        self.last_click,
                        now,
                        pos,
                        crate::platform::double_click_ms(),
                        crate::platform::double_click_rect(),
                    );
                    self.last_click = (!double).then_some((now, pos));
                    if double {
                        return Request::Action(Action::FullScreen);
                    }
                    self.dragging = self.cursor;
                    self.window.set_cursor(CursorIcon::Grabbing);
                }
                (MouseButton::Left, ElementState::Released) => {
                    self.dragging = None;
                    self.window.set_cursor(CursorIcon::Default);
                }
                (MouseButton::Back, ElementState::Pressed) => return Request::Action(Action::PreviousFile),
                (MouseButton::Forward, ElementState::Pressed) => return Request::Action(Action::NextFile),
                (MouseButton::Right, ElementState::Released) => {
                    let (x, y) = self.cursor.unwrap_or_default();
                    return Request::ContextMenu(x as f64, y as f64);
                }
                _ => {}
            },
            WindowEvent::MouseWheel { delta, .. } => self.on_wheel(gpu, s, *delta),
            WindowEvent::PinchGesture { delta, .. } => {
                self.view.zoom_by((1.0 + *delta as f32).max(0.1), self.cursor, s);
                self.after_view_change(gpu, s);
            }
            WindowEvent::HoveredFile(_) if self.drop_done => {
                self.dropped = 0;
                self.drop_done = false;
            }
            WindowEvent::DroppedFile(p) => {
                self.drop_done = true;
                self.dropped += 1;
                return Request::Open(p.clone(), self.dropped > 1);
            }
            _ => {}
        }
        Request::None
    }

    pub fn is_picking(&self) -> bool {
        self.picking
    }

    pub fn set_picking(&mut self, on: bool) {
        if self.picking != on {
            self.picking = on;
            self.window
                .set_cursor(if on { CursorIcon::Crosshair } else { CursorIcon::Default });
        }
    }

    /// The left button while color picking (UI clicks were already taken by egui). None = as usual.
    fn picking_event(&mut self, event: &WindowEvent) -> Option<Request> {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let p = (position.x as f32, position.y as f32);
                self.cursor = Some(p);
                let ctx = self.egui.egui_ctx();
                let at = egui::pos2(p.0, p.1) / ctx.pixels_per_point();
                if ctx.layer_id_at(at).is_none_or(|l| l.order == egui::Order::Background) {
                    self.window.set_cursor(CursorIcon::Crosshair);
                }
                Some(Request::None)
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => Some(self.cursor.map_or(Request::None, |(x, y)| Request::PickColor(x, y))),
            WindowEvent::MouseInput {
                button: MouseButton::Left,
                ..
            } => Some(Request::None),
            _ => None,
        }
    }

    /// Pointer events in Markup: the left button goes to the tools, the middle one pans. None =
    /// handle the event as the viewer does (wheel, keys, resize...).
    fn markup_event(&mut self, s: &Settings, event: &WindowEvent) -> Option<Request> {
        let t = self.view.screen_from_image();
        let inv = t.inverse();
        let mods = crate::markup::Mods {
            shift: self.mods.shift_key(),
            alt: self.mods.alt_key(),
        };
        let scale = self.view.scale;
        let to_image = |p: (f32, f32)| {
            let (x, y) = inv.apply(p.0, p.1);
            rv_core::edit::PointF { x, y }
        };
        match event {
            // typing into the edited text box: every key goes there, no shortcut fires
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
                    && self.markup.as_ref().is_some_and(|m| m.editing.is_some()) =>
            {
                use crate::markup::textedit::Cmd;
                use winit::keyboard::{Key, NamedKey};
                let (ctrl, shift) = (self.mods.control_key(), self.mods.shift_key());
                let m = self.markup.as_mut().unwrap();
                let cmd = match &event.logical_key {
                    Key::Named(NamedKey::Escape) => {
                        m.commit_text();
                        None
                    }
                    Key::Named(NamedKey::Backspace) => Some(Cmd::Backspace),
                    Key::Named(NamedKey::Delete) => Some(Cmd::Delete),
                    Key::Named(NamedKey::ArrowLeft) => Some(Cmd::Left),
                    Key::Named(NamedKey::ArrowRight) => Some(Cmd::Right),
                    Key::Named(NamedKey::ArrowUp) => Some(Cmd::Up),
                    Key::Named(NamedKey::ArrowDown) => Some(Cmd::Down),
                    Key::Named(NamedKey::Home) => Some(Cmd::Home),
                    Key::Named(NamedKey::End) => Some(Cmd::End),
                    // Ctrl+Enter: done typing and confirmed
                    Key::Named(NamedKey::Enter) if ctrl => {
                        m.confirm();
                        None
                    }
                    Key::Named(NamedKey::Enter) => Some(Cmd::Insert("\n".into())),
                    Key::Named(NamedKey::Tab) => None,
                    Key::Character(c) if ctrl => match c.to_lowercase().as_str() {
                        "a" => Some(Cmd::SelectAll),
                        "v" => crate::platform::clipboard_text().map(|t| Cmd::Insert(t.replace("\r\n", "\n"))),
                        "c" | "x" => {
                            if let Some(t) = m.selected_text() {
                                self.egui.egui_ctx().copy_text(t);
                            }
                            (c.eq_ignore_ascii_case("x")).then_some(Cmd::Delete)
                        }
                        _ => None,
                    },
                    _ => event
                        .text
                        .as_ref()
                        .map(|t| t.chars().filter(|c| !c.is_control()).collect::<String>())
                        .filter(|t| !t.is_empty())
                        .map(Cmd::Insert),
                };
                if let Some(c) = cmd {
                    m.text_cmd(c, shift);
                }
                self.refresh_title();
                self.window.request_redraw();
                Some(Request::None)
            }
            // text composed by an input method (East Asian languages, dead keys of some layouts)
            WindowEvent::Ime(winit::event::Ime::Commit(text))
                if self.markup.as_ref().is_some_and(|m| m.editing.is_some()) =>
            {
                let text: String = text.chars().filter(|c| !c.is_control()).collect();
                if let Some(m) = &mut self.markup
                    && !text.is_empty()
                {
                    m.text_cmd(crate::markup::textedit::Cmd::Insert(text), false);
                }
                self.refresh_title();
                self.window.request_redraw();
                Some(Request::None)
            }
            WindowEvent::CursorMoved { position, .. } => {
                let p = (position.x as f32, position.y as f32);
                self.cursor = Some(p);
                if let Some(last) = self.panning {
                    self.view.pan_by(p.0 - last.0, p.1 - last.1);
                    self.panning = Some(p);
                } else if let Some(m) = &mut self.markup
                    && m.dragging()
                {
                    m.motion(to_image(p), scale, mods);
                    self.refresh_title();
                } else {
                    // the pointer says what a press would do (resize, move, type, draw); over a
                    // panel the cursor is egui's
                    let ctx = self.egui.egui_ctx();
                    let at = egui::pos2(p.0, p.1) / ctx.pixels_per_point();
                    let over_panel = ctx.layer_id_at(at).is_some_and(|l| l.order != egui::Order::Background);
                    if let Some(m) = &self.markup
                        && !over_panel
                    {
                        use crate::markup::Hover;
                        use crate::markup::select::Handle;
                        let icon = match m.hover(to_image(p), p, &t, scale) {
                            Hover::Handle(Handle::Corner(0 | 2)) => CursorIcon::NwseResize,
                            Hover::Handle(Handle::Corner(_)) => CursorIcon::NeswResize,
                            Hover::Handle(Handle::Edge(0 | 2)) => CursorIcon::NsResize,
                            Hover::Handle(Handle::Edge(_)) => CursorIcon::EwResize,
                            Hover::Handle(Handle::Rotate) => CursorIcon::Grab,
                            Hover::Handle(Handle::End(_)) | Hover::Move => CursorIcon::Move,
                            Hover::Type => CursorIcon::Text,
                            Hover::Create => CursorIcon::Crosshair,
                            Hover::None => CursorIcon::Default,
                        };
                        self.window.set_cursor(icon);
                    }
                    return Some(Request::None);
                }
                self.window.request_redraw();
                Some(Request::None)
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let p = self.cursor.unwrap_or_default();
                match (button, state) {
                    (MouseButton::Left, ElementState::Pressed) => {
                        // a panel is there (known from the last frame even if the pointer just
                        // arrived): the press is the panel's
                        let ctx = self.egui.egui_ctx();
                        let at = egui::pos2(p.0, p.1) / ctx.pixels_per_point();
                        if ctx.layer_id_at(at).is_some_and(|l| l.order != egui::Order::Background) {
                            return Some(Request::None);
                        }
                        if let Some(m) = &mut self.markup {
                            m.press(to_image(p), p, &t, scale, mods);
                        }
                    }
                    (MouseButton::Left, ElementState::Released) => {
                        if let Some(m) = &mut self.markup {
                            m.release(to_image(p));
                        }
                        self.refresh_title();
                    }
                    (MouseButton::Middle, ElementState::Pressed) => {
                        self.panning = Some(p);
                        self.window.set_cursor(CursorIcon::Grabbing);
                    }
                    (MouseButton::Middle, ElementState::Released) => {
                        self.panning = None;
                        self.window.set_cursor(CursorIcon::Default);
                    }
                    _ => return None, // right click, Back/Forward: as in the viewer
                }
                self.sync_markup_view(s);
                self.window.request_redraw();
                Some(Request::None)
            }
            _ => None,
        }
    }

    /// The view follows the document: its orientation (view rotations are off in Markup)
    /// and its crop (the whole image while cropping). Refits only when one of them changed.
    pub fn sync_markup_view(&mut self, s: &Settings) {
        let Some(m) = &self.markup else {
            if self.markup_view.take().is_some() {
                self.view.orient = rv_core::orient::Orient::IDENTITY;
                self.view.set_region(None, s);
                self.window.request_redraw();
            }
            return;
        };
        let key = (m.doc().orient, m.view_region());
        if self.markup_view == Some(key) {
            return;
        }
        self.markup_view = Some(key);
        self.view.orient = key.0;
        self.view
            .set_region(key.1.map(|r| (r.x as f32, r.y as f32, r.w as f32, r.h as f32)), s);
        self.window.request_redraw();
    }

    /// Redaction patches of the document written into the image texture: the same
    /// pixels as the export, converted to the view space. When the set changes, the regions of the
    /// previous patches get their source pixels back first.
    fn sync_patches(&mut self, gpu: &Gpu) {
        let Some(m) = &mut self.markup else { return };
        let (Some(src), Some(img)) = (&m.source, &self.image) else {
            return;
        };
        // redactions being dragged show their outline only: their patch is computed on release
        let active = m.active_ids();
        let doc = m.history.current();
        let now = if active
            .iter()
            .any(|id| matches!(doc.find(*id), Some(rv_core::edit::ObjRef::Redaction(_))))
        {
            let mut still = (**doc).clone();
            still.redactions.retain(|r| !active.contains(&r.id));
            src.patches(&still)
        } else {
            src.patches(doc)
        };
        let same =
            now.len() == m.shown_patches.len() && now.iter().zip(&m.shown_patches).all(|(a, b)| Arc::ptr_eq(a, b));
        if same {
            return;
        }
        write_patches(gpu, img, src, self.view_space, &m.shown_patches, &now);
        m.shown_patches = now;
    }

    /// Put the source pixels back under the shown patches (leaving Markup).
    pub fn clear_patches(&mut self, gpu: &Gpu) {
        let Some(m) = &mut self.markup else { return };
        if let (Some(src), Some(img)) = (&m.source, &self.image) {
            write_patches(gpu, img, src, self.view_space, &m.shown_patches, &[]);
        }
        m.shown_patches.clear();
    }

    /// The input method follows the text editing: on while a box is edited, its candidates next to
    /// the caret.
    fn sync_ime(&mut self) {
        let caret = self.markup.as_ref().and_then(|m| m.caret_line());
        if self.ime != caret.is_some() {
            self.ime = caret.is_some();
            self.window.set_ime_allowed(self.ime);
        }
        if let Some((top, bottom)) = caret {
            let t = self.view.screen_from_image();
            let (x, y0) = t.apply(top.x, top.y);
            let (_, y1) = t.apply(bottom.x, bottom.y);
            self.window.set_ime_cursor_area(
                winit::dpi::PhysicalPosition::new(x as f64, y0.min(y1) as f64),
                winit::dpi::PhysicalSize::new(2.0, (y1 - y0).abs().max(1.0) as f64),
            );
        }
    }

    /// Bring the shape layers up to date for the current view; `now` = also after a view change
    /// (snapshots), else that waits until the view is still.
    fn sync_overlay(&mut self, gpu: &Gpu, now: bool) {
        let Some(m) = &mut self.markup else { return };
        let t = self.view.screen_from_image();
        let doc = m.history.current().clone();
        let active = m.active_ids();
        let vs = self.view_space;
        let vt =
            (!matches!(vs, ViewSpace::Srgb | ViewSpace::Raw)).then(|| rv_core::color::ViewTransform::new(None, vs));
        let colors = move |c: rv_core::edit::Color| match &vt {
            None => c,
            Some(vt) => {
                let mut px = [c.r, c.g, c.b, c.a];
                vt.apply(&mut px);
                rv_core::edit::Color {
                    r: px[0],
                    g: px[1],
                    b: px[2],
                    a: px[3],
                }
            }
        };
        // what loupes magnify: source + redaction patches, in the view space
        let src = m.source.clone();
        let patches = m.shown_patches.clone();
        let base = move |r: rv_core::edit::RectU| {
            let src = src.as_ref()?;
            let mut pm = rv_core::edit::render::base_pixmap((&*src.pixels).into(), src.w, r, &patches)?;
            let vt = rv_core::color::ViewTransform::new(src.icc.as_deref(), vs);
            rv_core::edit::render::unpremultiply(pm.data_mut());
            vt.apply(pm.data_mut());
            rv_core::edit::render::premultiply(pm.data_mut());
            Some(pm)
        };
        // the layers are redrawn when the source arrives or the patches change
        let key = (vs.id() as u64)
            ^ (m.source.is_some() as u64) << 8
            ^ m.shown_patches
                .iter()
                .fold(0u64, |h, p| h.rotate_left(7) ^ Arc::as_ptr(p) as u64)
                << 9;
        let due = crate::markup::overlay::Overlay::sync(
            &mut m.overlay,
            gpu,
            (self.config.width, self.config.height),
            &doc.shapes,
            &active,
            m.pending.as_ref(),
            &t,
            self.view.scale,
            &colors,
            &base,
            key,
            now,
        );
        if let Some(at) = due {
            self.repaint_at = Some(self.repaint_at.map_or(at, |r| r.min(at)));
        }
    }

    /// Wheel: ScrollZoom 0 zoom, 2 scroll, 1 auto (mouse zooms, touchpad scrolls); Ctrl
    /// inverts; Shift swaps axes; scroll halved.
    fn on_wheel(&mut self, gpu: &Gpu, s: &Settings, delta: MouseScrollDelta) {
        let (touchpad, dx, dy) = match delta {
            MouseScrollDelta::LineDelta(x, y) => (false, x * 120.0, y * 120.0),
            MouseScrollDelta::PixelDelta(p) => (true, p.x as f32, p.y as f32),
        };
        let mut zoom = match s.scroll_zoom {
            ScrollZoom::Zoom => true,
            ScrollZoom::Scroll => false,
            ScrollZoom::Auto => !touchpad,
        };
        if self.mods.control_key() {
            zoom = !zoom;
        }
        if zoom {
            let steps = dy / 120.0;
            let steps = if s.fractional_zoom || touchpad {
                steps
            } else {
                steps.signum()
            };
            if steps != 0.0 {
                self.view.zoom_by(step_factor(s).powf(steps), self.cursor, s);
            }
        } else {
            let (dx, dy) = if self.mods.shift_key() { (dy, dx) } else { (dx, dy) };
            self.view.pan_by(dx / 2.0, dy / 2.0);
        }
        self.after_view_change(gpu, s);
    }

    fn bg(&self, s: &Settings) -> [u8; 3] {
        if s.bg_color_enabled {
            s.bg_color
        } else {
            crate::platform::system_window_color()
        }
    }

    /// Draw the image (no UI) into `target`. Shared by the window and the E2E snapshot.
    pub fn draw_content(
        &self,
        gpu: &Gpu,
        s: &Settings,
        enc: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        pipeline: &wgpu::RenderPipeline,
        format: wgpu::TextureFormat,
    ) -> wgpu::RenderPass<'static> {
        let bg = self.bg(s);
        let mut pass = enc
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frame"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // non-sRGB target: clear with the encoded value
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: bg[0] as f64 / 255.0,
                            g: bg[1] as f64 / 255.0,
                            b: bg[2] as f64 / 255.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            })
            .forget_lifetime();
        // the document's crop: image and shapes are clipped to it on screen
        let (vw, vh) = (self.config.width, self.config.height);
        let clip = self.view.region().map(|(x, y, w, h)| {
            let t = self.view.screen_from_image();
            let (ax, ay) = t.apply(x, y);
            let (bx, by) = t.apply(x + w, y + h);
            let x0 = ax.min(bx).round().clamp(0.0, vw as f32) as u32;
            let y0 = ay.min(by).round().clamp(0.0, vh as f32) as u32;
            let x1 = ax.max(bx).round().clamp(0.0, vw as f32) as u32;
            let y1 = ay.max(by).round().clamp(0.0, vh as f32) as u32;
            (x0, y0, (x1 - x0).max(1), (y1 - y0).max(1))
        });
        if let Some((x, y, w, h)) = clip {
            pass.set_scissor_rect(x, y, w.min(vw - x.min(vw - 1)), h.min(vh - y.min(vh - 1)));
        }
        if let Some(img) = &self.image {
            let lin = bg.map(|c| rv_core::color::srgb_decode(c as f32 / 255.0));
            self.globals.write(&gpu.queue, &self.view.ndc_from_image(), lin);
            let filter = render::choose_filter(s, self.view.scale, img.mips);
            render::draw_image(&mut pass, pipeline, &self.globals, filter, img);
        }
        if let Some(o) = self.markup.as_ref().and_then(|m| m.overlay.as_ref()) {
            o.draw(gpu, &mut pass, format);
        }
        if clip.is_some() {
            pass.set_scissor_rect(0, 0, vw, vh);
        }
        pass
    }

    pub fn redraw(&mut self, gpu: &Gpu, s: &Settings) {
        self.sync_ime();
        self.sync_markup_view(s);
        self.sync_patches(gpu);
        self.sync_overlay(gpu, false);
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&gpu.device, &self.config);
                self.window.request_redraw();
                return;
            }
            other => {
                log::warn!("surface: {other:?}");
                return;
            }
        };
        let target = frame.texture.create_view(&Default::default());
        if let Some(rx) = &self.egui_pending {
            match rx.try_recv() {
                Ok(r) => self.egui_renderer = Some(r),
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                // worker thread died: build it here
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.egui_renderer = Some(egui_wgpu::Renderer::new(
                        &gpu.device,
                        self.egui_format,
                        Default::default(),
                    ))
                }
            }
            if self.egui_renderer.is_some() {
                self.egui_pending = None;
            }
        }
        let Some(mut egui_renderer) = self.egui_renderer.take() else {
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            drop(self.draw_content(gpu, s, &mut enc, &target, &self.pipeline.clone(), self.config.format));
            gpu.queue.submit([enc.finish()]);
            self.window.pre_present_notify();
            gpu.queue.present(frame);
            self.frame_presented();
            return;
        };

        // egui: only UI (here: the error message), never document content.
        let raw = self.egui.take_egui_input(&self.window);
        let error = self.error.clone();
        let bg = self.bg(s);
        let lum = (0.299 * bg[0] as f32 + 0.587 * bg[1] as f32 + 0.114 * bg[2] as f32) / 255.0;
        let text_color = if lum > 0.5 {
            egui::Color32::BLACK
        } else {
            egui::Color32::WHITE
        };
        let spinner = self.loading
            && self
                .loading_since
                .is_some_and(|t| t.elapsed() >= Duration::from_millis(150));
        let empty = self.image.is_none() && !self.loading && error.is_none();
        // FullScreenDetails: the title text at the top while in fullscreen
        let details = (s.fullscreen_details && self.is_fullscreen()).then(|| self.title());
        let (dialog, outcome) = (&mut self.dialog, &mut self.dialog_outcome);
        let (markup, markup_out) = (&mut self.markup, &mut self.markup_out);
        let (view_t, view_scale) = (self.view.screen_from_image(), self.view.scale);
        let (info_open, info_rows, info_pending) = (&mut self.info_open, &self.info_rows, self.info_pending);
        let mut out = self.egui.egui_ctx().run_ui(raw, |ui| {
            if *info_open {
                ui::info::show(ui.ctx(), info_open, info_rows, info_pending);
            }
            if let Some(m) = markup
                && let Some(o) = crate::markup::ui::show(ui.ctx(), m, &view_t, ui.ctx().pixels_per_point(), view_scale)
            {
                *markup_out = Some(o);
            }
            if let Some(d) = dialog
                && let Some(o) = d.show(ui.ctx())
            {
                *outcome = Some(o);
            }
            if let Some(t) = &details {
                egui::Area::new(egui::Id::unique("fullscreen-details"))
                    .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 8.0))
                    .show(ui.ctx(), |ui| {
                        ui.label(egui::RichText::new(t).color(text_color).size(14.0));
                    });
            }
            if spinner {
                egui::Area::new(egui::Id::unique("loading"))
                    .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, 12.0))
                    .show(ui.ctx(), |ui| {
                        ui.add(egui::Spinner::new().size(20.0).color(text_color));
                    });
            }
            if empty {
                ui::empty(ui.ctx());
            }
            if let Some(e) = &error {
                ui.centered_and_justified(|ui| {
                    ui.label(egui::RichText::new(e).color(text_color).size(15.0));
                });
            }
            ui::style::cursors(ui.ctx());
        });
        self.egui.handle_platform_output(&self.window, out.platform_output);
        if let Some(v) = out.viewport_output.get(&egui::ViewportId::ROOT) {
            if v.repaint_delay.is_zero() {
                self.window.request_redraw();
            } else if v.repaint_delay < Duration::from_secs(2) {
                self.repaint_at = Some(Instant::now() + v.repaint_delay);
            }
        }
        let ppp = out.pixels_per_point;
        let mut deltas = std::mem::take(&mut out.textures_delta);
        let jobs = self.egui.egui_ctx().tessellate(out.shapes, ppp);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: ppp,
        };
        for (id, list) in &deltas.set {
            for delta in list {
                egui_renderer.update_texture(&gpu.device, &gpu.queue, *id, delta);
            }
        }
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        let cmds = egui_renderer.update_buffers(&gpu.device, &gpu.queue, &mut enc, &jobs, &screen);
        {
            let mut pass = self.draw_content(gpu, s, &mut enc, &target, &self.pipeline.clone(), self.config.format);
            egui_renderer.render(&mut pass, &jobs, &screen);
        }
        gpu.queue.submit(cmds.into_iter().chain([enc.finish()]));
        self.window.pre_present_notify();
        gpu.queue.present(frame);
        for id in &deltas.free {
            egui_renderer.free_texture(id);
        }
        deltas.clear(); // egui 0.37 asserts that applied deltas are acknowledged
        self.egui_renderer = Some(egui_renderer);
        self.frame_presented();
    }

    fn frame_presented(&mut self) {
        self.presented = true;
        if std::mem::take(&mut self.awaiting_present) {
            let elapsed = self.loading_since.map(|t| t.elapsed());
            self.first_latency = self.first_latency.or(elapsed);
            if !self.showing_preview {
                self.full_latency = elapsed;
            }
        }
    }

    /// Offscreen render of the content at window size, read back as RGBA8 (E2E snapshot).
    pub fn snapshot(&mut self, gpu: &Gpu, s: &Settings) -> (u32, u32, Vec<u8>) {
        let (w, h) = self.viewport();
        (w, h, self.read_back(gpu, s, (0, 0, w, h)))
    }

    /// The colour shown at a window point (physical px): the image as drawn, without the UI.
    pub fn pixel_at(&mut self, gpu: &Gpu, s: &Settings, x: f32, y: f32) -> [u8; 3] {
        let (w, h) = self.viewport();
        let (x, y) = ((x.max(0.0) as u32).min(w - 1), (y.max(0.0) as u32).min(h - 1));
        let px = self.read_back(gpu, s, (x, y, 1, 1));
        [px[0], px[1], px[2]]
    }

    /// Render the content offscreen at window size; read back the region (x, y, w, h) as RGBA8.
    fn read_back(&mut self, gpu: &Gpu, s: &Settings, region: (u32, u32, u32, u32)) -> Vec<u8> {
        self.sync_markup_view(s);
        self.sync_patches(gpu);
        self.sync_overlay(gpu, true);
        let (w, h) = self.viewport();
        let fmt = wgpu::TextureFormat::Rgba8Unorm;
        let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("snapshot"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: fmt,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let (rx, ry, rw, rh) = region;
        let row = (rw * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("snapshot"),
            size: row as u64 * rh as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        drop(self.draw_content(
            gpu,
            s,
            &mut enc,
            &tex.create_view(&Default::default()),
            &gpu.image_pipeline(fmt),
            fmt,
        ));
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                origin: wgpu::Origin3d { x: rx, y: ry, z: 0 },
                ..tex.as_image_copy()
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d {
                width: rw,
                height: rh,
                depth_or_array_layers: 1,
            },
        );
        gpu.queue.submit([enc.finish()]);
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        let data = buf.slice(..).get_mapped_range().expect("mapped");
        let mut out = Vec::with_capacity((rw * rh * 4) as usize);
        for r in data.chunks_exact(row as usize) {
            out.extend_from_slice(&r[..(rw * 4) as usize]);
        }
        out
    }
}

/// Restore the regions of `old` from the source, then write `new` (in order), as view pixels.
fn write_patches(
    gpu: &Gpu,
    img: &crate::render::GpuImage,
    src: &rv_core::export::DocSource,
    space: ViewSpace,
    old: &[Arc<rv_core::edit::redact::Patch>],
    new: &[Arc<rv_core::edit::redact::Patch>],
) {
    use rv_core::decode::Pixels;
    let vt = rv_core::color::ViewTransform::new(src.icc.as_deref(), space);
    let view = |px: Vec<u8>| rv_core::color::finish_view(px, &vt);
    for p in old {
        let r = p.rect;
        let row = |y: u32| {
            ((y as usize * src.w as usize) + r.x as usize) * 4
                ..((y as usize * src.w as usize) + (r.x + r.w) as usize) * 4
        };
        let base: Vec<u8> = match &*src.pixels {
            Pixels::Rgba8(v) => (r.y..r.y + r.h).flat_map(|y| v[row(y)].to_vec()).collect(),
            Pixels::Rgba16(v) => rv_core::color::rgba8(Pixels::Rgba16(
                (r.y..r.y + r.h).flat_map(|y| v[row(y)].to_vec()).collect(),
            )),
        };
        gpu.write_region(img, r.x, r.y, r.w, r.h, &view(base));
    }
    for p in new {
        let r = p.rect;
        gpu.write_region(img, r.x, r.y, r.w, r.h, &view(rv_core::color::rgba8(p.pixels.clone())));
    }
}
