//! The look of every egui surface: translucent "glass" panels with soft corners and shadows, one
//! accent colour. Viewer overlays (Markup bar, Inspector, File Info, dialogs) are dark glass over
//! the image; tool windows follow the system theme.
//! egui has no backdrop blur: the glass is translucency, a light edge and a soft shadow.

use egui::{Color32, CornerRadius, Stroke};

pub const ACCENT: Color32 = Color32::from_rgb(0x0A, 0x84, 0xFF);

/// Apply the style to a context. `glass`: translucent panels (over the image); else opaque.
pub fn apply(ctx: &egui::Context, dark: bool, glass: bool) {
    let mut v = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    let (base, ink) = if dark {
        (Color32::from_rgb(30, 30, 34), Color32::from_rgb(235, 235, 245))
    } else {
        (Color32::from_rgb(246, 246, 248), Color32::from_rgb(28, 28, 30))
    };
    let tint = |a: u8| Color32::from_rgba_unmultiplied(ink.r(), ink.g(), ink.b(), a);
    let fill = if glass { base.gamma_multiply(0.78) } else { base };
    v.window_fill = fill;
    v.panel_fill = base;
    v.extreme_bg_color = if dark {
        Color32::from_rgb(18, 18, 20)
    } else {
        Color32::WHITE
    };
    v.window_stroke = Stroke::new(1.0, tint(34));
    v.window_corner_radius = CornerRadius::same(14);
    v.menu_corner_radius = CornerRadius::same(10);
    v.window_shadow = egui::Shadow {
        offset: [0, 10],
        blur: 32,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 110 } else { 50 }),
    };
    v.popup_shadow = egui::Shadow {
        offset: [0, 6],
        blur: 20,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 100 } else { 45 }),
    };
    v.selection.bg_fill = ACCENT.gamma_multiply(0.55);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;
    v.slider_trailing_fill = true;
    v.override_text_color = Some(ink);
    let r = CornerRadius::same(8);
    let w = &mut v.widgets;
    w.noninteractive.bg_stroke = Stroke::new(1.0, tint(26));
    w.noninteractive.fg_stroke = Stroke::new(1.0, tint(200));
    w.noninteractive.corner_radius = r;
    for (state, bg) in [
        (&mut w.inactive, 22),
        (&mut w.hovered, 44),
        (&mut w.active, 70),
        (&mut w.open, 44),
    ] {
        state.bg_fill = tint(bg);
        state.weak_bg_fill = tint(bg);
        state.bg_stroke = Stroke::NONE;
        state.fg_stroke = Stroke::new(1.0, ink);
        state.corner_radius = r;
        state.expansion = 0.0;
    }
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 7.0);
        s.spacing.button_padding = egui::vec2(10.0, 5.0);
        s.spacing.window_margin = egui::Margin::same(14);
        s.spacing.menu_margin = egui::Margin::same(8);
        s.spacing.slider_width = 120.0;
    });
}

/// Cursors of an egui surface, at the end of a pass: the pointer over anything clickable, grab
/// and grabbing over panels that can be moved. A cursor a widget chose itself (text, resize) wins.
pub fn cursors(ctx: &egui::Context) {
    use egui::CursorIcon;
    let is_panel = |id: egui::Id, layer: egui::LayerId| id == layer.id.with("move");
    if let Some(r) = ctx.dragged_id().and_then(|d| ctx.read_response(d))
        && is_panel(r.id, r.layer_id)
    {
        return ctx.set_cursor_icon(CursorIcon::Grabbing);
    }
    if ctx.output(|o| o.cursor_icon) != CursorIcon::Default {
        return;
    }
    let (click, drag) = ctx.viewport(|v| (v.hits.click, v.hits.drag));
    if click.is_some_and(|w| w.enabled && !is_panel(w.id, w.layer_id)) {
        ctx.set_cursor_icon(CursorIcon::PointingHand);
    } else if drag.is_some_and(|w| is_panel(w.id, w.layer_id) && w.sense.senses_drag()) {
        ctx.set_cursor_icon(CursorIcon::Grab);
    }
}

/// The frame of a floating bar over the image (Markup toolbar).
pub fn bar(style: &egui::Style) -> egui::Frame {
    egui::Frame::window(style)
        .corner_radius(13.0)
        .inner_margin(egui::Margin::symmetric(8, 5))
}

/// A filled accent button (the primary action of a panel).
pub fn primary(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text).color(Color32::WHITE).strong()).fill(ACCENT)
}

#[cfg(test)]
mod tests {
    use egui::{CursorIcon, Event, RawInput, pos2};

    /// Cursor after two passes with the pointer at `at` (hits come from the previous pass).
    fn cursor_at(at: egui::Pos2) -> CursorIcon {
        let ctx = egui::Context::default();
        let mut icon = CursorIcon::Default;
        for _ in 0..3 {
            let raw = RawInput {
                events: vec![Event::PointerMoved(at)],
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                egui::Window::new("panel")
                    .title_bar(false)
                    .resizable(false)
                    .fixed_pos(pos2(0.0, 0.0))
                    .movable(true)
                    .show(ui.ctx(), |ui| {
                        ui.add_sized([100.0, 30.0], egui::Button::new("button"));
                        ui.add_space(40.0);
                        ui.add_sized([100.0, 30.0], egui::Label::new("label"));
                    });
                super::cursors(ui.ctx());
            });
            out.textures_delta.clear();
            icon = out.platform_output.cursor_icon;
        }
        icon
    }

    #[test]
    fn pointer_on_buttons_grab_on_movable_panels() {
        assert_eq!(cursor_at(pos2(50.0, 20.0)), CursorIcon::PointingHand);
        assert_eq!(cursor_at(pos2(50.0, 60.0)), CursorIcon::Grab);
        assert_eq!(cursor_at(pos2(50.0, 95.0)), CursorIcon::Text);
        assert_eq!(cursor_at(pos2(400.0, 400.0)), CursorIcon::Default);
    }
}
