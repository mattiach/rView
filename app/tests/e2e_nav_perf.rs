//! Navigation targets, measured end to end (request → frame presented) in the real app with the
//! hardware adapter. Thresholds are asserted only in release builds:
//! `cargo test --release -p rview --test e2e_nav_perf`. Debug builds only print the numbers.
//!
//! Folder: a.png (small), b/c/d.jpg (12 MP), e.jpg (50 MP).
//! - Next a → b: preload hit (b was preloaded as a's neighbour)            target < 16 ms
//! - Reload b: cache bypassed = cache miss, 12 MP JPEG                      target < 120 ms
//! - Previous a → e (loop): 50 MP, never preloaded (> half the budget)       preview < 150 ms, full < 1 s

use image::ImageEncoder;
use rview::actions::Action;
use rview::driver::{Observation, Step};
use rview::{Config, run};
use std::time::Duration;

fn photo_jpeg(path: &std::path::Path, w: u32, h: u32) {
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        let n = (x.wrapping_mul(2654435761) ^ y.wrapping_mul(40503)) >> 28;
        image::Rgb([(x / 16) as u8 ^ n as u8, (y / 12) as u8, ((x + y) / 20) as u8])
    });
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)
        .write_image(&img, w, h, image::ExtendedColorType::Rgb8)
        .unwrap();
    std::fs::write(path, out).unwrap();
}

fn measure(label: &'static str, target_first: Option<u64>, target_full: u64) -> Step {
    Step::Check(Box::new(move |o: &Observation| {
        let (first, full) = (o.first_latency.unwrap_or_default(), o.full_latency.unwrap_or_default());
        println!(
            "PERF {label}: first image {first:?}, full image {full:?} ({:?})",
            o.image_size
        );
        if o.error.is_some() || o.full_latency.is_none() {
            return Err(format!("{label}: not loaded: {o:?}"));
        }
        if cfg!(debug_assertions) {
            return Ok(());
        }
        let first_ok = target_first.is_none_or(|t| first <= Duration::from_millis(t));
        if first_ok && full <= Duration::from_millis(target_full) {
            Ok(())
        } else {
            Err(format!(
                "{label}: first {first:?} (target {target_first:?} ms), full {full:?} (target {target_full} ms)"
            ))
        }
    }))
}

fn main() {
    let _ = env_logger::try_init(); // RUST_LOG=rview=debug,rv_core=debug for the phase timings
    let dir = std::env::temp_dir().join(format!("rview-e2e-perf-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    image::RgbaImage::from_pixel(64, 64, image::Rgba([90, 90, 90, 255]))
        .save(dir.join("a.png"))
        .unwrap();
    for n in ["b", "c", "d"] {
        photo_jpeg(&dir.join(format!("{n}.jpg")), 4000, 3000);
    }
    photo_jpeg(&dir.join("e.jpg"), 8660, 5774);

    let settle = Step::Sleep(Duration::from_millis(1500)); // preloads finish
    let steps = vec![
        Step::WaitIdle,
        settle,
        Step::Do(Action::NextFile),
        Step::WaitIdle,
        measure("next, preload hit (12 MP)", None, 16),
        Step::Do(Action::ReloadFile),
        Step::WaitIdle,
        measure("cache miss 12 MP JPEG (reload)", None, 120),
        Step::Do(Action::PreviousFile),
        Step::WaitIdle,
        Step::Sleep(Duration::from_millis(300)),
        Step::Do(Action::PreviousFile),
        Step::WaitIdle,
        measure("50 MP JPEG (preview, then full)", Some(150), 1000),
        Step::Check(Box::new(|o| {
            (o.image_size == Some((8660, 5774)) && !o.showing_preview)
                .then_some(())
                .ok_or(format!("full 50 MP image should have replaced the preview: {o:?}"))
        })),
    ];
    let r = run(Config {
        files: vec![dir.join("a.png")],
        driver: Some(steps),
        ..Default::default()
    });
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = r {
        eprintln!("e2e_nav_perf FAILED:\n{e:#}");
        std::process::exit(1);
    }
    println!(
        "e2e_nav_perf: OK{}",
        if cfg!(debug_assertions) {
            " (debug: thresholds not checked)"
        } else {
            ""
        }
    );
}
