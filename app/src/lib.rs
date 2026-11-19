//! rView app: winit + wgpu (DX12) + egui + muda. `run(Config)` is also the entry
//! point of the E2E tests.

pub mod actions;
mod app;
pub mod driver;
pub mod i18n;
pub mod layout;
pub mod markup;
pub mod menus;
#[doc(hidden)] // public for the Windows integration tests
pub mod platform;
#[doc(hidden)] // public only for the upload benchmark
pub mod render;
pub mod shortcuts;
pub mod ui;
pub mod viewer;
mod window;

use std::path::PathBuf;

#[derive(Default)]
pub struct Config {
    pub files: Vec<PathBuf>,
    /// Force the WARP software adapter (`--software`).
    pub software: bool,
    /// Print the time to the first frame (and to the image, if a file is given) and exit
    /// (`--measure-startup`).
    pub measure_startup: bool,
    pub driver: Option<Vec<driver::Step>>,
    /// Settings to use instead of `settings_file` (tests).
    pub settings: Option<rv_core::settings::Settings>,
    /// `%APPDATA%/rView/settings.toml` when started normally; None in tests (nothing saved) unless a
    /// test passes its own file.
    pub settings_file: Option<PathBuf>,
    /// State file (window geometry): `%APPDATA%/rView/state.toml` when started normally, None in
    /// tests (nothing persisted) unless a test passes its own file.
    pub state_file: Option<PathBuf>,
    /// Tests: answer every confirmation (Delete, Delete Permanently) with this instead of asking;
    /// with a driver, messages for the user are recorded (`Observation::message`), not shown.
    pub auto_confirm: Option<bool>,
    /// Tests: keep the windows on screen while a driver runs (they are cloaked otherwise). UI
    /// Automation leaves cloaked windows, and the dialogs and menus they own, out of its tree.
    pub visible: bool,
}

impl Config {
    /// `rview [--software] [--measure-startup] <file|folder|file://url>...`
    pub fn from_args(args: impl Iterator<Item = std::ffi::OsString>) -> Config {
        let mut cfg = Config {
            state_file: rv_core::state::default_path(),
            settings_file: rv_core::settings::default_path(),
            ..Default::default()
        };
        for a in args {
            match a.to_str() {
                Some("--software") => cfg.software = true,
                Some("--measure-startup") => cfg.measure_startup = true,
                _ => cfg.files.extend(resolve_arg(a)),
            }
        }
        cfg
    }
}

/// A folder opens its first compatible file; `file://` URLs become paths.
fn resolve_arg(a: std::ffi::OsString) -> Option<PathBuf> {
    let p = match a
        .to_str()
        .and_then(|s| s.strip_prefix("file:///").or_else(|| s.strip_prefix("file://")))
    {
        Some(rest) => PathBuf::from(percent_decode(rest)),
        None => PathBuf::from(a),
    };
    if p.is_dir() {
        return rv_core::folder::list(&p, rv_core::settings::Settings::default().skip_hidden)
            .into_iter()
            .next();
    }
    Some(p)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Run the event loop until all windows are closed (or the driver script ends).
pub fn run(cfg: Config) -> anyhow::Result<()> {
    let event_loop = winit::event_loop::EventLoop::<app::UserEvent>::with_user_event().build()?;
    let mut app = app::App::new(cfg, event_loop.create_proxy());
    event_loop.run_app(&mut app)?;
    if app.failures.is_empty() {
        Ok(())
    } else {
        anyhow::bail!("{}", app.failures.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_has_a_font_for_its_texts() {
        // Without egui's `default_fonts` feature every label is drawn as □.
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.label("x");
        });
        out.textures_delta.clear();
        // the error message shown by the viewer, plus Italian accents (UI strings, file names)
        let text = "Error occurred opening foto.jpg: Impossibile trovare il file (Error 5) àèéìòù";
        assert!(ctx.fonts_mut(|f| f.has_glyphs(&egui::FontId::proportional(15.0), text)));
    }

    #[test]
    fn file_url_decoding() {
        assert_eq!(percent_decode("C:/a%20b/%C3%A8.png"), "C:/a b/è.png");
        assert_eq!(percent_decode("100%"), "100%");
    }
}
