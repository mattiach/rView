//! Separate top-level windows made only of egui UI: Settings, Welcome
//! and the update notice. Each has its own surface and egui renderer on the shared GPU device.
//! Their content never acts by itself: it returns `ToolOut`s that the app runs (the E2E driver
//! presses the same buttons through `Content::press`).

use super::{notice, options, welcome};
use crate::render::{self, Gpu};
use rv_core::settings::Settings;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::event::{ElementState, WindowEvent};
use winit::keyboard::ModifiersState;
use winit::window::Window;

pub enum Content {
    Settings(options::Options),
    Welcome(welcome::Welcome),
    Update(notice::UpdateNotice),
}

/// What the app must do after a frame or an event of a tool window.
#[derive(Clone, Debug, PartialEq)]
pub enum ToolOut {
    /// Settings OK/Apply: the new settings and the custom shortcuts (action key → keys).
    Apply {
        settings: Settings,
        shortcuts: BTreeMap<String, Vec<String>>,
    },
    Close,
    OpenUrl(String),
    /// Welcome closed with this choice for update notifications.
    Welcome {
        update_notifications: bool,
    },
}

impl Content {
    /// Stable name (driver, singleton per kind).
    pub fn kind(&self) -> &'static str {
        match self {
            Content::Settings(_) => "settings",
            Content::Welcome(_) => "welcome",
            Content::Update(_) => "update",
        }
    }

    fn title(&self) -> String {
        use crate::i18n::t;
        match self {
            Content::Settings(_) => t("title-settings"),
            Content::Welcome(_) => t("welcome-title"),
            Content::Update(_) => t("update-title"),
        }
    }

    /// Logical size and whether the user may resize it.
    fn size(&self) -> (f64, f64, bool) {
        match self {
            Content::Settings(_) => (680.0, 600.0, true),
            Content::Welcome(_) => (500.0, 360.0, false),
            Content::Update(_) => (540.0, 440.0, true),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui) -> Vec<ToolOut> {
        match self {
            Content::Settings(o) => o.ui(ui),
            Content::Welcome(w) => w.ui(ui),
            Content::Update(u) => u.ui(ui),
        }
    }

    /// Press a button by name, as a click would ("ok", "apply", "cancel", "defaults", "close",
    /// "download").
    pub fn press(&mut self, button: &str) -> Vec<ToolOut> {
        match self {
            Content::Settings(o) => o.press(button),
            Content::Welcome(w) => w.press(button),
            Content::Update(u) => u.press(button),
        }
    }

    /// Re-read texts after a language change.
    fn refresh_title(&self, window: &Window) {
        window.set_title(&self.title());
    }
}

pub struct ToolWindow {
    pub window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    egui: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    mods: ModifiersState,
    pub presented: bool,
    pub repaint_at: Option<Instant>,
    pub content: Content,
}

fn restyle(ctx: &egui::Context, theme: Option<winit::window::Theme>) {
    super::style::apply(ctx, theme != Some(winit::window::Theme::Light), false);
}

impl ToolWindow {
    pub fn new(
        el: &winit::event_loop::ActiveEventLoop,
        gpu: &Gpu,
        content: Content,
        proxy: winit::event_loop::EventLoopProxy<crate::app::UserEvent>,
        cloaked: bool,
    ) -> anyhow::Result<ToolWindow> {
        let (w, h, resizable) = content.size();
        let attrs = Window::default_attributes()
            .with_title(content.title())
            .with_visible(false)
            .with_resizable(resizable)
            .with_enabled_buttons(if resizable {
                winit::window::WindowButtons::all()
            } else {
                winit::window::WindowButtons::CLOSE
            })
            .with_inner_size(winit::dpi::LogicalSize::new(w, h))
            .with_min_inner_size(winit::dpi::LogicalSize::new(w * 0.6, h * 0.6));
        let window = Arc::new(el.create_window(crate::platform::with_app_icon(attrs))?);
        crate::platform::set_dark_titlebar(&window, window.theme() != Some(winit::window::Theme::Light));
        crate::platform::grab_cursor_on_caption(&window);
        let egui = crate::window::ViewerWindow::ui_state(el, &window, proxy);
        restyle(egui.egui_ctx(), window.theme());
        let surface = gpu.instance.create_surface(window.clone())?;
        let caps = surface.get_capabilities(&gpu.adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| *f == render::SURFACE_FORMAT)
            .or_else(|| caps.formats.iter().copied().find(|f| !f.is_srgb()))
            .unwrap_or(caps.formats[0]);
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            color_space: wgpu::SurfaceColorSpace::Srgb,
        };
        surface.configure(&gpu.device, &config);
        let renderer = egui_wgpu::Renderer::new(&gpu.device, format, Default::default());
        if cloaked {
            crate::platform::cloak(&window);
        }
        window.set_visible(true);
        window.request_redraw();
        Ok(ToolWindow {
            window,
            surface,
            config,
            egui,
            renderer,
            mods: ModifiersState::empty(),
            presented: false,
            repaint_at: None,
            content,
        })
    }

    pub fn refresh_texts(&self) {
        self.content.refresh_title(&self.window);
        self.window.request_redraw();
    }

    pub fn on_event(&mut self, gpu: &Gpu, event: &WindowEvent) -> Vec<ToolOut> {
        // The shortcut editor records the next key combination itself (egui would take it).
        if let (Content::Settings(o), WindowEvent::KeyboardInput { event: k, .. }) = (&mut self.content, event)
            && o.is_recording()
        {
            if k.state == ElementState::Pressed
                && let Some(seq) = crate::shortcuts::key_sequence(k, self.mods)
            {
                o.record(seq);
                self.window.request_redraw();
            }
            return vec![];
        }
        if self.egui.on_window_event(&self.window, event).repaint {
            self.window.request_redraw();
        }
        match event {
            WindowEvent::CloseRequested => return self.content.press("close-window"),
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::Resized(sz) => {
                self.config.width = sz.width.max(1);
                self.config.height = sz.height.max(1);
                self.surface.configure(&gpu.device, &self.config);
                self.window.request_redraw();
            }
            WindowEvent::ThemeChanged(t) => {
                crate::platform::set_dark_titlebar(&self.window, *t == winit::window::Theme::Dark);
                restyle(self.egui.egui_ctx(), Some(*t));
                self.window.request_redraw();
            }
            _ => {}
        }
        vec![]
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

    /// Draw a frame; returns what the user asked for in it.
    pub fn redraw(&mut self, gpu: &Gpu) -> Vec<ToolOut> {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&gpu.device, &self.config);
                self.window.request_redraw();
                return vec![];
            }
            other => {
                log::warn!("surface: {other:?}");
                return vec![];
            }
        };
        let target = frame.texture.create_view(&Default::default());
        let raw = self.egui.take_egui_input(&self.window);
        let ctx = self.egui.egui_ctx().clone();
        let content = &mut self.content;
        let mut outs = vec![];
        let mut out = ctx.run_ui(raw, |ui| {
            egui::Frame::central_panel(ui.style()).show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                outs = content.ui(ui);
            });
            super::style::cursors(ui.ctx());
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
        let jobs = ctx.tessellate(out.shapes, ppp);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: ppp,
        };
        for (id, list) in &deltas.set {
            for delta in list {
                self.renderer.update_texture(&gpu.device, &gpu.queue, *id, delta);
            }
        }
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        let cmds = self
            .renderer
            .update_buffers(&gpu.device, &gpu.queue, &mut enc, &jobs, &screen);
        {
            // non-sRGB target: clear with the encoded panel colour
            let fill = ctx.global_style().visuals.panel_fill;
            let mut pass = enc
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("tool"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &target,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: fill.r() as f64 / 255.0,
                                g: fill.g() as f64 / 255.0,
                                b: fill.b() as f64 / 255.0,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                })
                .forget_lifetime();
            self.renderer.render(&mut pass, &jobs, &screen);
        }
        gpu.queue.submit(cmds.into_iter().chain([enc.finish()]));
        self.window.pre_present_notify();
        gpu.queue.present(frame);
        for id in &deltas.free {
            self.renderer.free_texture(id);
        }
        deltas.clear();
        self.presented = true;
        outs
    }
}
