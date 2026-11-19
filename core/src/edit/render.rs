//! Rendering. `render` is the source of truth (Export, Overwrite, Copy, golden
//! tests): deterministic, full resolution, in the document's colour space. `draw_shapes` is shared
//! with the preview, which calls it with the screen transform: same paths, same rasteriser.
//! Order: source pixels → redactions → shapes in z-order, source-over on premultiplied
//! encoded values, clipped to the crop → document orientation.

use super::geom::{Ink, shape_paths};
use super::model::*;
use super::redact::{Patch, PatchCache, SrcPx};
use crate::decode::Pixels;
use std::sync::Arc;
use tiny_skia::{FillRule, Paint, Pixmap, PixmapMut, PixmapPaint, Transform};

/// Colour of a shape in the target space: sRGB → document space (export) or view space (preview).
pub type ColorMap<'a> = &'a (dyn Fn(Color) -> Color + Sync);

/// Colours unchanged (document or view space is sRGB).
pub fn same(c: Color) -> Color {
    c
}

fn paint_for(c: Color, opacity: f32) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(c.r, c.g, c.b, ((c.a as f32) * opacity).round().clamp(0.0, 255.0) as u8);
    p.anti_alias = true;
    p
}

fn ink_color(shape: &Shape, ink: Ink) -> Option<Color> {
    match ink {
        Ink::Fill => shape.style.fill,
        Ink::Stroke => shape.style.stroke,
    }
}

/// Draw one shape. With an opacity below 1 and more than one path (fill + outline, arrow parts)
/// the shape is drawn opaque into a layer that is then composited with the opacity, so its own
/// overlaps don't show (one layer per shape).
fn draw_shape(pm: &mut PixmapMut, shape: &Shape, t: Transform, res: f32, colors: ColorMap, base: Base) {
    draw_prepared(pm, shape, &shape_paths(shape, res), t, colors, base);
}

/// `draw_shape` with the shape's paths already built.
fn draw_prepared(
    pm: &mut PixmapMut,
    shape: &Shape,
    paths: &[(tiny_skia::Path, Ink)],
    t: Transform,
    colors: ColorMap,
    base: Base,
) {
    let op = shape.style.opacity.clamp(0.0, 1.0);
    if op <= 0.0 {
        return;
    }
    if let Geom::Loupe { center, radius, zoom } = &shape.geom {
        draw_loupe(pm, *center, *radius, *zoom, op, t, base);
    }
    if let Geom::Image { rect, rotation, image } = &shape.geom {
        if let Some(d) = &image.data {
            draw_image(pm, rect, *rotation, d, op, t, colors);
        }
        return;
    }
    if op >= 1.0 || paths.len() <= 1 {
        for (path, ink) in paths {
            if let Some(c) = ink_color(shape, *ink) {
                pm.fill_path(path, &paint_for(colors(c), op), FillRule::Winding, t, None);
            }
        }
        return;
    }
    // device-space bounds of the shape, clipped to the target
    let mut l = f32::MAX;
    let (mut tp, mut r, mut b) = (f32::MAX, f32::MIN, f32::MIN);
    for (path, _) in paths {
        if let Some(p) = path.clone().transform(t) {
            let bb = p.bounds();
            l = l.min(bb.left());
            tp = tp.min(bb.top());
            r = r.max(bb.right());
            b = b.max(bb.bottom());
        }
    }
    let x0 = (l.floor().max(0.0)) as i32;
    let y0 = (tp.floor().max(0.0)) as i32;
    let x1 = (r.ceil() as i32).min(pm.width() as i32);
    let y1 = (b.ceil() as i32).min(pm.height() as i32);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let Some(mut layer) = Pixmap::new((x1 - x0) as u32, (y1 - y0) as u32) else {
        return;
    };
    let lt = t.post_translate(-x0 as f32, -y0 as f32);
    for (path, ink) in paths {
        if let Some(c) = ink_color(shape, *ink) {
            layer.fill_path(path, &paint_for(colors(c), 1.0), FillRule::Winding, lt, None);
        }
    }
    let pp = PixmapPaint {
        opacity: op,
        ..Default::default()
    };
    pm.draw_pixmap(x0, y0, layer.as_ref(), &pp, Transform::identity(), None);
}

/// Pixels of the image under the shapes (source + redactions) for an image-space rectangle, as a
/// premultiplied RGBA8 pixmap in the target's colour space; what loupes magnify.
pub type Base<'a> = &'a (dyn Fn(RectU) -> Option<Pixmap> + Sync);

fn no_base(_: RectU) -> Option<Pixmap> {
    None
}

/// The loupe's content: the image around `c` scaled by `zoom` about `c`, clipped to the circle.
fn draw_loupe(pm: &mut PixmapMut, c: PointF, radius: f32, zoom: f32, op: f32, t: Transform, base: Base) {
    let half = radius / zoom.max(1.0);
    let x0 = (c.x - half).floor().max(0.0);
    let y0 = (c.y - half).floor().max(0.0);
    let x1 = (c.x + half).ceil();
    let y1 = (c.y + half).ceil();
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let src = RectU {
        x: x0 as u32,
        y: y0 as u32,
        w: (x1 - x0) as u32,
        h: (y1 - y0) as u32,
    };
    let (Some(px), Some(circle)) = (base(src), tiny_skia::PathBuilder::from_circle(c.x, c.y, radius)) else {
        return;
    };
    // pattern pixel → image point → magnified about c → target
    let mag = Transform::from_translate(c.x, c.y)
        .pre_scale(zoom, zoom)
        .pre_translate(-c.x, -c.y)
        .pre_translate(src.x as f32, src.y as f32);
    let paint = Paint {
        anti_alias: true,
        shader: tiny_skia::Pattern::new(
            px.as_ref(),
            tiny_skia::SpreadMode::Pad,
            tiny_skia::FilterQuality::Bilinear,
            op,
            // in image space: fill_path applies `t` to the shader too
            mag,
        ),
        ..Default::default()
    };
    pm.fill_path(&circle, &paint, FillRule::Winding, t, None);
}

/// An image shape: its pixels in the target's colour space, stretched over the rectangle and
/// rotated with it; drawn from the mipmap level for its size on the target.
fn draw_image(
    pm: &mut PixmapMut,
    rect: &RectF,
    rotation: f32,
    d: &super::picture::ImageData,
    op: f32,
    t: Transform,
    colors: ColorMap,
) {
    // target pixels per image-space unit; the mipmap level for that size
    let scale = (t.sx * t.sy - t.kx * t.ky).abs().sqrt();
    let want = |side: f32| (side * scale).ceil().max(1.0) as u32;
    let (Some(px), Some(r)) = (
        d.pixmap(colors, want(rect.w), want(rect.h)),
        tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.w, rect.h),
    ) else {
        return;
    };
    let (w, h) = (px.width(), px.height());
    let turn = super::geom::rotation_about(rect, rotation);
    let Some(path) = tiny_skia::PathBuilder::from_rect(r).transform(turn) else {
        return;
    };
    // pattern pixel → the rectangle → rotated (image space: fill_path applies `t` to it too)
    let shader = turn
        .pre_translate(rect.x, rect.y)
        .pre_scale(rect.w / w as f32, rect.h / h as f32);
    let paint = Paint {
        anti_alias: true,
        shader: tiny_skia::Pattern::new(
            px.as_ref().as_ref(),
            tiny_skia::SpreadMode::Pad,
            tiny_skia::FilterQuality::Bilinear,
            1.0,
            shader,
        ),
        ..Default::default()
    };
    if op >= 1.0 {
        pm.fill_path(&path, &paint, FillRule::Winding, t, None);
        return;
    }
    // opaque into a layer, composited with the opacity: a pattern's own opacity is applied
    // unevenly on anti-aliased edge pixels (they came out more opaque than the inside)
    let Some(b) = path.clone().transform(t).map(|p| p.bounds()) else {
        return;
    };
    let x0 = b.left().floor().max(0.0) as i32;
    let y0 = b.top().floor().max(0.0) as i32;
    let x1 = (b.right().ceil() as i32).min(pm.width() as i32);
    let y1 = (b.bottom().ceil() as i32).min(pm.height() as i32);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let Some(mut layer) = Pixmap::new((x1 - x0) as u32, (y1 - y0) as u32) else {
        return;
    };
    let lt = t.post_translate(-x0 as f32, -y0 as f32);
    layer.fill_path(&path, &paint, FillRule::Winding, lt, None);
    let pp = PixmapPaint {
        opacity: op,
        ..Default::default()
    };
    pm.draw_pixmap(x0, y0, layer.as_ref(), &pp, Transform::identity(), None);
}

/// Draw shapes in z-order onto a premultiplied target. `t` maps image space to the target's
/// pixels; `res` = target pixels per image pixel (curve precision). Loupes show nothing inside
/// (only their outline): see `draw_shapes_on`.
pub fn draw_shapes(pm: &mut PixmapMut, shapes: &[Arc<Shape>], t: Transform, res: f32, colors: ColorMap) {
    draw_shapes_on(pm, shapes, t, res, colors, &no_base);
}

/// `draw_shapes_on` into a new `w`×`h` pixmap, in horizontal bands drawn in parallel (the screen
/// overlay: hundreds of shapes on a large monitor).
#[allow(clippy::too_many_arguments)]
pub fn draw_shapes_banded(
    w: u32,
    h: u32,
    shapes: &[Arc<Shape>],
    t: Transform,
    res: f32,
    colors: ColorMap,
    base: Base,
) -> Option<Pixmap> {
    use rayon::prelude::*;
    let mut pm = Pixmap::new(w, h)?;
    if shapes.is_empty() {
        return Some(pm);
    }
    // small bands: the shapes are rarely spread evenly, work stealing balances them
    let bh = h.div_ceil(rayon::current_num_threads().clamp(1, 32) as u32 * 4).max(16);
    let row = w as usize * 4;
    // paths once (strokes, text), then every band fills them
    let prepared: Vec<_> = shapes.par_iter().map(|s| shape_paths(s, res)).collect();
    pm.data_mut()
        .par_chunks_mut(row * bh as usize)
        .enumerate()
        .for_each(|(i, chunk)| {
            let rows = (chunk.len() / row) as u32;
            if let Some(mut band) = PixmapMut::from_bytes(chunk, w, rows) {
                let bt = t.post_translate(0.0, -((i as u32 * bh) as f32));
                for (s, paths) in shapes.iter().zip(&prepared) {
                    draw_prepared(&mut band, s, paths, bt, colors, base);
                }
            }
        });
    Some(pm)
}

/// `draw_shapes` with the image under them, for loupes.
pub fn draw_shapes_on(pm: &mut PixmapMut, shapes: &[Arc<Shape>], t: Transform, res: f32, colors: ColorMap, base: Base) {
    for s in shapes {
        draw_shape(pm, s, t, res, colors, base);
    }
}

/// The image under the shapes in `rect` (source + redaction patches), premultiplied RGBA8, for
/// loupes in the export (8 bit: a loupe of a 16-bit document is drawn from 8-bit pixels).
pub fn base_pixmap(src: SrcPx, img_w: u32, rect: RectU, patches: &[Arc<Patch>]) -> Option<Pixmap> {
    let row = rect.w as usize * 4;
    let img_h = match src {
        SrcPx::R8(v) => v.len() / 4 / img_w as usize,
        SrcPx::R16(v) => v.len() / 4 / img_w as usize,
    } as u32;
    if rect.w == 0 || rect.h == 0 || rect.x + rect.w > img_w || rect.y + rect.h > img_h {
        return None;
    }
    let mut buf = vec![0u8; row * rect.h as usize];
    for y in 0..rect.h as usize {
        let s = ((rect.y as usize + y) * img_w as usize + rect.x as usize) * 4;
        let d = &mut buf[y * row..(y + 1) * row];
        match src {
            SrcPx::R8(v) => d.copy_from_slice(&v[s..s + row]),
            SrcPx::R16(v) => {
                for (o, i) in d.iter_mut().zip(&v[s..s + row]) {
                    *o = ((*i as u32 * 255 + 32767) / 65535) as u8;
                }
            }
        }
    }
    let p8: Vec<Arc<Patch>> = patches
        .iter()
        .map(|p| {
            Arc::new(Patch {
                rect: p.rect,
                pixels: Pixels::Rgba8(crate::color::rgba8(p.pixels.clone())),
            })
        })
        .collect();
    super::redact::paste(&mut buf, rect, &p8, |p| match p {
        Pixels::Rgba8(v) => Some(v.as_slice()),
        Pixels::Rgba16(_) => None,
    });
    premultiply(&mut buf);
    Pixmap::from_vec(buf, tiny_skia::IntSize::from_wh(rect.w, rect.h)?)
}

/// Straight RGBA8 → premultiplied (encoded values), rounding to nearest.
pub fn premultiply(rgba: &mut [u8]) {
    for p in rgba.chunks_exact_mut(4) {
        let a = p[3] as u32;
        if a != 255 {
            for c in &mut p[..3] {
                *c = ((*c as u32 * a + 127) / 255) as u8;
            }
        }
    }
}

/// Premultiplied → straight RGBA8.
pub fn unpremultiply(rgba: &mut [u8]) {
    for p in rgba.chunks_exact_mut(4) {
        let a = p[3] as u32;
        if a != 255 {
            for c in &mut p[..3] {
                // alpha 0 → 0
                *c = (*c as u32 * 255 + a / 2).checked_div(a).unwrap_or(0).min(255) as u8;
            }
        }
    }
}

/// Source pixels of the document: straight RGBA8 in the document's colour space.
pub struct Source<'a> {
    pub w: u32,
    pub h: u32,
    pub rgba: &'a [u8],
}

/// The document's base: the source with its adjustments (borrowed when neutral).
pub fn adjusted<'a>(doc: &DocState, px: &'a Pixels) -> std::borrow::Cow<'a, Pixels> {
    if doc.adjust.is_neutral() {
        std::borrow::Cow::Borrowed(px)
    } else {
        std::borrow::Cow::Owned(super::adjust::apply(px, doc.image_size.w, &doc.adjust))
    }
}

/// `render` for 8- or 16-bit sources (the document keeps 16 bit). 16-bit: the shapes are
/// rasterised into one transparent premultiplied RGBA8 layer, composited source-over onto the
/// premultiplied 16-bit region (the same result as drawing them on it, within rounding).
pub fn render_pixels(doc: &DocState, w: u32, h: u32, px: &Pixels, colors: ColorMap) -> (u32, u32, Pixels) {
    let base = adjusted(doc, px);
    let mut cache = PatchCache::default();
    let patches = super::redact::patches(doc, w, h, (&*base).into(), colors, &mut cache);
    render_pixels_with(doc, w, h, &base, colors, &patches)
}

/// `render_pixels` from the document's base (`adjusted`) with the redaction patches already
/// computed (from a cache).
pub fn render_pixels_with(
    doc: &DocState,
    w: u32,
    h: u32,
    px: &Pixels,
    colors: ColorMap,
    patches: &[Arc<Patch>],
) -> (u32, u32, Pixels) {
    let src = match px {
        Pixels::Rgba8(v) => {
            let (w, h, out) = render_with(doc, &Source { w, h, rgba: v }, colors, patches);
            return (w, h, Pixels::Rgba8(out));
        }
        Pixels::Rgba16(v) => v,
    };
    assert_eq!(
        (w, h),
        (doc.image_size.w, doc.image_size.h),
        "source and document sizes"
    );
    let r = doc.region();
    let row = r.w as usize * 4;
    let mut buf = vec![0u16; row * r.h as usize];
    for y in 0..r.h as usize {
        let s = ((r.y as usize + y) * w as usize + r.x as usize) * 4;
        buf[y * row..(y + 1) * row].copy_from_slice(&src[s..s + row]);
    }
    super::redact::paste(&mut buf, r, patches, |p| match p {
        Pixels::Rgba16(v) => Some(v.as_slice()),
        Pixels::Rgba8(_) => None,
    });
    if !doc.shapes.is_empty()
        && let Some(mut layer) = Pixmap::new(r.w, r.h)
    {
        let t = Transform::from_translate(-(r.x as f32), -(r.y as f32));
        let base = |rect: RectU| base_pixmap(SrcPx::R16(src), w, rect, patches);
        draw_shapes_on(&mut layer.as_mut(), &doc.shapes, t, 1.0, colors, &base);
        for (d, l) in buf.chunks_exact_mut(4).zip(layer.data().chunks_exact(4)) {
            let la = l[3] as u32;
            if la == 0 {
                continue;
            }
            // premultiply the 16-bit pixel, add the layer (×257), back to straight
            let da = d[3] as u32;
            let inv = 255 - la;
            let a = la * 257 + (da * inv + 127) / 255;
            for c in 0..3 {
                let dp = (d[c] as u32 * da + 32767) / 65535;
                let v = l[c] as u32 * 257 + (dp * inv + 127) / 255;
                d[c] = (v * 65535 + a / 2).checked_div(a).unwrap_or(0).min(65535) as u16;
            }
            d[3] = a.min(65535) as u16;
        }
    }
    let (ow, oh) = doc.orient.apply_size(r.w, r.h);
    (
        ow,
        oh,
        Pixels::Rgba16(crate::orient::apply_orient(buf, r.w, r.h, doc.orient)),
    )
}

/// The flattened document: (width, height, straight RGBA8 in the document space).
pub fn render(doc: &DocState, src: &Source, colors: ColorMap) -> (u32, u32, Vec<u8>) {
    let owned =
        (!doc.adjust.is_neutral()).then(|| super::adjust::apply(&Pixels::Rgba8(src.rgba.to_vec()), src.w, &doc.adjust));
    let base = match &owned {
        Some(Pixels::Rgba8(v)) => Source {
            w: src.w,
            h: src.h,
            rgba: v,
        },
        _ => Source { ..*src },
    };
    let mut cache = PatchCache::default();
    let patches = super::redact::patches(doc, base.w, base.h, SrcPx::R8(base.rgba), colors, &mut cache);
    render_with(doc, &base, colors, &patches)
}

/// `render` from the document's base (`adjusted`) with the redaction patches already computed.
pub fn render_with(doc: &DocState, src: &Source, colors: ColorMap, patches: &[Arc<Patch>]) -> (u32, u32, Vec<u8>) {
    assert_eq!(
        (src.w, src.h),
        (doc.image_size.w, doc.image_size.h),
        "source and document sizes"
    );
    let r = doc.region();
    let mut buf = vec![0u8; r.w as usize * r.h as usize * 4];
    let row = r.w as usize * 4;
    for y in 0..r.h as usize {
        let s = ((r.y as usize + y) * src.w as usize + r.x as usize) * 4;
        buf[y * row..(y + 1) * row].copy_from_slice(&src.rgba[s..s + row]);
    }
    // redactions below every shape
    super::redact::paste(&mut buf, r, patches, |p| match p {
        Pixels::Rgba8(v) => Some(v.as_slice()),
        Pixels::Rgba16(_) => None,
    });
    premultiply(&mut buf);
    if let Some(mut pm) = PixmapMut::from_bytes(&mut buf, r.w, r.h) {
        let t = Transform::from_translate(-(r.x as f32), -(r.y as f32));
        let base = |rect: RectU| base_pixmap(SrcPx::R8(src.rgba), src.w, rect, patches);
        draw_shapes_on(&mut pm, &doc.shapes, t, 1.0, colors, &base);
    }
    unpremultiply(&mut buf);
    let (w, h) = doc.orient.apply_size(r.w, r.h);
    (w, h, crate::orient::apply_orient(buf, r.w, r.h, doc.orient))
}
