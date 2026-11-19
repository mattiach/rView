//! egui dialogs. Inside a viewer window: Rename, Open URL with its download progress,
//! Export (Markup), the non-modal File Info. In their own windows (`tool`): Settings, Welcome, update notice, Color Picker.
//! A dialog never acts by itself: it returns an outcome the app runs, the same one the E2E driver
//! gets by pressing its buttons.

pub mod color;
pub mod colorpick;
pub mod export;
pub mod icons;
pub mod info;
pub mod notice;
pub mod openurl;
pub mod options;
pub mod rename;
pub mod style;
pub mod tool;
pub mod welcome;

use crate::i18n::{t, tf};
use std::path::PathBuf;

/// What a window without an image shows: the app's name and version, and how to open a file.
pub fn empty(ctx: &egui::Context) {
    egui::Area::new(egui::Id::unique("empty"))
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::window(ui.style())
                .inner_margin(egui::Margin::symmetric(36, 28))
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new("rView").size(34.0).strong());
                        ui.label(
                            egui::RichText::new(tf("empty-version", &[("version", env!("CARGO_PKG_VERSION"))])).weak(),
                        );
                        ui.add_space(18.0);
                        ui.label(egui::RichText::new(t("empty-open")).size(15.0));
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(t("empty-drop")).size(15.0));
                    });
                });
        });
}

/// The modal dialog of a window (at most one at a time).
pub enum Dialog {
    Rename(rename::Rename),
    OpenUrl(openurl::OpenUrl),
    Download(openurl::Progress),
    Export(export::Export),
}

/// What the app must do when a dialog closes.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// Rename `path` to `name` (validated by the app).
    Rename(PathBuf, String),
    OpenUrl(String),
    CancelDownload,
    /// Export the Markup document with these options (the file name is asked next).
    Export(rv_core::export::ExportOptions),
    /// Save this frame (0-based) of the animation with these options (the file name is next).
    ExportFrame(rv_core::export::ExportOptions, usize),
    Closed,
}

impl Dialog {
    /// (kind, text, selection in chars) for tests.
    pub fn describe(&self) -> (&'static str, String, (usize, usize)) {
        match self {
            Dialog::Rename(r) => ("rename", r.text.clone(), r.selection),
            Dialog::OpenUrl(o) => ("openurl", o.text.clone(), (0, 0)),
            Dialog::Download(p) => ("download", p.url.clone(), (0, 0)),
            Dialog::Export(e) => ("export", e.opts.format.name().to_string(), (0, 0)),
        }
    }

    pub fn set_text(&mut self, t: &str) {
        match self {
            Dialog::Rename(r) => r.text = t.into(),
            Dialog::OpenUrl(o) => o.text = t.into(),
            Dialog::Export(e) => e.set_format(t),
            Dialog::Download(_) => {}
        }
    }

    /// OK button / Enter.
    pub fn accept(&self) -> Outcome {
        match self {
            Dialog::Rename(r) => Outcome::Rename(r.path.clone(), r.text.trim().to_string()),
            Dialog::OpenUrl(o) => Outcome::OpenUrl(o.text.trim().to_string()),
            Dialog::Download(_) => Outcome::CancelDownload,
            Dialog::Export(e) => match e.frame {
                Some(n) => Outcome::ExportFrame(e.opts, n),
                None => Outcome::Export(e.opts),
            },
        }
    }

    /// Cancel button / Esc / click outside.
    pub fn cancel(&self) -> Outcome {
        match self {
            Dialog::Download(_) => Outcome::CancelDownload,
            _ => Outcome::Closed,
        }
    }

    /// Draw; Some(outcome) when the dialog closes this frame.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Outcome> {
        let (id, title) = match self {
            Dialog::Rename(_) => ("rename", t("rename-title")),
            Dialog::OpenUrl(_) => ("openurl", t("openurl-title")),
            Dialog::Download(_) => ("download", t("download-title")),
            Dialog::Export(e) if e.frame.is_some() => ("export", t("title-saveframe")),
            Dialog::Export(_) => ("export", t("export-title")),
        };
        let mut done = None;
        let r = egui::Modal::new(egui::Id::unique(id)).show(ctx, |ui| {
            ui.heading(title);
            ui.add_space(6.0);
            let enter = match self {
                Dialog::Rename(r) => r.ui(ui),
                Dialog::OpenUrl(o) => o.ui(ui),
                Dialog::Export(e) => e.ui(ui),
                Dialog::Download(p) => {
                    p.ui(ui);
                    false
                }
            };
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let downloading = matches!(self, Dialog::Download(_));
                if !downloading && (ui.button(t("button-ok")).clicked() || enter) {
                    done = Some(self.accept());
                }
                if ui.button(t("button-cancel")).clicked() {
                    done = Some(self.cancel());
                }
            });
        });
        if done.is_none() && r.should_close() {
            done = Some(self.cancel());
        }
        done
    }
}

/// Single-line text field, focused on the first frame with `select` (chars) selected.
/// Returns true when Enter was pressed in it.
fn text_field(ui: &mut egui::Ui, text: &mut String, first: &mut bool, select: (usize, usize)) -> bool {
    let out = egui::TextEdit::singleline(text).desired_width(f32::INFINITY).show(ui);
    let resp = &out.response.response;
    if std::mem::take(first) {
        resp.request_focus();
        let mut st = out.state;
        st.cursor.set_char_range(Some(egui::text_selection::CCursorRange::two(
            egui::text::CCursor::new(select.0),
            egui::text::CCursor::new(select.1),
        )));
        st.store(ui.ctx(), resp.id);
    }
    resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
}
