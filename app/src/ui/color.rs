//! Opaque colour button with its picker popup: saturation/value square, hue bar, R G B values.
//! Replaces egui's picker, whose copy button is an emoji glyph the UI fonts don't have.

use egui::ecolor::{Hsva, HsvaGamma};
use egui::{Color32, Mesh, Pos2, Rect, Response, Sense, Stroke, Vec2, vec2};

const WIDTH: f32 = 256.0;
/// Gradient quads per side.
const STEPS: usize = 16;

/// Returns the button's response, `changed()` when the colour was edited.
pub fn edit_button(ui: &mut egui::Ui, rgb: &mut [u8; 3]) -> Response {
    let [r, g, b] = *rgb;
    let mut button = ui.add(
        egui::Button::new("")
            .fill(Color32::from_rgb(r, g, b))
            .min_size(ui.spacing().interact_size),
    );
    let popup_id = button.id.with("colour popup");
    egui::Popup::menu(&button)
        .id(popup_id)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            // hue and saturation survive while the colour is grey or black
            let mut hsv = ui
                .data(|d| d.get_temp::<HsvaGamma>(popup_id))
                .filter(|h| to_rgb(*h) == *rgb)
                .unwrap_or_else(|| HsvaGamma::from(Hsva::from_srgb(*rgb)));
            let before = hsv;
            sv_square(ui, &mut hsv);
            hue_bar(ui, &mut hsv);
            let mut values = to_rgb(hsv);
            let mut typed = false;
            ui.horizontal(|ui| {
                for (v, p) in values.iter_mut().zip(["R ", "G ", "B "]) {
                    typed |= ui.add(egui::DragValue::new(v).speed(0.5).prefix(p)).changed();
                }
            });
            if typed {
                hsv = HsvaGamma::from(Hsva::from_srgb(values));
            }
            if hsv != before {
                *rgb = to_rgb(hsv);
                button.mark_changed();
            }
            ui.data_mut(|d| d.insert_temp(popup_id, hsv));
        });
    button
}

fn to_rgb(h: HsvaGamma) -> [u8; 3] {
    let [r, g, b, _] = Color32::from(h).to_array();
    [r, g, b]
}

fn opaque(h: f32, s: f32, v: f32) -> Color32 {
    HsvaGamma { h, s, v, a: 1.0 }.into()
}

/// Vertex-coloured grid over `rect`, colour from (x, y) in 0..=1.
fn gradient(ui: &egui::Ui, rect: Rect, rows: usize, color_at: impl Fn(f32, f32) -> Color32) {
    let mut mesh = Mesh::default();
    for j in 0..=rows {
        for i in 0..=STEPS {
            let (x, y) = (i as f32 / STEPS as f32, j as f32 / rows.max(1) as f32);
            mesh.colored_vertex(rect.lerp_inside(vec2(x, y)), color_at(x, y));
            if i < STEPS && j < rows {
                let at = (j * (STEPS + 1) + i) as u32;
                let next = at + STEPS as u32 + 1;
                mesh.add_triangle(at, at + 1, next);
                mesh.add_triangle(at + 1, next + 1, next);
            }
        }
    }
    ui.painter().add(mesh);
}

/// Pointer position as (x, y) in 0..=1 while the area is pressed.
fn drag(ui: &mut egui::Ui, size: Vec2) -> (Rect, Option<(f32, f32)>) {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let at = response.interact_pointer_pos().map(|p| {
        let t = (p - rect.min) / rect.size();
        (t.x.clamp(0.0, 1.0), t.y.clamp(0.0, 1.0))
    });
    (rect, at)
}

fn sv_square(ui: &mut egui::Ui, hsv: &mut HsvaGamma) {
    let (rect, at) = drag(ui, Vec2::splat(WIDTH));
    if let Some((x, y)) = at {
        (hsv.s, hsv.v) = (x, 1.0 - y);
    }
    let h = hsv.h;
    gradient(ui, rect, STEPS, |x, y| opaque(h, x, 1.0 - y));
    ring(ui, rect.lerp_inside(vec2(hsv.s, 1.0 - hsv.v)), hsv.v);
}

fn hue_bar(ui: &mut egui::Ui, hsv: &mut HsvaGamma) {
    let (rect, at) = drag(ui, vec2(WIDTH, ui.spacing().interact_size.y));
    if let Some((x, _)) = at {
        hsv.h = x;
    }
    gradient(ui, rect, 1, |x, _| opaque(x, 1.0, 1.0));
    ring(ui, rect.lerp_inside(vec2(hsv.h, 0.5)), 1.0);
}

/// Marker readable on light and dark colours.
fn ring(ui: &egui::Ui, at: Pos2, value: f32) {
    let color = if value > 0.5 { Color32::BLACK } else { Color32::WHITE };
    ui.painter().circle_stroke(at, 5.0, Stroke::new(1.5, color));
}
