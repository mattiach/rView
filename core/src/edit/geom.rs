//! Shape geometry: every shape becomes tiny-skia paths in image space — fills
//! and stroke outlines made by tiny-skia's stroker, arrow heads, ellipses. The same paths are
//! rasterised by the export (image resolution) and the preview (screen resolution).
//! Also: bounding boxes and hit-testing for the selection.

use super::model::*;
use tiny_skia::{LineCap, LineJoin, Path, PathBuilder, Rect, Stroke, Transform};

/// What to fill a path with.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Ink {
    Fill,
    Stroke,
}

/// Arrow head length and half width for a line width.
pub fn arrow_head(width: f32) -> (f32, f32) {
    (4.0 * width + 8.0, 1.8 * width + 4.0)
}

fn rect_path(r: &RectF, corner: f32) -> Option<Path> {
    let rect = Rect::from_xywh(r.x, r.y, r.w, r.h)?;
    let c = corner.min(r.w / 2.0).min(r.h / 2.0).max(0.0);
    if c <= 0.0 {
        return Some(PathBuilder::from_rect(rect));
    }
    // rounded corners: cubic quarter circles (k = 0.5523)
    let k = 0.552_284_8 * c;
    let (x0, y0, x1, y1) = (r.x, r.y, r.x + r.w, r.y + r.h);
    let mut pb = PathBuilder::new();
    pb.move_to(x0 + c, y0);
    pb.line_to(x1 - c, y0);
    pb.cubic_to(x1 - c + k, y0, x1, y0 + c - k, x1, y0 + c);
    pb.line_to(x1, y1 - c);
    pb.cubic_to(x1, y1 - c + k, x1 - c + k, y1, x1 - c, y1);
    pb.line_to(x0 + c, y1);
    pb.cubic_to(x0 + c - k, y1, x0, y1 - c + k, x0, y1 - c);
    pb.line_to(x0, y0 + c);
    pb.cubic_to(x0, y0 + c - k, x0 + c - k, y0, x0 + c, y0);
    pb.close();
    pb.finish()
}

fn rotation_about(r: &RectF, rotation: f32) -> Transform {
    let c = r.center();
    Transform::from_rotate_at(rotation.to_degrees(), c.x, c.y)
}

fn polyline(points: &[PointF]) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let first = points.first()?;
    pb.move_to(first.x, first.y);
    if points.len() == 1 {
        // a dot: a zero-length segment gets round caps
        pb.line_to(first.x + 0.01, first.y);
    }
    for p in &points[1..] {
        pb.line_to(p.x, p.y);
    }
    pb.finish()
}

fn stroke(path: &Path, width: f32, cap: LineCap, join: LineJoin, res_scale: f32) -> Option<Path> {
    let s = Stroke {
        width,
        line_cap: cap,
        line_join: join,
        miter_limit: 4.0,
        dash: None,
    };
    path.stroke(&s, res_scale.max(0.01))
}

fn triangle(tip: PointF, from: PointF, len: f32, half: f32) -> Option<Path> {
    let (dx, dy) = (tip.x - from.x, tip.y - from.y);
    let d = dx.hypot(dy).max(1e-6);
    let (ux, uy) = (dx / d, dy / d);
    let (bx, by) = (tip.x - ux * len, tip.y - uy * len);
    let mut pb = PathBuilder::new();
    pb.move_to(tip.x, tip.y);
    pb.line_to(bx - uy * half, by + ux * half);
    pb.line_to(bx + uy * half, by - ux * half);
    pb.close();
    pb.finish()
}

/// Control point of a curved arrow: the middle of a→b moved sideways by `bend` × its length.
pub fn arrow_control(a: PointF, b: PointF, bend: f32) -> PointF {
    PointF {
        x: (a.x + b.x) / 2.0 - (b.y - a.y) * bend,
        y: (a.y + b.y) / 2.0 + (b.x - a.x) * bend,
    }
}

/// The shape's paths in image space, bottom first: (path, ink). `res_scale` = device pixels per
/// image pixel, so curves are flattened finely enough at the output resolution.
pub fn shape_paths(shape: &Shape, res_scale: f32) -> Vec<(Path, Ink)> {
    let st = &shape.style;
    let w = st.width.max(0.1);
    let mut out = Vec::new();
    let mut push = |p: Option<Path>, ink| {
        if let Some(p) = p {
            out.push((p, ink));
        }
    };
    match &shape.geom {
        Geom::Rect {
            rect,
            rotation,
            corner_radius,
        } => {
            let t = rotation_about(rect, *rotation);
            let base = rect_path(rect, *corner_radius);
            if let Some(base) = &base {
                if st.fill.is_some() {
                    push(base.clone().transform(t), Ink::Fill);
                }
                if st.stroke.is_some() {
                    let join = if *corner_radius > 0.0 {
                        LineJoin::Round
                    } else {
                        LineJoin::Miter
                    };
                    push(
                        stroke(base, w, LineCap::Butt, join, res_scale).and_then(|p| p.transform(t)),
                        Ink::Stroke,
                    );
                }
            }
        }
        Geom::Ellipse { rect, rotation } => {
            let t = rotation_about(rect, *rotation);
            if let Some(oval) = Rect::from_xywh(rect.x, rect.y, rect.w, rect.h).and_then(PathBuilder::from_oval) {
                if st.fill.is_some() {
                    push(oval.clone().transform(t), Ink::Fill);
                }
                if st.stroke.is_some() {
                    push(
                        stroke(&oval, w, LineCap::Butt, LineJoin::Round, res_scale).and_then(|p| p.transform(t)),
                        Ink::Stroke,
                    );
                }
            }
        }
        Geom::Line { a, b } => {
            push(
                polyline(&[*a, *b]).and_then(|p| stroke(&p, w, LineCap::Round, LineJoin::Round, res_scale)),
                Ink::Stroke,
            );
        }
        Geom::Arrow { a, b, heads, bend } if *bend != 0.0 => {
            // a quadratic curve bowing sideways; the heads follow its tangents
            let (len, half) = arrow_head(w);
            let c = arrow_control(*a, *b, *bend);
            let toward = |from: PointF, to: PointF, by: f32| {
                let d = (to.x - from.x).hypot(to.y - from.y).max(1e-6);
                PointF {
                    x: from.x + (to.x - from.x) / d * by,
                    y: from.y + (to.y - from.y) / d * by,
                }
            };
            let cut = (len * 0.7).min((b.x - a.x).hypot(b.y - a.y) / 2.0);
            let end = toward(*b, c, cut);
            let start = if *heads == ArrowHeads::Both {
                toward(*a, c, cut)
            } else {
                *a
            };
            let mut pb = PathBuilder::new();
            pb.move_to(start.x, start.y);
            pb.quad_to(c.x, c.y, end.x, end.y);
            push(
                pb.finish()
                    .and_then(|p| stroke(&p, w, LineCap::Round, LineJoin::Round, res_scale)),
                Ink::Stroke,
            );
            push(triangle(*b, c, len, half), Ink::Stroke);
            if *heads == ArrowHeads::Both {
                push(triangle(*a, c, len, half), Ink::Stroke);
            }
        }
        Geom::Arrow { a, b, heads, .. } => {
            let (len, half) = arrow_head(w);
            let d = (b.x - a.x).hypot(b.y - a.y).max(1e-6);
            let (ux, uy) = ((b.x - a.x) / d, (b.y - a.y) / d);
            // the shaft stops inside the heads, so its end doesn't poke out of the tip
            let cut = (len * 0.7).min(d / 2.0);
            let end = PointF {
                x: b.x - ux * cut,
                y: b.y - uy * cut,
            };
            let start = if *heads == ArrowHeads::Both {
                PointF {
                    x: a.x + ux * cut,
                    y: a.y + uy * cut,
                }
            } else {
                *a
            };
            push(
                polyline(&[start, end]).and_then(|p| stroke(&p, w, LineCap::Butt, LineJoin::Miter, res_scale)),
                Ink::Stroke,
            );
            push(triangle(*b, *a, len, half), Ink::Stroke);
            if *heads == ArrowHeads::Both {
                push(triangle(*a, *b, len, half), Ink::Stroke);
            }
        }
        Geom::Pen { points } | Geom::Highlight { points } => {
            push(
                polyline(points).and_then(|p| stroke(&p, w, LineCap::Round, LineJoin::Round, res_scale)),
                Ink::Stroke,
            );
        }
        // the magnified content is drawn by `render` (it needs the image); here the outline
        Geom::Loupe { center, radius, .. } => {
            if st.stroke.is_some() {
                let circle = PathBuilder::from_circle(center.x, center.y, radius.max(0.5));
                push(
                    circle.and_then(|c| stroke(&c, w, LineCap::Butt, LineJoin::Round, res_scale)),
                    Ink::Stroke,
                );
            }
        }
        Geom::Text {
            rect,
            rotation,
            text,
            font,
            align,
        } => {
            // the box is as tall as the laid-out text: background and glyphs use the same box
            let l = super::text::layout(text, font, rect.w, *align);
            let r = RectF { h: l.height, ..*rect };
            if st.fill.is_some() {
                push(
                    rect_path(&r, 0.0).and_then(|p| p.transform(rotation_about(&r, *rotation))),
                    Ink::Fill,
                );
            }
            if st.stroke.is_some() {
                push(super::text::glyph_path(&r, *rotation, &l), Ink::Stroke);
            }
        }
    }
    out
}

/// Axis-aligned bounds in image space (including the stroke; a text: its whole box).
pub fn bounds(shape: &Shape) -> Option<RectF> {
    if let Geom::Text { rect, rotation, .. } = &shape.geom {
        return Some(area_bounds(&Area::Rect {
            rect: *rect,
            rotation: *rotation,
        }));
    }
    if let Geom::Loupe { center, radius, .. } = &shape.geom {
        let r = radius + shape.style.width / 2.0;
        return Some(RectF {
            x: center.x - r,
            y: center.y - r,
            w: 2.0 * r,
            h: 2.0 * r,
        });
    }
    let paths = shape_paths(shape, 1.0);
    let mut it = paths.iter().map(|(p, _)| p.bounds());
    let first = it.next()?;
    let (mut l, mut t, mut r, mut b) = (first.left(), first.top(), first.right(), first.bottom());
    for rb in it {
        l = l.min(rb.left());
        t = t.min(rb.top());
        r = r.max(rb.right());
        b = b.max(rb.bottom());
    }
    Some(RectF {
        x: l,
        y: t,
        w: r - l,
        h: b - t,
    })
}

/// Bounds of a redaction area (rotation included).
pub fn area_bounds(a: &Area) -> RectF {
    let (Area::Rect { rect, rotation } | Area::Ellipse { rect, rotation }) = a;
    let c = rect.center();
    let (s, co) = rotation.sin_cos();
    let (hw, hh) = (rect.w / 2.0, rect.h / 2.0);
    let ex = (hw * co).abs() + (hh * s).abs();
    let ey = (hw * s).abs() + (hh * co).abs();
    RectF {
        x: c.x - ex,
        y: c.y - ey,
        w: 2.0 * ex,
        h: 2.0 * ey,
    }
}

/// `p` in the rect's own (unrotated) frame.
fn local(p: PointF, rect: &RectF, rotation: f32) -> (f32, f32) {
    let c = rect.center();
    let (s, co) = (-rotation).sin_cos();
    let (dx, dy) = (p.x - c.x, p.y - c.y);
    (dx * co - dy * s, dx * s + dy * co)
}

fn seg_dist(p: PointF, a: PointF, b: PointF) -> f32 {
    let (vx, vy) = (b.x - a.x, b.y - a.y);
    let len2 = vx * vx + vy * vy;
    let t = if len2 <= 0.0 {
        0.0
    } else {
        (((p.x - a.x) * vx + (p.y - a.y) * vy) / len2).clamp(0.0, 1.0)
    };
    (p.x - (a.x + t * vx)).hypot(p.y - (a.y + t * vy))
}

/// The point (image space) is on the shape: inside a filled shape, or within half the stroke
/// width + `tol` (image px) of its outline.
pub fn hit(shape: &Shape, p: PointF, tol: f32) -> bool {
    let half = shape.style.width / 2.0 + tol;
    let filled = shape.style.fill.is_some();
    match &shape.geom {
        Geom::Rect { rect, rotation, .. } => {
            let (x, y) = local(p, rect, *rotation);
            let (hw, hh) = (rect.w / 2.0, rect.h / 2.0);
            let inside_outer = x.abs() <= hw + half && y.abs() <= hh + half;
            let inside_inner = x.abs() < hw - half && y.abs() < hh - half;
            inside_outer && (filled || !inside_inner)
        }
        Geom::Ellipse { rect, rotation } => {
            let (x, y) = local(p, rect, *rotation);
            let (a, b) = ((rect.w / 2.0).max(0.5), (rect.h / 2.0).max(0.5));
            let r = ((x / a).powi(2) + (y / b).powi(2)).sqrt();
            // distance to the outline, approximately (exact on circles)
            let d = (r - 1.0) * a.min(b);
            if filled { d <= half } else { d.abs() <= half }
        }
        Geom::Line { a, b } => seg_dist(p, *a, *b) <= half,
        Geom::Arrow { a, b, bend, .. } => {
            let (len, w) = arrow_head(shape.style.width);
            let reach = half.max(w.min(len) / 2.0 + tol);
            if *bend == 0.0 {
                return seg_dist(p, *a, *b) <= reach;
            }
            // the curve as 16 chords
            let c = arrow_control(*a, *b, *bend);
            let at = |t: f32| PointF {
                x: (1.0 - t) * (1.0 - t) * a.x + 2.0 * t * (1.0 - t) * c.x + t * t * b.x,
                y: (1.0 - t) * (1.0 - t) * a.y + 2.0 * t * (1.0 - t) * c.y + t * t * b.y,
            };
            (0..16).any(|i| seg_dist(p, at(i as f32 / 16.0), at((i + 1) as f32 / 16.0)) <= reach)
        }
        Geom::Loupe { center, radius, .. } => (p.x - center.x).hypot(p.y - center.y) <= radius + half,
        // anywhere in its box
        Geom::Text { rect, rotation, .. } => {
            let (x, y) = local(p, rect, *rotation);
            x.abs() <= rect.w / 2.0 + tol && y.abs() <= rect.h / 2.0 + tol
        }
        Geom::Pen { points } | Geom::Highlight { points } => match points.len() {
            0 => false,
            1 => (p.x - points[0].x).hypot(p.y - points[0].y) <= half,
            _ => points.windows(2).any(|s| seg_dist(p, s[0], s[1]) <= half),
        },
    }
}

/// The point is inside a redaction's area (rotation included).
pub fn hit_area(a: &Area, p: PointF, tol: f32) -> bool {
    match a {
        Area::Rect { rect, rotation } => {
            let (x, y) = local(p, rect, *rotation);
            x.abs() <= rect.w / 2.0 + tol && y.abs() <= rect.h / 2.0 + tol
        }
        Area::Ellipse { rect, rotation } => {
            let (x, y) = local(p, rect, *rotation);
            let (a, b) = ((rect.w / 2.0 + tol).max(0.5), (rect.h / 2.0 + tol).max(0.5));
            (x / a).powi(2) + (y / b).powi(2) <= 1.0
        }
    }
}
