//! The "new version" notice and the project link.

use super::tool::ToolOut;
use crate::i18n::{t, tf};

pub const PROJECT_URL: &str = "https://github.com/mattiach/rview";

/// A newer release was found.
pub struct UpdateNotice {
    pub version: String,
    /// Release notes without the first "Changelog" line.
    pub notes: String,
    /// Release page.
    pub url: String,
}

impl UpdateNotice {
    pub fn press(&mut self, button: &str) -> Vec<ToolOut> {
        match button {
            "download" => vec![ToolOut::OpenUrl(self.url.clone()), ToolOut::Close],
            _ => vec![ToolOut::Close],
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> Vec<ToolOut> {
        let mut outs = vec![];
        ui.heading(tf("update-available", &[("version", &self.version)]));
        ui.label(tf("update-current", &[("version", env!("CARGO_PKG_VERSION"))]));
        ui.add_space(6.0);
        ui.label(egui::RichText::new(t("update-changelog")).strong());
        let h = (ui.available_height() - 40.0).max(60.0);
        egui::ScrollArea::vertical()
            .max_height(h)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.label(&self.notes);
            });
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button(t("update-download")).clicked() {
                outs.extend(self.press("download"));
            }
            if ui.button(t("button-close")).clicked() {
                outs.extend(self.press("close"));
            }
        });
        outs
    }
}
