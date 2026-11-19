//! Icons drawn as vectors (egui's fonts carry no symbols): sharp at every display scale, one
//! look. `button` is an icon-only button with a tooltip.

use egui::{Color32, Pos2, Rect, Stroke, pos2, vec2};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Select,
    Crop,
    Rect,
    Ellipse,
    Line,
    Arrow,
    Pen,
    Text,
    Loupe,
    Blur,
    RotateLeft,
    RotateRight,
    FlipH,
    FlipV,
    Undo,
    Redo,
    Close,
    Copy,
    /// Two columns of dots: the universal "drag here" handle.
    Grip,
}

/// Icon-only button (28×26 pt), highlighted while `selected`.
pub fn button(ui: &mut egui::Ui, icon: Icon, selected: bool, tip: impl Into<egui::WidgetText>) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(28.0, 26.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let v = ui.style().interact_selectable(&resp, selected);
        if selected || resp.hovered() || resp.is_pointer_button_down_on() {
            let bg = if selected { super::style::ACCENT } else { v.bg_fill };
            ui.painter().rect_filled(rect, 7.0, bg);
        }
        let ink = if selected { Color32::WHITE } else { v.fg_stroke.color };
        paint(ui.painter(), rect.shrink2(vec2(6.5, 5.5)), icon, ink);
    }
    resp.on_hover_text(tip)
}

/// Points of an arc (centre and radius in unit coordinates, angles in degrees, clockwise on
/// screen from the positive x axis).
fn arc(cx: f32, cy: f32, r: f32, from: f32, to: f32) -> Vec<(f32, f32)> {
    (0..=24)
        .map(|i| {
            let a = (from + (to - from) * i as f32 / 24.0).to_radians();
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

/// Draw `icon` inside `r` with `color`.
pub fn paint(p: &egui::Painter, r: Rect, icon: Icon, color: Color32) {
    use Icon::*;
    let s = Stroke::new(1.6, color);
    // the left-handed twins are the mirror image of the right-handed ones
    let mirror = matches!(icon, RotateLeft | Undo);
    let at = |(x, y): (f32, f32)| -> Pos2 {
        let x = if mirror { 1.0 - x } else { x };
        pos2(r.left() + x * r.width(), r.top() + y * r.height())
    };
    let line = |pts: &[(f32, f32)]| {
        p.add(egui::Shape::line(pts.iter().map(|q| at(*q)).collect(), s));
    };
    // arrow head at the end of a polyline (unit coordinates, corrected for the aspect)
    let head = |pts: &[(f32, f32)], len: f32| {
        let (tip, prev) = (at(pts[pts.len() - 1]), at(pts[pts.len() - 2]));
        let d = (tip - prev).normalized() * len * r.width();
        for a in [0.5f32, -0.5] {
            let (sn, cs) = a.sin_cos();
            let back = vec2(d.x * cs - d.y * sn, d.x * sn + d.y * cs);
            p.line_segment([tip, tip - back], s);
        }
    };
    match icon {
        Select => {
            let pts = [
                (0.22, 0.02),
                (0.22, 0.86),
                (0.42, 0.66),
                (0.56, 0.98),
                (0.70, 0.92),
                (0.57, 0.61),
                (0.84, 0.61),
            ];
            p.add(egui::Shape::closed_line(pts.iter().map(|q| at(*q)).collect(), s));
        }
        Crop => {
            line(&[(0.24, 0.0), (0.24, 0.76), (1.0, 0.76)]);
            line(&[(0.0, 0.24), (0.76, 0.24), (0.76, 1.0)]);
        }
        Rect => {
            p.rect_stroke(
                egui::Rect::from_two_pos(at((0.04, 0.16)), at((0.96, 0.84))),
                2.0,
                s,
                egui::StrokeKind::Middle,
            );
        }
        Ellipse => {
            p.add(egui::Shape::ellipse_stroke(
                r.center(),
                vec2(r.width() * 0.48, r.height() * 0.36),
                s,
            ));
        }
        Line => line(&[(0.08, 0.92), (0.92, 0.08)]),
        Arrow => {
            let pts = [(0.08, 0.92), (0.92, 0.08)];
            line(&pts);
            head(&pts, 0.42);
        }
        Pen => {
            let pts: Vec<(f32, f32)> = (0..=20)
                .map(|i| {
                    let x = i as f32 / 20.0;
                    (0.04 + 0.92 * x, 0.5 - 0.32 * (x * 7.5).sin() * (0.4 + 0.6 * x))
                })
                .collect();
            line(&pts);
        }
        Text => {
            line(&[(0.12, 0.1), (0.88, 0.1)]);
            line(&[(0.5, 0.1), (0.5, 0.94)]);
            line(&[(0.34, 0.94), (0.66, 0.94)]);
        }
        Loupe => {
            p.circle_stroke(at((0.42, 0.42)), r.width() * 0.34, s);
            line(&[(0.68, 0.68), (0.98, 0.98)]);
        }
        Blur => {
            // a mosaic fading out: what blurring and pixelating do
            for (i, a) in [255u8, 150, 70].into_iter().enumerate() {
                for (j, b) in [255u8, 170, 90].into_iter().enumerate() {
                    let c = color.gamma_multiply(a as f32 / 255.0 * b as f32 / 255.0);
                    let q = egui::Rect::from_two_pos(
                        at((0.02 + 0.34 * j as f32, 0.02 + 0.34 * i as f32)),
                        at((0.30 + 0.34 * j as f32, 0.30 + 0.34 * i as f32)),
                    );
                    p.rect_filled(q, 1.5, c);
                }
            }
        }
        RotateLeft | RotateRight => {
            let pts = arc(0.5, 0.54, 0.42, -205.0, 45.0);
            line(&pts);
            head(&pts[pts.len() - 3..], 0.34);
        }
        Undo | Redo => {
            let pts = arc(0.5, 0.78, 0.52, -165.0, -18.0);
            line(&pts);
            head(&pts[pts.len() - 3..], 0.36);
        }
        FlipH | FlipV => {
            // FlipV is FlipH turned by a quarter
            let q = |(x, y): (f32, f32)| if icon == FlipV { at((y, x)) } else { at((x, y)) };
            p.add(egui::Shape::dashed_line(
                &[q((0.5, 0.0)), q((0.5, 1.0))],
                Stroke::new(1.2, color),
                2.5,
                2.0,
            ));
            p.add(egui::Shape::closed_line(
                vec![q((0.36, 0.2)), q((0.36, 0.8)), q((0.0, 0.8))],
                s,
            ));
            p.add(egui::Shape::convex_polygon(
                vec![q((0.64, 0.2)), q((0.64, 0.8)), q((1.0, 0.8))],
                color,
                Stroke::NONE,
            ));
        }
        Close => {
            line(&[(0.15, 0.15), (0.85, 0.85)]);
            line(&[(0.85, 0.15), (0.15, 0.85)]);
        }
        Copy => {
            let sheet = |a, b| egui::Rect::from_two_pos(at(a), at(b));
            p.rect_stroke(sheet((0.34, 0.3), (0.96, 1.0)), 2.0, s, egui::StrokeKind::Middle);
            line(&[(0.66, 0.16), (0.66, 0.0), (0.04, 0.0), (0.04, 0.7), (0.2, 0.7)]);
        }
        Grip => {
            for x in [0.3, 0.7] {
                for y in [0.15, 0.5, 0.85] {
                    p.circle_filled(at((x, y)), 1.6, color);
                }
            }
        }
    }
}
