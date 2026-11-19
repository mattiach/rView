//! Redactions. Each redaction becomes a *patch*: the
//! pixels of its bounding box after the effect, computed from the source with the earlier patches
//! applied. Pasting the patches in order onto the source gives the redacted image, so export and
//! preview use the same pixels, and undo/redo find them again in the cache.
//!
//! - Blur: 3 separable box passes on premultiplied values (integer sums: exact and deterministic),
//!   σ ≈ radius/2, clamped at the image edges (never reads outside the image), AA area mask.
//! - Pixelate: premultiplied mean per block, blocks aligned to the area's origin, AA area mask.
//! - Solid: every pixel the area touches, even partly, becomes the opaque colour (binary,
//!   conservative mask computed geometrically: no anti-aliasing can mix original values in).
//!
//! Patches are straight pixels in the source's depth and colour space (the document space).

use super::geom::area_bounds;
use super::model::*;
use super::render::ColorMap;
use crate::decode::Pixels;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;
use tiny_skia::{FillRule, Mask, PathBuilder, Rect, Transform};

/// Source pixels by reference.
#[derive(Clone, Copy)]
pub enum SrcPx<'a> {
    R8(&'a [u8]),
    R16(&'a [u16]),
}

impl<'a> From<&'a Pixels> for SrcPx<'a> {
    fn from(p: &'a Pixels) -> Self {
        match p {
            Pixels::Rgba8(v) => SrcPx::R8(v),
            Pixels::Rgba16(v) => SrcPx::R16(v),
        }
    }
}

/// The redacted pixels of one redaction's bounding box (image space).
#[derive(Debug)]
pub struct Patch {
    pub rect: RectU,
    /// Straight, the source's depth, the document space.
    pub pixels: Pixels,
}

const BUDGET: usize = 256 << 20;

/// Patches by key: the key covers the redaction, its read area and the keys of the earlier
/// patches it reads, so a change recomputes only what depends on it.
#[derive(Default)]
pub struct PatchCache {
    map: HashMap<u64, (Arc<Patch>, u64)>,
    bytes: usize,
    tick: u64,
    /// Lookups answered from the cache / computed (tests check that undo/redo don't recompute).
    pub hits: u64,
    pub misses: u64,
}

impl std::fmt::Debug for PatchCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PatchCache({} patches, {} bytes, {} hits, {} misses)",
            self.map.len(),
            self.bytes,
            self.hits,
            self.misses
        )
    }
}

impl PatchCache {
    fn get(&mut self, key: u64) -> Option<Arc<Patch>> {
        self.tick += 1;
        let t = self.tick;
        self.map.get_mut(&key).map(|(p, used)| {
            *used = t;
            p.clone()
        })
    }

    fn put(&mut self, key: u64, p: Arc<Patch>) {
        self.tick += 1;
        self.bytes += size(&p);
        self.map.insert(key, (p, self.tick));
        // least recently used first
        while self.bytes > BUDGET && self.map.len() > 1 {
            let Some((&k, _)) = self.map.iter().min_by_key(|(_, (_, used))| *used) else {
                break;
            };
            if let Some((old, _)) = self.map.remove(&k) {
                self.bytes -= size(&old);
            }
        }
    }
}

fn size(p: &Patch) -> usize {
    match &p.pixels {
        Pixels::Rgba8(v) => v.len(),
        Pixels::Rgba16(v) => v.len() * 2,
    }
}

/// Box half-width of the three passes for a blur radius (σ = radius / 2: three boxes of width w
/// have σ² = (w² − 1) / 4).
fn box_k(radius: f32) -> u32 {
    let w = (radius * radius + 1.0).sqrt();
    (((w - 1.0) / 2.0).round() as u32).max(1)
}

/// Pixels the area touches (its bounds rounded outwards), inside the image.
fn pixel_bounds(a: &Area, w: u32, h: u32) -> Option<RectU> {
    let b = area_bounds(a);
    let x0 = b.x.floor().max(0.0);
    let y0 = b.y.floor().max(0.0);
    let x1 = (b.x + b.w).ceil().min(w as f32);
    let y1 = (b.y + b.h).ceil().min(h as f32);
    (x1 > x0 && y1 > y0).then_some(RectU {
        x: x0 as u32,
        y: y0 as u32,
        w: (x1 - x0) as u32,
        h: (y1 - y0) as u32,
    })
}

fn expand(r: RectU, by: u32, w: u32, h: u32) -> RectU {
    let x0 = r.x.saturating_sub(by);
    let y0 = r.y.saturating_sub(by);
    let x1 = (r.x + r.w + by).min(w);
    let y1 = (r.y + r.h + by).min(h);
    RectU {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    }
}

fn intersect(a: RectU, b: RectU) -> Option<RectU> {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.w).min(b.x + b.w);
    let y1 = (a.y + a.h).min(b.y + b.h);
    // not `then_some`: the sizes underflow when the rectangles are apart
    if x1 > x0 && y1 > y0 {
        Some(RectU {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        })
    } else {
        None
    }
}

fn key_of(r: &Redaction, read: RectU, deps: &[u64]) -> u64 {
    let mut s = DefaultHasher::new();
    let (Area::Rect { rect, rotation } | Area::Ellipse { rect, rotation }) = &r.area;
    matches!(r.area, Area::Ellipse { .. }).hash(&mut s);
    for v in [rect.x, rect.y, rect.w, rect.h, *rotation] {
        v.to_bits().hash(&mut s);
    }
    match &r.kind {
        RedactKind::Blur { radius } => (0u8, radius.to_bits()).hash(&mut s),
        RedactKind::Pixelate { block } => (1u8, *block).hash(&mut s),
        RedactKind::Solid { color } => (2u8, *color).hash(&mut s),
    }
    (read.x, read.y, read.w, read.h).hash(&mut s);
    deps.hash(&mut s);
    s.finish()
}

/// The patches of the document's redactions, in order (one per redaction that touches the image).
pub fn patches(
    doc: &DocState,
    w: u32,
    h: u32,
    src: SrcPx,
    colors: ColorMap,
    cache: &mut PatchCache,
) -> Vec<Arc<Patch>> {
    let mut done: Vec<(u64, Arc<Patch>)> = Vec::new();
    for r in &doc.redactions {
        let Some(bbox) = pixel_bounds(&r.area, w, h) else {
            continue;
        };
        let reach = match r.kind {
            RedactKind::Blur { radius } => 3 * box_k(radius),
            RedactKind::Pixelate { block } => block.max(1),
            RedactKind::Solid { .. } => 0,
        };
        let read = expand(bbox, reach, w, h);
        let deps: Vec<u64> = done
            .iter()
            .filter(|(_, p)| intersect(p.rect, read).is_some())
            .map(|(k, _)| *k)
            .collect();
        let key = key_of(r, read, &deps);
        let patch = match cache.get(key) {
            Some(p) => {
                cache.hits += 1;
                p
            }
            None => {
                cache.misses += 1;
                let earlier: Vec<&Patch> = done.iter().map(|(_, p)| &**p).collect();
                let p = Arc::new(compute(r, bbox, read, w, src, &earlier, colors));
                cache.put(key, p.clone());
                p
            }
        };
        done.push((key, patch));
    }
    done.into_iter().map(|(_, p)| p).collect()
}

/// Premultiplied, 16-bit scale.
type Px = [u32; 4];

fn premul16(c: [u32; 4]) -> Px {
    let a = c[3];
    let m = |v: u32| (v * a + 32767) / 65535;
    [m(c[0]), m(c[1]), m(c[2]), a]
}

fn load(src: SrcPx, at: usize) -> [u32; 4] {
    match src {
        SrcPx::R8(v) => {
            let p = &v[at * 4..at * 4 + 4];
            [
                p[0] as u32 * 257,
                p[1] as u32 * 257,
                p[2] as u32 * 257,
                p[3] as u32 * 257,
            ]
        }
        SrcPx::R16(v) => {
            let p = &v[at * 4..at * 4 + 4];
            [p[0] as u32, p[1] as u32, p[2] as u32, p[3] as u32]
        }
    }
}

fn straight16(p: Px) -> [u32; 4] {
    let a = p[3];
    let s = |v: u32| (v * 65535 + a / 2).checked_div(a).unwrap_or(0).min(65535);
    [s(p[0]), s(p[1]), s(p[2]), a]
}

/// The area as a path (rotation about its centre), image space.
fn area_path(a: &Area) -> Option<tiny_skia::Path> {
    let (Area::Rect { rect, rotation } | Area::Ellipse { rect, rotation }) = a;
    let r = Rect::from_xywh(rect.x, rect.y, rect.w, rect.h)?;
    let p = match a {
        Area::Rect { .. } => PathBuilder::from_rect(r),
        Area::Ellipse { .. } => PathBuilder::from_oval(r)?,
    };
    let c = rect.center();
    p.transform(Transform::from_rotate_at(rotation.to_degrees(), c.x, c.y))
}

/// The pixel square [px, px+1] × [py, py+1] meets the area (exact geometry, no sampling).
fn touches(a: &Area, px: f32, py: f32) -> bool {
    let (Area::Rect { rect, rotation } | Area::Ellipse { rect, rotation }) = a;
    let c = rect.center();
    let (s, co) = rotation.sin_cos();
    let (hw, hh) = (rect.w / 2.0, rect.h / 2.0);
    // corners in the area's own frame (centre at 0, unrotated)
    let corners = [(px, py), (px + 1.0, py), (px + 1.0, py + 1.0), (px, py + 1.0)].map(|(x, y)| {
        let (dx, dy) = (x - c.x, y - c.y);
        (dx * co + dy * s, -dx * s + dy * co)
    });
    match a {
        Area::Rect { .. } => {
            // separating axes: the area's own axes, then the image axes (its exact bounds)
            let (u0, u1) = corners
                .iter()
                .fold((f32::MAX, f32::MIN), |m, q| (m.0.min(q.0), m.1.max(q.0)));
            let (v0, v1) = corners
                .iter()
                .fold((f32::MAX, f32::MIN), |m, q| (m.0.min(q.1), m.1.max(q.1)));
            let ex = (hw * co).abs() + (hh * s).abs();
            let ey = (hw * s).abs() + (hh * co).abs();
            u0 <= hw
                && u1 >= -hw
                && v0 <= hh
                && v1 >= -hh
                && px <= c.x + ex
                && px + 1.0 >= c.x - ex
                && py <= c.y + ey
                && py + 1.0 >= c.y - ey
        }
        Area::Ellipse { .. } => {
            // in unit-circle space the square is a parallelogram
            let (a_, b_) = (hw.max(1e-6), hh.max(1e-6));
            let q = corners.map(|(x, y)| (x / a_, y / b_));
            if q.iter().any(|(x, y)| x * x + y * y <= 1.0) {
                return true;
            }
            // the centre inside the parallelogram (same side of every edge)
            let cross = |i: usize| {
                let (ax, ay) = q[i];
                let (bx, by) = q[(i + 1) % 4];
                (bx - ax) * (0.0 - ay) - (by - ay) * (0.0 - ax)
            };
            let signs = [cross(0), cross(1), cross(2), cross(3)];
            if signs.iter().all(|v| *v >= 0.0) || signs.iter().all(|v| *v <= 0.0) {
                return true;
            }
            // an edge crossing the circle
            (0..4).any(|i| {
                let (ax, ay) = q[i];
                let (bx, by) = q[(i + 1) % 4];
                let (vx, vy) = (bx - ax, by - ay);
                let t = ((-ax * vx - ay * vy) / (vx * vx + vy * vy).max(1e-12)).clamp(0.0, 1.0);
                let (cx, cy) = (ax + t * vx, ay + t * vy);
                cx * cx + cy * cy <= 1.0
            })
        }
    }
}

/// Three box passes, horizontal then vertical each time; edges clamped (the buffer's edge is the
/// image's edge wherever the read area reaches it, and elsewhere farther than the blur reaches).
/// Rows in parallel; the vertical pass walks rows with one running sum per column (cache-friendly),
/// in parallel bands of columns. Integer sums: the result doesn't depend on the split.
fn blur(buf: &mut [Px], w: usize, h: usize, k: usize) {
    use rayon::prelude::*;
    let n = (2 * k + 1) as u32;
    let div = |s: u32| (s + n / 2) / n;
    let mut tmp = vec![[0u32; 4]; buf.len()];
    let horizontal = |src: &[Px], dst: &mut [Px]| {
        dst.par_chunks_mut(w).enumerate().for_each(|(y, out)| {
            let row = &src[y * w..(y + 1) * w];
            let get = |i: isize| row[i.clamp(0, w as isize - 1) as usize];
            let mut sum = [0u32; 4];
            for i in -(k as isize)..=(k as isize) {
                let p = get(i);
                for c in 0..4 {
                    sum[c] += p[c];
                }
            }
            for (x, o) in out.iter_mut().enumerate() {
                *o = sum.map(div);
                let (add, sub) = (get(x as isize + k as isize + 1), get(x as isize - k as isize));
                for c in 0..4 {
                    sum[c] = sum[c] + add[c] - sub[c];
                }
            }
        });
    };
    // blocks of rows, each with one running sum per column started from its own first row:
    // independent blocks written in place, rows read in order (cache-friendly)
    let vertical = |src: &[Px], dst: &mut [Px]| {
        let block = h.div_ceil(rayon::current_num_threads().max(1) * 2).max(8);
        let row = |y: isize| &src[y.clamp(0, h as isize - 1) as usize * w..][..w];
        dst.par_chunks_mut(block * w).enumerate().for_each(|(b, out)| {
            let y0 = (b * block) as isize;
            let mut sum = vec![[0u32; 4]; w];
            for y in y0 - k as isize..=y0 + k as isize {
                for (s, p) in sum.iter_mut().zip(row(y)) {
                    for c in 0..4 {
                        s[c] += p[c];
                    }
                }
            }
            for (dy, line) in out.chunks_exact_mut(w).enumerate() {
                let y = y0 + dy as isize;
                for (o, s) in line.iter_mut().zip(&sum) {
                    *o = s.map(div);
                }
                let (add, sub) = (row(y + k as isize + 1), row(y - k as isize));
                for ((s, a), b) in sum.iter_mut().zip(add).zip(sub) {
                    for c in 0..4 {
                        s[c] = s[c] + a[c] - b[c];
                    }
                }
            }
        });
    };
    for _ in 0..3 {
        horizontal(buf, &mut tmp);
        vertical(&tmp, buf);
    }
}

fn compute(
    r: &Redaction,
    bbox: RectU,
    read: RectU,
    img_w: u32,
    src: SrcPx,
    earlier: &[&Patch],
    colors: ColorMap,
) -> Patch {
    let (rw, rh) = (read.w as usize, read.h as usize);
    // the read area: source with the earlier patches pasted, premultiplied 16-bit
    use rayon::prelude::*;
    let mut buf: Vec<Px> = vec![[0; 4]; rw * rh];
    buf.par_chunks_mut(rw).enumerate().for_each(|(y, row)| {
        let at0 = (read.y as usize + y) * img_w as usize + read.x as usize;
        for (x, p) in row.iter_mut().enumerate() {
            *p = load(src, at0 + x);
        }
    });
    for p in earlier {
        let Some(i) = intersect(p.rect, read) else { continue };
        for y in i.y..i.y + i.h {
            for x in i.x..i.x + i.w {
                let at = ((y - p.rect.y) * p.rect.w + (x - p.rect.x)) as usize;
                let v = load(SrcPx::from(&p.pixels), at);
                buf[(y - read.y) as usize * rw + (x - read.x) as usize] = v;
            }
        }
    }
    buf.par_iter_mut().for_each(|p| *p = premul16(*p));
    let (ox, oy) = ((bbox.x - read.x) as usize, (bbox.y - read.y) as usize);
    let (bw, bh) = (bbox.w as usize, bbox.h as usize);
    let mut out: Vec<Px> = (0..bh)
        .flat_map(|y| buf[(oy + y) * rw + ox..(oy + y) * rw + ox + bw].to_vec())
        .collect();
    match r.kind {
        RedactKind::Solid { color } => {
            let c = colors(Color::rgb(color[0], color[1], color[2]));
            let solid = [c.r as u32 * 257, c.g as u32 * 257, c.b as u32 * 257, 65535];
            out.par_chunks_mut(bw).enumerate().for_each(|(y, row)| {
                for (x, o) in row.iter_mut().enumerate() {
                    if touches(&r.area, (bbox.x as usize + x) as f32, (bbox.y as usize + y) as f32) {
                        *o = solid;
                    }
                }
            });
        }
        RedactKind::Blur { .. } | RedactKind::Pixelate { .. } => {
            let effect: Vec<Px> = match r.kind {
                RedactKind::Blur { radius } => {
                    blur(&mut buf, rw, rh, box_k(radius) as usize);
                    (0..bh)
                        .flat_map(|y| buf[(oy + y) * rw + ox..(oy + y) * rw + ox + bw].to_vec())
                        .collect()
                }
                RedactKind::Pixelate { block } => pixelate(&buf, read, bbox, &r.area, block.max(1)),
                RedactKind::Solid { .. } => unreachable!(),
            };
            let mut mask = Mask::new(bbox.w, bbox.h).expect("non-empty box");
            if let Some(path) = area_path(&r.area) {
                let t = Transform::from_translate(-(bbox.x as f32), -(bbox.y as f32));
                mask.fill_path(&path, FillRule::Winding, true, t);
            }
            out.par_iter_mut()
                .zip(effect.par_iter())
                .zip(mask.data().par_iter())
                .for_each(|((o, e), m)| {
                    let m = *m as i64;
                    if m == 0 {
                        return;
                    }
                    for c in 0..4 {
                        let (b, e) = (o[c] as i64, e[c] as i64);
                        o[c] = (b + ((e - b) * m + 127 * (e - b).signum()) / 255).clamp(0, 65535) as u32;
                    }
                });
        }
    }
    let pixels = match src {
        SrcPx::R8(_) => Pixels::Rgba8(
            out.par_iter()
                .flat_map_iter(|p| straight16(*p).map(|v| ((v * 255 + 32767) / 65535) as u8))
                .collect(),
        ),
        SrcPx::R16(_) => Pixels::Rgba16(
            out.par_iter()
                .flat_map_iter(|p| straight16(*p).map(|v| v as u16))
                .collect(),
        ),
    };
    Patch { rect: bbox, pixels }
}

/// Mean (premultiplied) per block of the grid anchored at the area's origin, for the box.
fn pixelate(buf: &[Px], read: RectU, bbox: RectU, a: &Area, block: u32) -> Vec<Px> {
    use rayon::prelude::*;
    let (Area::Rect { rect, .. } | Area::Ellipse { rect, .. }) = a;
    let (gx, gy) = (rect.x.floor() as i64, rect.y.floor() as i64);
    let b = block as i64;
    let rw = read.w as usize;
    let cell = |v: i64, g: i64| (v - g).div_euclid(b);
    // the blocks the box touches, each the mean of the block ∩ the read area (= ∩ the image:
    // the read area covers every block)
    let (cx0, cy0) = (cell(bbox.x as i64, gx), cell(bbox.y as i64, gy));
    let cx1 = cell((bbox.x + bbox.w - 1) as i64, gx);
    let cy1 = cell((bbox.y + bbox.h - 1) as i64, gy);
    let cols = (cx1 - cx0 + 1) as usize;
    let means: Vec<Px> = (0..cols * (cy1 - cy0 + 1) as usize)
        .into_par_iter()
        .map(|k| {
            let (kx, ky) = (cx0 + (k % cols) as i64, cy0 + (k / cols) as i64);
            let x0 = (gx + kx * b).max(read.x as i64);
            let y0 = (gy + ky * b).max(read.y as i64);
            let x1 = (gx + kx * b + b).min((read.x + read.w) as i64);
            let y1 = (gy + ky * b + b).min((read.y + read.h) as i64);
            let mut s = [0u64; 4];
            for yy in y0..y1 {
                let r = (yy - read.y as i64) as usize * rw;
                for p in &buf[r + (x0 - read.x as i64) as usize..r + (x1 - read.x as i64) as usize] {
                    for c in 0..4 {
                        s[c] += p[c] as u64;
                    }
                }
            }
            let n = (((x1 - x0) * (y1 - y0)).max(1)) as u64;
            s.map(|v| ((v + n / 2) / n) as u32)
        })
        .collect();
    let mut out = vec![[0u32; 4]; (bbox.w * bbox.h) as usize];
    out.par_chunks_mut(bbox.w as usize).enumerate().for_each(|(dy, line)| {
        let ky = (cell(bbox.y as i64 + dy as i64, gy) - cy0) as usize;
        for (dx, o) in line.iter_mut().enumerate() {
            let kx = (cell(bbox.x as i64 + dx as i64, gx) - cx0) as usize;
            *o = means[ky * cols + kx];
        }
    });
    out
}

/// Paste the patches (in order) into a straight region buffer `region` (image space) of the same
/// depth.
pub fn paste<T: Copy>(buf: &mut [T], region: RectU, patches: &[Arc<Patch>], pick: impl Fn(&Pixels) -> Option<&[T]>) {
    for p in patches {
        let (Some(i), Some(px)) = (intersect(p.rect, region), pick(&p.pixels)) else {
            continue;
        };
        for y in i.y..i.y + i.h {
            let s = (((y - p.rect.y) * p.rect.w + (i.x - p.rect.x)) * 4) as usize;
            let d = (((y - region.y) * region.w + (i.x - region.x)) * 4) as usize;
            let n = i.w as usize * 4;
            buf[d..d + n].copy_from_slice(&px[s..s + n]);
        }
    }
}
