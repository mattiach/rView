//! Image adjustments: per-pixel operations on the source, below the redactions and the shapes
//! (only the image is adjusted). They work on the straight, encoded values of the document space
//! at its bit depth. Exposure and temperature act on light, so for them the values are decoded
//! with the sRGB curve first: exact for sRGB documents, an approximation for other profiles.
//!
//! Per channel, as one table: exposure and temperature (gains in linear light), brightness (a
//! gamma curve: black and white stay put), contrast (a slope around mid-grey). Then, as one 3×3
//! matrix: saturation (towards or away from the Rec. 709 luma) and hue (a rotation about the grey
//! axis, luma kept). Last, sharpness: an unsharp mask of the luma (Gaussian, σ = 1 px, edges
//! clamped) added equally to the three channels, so edges get crisper without colour fringes.
//! Alpha is never changed.

use crate::decode::Pixels;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

/// All neutral by default (no change).
#[derive(Clone, Copy, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Adjustments {
    /// Stops, −3 … +3.
    pub exposure: f32,
    /// −100 … +100.
    pub brightness: f32,
    /// −100 … +100.
    pub contrast: f32,
    /// −100 (grey) … +100 (twice as saturated).
    pub saturation: f32,
    /// −100 (cooler, blue) … +100 (warmer, amber).
    pub temperature: f32,
    /// Degrees, −180 … +180.
    pub hue: f32,
    /// 0 (off) … 100 (an unsharp mask of 200 %).
    pub sharpness: f32,
}

/// A parameter: its field, message id, range.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Param {
    Exposure,
    Brightness,
    Contrast,
    Saturation,
    Temperature,
    Hue,
    Sharpness,
}

impl Param {
    pub const ALL: &'static [Param] = &[
        Param::Exposure,
        Param::Brightness,
        Param::Contrast,
        Param::Saturation,
        Param::Temperature,
        Param::Hue,
        Param::Sharpness,
    ];

    pub fn range(self) -> (f32, f32) {
        match self {
            Param::Exposure => (-3.0, 3.0),
            Param::Hue => (-180.0, 180.0),
            Param::Sharpness => (0.0, 100.0),
            _ => (-100.0, 100.0),
        }
    }

    /// Stable name (driver buttons, messages `markup-adjust-<name>`).
    pub fn name(self) -> &'static str {
        match self {
            Param::Exposure => "exposure",
            Param::Brightness => "brightness",
            Param::Contrast => "contrast",
            Param::Saturation => "saturation",
            Param::Temperature => "temperature",
            Param::Hue => "hue",
            Param::Sharpness => "sharpness",
        }
    }

    pub fn from_name(s: &str) -> Option<Param> {
        Param::ALL.iter().copied().find(|p| p.name() == s)
    }
}

impl Adjustments {
    pub fn get(&self, p: Param) -> f32 {
        match p {
            Param::Exposure => self.exposure,
            Param::Brightness => self.brightness,
            Param::Contrast => self.contrast,
            Param::Saturation => self.saturation,
            Param::Temperature => self.temperature,
            Param::Hue => self.hue,
            Param::Sharpness => self.sharpness,
        }
    }

    pub fn set(&mut self, p: Param, v: f32) {
        let f = match p {
            Param::Exposure => &mut self.exposure,
            Param::Brightness => &mut self.brightness,
            Param::Contrast => &mut self.contrast,
            Param::Saturation => &mut self.saturation,
            Param::Temperature => &mut self.temperature,
            Param::Hue => &mut self.hue,
            Param::Sharpness => &mut self.sharpness,
        };
        *f = v;
    }

    pub fn is_neutral(&self) -> bool {
        *self == Adjustments::default()
    }

    /// Every value finite and in its range (edits and projects are checked with it).
    pub fn valid(&self) -> bool {
        Param::ALL.iter().all(|p| {
            let (lo, hi) = p.range();
            let v = self.get(*p);
            v.is_finite() && (lo..=hi).contains(&v)
        })
    }

    /// Identity of the values (cache keys): the bits, so −0 and 0 differ only harmlessly.
    pub fn key(&self) -> u64 {
        Param::ALL.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, p| {
            (h ^ self.get(*p).to_bits() as u64).wrapping_mul(0x100_0000_01b3)
        })
    }
}

fn srgb_dec(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn srgb_enc(l: f32) -> f32 {
    if l <= 0.003_130_8 {
        l * 12.92
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    }
}

/// The per-channel curve (0…1 → 0…1) of channel `c` (0 = red, 2 = blue).
fn curve(a: &Adjustments, c: usize, v: f32) -> f32 {
    let t = a.temperature / 100.0;
    let gain = 2f32.powf(a.exposure)
        * match c {
            0 => 2f32.powf(0.3 * t),
            2 => 2f32.powf(-0.3 * t),
            _ => 1.0,
        };
    let mut v = if gain == 1.0 {
        v
    } else {
        srgb_enc((srgb_dec(v) * gain).min(1.0))
    };
    if a.brightness != 0.0 {
        v = v.max(0.0).powf(2f32.powf(-a.brightness / 100.0));
    }
    if a.contrast != 0.0 {
        v = (v - 0.5) * 4f32.powf(a.contrast / 100.0) + 0.5;
    }
    v.clamp(0.0, 1.0)
}

/// Saturation then hue as one matrix (row-major), None = identity.
fn matrix(a: &Adjustments) -> Option<[f32; 9]> {
    if a.saturation == 0.0 && a.hue == 0.0 {
        return None;
    }
    let (lr, lg, lb) = (0.2126f32, 0.7152f32, 0.0722f32);
    let s = 1.0 + a.saturation / 100.0;
    let sat = [
        lr + (1.0 - lr) * s,
        lg - lg * s,
        lb - lb * s,
        lr - lr * s,
        lg + (1.0 - lg) * s,
        lb - lb * s,
        lr - lr * s,
        lg - lg * s,
        lb + (1.0 - lb) * s,
    ];
    // rotation about the grey axis, corrected so the luma doesn't change
    let (sn, cs) = a.hue.to_radians().sin_cos();
    let hue = [
        lr + cs * (1.0 - lr) - sn * lr,
        lg - cs * lg - sn * lg,
        lb - cs * lb + sn * (1.0 - lb),
        lr - cs * lr + sn * 0.143,
        lg + cs * (1.0 - lg) + sn * 0.140,
        lb - cs * lb - sn * 0.283,
        lr - cs * lr - sn * (1.0 - lr),
        lg - cs * lg + sn * lg,
        lb + cs * (1.0 - lb) + sn * lb,
    ];
    let mut m = [0f32; 9];
    for r in 0..3 {
        for c in 0..3 {
            m[r * 3 + c] = (0..3).map(|k| hue[r * 3 + k] * sat[k * 3 + c]).sum();
        }
    }
    Some(m)
}

/// The source (`w` pixels wide) with the adjustments applied, at the same depth (a copy; the
/// caller skips neutral adjustments).
pub fn apply(px: &Pixels, w: u32, a: &Adjustments) -> Pixels {
    let w = (w as usize).max(1);
    match px {
        Pixels::Rgba8(v) => Pixels::Rgba8(sharpen(run(v, a, 255), w, a.sharpness, 255.0)),
        Pixels::Rgba16(v) => Pixels::Rgba16(sharpen(run(v, a, 65535), w, a.sharpness, 65535.0)),
    }
}

/// Gaussian σ = 1 px, radius 3.
const KERNEL: [f32; 7] = {
    // exp(-k²/2) for k = 0…3, normalised over −3…3
    let e = [1.0, 0.606_530_7, 0.135_335_28, 0.011_108_997];
    let sum = e[0] + 2.0 * (e[1] + e[2] + e[3]);
    [
        e[3] / sum,
        e[2] / sum,
        e[1] / sum,
        e[0] / sum,
        e[1] / sum,
        e[2] / sum,
        e[3] / sum,
    ]
};

fn luma<T: Sample>(p: &[T]) -> f32 {
    0.2126 * p[0].into() as f32 + 0.7152 * p[1].into() as f32 + 0.0722 * p[2].into() as f32
}

/// Unsharp mask of the luma: one blurred-row buffer (4 bytes per pixel), the result in place.
fn sharpen<T: Sample>(mut v: Vec<T>, w: usize, sharpness: f32, max: f32) -> Vec<T> {
    if sharpness <= 0.0 || v.is_empty() {
        return v;
    }
    let amount = sharpness / 100.0 * 2.0;
    let h = v.len() / 4 / w;
    let mut rows = vec![0f32; w * h];
    rows.par_chunks_mut(w).enumerate().for_each(|(y, out)| {
        let line = &v[y * w * 4..(y + 1) * w * 4];
        for (x, o) in out.iter_mut().enumerate() {
            *o = KERNEL
                .iter()
                .enumerate()
                .map(|(k, kw)| {
                    let xx = (x as isize + k as isize - 3).clamp(0, w as isize - 1) as usize;
                    kw * luma(&line[xx * 4..xx * 4 + 4])
                })
                .sum();
        }
    });
    v.par_chunks_mut(w * 4).enumerate().for_each(|(y, line)| {
        for (x, p) in line.chunks_exact_mut(4).enumerate() {
            let blurred: f32 = KERNEL
                .iter()
                .enumerate()
                .map(|(k, kw)| kw * rows[(y as isize + k as isize - 3).clamp(0, h as isize - 1) as usize * w + x])
                .sum();
            let d = amount * (luma(p) - blurred);
            for c in &mut p[..3] {
                *c = T::from_u32(((*c).into() as f32 + d).round().clamp(0.0, max) as u32);
            }
        }
    });
    v
}

trait Sample: Copy + Send + Sync + Into<u32> {
    fn from_u32(v: u32) -> Self;
}
impl Sample for u8 {
    fn from_u32(v: u32) -> Self {
        v as u8
    }
}
impl Sample for u16 {
    fn from_u32(v: u32) -> Self {
        v as u16
    }
}

fn run<T: Sample>(v: &[T], a: &Adjustments, max: u32) -> Vec<T> {
    let mf = max as f32;
    // one table per channel, over every input value
    let lut: [Vec<T>; 3] = std::array::from_fn(|c| {
        (0..=max)
            .into_par_iter()
            .map(|i| T::from_u32((curve(a, c, i as f32 / mf) * mf).round() as u32))
            .collect()
    });
    let m = matrix(a);
    let mut out = v.to_vec();
    out.par_chunks_mut(4 << 12).for_each(|chunk| {
        for p in chunk.chunks_exact_mut(4) {
            let rgb = [0, 1, 2].map(|c| lut[c][p[c].into() as usize]);
            match &m {
                None => p[..3].copy_from_slice(&rgb),
                Some(m) => {
                    let f = rgb.map(|s| s.into() as f32);
                    for r in 0..3 {
                        let x = m[r * 3] * f[0] + m[r * 3 + 1] * f[1] + m[r * 3 + 2] * f[2];
                        p[r] = T::from_u32(x.round().clamp(0.0, mf) as u32);
                    }
                }
            }
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(a: Adjustments, rgb: [u8; 3]) -> [u8; 3] {
        let Pixels::Rgba8(v) = apply(&Pixels::Rgba8(vec![rgb[0], rgb[1], rgb[2], 77]), 1, &a) else {
            unreachable!()
        };
        assert_eq!(v[3], 77, "alpha kept");
        [v[0], v[1], v[2]]
    }

    fn with(p: Param, v: f32) -> Adjustments {
        let mut a = Adjustments::default();
        a.set(p, v);
        a
    }

    #[test]
    fn neutral_is_the_identity_at_both_depths() {
        let a = Adjustments::default();
        assert!(a.is_neutral() && a.valid());
        let v8: Vec<u8> = (0..=255).flat_map(|i| [i, 255 - i, i / 2, i]).collect();
        assert!(matches!(apply(&Pixels::Rgba8(v8.clone()), 16, &a), Pixels::Rgba8(o) if o == v8));
        let v16: Vec<u16> = (0..4096u16).flat_map(|i| [i * 16, 65535 - i * 16, i, 1000]).collect();
        assert!(matches!(apply(&Pixels::Rgba16(v16.clone()), 64, &a), Pixels::Rgba16(o) if o == v16));
    }

    #[test]
    fn each_parameter_moves_the_right_way() {
        let mid = [120, 100, 80];
        let up = |p| one(with(p, Param::range(p).1 / 2.0), mid);
        let down = |p| one(with(p, Param::range(p).0 / 2.0), mid);
        for p in [Param::Exposure, Param::Brightness] {
            assert!(up(p).iter().zip(mid).all(|(a, b)| *a > b), "{p:?} up {:?}", up(p));
            assert!(down(p).iter().zip(mid).all(|(a, b)| *a < b), "{p:?} down {:?}", down(p));
        }
        // contrast spreads the channels around mid-grey, and narrows them when negative
        let spread = |c: [u8; 3]| c[0] as i32 - c[2] as i32;
        assert!(spread(up(Param::Contrast)) > 40 && spread(down(Param::Contrast)) < 40);
        assert!(spread(up(Param::Saturation)) > 40 && spread(down(Param::Saturation)) < 40);
        assert_eq!(
            spread(one(with(Param::Saturation, -100.0), mid)),
            0,
            "fully desaturated: grey"
        );
        // warmer: more red, less blue
        let w = up(Param::Temperature);
        assert!(w[0] > mid[0] && w[2] < mid[2], "{w:?}");
        let c = down(Param::Temperature);
        assert!(c[0] < mid[0] && c[2] > mid[2], "{c:?}");
        // a third of a turn moves red towards green; there and back returns
        let r = one(with(Param::Hue, 120.0), [200, 40, 40]);
        assert!(r[1] > r[0] && r[1] > r[2], "{r:?}");
        let back = one(with(Param::Hue, 60.0), one(with(Param::Hue, -60.0), [160, 90, 70]));
        assert!(
            back.iter().zip([160, 90, 70]).all(|(a, b)| a.abs_diff(b) <= 3),
            "{back:?}"
        );
    }

    #[test]
    fn greys_and_extremes() {
        // hue and saturation leave greys alone; black and white stay put for brightness/hue
        for v in [0u8, 128, 255] {
            for p in [Param::Hue, Param::Saturation] {
                let g = one(with(p, Param::range(p).1), [v; 3]);
                assert!(g.iter().all(|c| c.abs_diff(v) <= 1), "{p:?} {v} {g:?}");
            }
        }
        for b in [-100.0, 100.0] {
            assert_eq!(one(with(Param::Brightness, b), [0, 255, 0]), [0, 255, 0]);
        }
        // every value at both ends of every range stays in range and monotonic
        for p in Param::ALL.iter().copied() {
            for v in [Param::range(p).0, Param::range(p).1] {
                let a = with(p, v);
                assert!(a.valid());
                for c in [0, 1, 2] {
                    let ys: Vec<f32> = (0..=64).map(|i| curve(&a, c, i as f32 / 64.0)).collect();
                    assert!(ys.windows(2).all(|w| w[1] >= w[0]), "{p:?} {v} channel {c}");
                }
            }
        }
    }

    #[test]
    fn sixteen_bit_keeps_its_precision() {
        let a = with(Param::Exposure, 0.5);
        let Pixels::Rgba16(v) = apply(&Pixels::Rgba16(vec![30000, 30001, 30002, 65535]), 1, &a) else {
            panic!("depth changed")
        };
        assert!(v[0] < v[1] && v[1] < v[2], "{v:?}: neighbours stay distinct");
    }

    /// A 16×9 image, left half `dark`, right half `light` (grey, opaque), sharpened.
    fn edge(sharpness: f32, dark: u8, light: u8) -> Vec<u8> {
        let v: Vec<u8> = (0..16 * 9)
            .flat_map(|i| {
                let g = if i % 16 < 8 { dark } else { light };
                [g, g, g, 255]
            })
            .collect();
        match apply(&Pixels::Rgba8(v), 16, &with(Param::Sharpness, sharpness)) {
            Pixels::Rgba8(o) => o,
            Pixels::Rgba16(_) => unreachable!(),
        }
    }

    #[test]
    fn sharpness_steepens_edges_only() {
        fn at(v: &[u8], x: usize) -> &[u8] {
            &v[(4 * 16 + x) * 4..][..4]
        }
        let soft = edge(30.0, 80, 160);
        let hard = edge(100.0, 80, 160);
        // overshoot on both sides of the edge, more with more sharpness; greys stay grey
        assert!(
            at(&soft, 7)[0] < 80 && at(&soft, 8)[0] > 160,
            "{:?} {:?}",
            at(&soft, 7),
            at(&soft, 8)
        );
        assert!(at(&hard, 7)[0] < at(&soft, 7)[0] && at(&hard, 8)[0] > at(&soft, 8)[0]);
        for p in hard.chunks_exact(4) {
            assert!(p[0] == p[1] && p[1] == p[2] && p[3] == 255, "{p:?}");
        }
        // far from the edge (more than the kernel's reach), and flat images: unchanged
        assert_eq!(at(&hard, 0), [80, 80, 80, 255]);
        assert_eq!(at(&hard, 15), [160, 160, 160, 255]);
        assert!(edge(100.0, 120, 120).chunks_exact(4).all(|p| p == [120, 120, 120, 255]));
        // clamped, never wrapped
        let extreme = edge(100.0, 0, 255);
        assert_eq!((at(&extreme, 7)[0], at(&extreme, 8)[0]), (0, 255));
        // tiny images don't panic
        let one = apply(&Pixels::Rgba16(vec![1, 2, 3, 4]), 1, &with(Param::Sharpness, 100.0));
        assert!(matches!(one, Pixels::Rgba16(v) if v == [1, 2, 3, 4]));
    }

    #[test]
    fn validity_and_keys() {
        assert!(!with(Param::Exposure, 3.5).valid());
        assert!(!with(Param::Hue, f32::NAN).valid());
        assert!(!with(Param::Contrast, -101.0).valid());
        assert_ne!(with(Param::Contrast, 10.0).key(), with(Param::Brightness, 10.0).key());
        assert_eq!(with(Param::Hue, 5.0).key(), with(Param::Hue, 5.0).key());
        for p in Param::ALL {
            assert_eq!(Param::from_name(p.name()), Some(*p));
        }
    }
}
