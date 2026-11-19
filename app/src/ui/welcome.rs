//! Welcome window at the first start: the offer to make rView the default app for images.

use super::tool::ToolOut;
use crate::i18n::t;

pub struct Welcome {
    /// Windows Settings page for rView's default apps; `None` when rView isn't installed.
    default_apps: Option<String>,
}

impl Default for Welcome {
    fn default() -> Self {
        Self {
            default_apps: crate::platform::default_apps_page(),
        }
    }
}

impl Welcome {
    /// Any way of closing it records that the welcome was shown.
    pub fn press(&mut self, _button: &str) -> Vec<ToolOut> {
        vec![ToolOut::Welcome, ToolOut::Close]
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> Vec<ToolOut> {
        let mut outs = vec![];
        ui.vertical_centered(|ui| {
            ui.add_space(8.0);
            ui.heading(egui::RichText::new(t("welcome-heading")).size(24.0));
        });
        ui.add_space(8.0);
        ui.label(t("welcome-text"));
        ui.add_space(6.0);
        ui.label(t("welcome-tips"));
        if let Some(page) = &self.default_apps {
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.label(t("welcome-default"));
                if ui.button(t("welcome-default-button")).clicked() {
                    outs.push(ToolOut::OpenUrl(page.clone()));
                }
            });
            ui.label(egui::RichText::new(t("welcome-default-detail")).weak().small());
        }
        ui.add_space(12.0);
        ui.vertical_centered(|ui| {
            if ui.button(t("welcome-start")).clicked() {
                outs.extend(self.press("ok"));
            }
        });
        outs
    }
}
