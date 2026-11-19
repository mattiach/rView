//! Benches: folder scan 10k, decode 12 MP (JPEG/PNG/WebP, incl. view preparation), JPEG XL 25 MP from the
//! real-file fixture (SKIPPED without it), WIC preview of a 50 MP JPEG.
//! `cargo bench -p rv-core --bench decode`

use image::ImageEncoder;
use rv_core::decode::{self, DecodeOptions};
use rv_core::folder::Folder;
use rv_core::settings::Settings;
use std::path::PathBuf;

fn photo(w: u32, h: u32) -> image::RgbImage {
    // smooth gradients + some texture, roughly photo-like compressibility
    image::RgbImage::from_fn(w, h, |x, y| {
        let n = (x.wrapping_mul(2654435761) ^ y.wrapping_mul(40503)) >> 28;
        image::Rgb([(x / 16) as u8 ^ n as u8, (y / 12) as u8, ((x + y) / 20) as u8])
    })
}

fn jpeg(img: &image::RgbImage) -> Vec<u8> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)
        .write_image(img, img.width(), img.height(), image::ExtendedColorType::Rgb8)
        .unwrap();
    out
}

fn folder_10k() -> PathBuf {
    let dir = std::env::temp_dir().join("rview-bench-10k");
    if std::fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0) != 10_000 {
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..10_000 {
            std::fs::write(dir.join(format!("IMG_{i}.jpg")), b"x").unwrap();
        }
    }
    dir
}

fn main() {
    let dir = folder_10k();
    let s = Settings::default();
    bench("folder scan+sort 10k (name)", 20, || Folder::load(&dir, &s, None));

    let img12 = photo(4000, 3000);
    let opts = DecodeOptions::default();
    let j12 = jpeg(&img12);
    let mut p12 = Vec::new();
    image::codecs::png::PngEncoder::new(&mut p12)
        .write_image(&img12, 4000, 3000, image::ExtendedColorType::Rgb8)
        .unwrap();
    let mut w12 = Vec::new();
    image::codecs::webp::WebPEncoder::new_lossless(&mut w12)
        .write_image(&img12, 4000, 3000, image::ExtendedColorType::Rgb8)
        .unwrap();
    for (name, bytes) in [("jpeg", &j12), ("png", &p12), ("webp lossless", &w12)] {
        bench(&format!("decode 12 MP + prepare_view: {name}"), 10, || {
            rv_core::color::prepare_view(&decode::decode(bytes, None, &opts).unwrap())
        });
    }
    // no JPEG XL encoder: the 25 MP real-file fixture (libjxl, single pass), when downloaded/generated
    let jxl = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/large.jxl");
    match std::fs::read(&jxl) {
        Ok(b) => bench("decode 25 MP + prepare_view: jxl (large.jxl)", 5, || {
            rv_core::color::prepare_view(&decode::decode(&b, None, &opts).unwrap())
        }),
        Err(_) => println!("SKIPPED: decode JXL (tests/fixtures/large.jxl not present)"),
    }

    let j50 = jpeg(&photo(8660, 5774));
    bench("50 MP JPEG: WIC preview >=2560 + prepare_view", 10, || {
        rv_core::color::prepare_view(&decode::decode_preview(&j50, rv_core::formats::Format::Jpeg, 2560).unwrap())
    });
    bench("50 MP JPEG: full decode + prepare_view", 10, || {
        rv_core::color::prepare_view(&decode::decode(&j50, None, &opts).unwrap())
    });
}

/// Median and best of `n` runs of `f` (after one warm-up), printed as `BENCH <name>: ...`.
/// Plain timing instead of criterion (which pulls C code through `alloca`).
fn bench<T>(name: &str, n: usize, mut f: impl FnMut() -> T) {
    std::hint::black_box(f());
    let mut t: Vec<f64> = (0..n)
        .map(|_| {
            let s = std::time::Instant::now();
            std::hint::black_box(f());
            s.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    t.sort_by(f64::total_cmp);
    println!(
        "BENCH {name}: median {:.1} ms, best {:.1} ms ({n} runs)",
        t[n / 2],
        t[0]
    );
}
