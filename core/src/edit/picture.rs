//! Image shapes (watermarks): pixels the document carries, drawn like any shape (z-order,
//! opacity, rotation, the same renderer for the preview and the export). Inserted images are
//! decoded with the normal limits, converted to sRGB and reduced to at most `MAX_SIDE` on their
//! long side, and kept as PNG: that PNG is what a `.rvm` stores, its hash is the image's ID.

use super::model::Color;
use crate::decode::{DecodeOptions, Pixels};
use crate::formats::Format;
use std::sync::{Arc, Mutex};
use tiny_skia::Pixmap;

/// Longest side of an image shape's pixels (a logo or a signature needs no more).
pub const MAX_SIDE: u32 = 2048;
/// Largest PNG of an image shape (a 2048² RGBA PNG of noise is about 16 MiB).
pub const MAX_BYTES: usize = 32 << 20;

/// Mipmap chains by colour space: (fingerprint of the space, levels: full size, then halved).
type Chains = Vec<(u64, Vec<Arc<Pixmap>>)>;

/// Pixels of an image shape: sRGB, straight RGBA8, and the PNG they come from.
pub struct ImageData {
    pub id: String,
    pub w: u32,
    pub h: u32,
    pub png: Arc<[u8]>,
    rgba: Vec<u8>,
    /// Premultiplied, in the colour spaces it was drawn in (the screen's, the document's), most
    /// recent first; the halved levels are added when first needed.
    drawn: Mutex<Chains>,
}

impl std::fmt::Debug for ImageData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ImageData({} {}x{})", self.id, self.w, self.h)
    }
}

/// FNV-1a: stable across builds (the ID is saved in projects).
fn hash(b: &[u8]) -> String {
    let h = b.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, x| {
        (h ^ *x as u64).wrapping_mul(0x100_0000_01b3)
    });
    format!("{h:016x}")
}

fn png_of(rgba: &[u8], w: u32, h: u32) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    image::ImageEncoder::write_image(
        image::codecs::png::PngEncoder::new(&mut out),
        rgba,
        w,
        h,
        image::ExtendedColorType::Rgba8,
    )
    .map_err(|e| e.to_string())?;
    Ok(out)
}

impl ImageData {
    fn build(png: Vec<u8>, w: u32, h: u32, rgba: Vec<u8>) -> Arc<ImageData> {
        Arc::new(ImageData {
            id: hash(&png),
            w,
            h,
            png: png.into(),
            rgba,
            drawn: Mutex::default(),
        })
    }

    /// An image file to insert: any supported format (its first frame), colour-managed to sRGB,
    /// reduced to `MAX_SIDE`.
    pub fn from_file(bytes: &[u8], format: Option<Format>, opts: &DecodeOptions) -> Result<Arc<ImageData>, String> {
        let img = crate::decode::decode(bytes, format, opts).map_err(|e| e.to_string())?;
        if img.format == Format::Project {
            return Err("a project can't be inserted as an image".into());
        }
        let (w, h) = (img.size.w, img.size.h);
        let rgba = crate::color::into_srgb_straight(img);
        let s = (MAX_SIDE as f64 / w.max(h) as f64).min(1.0);
        let (nw, nh) = (
            ((w as f64 * s).round() as u32).max(1),
            ((h as f64 * s).round() as u32).max(1),
        );
        let rgba = if (nw, nh) == (w, h) {
            rgba
        } else {
            match crate::resample::resize(&Pixels::Rgba8(rgba), (w, h), (nw, nh)) {
                Pixels::Rgba8(v) => v,
                Pixels::Rgba16(_) => unreachable!("8 bit in, 8 bit out"),
            }
        };
        let png = png_of(&rgba, nw, nh)?;
        Ok(Self::build(png, nw, nh, rgba))
    }

    /// The PNG of a project's image entry: at most `MAX_BYTES` and `MAX_SIDE`, sRGB.
    pub fn from_png(png: &[u8]) -> Result<Arc<ImageData>, String> {
        if png.len() > MAX_BYTES {
            return Err("image larger than allowed".into());
        }
        let probe = crate::decode::probe(png, Some(Format::Png)).ok_or("not a PNG")?;
        if probe.format != Format::Png || probe.size.w.max(probe.size.h) > MAX_SIDE {
            return Err("not a PNG within the size limit".into());
        }
        let img =
            crate::decode::decode(png, Some(Format::Png), &DecodeOptions::default()).map_err(|e| e.to_string())?;
        let (w, h) = (img.size.w, img.size.h);
        let rgba = crate::color::into_srgb_straight(img);
        Ok(Self::build(png.to_vec(), w, h, rgba))
    }

    /// Premultiplied pixels in the target colour space (`colors`: sRGB → document or view
    /// space), for drawing about `w`×`h` target pixels: the smallest level of a mipmap chain
    /// (full size, then halved) still at least that large, so a bilinear sample never skips
    /// pixels (no aliasing). The colours are converted once per space and the levels are kept:
    /// a resize drag finds them ready. Computed under the lock, so the parallel bands of one
    /// frame don't compute the same level each.
    pub fn pixmap(&self, colors: super::render::ColorMap, w: u32, h: u32) -> Option<Arc<Pixmap>> {
        let probes = [
            Color::rgb(255, 0, 0),
            Color::rgb(0, 255, 0),
            Color::rgb(0, 0, 255),
            Color::rgb(200, 150, 100),
            Color::rgb(30, 60, 90),
        ];
        let identity = probes.iter().all(|c| colors(*c) == *c);
        let space = probes.iter().fold(0u64, |h, c| {
            let m = colors(*c);
            h.rotate_left(24) ^ ((m.r as u64) << 16 | (m.g as u64) << 8 | m.b as u64)
        });
        let mut chains = self.drawn.lock().unwrap_or_else(|e| e.into_inner());
        let i = match chains.iter().position(|(k, _)| *k == space) {
            Some(i) => i,
            None => {
                chains.insert(0, (space, vec![Arc::new(self.converted(colors, identity)?)]));
                // the screen's space and the document's
                chains.truncate(3);
                0
            }
        };
        let levels = &mut chains[i].1;
        let mut l = 0;
        loop {
            let (lw, lh) = (levels[l].width(), levels[l].height());
            if lw / 2 < w.max(1) || lh / 2 < h.max(1) {
                break;
            }
            if l + 1 == levels.len() {
                levels.push(Arc::new(halved(&levels[l])?));
            }
            l += 1;
        }
        Some(levels[l].clone())
    }

    /// Full size, premultiplied, in the target space (sRGB as it is).
    fn converted(&self, colors: super::render::ColorMap, identity: bool) -> Option<Pixmap> {
        use rayon::prelude::*;
        let mut px = self.rgba.clone();
        if !identity {
            px.par_chunks_mut(4).for_each(|p| {
                let c = colors(Color {
                    r: p[0],
                    g: p[1],
                    b: p[2],
                    a: 255,
                });
                p[..3].copy_from_slice(&[c.r, c.g, c.b]);
            });
        }
        super::render::premultiply(&mut px);
        Pixmap::from_vec(px, tiny_skia::IntSize::from_wh(self.w, self.h)?)
    }
}

/// The next mipmap level: each pixel the mean of a 2×2 block (premultiplied: the colours of
/// transparent pixels don't bleed); an odd last row or column is averaged with itself.
fn halved(p: &Pixmap) -> Option<Pixmap> {
    use rayon::prelude::*;
    let (w, h) = (p.width() as usize, p.height() as usize);
    let (nw, nh) = (w.div_ceil(2), h.div_ceil(2));
    let src = p.data();
    let mut out = vec![0u8; nw * nh * 4];
    out.par_chunks_mut(nw * 4).enumerate().for_each(|(y, row)| {
        let (y0, y1) = (2 * y, (2 * y + 1).min(h - 1));
        for (x, o) in row.chunks_exact_mut(4).enumerate() {
            let (x0, x1) = (2 * x, (2 * x + 1).min(w - 1));
            for c in 0..4 {
                let at = |xx: usize, yy: usize| src[(yy * w + xx) * 4 + c] as u32;
                o[c] = ((at(x0, y0) + at(x1, y0) + at(x0, y1) + at(x1, y1) + 2) / 4) as u8;
            }
        }
    });
    Pixmap::from_vec(out, tiny_skia::IntSize::from_wh(nw as u32, nh as u32)?)
}

/// An image shape's pixels: saved by ID; the data is attached when inserted or loaded.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ImageRef {
    pub id: String,
    #[serde(skip)]
    pub data: Option<Arc<ImageData>>,
}

impl PartialEq for ImageRef {
    fn eq(&self, o: &Self) -> bool {
        self.id == o.id
    }
}

impl ImageRef {
    pub fn new(data: Arc<ImageData>) -> ImageRef {
        ImageRef {
            id: data.id.clone(),
            data: Some(data),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let rgba: Vec<u8> = (0..w * h)
            .flat_map(|i| [(i % 251) as u8, 40, 200, (i % 7 * 30) as u8])
            .collect();
        png_of(&rgba, w, h).unwrap()
    }

    #[test]
    fn insert_reduces_and_ids_are_stable() {
        let big = ImageData::from_file(&png(3000, 1000), None, &DecodeOptions::default()).unwrap();
        assert_eq!((big.w, big.h), (MAX_SIDE, 683));
        let again = ImageData::from_png(&big.png).unwrap();
        assert_eq!((again.id.as_str(), again.w, again.h), (big.id.as_str(), big.w, big.h));
        assert_eq!(big.id.len(), 16);
        let small = ImageData::from_file(&png(40, 30), None, &DecodeOptions::default()).unwrap();
        assert_eq!((small.w, small.h), (40, 30), "small images kept as they are");
        assert_ne!(small.id, big.id);
    }

    #[test]
    fn project_entries_are_checked() {
        assert!(ImageData::from_png(b"not a png").is_err());
        assert!(
            ImageData::from_png(&png(MAX_SIDE + 1, 4)).is_err(),
            "above the side limit"
        );
        let jpeg = {
            let mut out = Vec::new();
            image::RgbImage::new(8, 8)
                .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Jpeg)
                .unwrap();
            out
        };
        assert!(ImageData::from_png(&jpeg).is_err(), "PNG only");
    }

    #[test]
    fn pixmaps_are_premultiplied_converted_and_cached() {
        let d = ImageData::from_png(&png(16, 8)).unwrap();
        let same = |c: Color| c;
        let a = d.pixmap(&same, 16, 8).unwrap();
        assert!(Arc::ptr_eq(&a, &d.pixmap(&same, 16, 8).unwrap()), "cached");
        let p = a.pixel(3, 0).unwrap();
        assert!(p.red() <= p.alpha() && p.blue() <= p.alpha(), "premultiplied");
        let swap = |c: Color| Color {
            r: c.b,
            g: c.g,
            b: c.r,
            a: c.a,
        };
        let b = d.pixmap(&swap, 16, 8).unwrap();
        assert!(!Arc::ptr_eq(&a, &b), "another colour space: another pixmap");
        // levels: 16×8, 8×4, 4×2, 2×1: the smallest still at least the size asked for
        assert_eq!(d.pixmap(&same, 4, 2).unwrap().width(), 4, "reduced");
        assert_eq!(d.pixmap(&same, 5, 2).unwrap().width(), 8, "never below the target");
        assert_eq!(d.pixmap(&same, 100, 100).unwrap().width(), 16, "never above full size");
        let small = d.pixmap(&same, 4, 2).unwrap();
        assert!(Arc::ptr_eq(&small, &d.pixmap(&same, 3, 2).unwrap()), "levels kept");
        // odd sizes halve to ceil, every pixel averaged with its neighbours
        let odd = Pixmap::from_vec(
            [200, 100, 0, 255].repeat(15),
            tiny_skia::IntSize::from_wh(5, 3).unwrap(),
        )
        .unwrap();
        let h = halved(&odd).unwrap();
        assert_eq!((h.width(), h.height()), (3, 2));
        assert!(
            h.data().chunks_exact(4).all(|p| p == [200, 100, 0, 255]),
            "a flat colour stays flat"
        );
    }
}
