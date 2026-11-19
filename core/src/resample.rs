//! Resizing for export: a separable Lanczos-3 filter on premultiplied values, so transparent
//! pixels don't bleed their colour into the edges. When shrinking, the filter is widened by the
//! scale factor, so it averages every source pixel it covers (no aliasing). It works in the
//! encoded values of the document space, as the rest of the export does, and keeps the bit depth.

use crate::decode::Pixels;
use rayon::prelude::*;

/// Largest upscaled output (pixels). Shrinking, or keeping the source's pixel count, is always
/// allowed.
pub const MAX_UPSCALE_PIXELS: u64 = 1 << 28;

/// Whether `w`×`h` → `nw`×`nh` is allowed.
pub fn allowed(w: u32, h: u32, nw: u32, nh: u32) -> bool {
    let (src, dst) = (w as u64 * h as u64, nw as u64 * nh as u64);
    nw > 0 && nh > 0 && (dst <= src || dst <= MAX_UPSCALE_PIXELS)
}

fn lanczos3(x: f32) -> f32 {
    if x == 0.0 {
        return 1.0;
    }
    if x.abs() >= 3.0 {
        return 0.0;
    }
    let px = std::f32::consts::PI * x;
    3.0 * px.sin() * (px / 3.0).sin() / (px * px)
}

/// For each output index: the first source index and the normalised weights from there.
fn weights(src: u32, dst: u32) -> Vec<(usize, Vec<f32>)> {
    let scale = src as f32 / dst as f32;
    let widen = scale.max(1.0);
    let support = 3.0 * widen;
    (0..dst)
        .map(|i| {
            let centre = (i as f32 + 0.5) * scale;
            let first = (centre - support).floor().max(0.0) as usize;
            let last = ((centre + support).ceil() as usize).min(src as usize);
            let mut w: Vec<f32> = (first..last)
                .map(|j| lanczos3((j as f32 + 0.5 - centre) / widen))
                .collect();
            let sum: f32 = w.iter().sum();
            w.iter_mut().for_each(|v| *v /= sum);
            (first, w)
        })
        .collect()
}

/// Straight samples in 0..=max, premultiplied as f32 per row, resized, then made straight again.
fn run<T: Copy + Send + Sync>(
    v: &[T],
    (w, h): (u32, u32),
    (nw, nh): (u32, u32),
    max: f32,
    get: impl Fn(T) -> f32 + Sync,
    put: impl Fn(f32) -> T + Sync,
) -> Vec<T> {
    let (hx, vy) = (weights(w, nw), weights(h, nh));
    // ponytail: one full intermediate (nw × h × 16 bytes); per-band passes if huge images need it
    let mut tmp = vec![0f32; nw as usize * h as usize * 4];
    tmp.par_chunks_mut(nw as usize * 4).enumerate().for_each(|(y, row)| {
        let src = &v[y * w as usize * 4..(y + 1) * w as usize * 4];
        for (o, (first, ws)) in row.chunks_exact_mut(4).zip(&hx) {
            let mut acc = [0f32; 4];
            for (k, wt) in ws.iter().enumerate() {
                let p = &src[(first + k) * 4..(first + k) * 4 + 4];
                let a = get(p[3]) / max;
                for c in 0..3 {
                    acc[c] += get(p[c]) * a * wt;
                }
                acc[3] += a * wt;
            }
            o.copy_from_slice(&acc);
        }
    });
    let mut out = vec![put(0.0); nw as usize * nh as usize * 4];
    out.par_chunks_mut(nw as usize * 4)
        .zip(&vy)
        .for_each(|(row, (first, ws))| {
            for (x, o) in row.chunks_exact_mut(4).enumerate() {
                let mut acc = [0f32; 4];
                for (k, wt) in ws.iter().enumerate() {
                    let p = &tmp[((first + k) * nw as usize + x) * 4..][..4];
                    for c in 0..4 {
                        acc[c] += p[c] * wt;
                    }
                }
                // straight again by the unclamped alpha: its overshoot at an edge doesn't tint
                let a = acc[3];
                for c in 0..3 {
                    o[c] = put(if a > 0.0 { (acc[c] / a).clamp(0.0, max) } else { 0.0 });
                }
                o[3] = put(a.clamp(0.0, 1.0) * max);
            }
        });
    out
}

/// `px` (`w`×`h`, straight RGBA) resized to `nw`×`nh`, at the same bit depth.
pub fn resize(px: &Pixels, (w, h): (u32, u32), (nw, nh): (u32, u32)) -> Pixels {
    match px {
        Pixels::Rgba8(v) => Pixels::Rgba8(run(v, (w, h), (nw, nh), 255.0, |s| s as f32, |f| f.round() as u8)),
        Pixels::Rgba16(v) => Pixels::Rgba16(run(v, (w, h), (nw, nh), 65535.0, |s| s as f32, |f| f.round() as u16)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_sum_to_one_and_stay_in_range() {
        for (s, d) in [(1, 1), (1, 7), (7, 1), (10, 3), (3, 10), (1000, 999), (6000, 3000)] {
            for (first, w) in weights(s, d) {
                assert!(first + w.len() <= s as usize, "{s}->{d}");
                assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-4, "{s}->{d}");
            }
        }
    }

    #[test]
    fn flat_colour_stays_flat_and_transparent_colour_doesnt_bleed() {
        // left half opaque orange, right half fully transparent green
        let (w, h) = (8u32, 2u32);
        let v: Vec<u8> = (0..w * h)
            .flat_map(|i| if i % w < 4 { [200, 100, 50, 255] } else { [0, 255, 0, 0] })
            .collect();
        for size in [(4, 1), (3, 2), (16, 4), (5, 3)] {
            let Pixels::Rgba8(o) = resize(&Pixels::Rgba8(v.clone()), (w, h), size) else {
                unreachable!()
            };
            assert_eq!(o.len(), (size.0 * size.1 * 4) as usize);
            for p in o.chunks_exact(4).filter(|p| p[3] > 0) {
                assert_eq!(&p[..3], &[200, 100, 50], "{size:?}: no fringe or overshoot in {p:?}");
            }
        }
    }
}
