//! Color Picker window: pick a colour on the image or by hand, read it as RGB, HEX, HSV, HSL and
//! CMYK, copy any of them as text for other apps. The last colours picked or copied stay in a
//! history (saved with the app state): a click on one brings it back.

use super::icons::{self, Icon};
use super::tool::ToolOut;
use crate::i18n::t;
use egui::{Color32, Sense, Stroke, StrokeKind, vec2};
use rv_core::state::MAX_COLORS;

pub struct ColorPicker {
    pub rgb: [u8; 3],
    /// Clicks on the image pick a colour (the app keeps it in step with the viewer).
    pub picking: bool,
    /// Most recent first; the app owns it (state.toml) and refreshes this copy.
    pub history: Vec<[u8; 3]>,
    /// Row copied last, while the colour hasn't changed since.
    copied: Option<usize>,
}

impl ColorPicker {
    /// Starts on the last colour used, else on the accent colour.
    pub fn new(history: Vec<[u8; 3]>) -> Self {
        let [r, g, b, _] = super::style::ACCENT.to_array();
        Self {
            rgb: history.first().copied().unwrap_or([r, g, b]),
            picking: true,
            history,
            copied: None,
        }
    }

    pub fn set(&mut self, rgb: [u8; 3]) {
        self.rgb = rgb;
        self.copied = None;
    }

    /// "pick", "clear", "recent:<i>" (a history swatch), anything else closes.
    pub fn press(&mut self, button: &str) -> Vec<ToolOut> {
        if let Some(i) = button.strip_prefix("recent:").and_then(|i| i.parse::<usize>().ok()) {
            if let Some(c) = self.history.get(i).copied() {
                self.set(c);
            }
            return vec![];
        }
        match button {
            "pick" => vec![ToolOut::PickColor],
            "clear" => vec![ToolOut::ClearColors],
            _ => vec![ToolOut::Close],
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> Vec<ToolOut> {
        let mut outs = vec![];
        let width = ui.available_width();
        if ui
            .add_enabled(!self.picking, egui::Button::new(t("color-pick")))
            .clicked()
        {
            outs.extend(self.press("pick"));
        }
        if self.picking {
            ui.add(egui::Label::new(egui::RichText::new(t("color-picking")).weak()).wrap());
        }
        ui.add_space(8.0);
        let sv = vec2(width, (width * 0.6).round());
        if super::color::picker(ui, egui::Id::unique("color picker"), &mut self.rgb, sv) {
            self.copied = None;
        }
        ui.add_space(10.0);
        outs.extend(self.history_ui(ui, width));
        ui.add_space(4.0);
        for (i, (name, value)) in formats(self.rgb).into_iter().enumerate() {
            ui.horizontal(|ui| {
                let name_box = vec2(44.0, 26.0);
                ui.allocate_ui_with_layout(name_box, egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.set_min_size(name_box);
                    ui.label(egui::RichText::new(name).strong());
                });
                ui.label(egui::RichText::new(&value).monospace());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icons::button(ui, Icon::Copy, false, t("color-copy")).clicked() {
                        ui.ctx().copy_text(value);
                        self.copied = Some(i);
                        outs.push(ToolOut::RememberColor(self.rgb));
                    }
                    if self.copied == Some(i) {
                        ui.label(egui::RichText::new(t("color-copied")).weak().small());
                    }
                });
            });
        }
        outs
    }

    /// Title with Clear, then MAX_COLORS slots across the width: the colours used, most recent
    /// first, then empty outlines (the room left). The current colour is ringed. A hint below
    /// while there are none.
    fn history_ui(&mut self, ui: &mut egui::Ui, width: f32) -> Vec<ToolOut> {
        let mut outs = vec![];
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(t("color-recent")).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let clear = ui.add_enabled(
                    !self.history.is_empty(),
                    egui::Button::new(egui::RichText::new(t("color-recent-clear")).small()).frame(false),
                );
                if clear.clicked() {
                    outs.extend(self.press("clear"));
                }
            });
        });
        let gap = 6.0;
        let side = ((width - gap * (MAX_COLORS - 1) as f32) / MAX_COLORS as f32).floor();
        let faint = ui.visuals().widgets.noninteractive.bg_stroke;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for i in 0..MAX_COLORS {
                let Some(&[r, g, b]) = self.history.get(i) else {
                    let (rect, _) = ui.allocate_exact_size(vec2(side, side), Sense::hover());
                    ui.painter()
                        .rect_stroke(rect.shrink(1.0), 5.0, faint, StrokeKind::Inside);
                    continue;
                };
                let (rect, resp) = ui.allocate_exact_size(vec2(side, side), Sense::click());
                let fill = Color32::from_rgb(r, g, b);
                let p = ui.painter();
                let current = self.rgb == [r, g, b];
                // grown a little under the pointer; the ring marks the colour on show
                let swatch = if resp.hovered() { rect } else { rect.shrink(1.5) };
                p.rect_filled(swatch, 5.0, fill);
                // a faint edge: dark colours still stand out on the dark theme
                p.rect_stroke(swatch, 5.0, Stroke::new(1.0, faint.color), StrokeKind::Inside);
                if current {
                    p.rect_stroke(
                        rect.expand(1.5),
                        6.5,
                        Stroke::new(2.0, super::style::ACCENT),
                        StrokeKind::Outside,
                    );
                }
                let hex = format!("#{r:02X}{g:02X}{b:02X}");
                if resp.on_hover_text(&hex).clicked() {
                    outs.extend(self.press(&format!("recent:{i}")));
                }
            }
        });
        // the hint's row is kept when it is hidden: nothing below moves when the first colour comes
        let font = egui::TextStyle::Small.resolve(ui.style());
        let (rect, _) = ui.allocate_exact_size(vec2(width, ui.fonts_mut(|f| f.row_height(&font))), Sense::hover());
        if self.history.is_empty() {
            let color = ui.visuals().weak_text_color();
            ui.painter().text(
                rect.left_center(),
                egui::Align2::LEFT_CENTER,
                t("color-recent-empty"),
                font,
                color,
            );
        }
        outs
    }
}

/// The colour as the usual text notations (CSS syntax where there is one).
pub fn formats([r, g, b]: [u8; 3]) -> [(&'static str, String); 5] {
    let (rf, gf, bf) = (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let d = max - min;
    let hue = if d == 0.0 {
        0.0
    } else if max == rf {
        60.0 * ((gf - bf) / d).rem_euclid(6.0)
    } else if max == gf {
        60.0 * ((bf - rf) / d + 2.0)
    } else {
        60.0 * ((rf - gf) / d + 4.0)
    };
    let h = hue.round() as u32 % 360;
    let pct = |v: f64| (v * 100.0).round() as u32;
    let sv = if max == 0.0 { 0.0 } else { d / max };
    let l = (max + min) / 2.0;
    let sl = if d == 0.0 {
        0.0
    } else {
        d / (1.0 - (2.0 * l - 1.0).abs())
    };
    let k = 1.0 - max;
    let ink = |c: f64| if max == 0.0 { 0.0 } else { (max - c) / max };
    [
        ("RGB", format!("rgb({r}, {g}, {b})")),
        ("HEX", format!("#{r:02X}{g:02X}{b:02X}")),
        ("HSV", format!("hsv({h}, {}%, {}%)", pct(sv), pct(max))),
        ("HSL", format!("hsl({h}, {}%, {}%)", pct(sl), pct(l))),
        (
            "CMYK",
            format!(
                "cmyk({}%, {}%, {}%, {}%)",
                pct(ink(rf)),
                pct(ink(gf)),
                pct(ink(bf)),
                pct(k)
            ),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::formats;

    fn values(rgb: [u8; 3]) -> Vec<String> {
        formats(rgb).into_iter().map(|(_, v)| v).collect()
    }

    #[test]
    fn notations() {
        assert_eq!(
            values([255, 0, 0]),
            [
                "rgb(255, 0, 0)",
                "#FF0000",
                "hsv(0, 100%, 100%)",
                "hsl(0, 100%, 50%)",
                "cmyk(0%, 100%, 100%, 0%)"
            ]
        );
        assert_eq!(
            values([0, 128, 128]),
            [
                "rgb(0, 128, 128)",
                "#008080",
                "hsv(180, 100%, 50%)",
                "hsl(180, 100%, 25%)",
                "cmyk(100%, 0%, 0%, 50%)"
            ]
        );
        assert_eq!(
            values([0, 0, 0]),
            [
                "rgb(0, 0, 0)",
                "#000000",
                "hsv(0, 0%, 0%)",
                "hsl(0, 0%, 0%)",
                "cmyk(0%, 0%, 0%, 100%)"
            ]
        );
        assert_eq!(values([255, 255, 255])[3], "hsl(0, 0%, 100%)");
        assert_eq!(values([59, 130, 246])[2], "hsv(217, 76%, 96%)");
    }
}
