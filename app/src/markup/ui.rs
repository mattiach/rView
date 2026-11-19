//! Markup interface in egui (egui draws only the UI — toolbar, Inspector,
//! handles, selection and crop frames — never document content): a floating toolbar centred at the
//! top and a compact Inspector for the selection or the next shape.

use super::select::{Handle, SelKind};
use super::{Markup, Out, Tool, crop};
use crate::i18n::{t, tf};
use crate::viewer::Affine;
use egui::{Color32, Pos2, Stroke};
use rv_core::edit::*;

use crate::ui::style::ACCENT;

/// Image point → egui position (screen px / pixels per point).
fn to_pos(t: &Affine, ppp: f32, p: PointF) -> Pos2 {
    let (x, y) = t.apply(p.x, p.y);
    Pos2::new(x / ppp, y / ppp)
}

/// Draw the UI; Some(out) when Done, Save or Export was pressed.
pub fn show(ctx: &egui::Context, m: &mut Markup, t: &Affine, ppp: f32, scale: f32) -> Option<Out> {
    paint_frames(ctx, m, t, ppp, scale);
    let mut out = None;
    toolbar(ctx, m, &mut out);
    inspector(ctx, m);
    // an Inspector change is one history entry until its control is let go
    if m.inspector_gesture && !ctx.egui_is_using_pointer() && !egui::Popup::is_any_open(ctx) {
        m.history.end_gesture();
        m.inspector_gesture = false;
    }
    out
}

fn toolbar(ctx: &egui::Context, m: &mut Markup, out: &mut Option<Out>) {
    use crate::ui::icons::{self, Icon};
    let id = egui::Id::unique("markup toolbar");
    // moved only by its grip; the offset is kept for the window's life
    let offset = ctx.data(|d| d.get_temp::<egui::Vec2>(id)).unwrap_or_default();
    egui::Area::new(id)
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 6.0) + offset)
        .movable(false)
        .show(ctx, |ui| {
            crate::ui::style::bar(ui.style()).show(ui, |ui| {
                // narrow windows: the bar wraps onto more rows instead of being cut
                ui.set_max_width((ctx.content_rect().width() - 24.0).max(120.0));
                ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
                ui.horizontal_wrapped(|ui| {
                    grip(ui, id, offset);
                    for (tool, icon, id, key) in [
                        (Tool::Select, Icon::Select, "markup-tool-select", "V"),
                        (Tool::Crop, Icon::Crop, "markup-tool-crop", "C"),
                        (Tool::Draw(ShapeKind::Rect), Icon::Rect, "markup-tool-rect", "R"),
                        (
                            Tool::Draw(ShapeKind::Ellipse),
                            Icon::Ellipse,
                            "markup-tool-ellipse",
                            "E",
                        ),
                        (Tool::Draw(ShapeKind::Line), Icon::Line, "markup-tool-line", "L"),
                        (Tool::Draw(ShapeKind::Arrow), Icon::Arrow, "markup-tool-arrow", "A"),
                        (Tool::Draw(ShapeKind::Pen), Icon::Pen, "markup-tool-pen", "P"),
                        (Tool::Text, Icon::Text, "markup-tool-text", "T"),
                        (Tool::Draw(ShapeKind::Loupe), Icon::Loupe, "markup-tool-loupe", ""),
                        (Tool::Redact, Icon::Blur, "markup-tool-redact", "B"),
                    ] {
                        let tip = if key.is_empty() {
                            t(id)
                        } else {
                            format!("{} ({key})", t(id))
                        };
                        if icons::button(ui, icon, m.tool == tool, tip).clicked() {
                            m.set_tool(tool);
                        }
                    }
                    ui.separator();
                    for (name, icon, id) in [
                        ("rotate-left", Icon::RotateLeft, "markup-rotate-left"),
                        ("rotate-right", Icon::RotateRight, "markup-rotate-right"),
                        ("flip-h", Icon::FlipH, "markup-flip-h"),
                        ("flip-v", Icon::FlipV, "markup-flip-v"),
                    ] {
                        if icons::button(ui, icon, false, t(id)).clicked() {
                            m.button(name);
                        }
                    }
                    ui.separator();
                    for (icon, id, key, can) in [
                        (Icon::Undo, "markup-undo", "Ctrl+Z", m.history.can_undo()),
                        (Icon::Redo, "markup-redo", "Ctrl+Y", m.history.can_redo()),
                    ] {
                        let r =
                            ui.add_enabled_ui(can, |ui| icons::button(ui, icon, false, format!("{} ({key})", t(id))));
                        if r.inner.clicked() {
                            if icon == Icon::Undo { m.undo() } else { m.redo() }
                        }
                    }
                    ui.separator();
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if ui.button(t("markup-export")).on_hover_text("Ctrl+Shift+S").clicked() {
                        *out = Some(m.button("export"));
                    }
                    if ui.add(crate::ui::style::primary(t("markup-done"))).clicked() {
                        *out = Some(Out::Exit);
                    }
                });
            });
        });
}

/// The handle at the left end of the toolbar: dragging it moves the bar, kept on screen.
fn grip(ui: &mut egui::Ui, id: egui::Id, offset: egui::Vec2) {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(14.0, 26.0), egui::Sense::drag());
    let resp = resp.on_hover_text(t("markup-move-bar"));
    let color = ui.style().interact(&resp).fg_stroke.color;
    crate::ui::icons::paint(
        ui.painter(),
        rect.shrink2(egui::vec2(3.0, 7.0)),
        crate::ui::icons::Icon::Grip,
        color,
    );
    if resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        let screen = ui.ctx().content_rect();
        let bar = ui.ctx().memory(|m| m.area_rect(id)).unwrap_or(screen);
        let d = resp.drag_delta();
        let dx = d.x.clamp(screen.left() - bar.left(), screen.right() - bar.right());
        let dy = d.y.clamp(screen.top() - bar.top(), screen.bottom() - bar.bottom());
        ui.ctx().data_mut(|m| m.insert_temp(id, offset + egui::vec2(dx, dy)));
    } else if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
}

fn start_gesture(m: &mut Markup) {
    m.pause_text();
    if !m.inspector_gesture {
        m.history.begin_gesture();
        m.inspector_gesture = true;
    }
}

fn inspector(ctx: &egui::Context, m: &mut Markup) {
    egui::Window::new(t("markup-inspector"))
        .id(egui::Id::unique("markup inspector"))
        // starts at the top right; the user can drag it off the shapes
        .pivot(egui::Align2::RIGHT_TOP)
        .default_pos(egui::pos2(ctx.content_rect().right() - 12.0, 56.0))
        .resizable(false)
        .title_bar(false)
        .show(ctx, |ui| {
            ui.set_width(236.0);
            if !m.missing_fonts.is_empty() {
                let fams = m.missing_fonts.join(", ");
                ui.label(
                    egui::RichText::new(tf("markup-fonts-missing", &[("fonts", &fams)]))
                        .small()
                        .color(Color32::from_rgb(0xE0, 0xA0, 0x30)),
                );
            }
            if m.tool == Tool::Crop {
                crop_panel(ui, m);
                return;
            }
            let doc = m.doc().clone();
            match m.sel.kind(&doc) {
                SelKind::Shapes => {
                    let first = m.sel.ids.iter().find_map(|id| match doc.find(*id) {
                        Some(ObjRef::Shape(i)) => Some(doc.shapes[i].clone()),
                        _ => None,
                    });
                    if let Some(s) = first {
                        let n = m.sel.ids.len();
                        title(
                            ui,
                            &if n == 1 {
                                t(kind_id(s.geom.kind()))
                            } else {
                                tf("markup-n-selected", &[("n", &n.to_string())])
                            },
                        );
                        let mixed = m.sel.ids.iter().any(|id| match doc.find(*id) {
                            Some(ObjRef::Shape(i)) => doc.shapes[i].style != s.style,
                            _ => false,
                        });
                        if mixed {
                            ui.label(egui::RichText::new(t("markup-mixed")).weak());
                        }
                        style_editor(ui, m, &s.style, Some(&s.geom));
                        selection_buttons(ui, m);
                    }
                }
                SelKind::Redactions => {
                    let first = m.sel.ids.iter().find_map(|id| match doc.find(*id) {
                        Some(ObjRef::Redaction(i)) => Some(doc.redactions[i].clone()),
                        _ => None,
                    });
                    if let Some(r) = first {
                        let n = m.sel.ids.len();
                        title(
                            ui,
                            &if n == 1 {
                                t("markup-kind-redaction")
                            } else {
                                tf("markup-n-selected", &[("n", &n.to_string())])
                            },
                        );
                        redaction_editor(ui, m, &r.kind, matches!(r.area, Area::Ellipse { .. }));
                    }
                    selection_buttons(ui, m);
                }
                SelKind::Mixed => {
                    title(ui, &tf("markup-n-selected", &[("n", &m.sel.ids.len().to_string())]));
                    selection_buttons(ui, m);
                }
                SelKind::Nothing => match m.tool {
                    Tool::Redact => {
                        title(ui, &t("markup-tool-redact"));
                        let (k, e) = (m.redact_kind.clone(), m.redact_ellipse);
                        redaction_editor(ui, m, &k, e);
                    }
                    Tool::Text => {
                        title(ui, &t("markup-tool-text"));
                        let st = m.style_for(ShapeKind::Text);
                        style_editor(ui, m, &st, None);
                        let (f, a) = m.text_font.clone();
                        font_editor(ui, m, &f, a);
                    }
                    Tool::Draw(kind) => {
                        title(ui, &t(kind_id(kind)));
                        let st = m.style_for(kind);
                        style_editor(ui, m, &st, None);
                    }
                    _ => {
                        title(ui, &t("markup-inspector"));
                        ui.label(egui::RichText::new(t("markup-hint")).weak());
                    }
                },
            }
        });
}

/// Message id of a shape kind's name.
fn kind_id(k: ShapeKind) -> &'static str {
    match k {
        ShapeKind::Rect => "markup-kind-rect",
        ShapeKind::Ellipse => "markup-kind-ellipse",
        ShapeKind::Line => "markup-kind-line",
        ShapeKind::Arrow => "markup-kind-arrow",
        ShapeKind::Pen => "markup-kind-pen",
        ShapeKind::Highlight => "markup-kind-highlight",
        ShapeKind::Text => "markup-kind-text",
        ShapeKind::Loupe => "markup-kind-loupe",
    }
}

/// The panel's heading: what is selected (or the tool), then a rule.
fn title(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).strong().size(15.0));
    ui.separator();
}

/// The panel's footer: Delete and Duplicate on the left, Confirm (while the element just inserted
/// is being adjusted) on the right.
fn selection_buttons(ui: &mut egui::Ui, m: &mut Markup) {
    let confirming = m.adjusting() || m.editing.is_some();
    ui.add_space(4.0);
    if confirming {
        note(ui, "markup-confirm-hint");
    }
    ui.separator();
    ui.horizontal(|ui| {
        if ui.button(t("markup-delete")).clicked() {
            m.delete_selection();
        }
        if ui.button(t("markup-duplicate")).clicked() {
            m.duplicate();
        }
        if confirming {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let key = if m.editing.is_some() { "Ctrl+Enter" } else { "Enter" };
                if ui
                    .add(crate::ui::style::primary(t("markup-confirm")))
                    .on_hover_text(key)
                    .clicked()
                {
                    m.confirm();
                }
            });
        }
    });
}

/// Style controls; `geom` = the selected shape's geometry (its kind's extra controls).
fn style_editor(ui: &mut egui::Ui, m: &mut Markup, st: &Style, geom: Option<&Geom>) {
    let kind = geom.map(|g| g.kind()).or(match m.tool {
        Tool::Draw(k) => Some(k),
        Tool::Text => Some(ShapeKind::Text),
        _ => None,
    });
    let is_text = kind == Some(ShapeKind::Text);
    // lines and loupes: an outline only
    let line_like = matches!(
        kind,
        Some(ShapeKind::Line | ShapeKind::Arrow | ShapeKind::Pen | ShapeKind::Highlight | ShapeKind::Loupe)
    );
    let mut next = st.clone();
    egui::Grid::new("markup style")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label(t(if is_text { "markup-text-colour" } else { "markup-stroke" }));
            ui.horizontal(|ui| {
                let mut on = next.stroke.is_some();
                if !line_like && !is_text && ui.checkbox(&mut on, "").changed() {
                    next.stroke = on.then_some(st.stroke.unwrap_or(Color::rgb(0xE5, 0x39, 0x35)));
                }
                if let Some(c) = next.stroke {
                    let mut rgb = [c.r, c.g, c.b];
                    if crate::ui::color::edit_button(ui, &mut rgb).changed() {
                        next.stroke = Some(Color::rgb(rgb[0], rgb[1], rgb[2]));
                    }
                }
            });
            ui.end_row();
            if !line_like {
                ui.label(t(if is_text {
                    "markup-text-background"
                } else {
                    "markup-fill"
                }));
                ui.horizontal(|ui| {
                    let mut on = next.fill.is_some();
                    if ui.checkbox(&mut on, "").changed() {
                        next.fill = on.then_some(st.fill.unwrap_or(Color::rgb(255, 255, 255)));
                    }
                    if let Some(c) = next.fill {
                        let mut rgb = [c.r, c.g, c.b];
                        if crate::ui::color::edit_button(ui, &mut rgb).changed() {
                            next.fill = Some(Color::rgb(rgb[0], rgb[1], rgb[2]));
                        }
                    }
                });
                ui.end_row();
            }
            if !is_text {
                ui.label(t("markup-width"));
                ui.add(egui::Slider::new(&mut next.width, 1.0..=40.0).suffix(" px"));
                ui.end_row();
            }
            ui.label(t("markup-opacity"));
            let mut pct = (next.opacity * 100.0).round();
            if ui.add(egui::Slider::new(&mut pct, 0.0..=100.0).suffix(" %")).changed() {
                next.opacity = pct / 100.0;
            }
            ui.end_row();
        });
    if matches!(kind, Some(ShapeKind::Rect | ShapeKind::Ellipse))
        && next.fill.is_some_and(|c| c.a == 255)
        && next.opacity >= 1.0
    {
        note(ui, "markup-fill-not-redaction");
    }
    if next != *st {
        start_gesture(m);
        let n = next.clone();
        m.set_style(&move |s: &mut Style| *s = n.clone());
    }
    // per-kind geometry options
    match geom {
        Some(Geom::Rect { corner_radius, .. }) => {
            let mut r = *corner_radius;
            ui.horizontal(|ui| {
                ui.label(t("markup-corners"));
                if ui.add(egui::Slider::new(&mut r, 0.0..=100.0).suffix(" px")).changed() {
                    start_gesture(m);
                    m.set_geom(&move |g| match g {
                        Geom::Rect { rect, rotation, .. } => Some(Geom::Rect {
                            rect: *rect,
                            rotation: *rotation,
                            corner_radius: r,
                        }),
                        _ => None,
                    });
                }
            });
        }
        Some(Geom::Arrow { heads, bend, .. }) => {
            let (mut both, mut curve) = (*heads == ArrowHeads::Both, (*bend * 100.0).round());
            let mut held = false;
            ui.horizontal(|ui| {
                ui.label(t("markup-arrow-curve"));
                held = ui
                    .add(egui::Slider::new(&mut curve, -50.0..=50.0).suffix(" %"))
                    .dragged();
            });
            ui.checkbox(&mut both, t("markup-arrow-both"));
            let (h, k) = (if both { ArrowHeads::Both } else { ArrowHeads::End }, curve / 100.0);
            if h != *heads || k != *bend {
                if held {
                    start_gesture(m);
                }
                m.set_geom(&move |g| match g {
                    Geom::Arrow { a, b, .. } => Some(Geom::Arrow {
                        a: *a,
                        b: *b,
                        heads: h,
                        bend: k,
                    }),
                    _ => None,
                });
            }
        }
        Some(Geom::Text { font, align, .. }) => font_editor(ui, m, font, *align),
        Some(Geom::Loupe { zoom, .. }) => {
            let mut z = *zoom;
            ui.horizontal(|ui| {
                ui.label(t("markup-loupe-zoom"));
                let r = ui.add(egui::Slider::new(&mut z, 2.0..=4.0).suffix("×"));
                if r.changed() {
                    if r.dragged() {
                        start_gesture(m);
                    }
                    m.set_geom(&move |g| match g {
                        Geom::Loupe { center, radius, .. } => Some(Geom::Loupe {
                            center: *center,
                            radius: *radius,
                            zoom: z,
                        }),
                        _ => None,
                    });
                }
            });
        }
        _ => {}
    }
}

fn note(ui: &mut egui::Ui, id: &str) {
    ui.add(egui::Label::new(egui::RichText::new(t(id)).small().weak()).wrap());
}

/// Mode, intensity/colour and shape of redactions, with the protection text of the mode.
fn redaction_editor(ui: &mut egui::Ui, m: &mut Markup, kind: &RedactKind, ellipse: bool) {
    let (mut k, mut e) = (kind.clone(), ellipse);
    let mut held = false;
    egui::Grid::new("redaction")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label(t("markup-redact-mode"));
            ui.horizontal(|ui| {
                let (solid, blur, pixelate) = (
                    matches!(k, RedactKind::Solid { .. }),
                    matches!(k, RedactKind::Blur { .. }),
                    matches!(k, RedactKind::Pixelate { .. }),
                );
                if ui.selectable_label(solid, t("markup-redact-solid")).clicked() && !solid {
                    k = RedactKind::Solid { color: [0, 0, 0] };
                }
                if ui.selectable_label(blur, t("markup-redact-blur")).clicked() && !blur {
                    k = RedactKind::Blur { radius: 20.0 };
                }
                if ui.selectable_label(pixelate, t("markup-redact-pixelate")).clicked() && !pixelate {
                    k = RedactKind::Pixelate { block: 16 };
                }
            });
            ui.end_row();
            match &mut k {
                RedactKind::Blur { radius } => {
                    ui.label(t("markup-redact-radius"));
                    held = ui.add(egui::Slider::new(radius, 1.0..=100.0).suffix(" px")).dragged();
                }
                RedactKind::Pixelate { block } => {
                    ui.label(t("markup-redact-block"));
                    held = ui.add(egui::Slider::new(block, 2..=100).suffix(" px")).dragged();
                }
                RedactKind::Solid { color } => {
                    ui.label(t("markup-redact-colour"));
                    crate::ui::color::edit_button(ui, color);
                }
            }
            ui.end_row();
            ui.label(t("markup-redact-shape"));
            ui.horizontal(|ui| {
                ui.selectable_value(&mut e, false, t("markup-kind-rect"));
                ui.selectable_value(&mut e, true, t("markup-kind-ellipse"));
            });
            ui.end_row();
        });
    note(
        ui,
        match k {
            RedactKind::Solid { .. } => "markup-protect-solid",
            RedactKind::Blur { .. } => "markup-protect-blur",
            RedactKind::Pixelate { .. } => "markup-protect-pixelate",
        },
    );
    if k != *kind || e != ellipse {
        if held {
            start_gesture(m);
        }
        m.set_redaction(&move |kk, ee| {
            *kk = k.clone();
            *ee = e;
        });
    }
}

/// Font family, size, bold/italic and alignment of text boxes.
fn font_editor(ui: &mut egui::Ui, m: &mut Markup, font: &FontSpec, align: TextAlign) {
    let (mut f, mut a) = (font.clone(), align);
    let mut held = false;
    egui::Grid::new("font")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label(t("markup-font"));
            egui::ComboBox::from_id_salt("font family")
                .selected_text(&f.family)
                .height(300.0)
                .show_ui(ui, |ui| {
                    for fam in rv_core::edit::text::families() {
                        let sel = fam == f.family;
                        if ui.selectable_label(sel, &fam).clicked() {
                            f.family = fam;
                        }
                    }
                });
            ui.end_row();
            ui.label(t("markup-font-size"));
            let r = ui.add(
                egui::Slider::new(
                    &mut f.size,
                    *rv_core::project::TEXT_SIZES.start()..=*rv_core::project::TEXT_SIZES.end(),
                )
                .logarithmic(true)
                .suffix(" px"),
            );
            held = r.dragged();
            ui.end_row();
            ui.label("");
            ui.horizontal(|ui| {
                let mut bold = f.weight >= 600;
                if ui.toggle_value(&mut bold, egui::RichText::new("B").strong()).changed() {
                    f.weight = if bold { 700 } else { 400 };
                }
                ui.toggle_value(&mut f.italic, egui::RichText::new("I").italics());
                ui.separator();
                for (al, id) in [
                    (TextAlign::Left, "markup-align-left"),
                    (TextAlign::Center, "markup-align-center"),
                    (TextAlign::Right, "markup-align-right"),
                ] {
                    ui.selectable_value(&mut a, al, t(id));
                }
            });
            ui.end_row();
        });
    if f != *font || a != align {
        if held {
            start_gesture(m);
        }
        m.set_text_font(&move |ff, aa| {
            *ff = f.clone();
            *aa = a;
        });
    }
}

fn crop_panel(ui: &mut egui::Ui, m: &mut Markup) {
    let doc = m.doc().clone();
    let Some(c) = &mut m.crop else { return };
    title(ui, &t("markup-tool-crop"));
    let current = crop::PRESETS
        .iter()
        .find(|(_, a)| *a == c.aspect)
        .map(|(id, _)| t(id))
        .unwrap_or_default();
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("crop aspect")
            .selected_text(current)
            .show_ui(ui, |ui| {
                for (id, a) in crop::PRESETS {
                    if ui.selectable_label(c.aspect == *a, t(id)).clicked() {
                        c.set_aspect(&doc, *a);
                    }
                }
            });
        if ui.button(t("markup-crop-swap")).on_hover_text("X").clicked() {
            c.invert(&doc);
        }
    });
    let shown = doc.orient.apply_size(c.rect.w.round() as u32, c.rect.h.round() as u32);
    ui.label(tf(
        "markup-crop-size",
        &[("w", &shown.0.to_string()), ("h", &shown.1.to_string())],
    ));
    ui.horizontal(|ui| {
        if ui.button(t("markup-crop-apply")).on_hover_text("Enter").clicked() {
            m.button("crop-apply");
        }
        if ui.button(t("markup-crop-reset")).clicked() {
            m.button("crop-reset");
        }
        if ui.button(t("button-cancel")).on_hover_text("Esc").clicked() {
            m.button("crop-cancel");
        }
    });
}

/// Selection frames, handles, rubber band, crop frame — painted under the egui windows.
fn paint_frames(ctx: &egui::Context, m: &Markup, tr: &Affine, ppp: f32, scale: f32) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let pos = |p: PointF| to_pos(tr, ppp, p);
    let doc = m.doc();
    let thin = Stroke::new(1.5, ACCENT);
    if let Some(c) = &m.crop {
        let r = c.rect;
        let corners = [
            PointF { x: r.x, y: r.y },
            PointF { x: r.x + r.w, y: r.y },
            PointF {
                x: r.x + r.w,
                y: r.y + r.h,
            },
            PointF { x: r.x, y: r.y + r.h },
        ]
        .map(pos);
        let (img_w, img_h) = (doc.image_size.w as f32, doc.image_size.h as f32);
        let all = egui::Rect::from_two_pos(pos(PointF { x: 0.0, y: 0.0 }), pos(PointF { x: img_w, y: img_h }));
        let inner = egui::Rect::from_two_pos(corners[0], corners[2]);
        let dim = Color32::from_black_alpha(150);
        // the outside darkened: four bands around the draft
        for band in [
            egui::Rect::from_min_max(all.min, egui::pos2(all.max.x, inner.min.y)),
            egui::Rect::from_min_max(egui::pos2(all.min.x, inner.max.y), all.max),
            egui::Rect::from_min_max(egui::pos2(all.min.x, inner.min.y), egui::pos2(inner.min.x, inner.max.y)),
            egui::Rect::from_min_max(egui::pos2(inner.max.x, inner.min.y), egui::pos2(all.max.x, inner.max.y)),
        ] {
            painter.rect_filled(band, 0.0, dim);
        }
        painter.rect_stroke(inner, 0.0, Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Middle);
        if c.drag.is_some() {
            // rule of thirds while dragging
            for i in 1..3 {
                let f = i as f32 / 3.0;
                let x = inner.min.x + inner.width() * f;
                let y = inner.min.y + inner.height() * f;
                let s = Stroke::new(1.0, Color32::from_white_alpha(140));
                painter.line_segment([egui::pos2(x, inner.min.y), egui::pos2(x, inner.max.y)], s);
                painter.line_segment([egui::pos2(inner.min.x, y), egui::pos2(inner.max.x, y)], s);
            }
        }
        for (h, p) in super::select::frame_handles(&r, 0.0, 0.0) {
            if h != Handle::Rotate {
                handle_box(&painter, pos(p));
            }
        }
        return;
    }
    // redactions: a dashed outline (their look is not a border)
    let dashed = |pts: Vec<Pos2>| {
        let mut closed = pts.clone();
        closed.push(pts[0]);
        painter.extend(egui::Shape::dashed_line(&closed, thin, 5.0, 4.0));
    };
    if let Some(r) = &m.pending_red {
        let (Area::Rect { rect, rotation } | Area::Ellipse { rect, rotation }) = &r.area;
        let hs = super::select::frame_handles(rect, *rotation, 0.0);
        dashed(hs[..4].iter().map(|(_, p)| pos(*p)).collect());
    }
    if let Some(e) = &m.editing {
        paint_caret(&painter, m, e, &pos);
    }
    for id in &m.sel.ids {
        if let Some((rect, rot)) = super::select::frame(doc, *id) {
            let hs = super::select::frame_handles(&rect, rot, 0.0);
            let pts: Vec<Pos2> = hs[..4].iter().map(|(_, p)| pos(*p)).collect();
            if matches!(doc.find(*id), Some(ObjRef::Redaction(_))) {
                dashed(pts);
            } else {
                painter.add(egui::Shape::closed_line(pts, thin));
            }
        } else if let Some(b) = super::select::object_bounds(doc, *id) {
            let r = egui::Rect::from_two_pos(
                pos(PointF { x: b.x, y: b.y }),
                pos(PointF {
                    x: b.x + b.w,
                    y: b.y + b.h,
                }),
            );
            painter.rect_stroke(r, 0.0, thin, egui::StrokeKind::Outside);
        }
    }
    let handles = m.handles(scale);
    if let Some((_, top)) = handles.iter().find(|(h, _)| *h == Handle::Edge(0))
        && let Some((_, rot)) = handles.iter().find(|(h, _)| *h == Handle::Rotate)
    {
        painter.line_segment([pos(*top), pos(*rot)], thin);
    }
    for (h, p) in handles {
        if h == Handle::Rotate {
            painter.circle(pos(p), 5.0, Color32::WHITE, thin);
        } else {
            handle_box(&painter, pos(p));
        }
    }
    if let Some(b) = m.band() {
        let r = egui::Rect::from_two_pos(
            pos(PointF { x: b.x, y: b.y }),
            pos(PointF {
                x: b.x + b.w,
                y: b.y + b.h,
            }),
        );
        painter.rect(
            r,
            0.0,
            Color32::from_rgba_unmultiplied(0x2D, 0x8C, 0xFF, 30),
            thin,
            egui::StrokeKind::Middle,
        );
    }
}

/// Caret and selection of the edited text box (egui draws only these).
fn paint_caret(painter: &egui::Painter, m: &Markup, e: &super::textedit::TextEdit, pos: &dyn Fn(PointF) -> Pos2) {
    let doc = m.doc();
    let Some(ObjRef::Shape(i)) = doc.find(e.id) else { return };
    let Geom::Text {
        rect,
        rotation,
        text,
        font,
        align,
    } = &doc.shapes[i].geom
    else {
        return;
    };
    let l = rv_core::edit::text::layout(text, font, rect.w, *align);
    // box coordinates (from its top-left, unrotated) → image space
    let c = rect.center();
    let (s, co) = rotation.sin_cos();
    let img = |x: f32, y: f32| {
        let (dx, dy) = (x - rect.w / 2.0, y - rect.h / 2.0);
        PointF {
            x: c.x + dx * co - dy * s,
            y: c.y + dx * s + dy * co,
        }
    };
    let quad =
        |x0: f32, y0: f32, x1: f32, y1: f32| [img(x0, y0), img(x1, y0), img(x1, y1), img(x0, y1)].map(pos).to_vec();
    if let Some((a, b)) = e.selection() {
        for (x0, top, x1, h) in super::textedit::selection_rects(&l, a, b) {
            painter.add(egui::Shape::convex_polygon(
                quad(x0, top, x1.max(x0 + 2.0), top + h),
                Color32::from_rgba_unmultiplied(0x2D, 0x8C, 0xFF, 70),
                Stroke::NONE,
            ));
        }
    }
    let (x, top, h) = super::textedit::caret(&l, e.cursor);
    painter.line_segment([pos(img(x, top)), pos(img(x, top + h))], Stroke::new(2.0, ACCENT));
    // the box being edited
    painter.add(egui::Shape::closed_line(
        quad(0.0, 0.0, rect.w, l.height),
        Stroke::new(1.0, ACCENT.gamma_multiply(0.6)),
    ));
}

fn handle_box(p: &egui::Painter, at: Pos2) {
    let r = egui::Rect::from_center_size(at, egui::vec2(8.0, 8.0));
    p.rect(
        r,
        1.0,
        Color32::WHITE,
        Stroke::new(1.5, ACCENT),
        egui::StrokeKind::Middle,
    );
}
