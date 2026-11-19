//! E2E "holding →": 10 × Next at the keyboard repeat rate (33 ms) over 12 MP JPEGs.
//! Measures the time from the last key to the final image on screen and how many images were
//! shown on the way. No threshold (the criterion is "the last request is always shown"; that is
//! asserted by e2e_navigate): numbers for comparing loader strategies, release build, hardware GPU.

use image::ImageEncoder;
use rview::actions::Action;
use rview::driver::{Observation, Step};
use rview::{Config, run};
use std::time::Duration;

fn photo_jpeg(path: &std::path::Path, w: u32, h: u32, seed: u32) {
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        let n = (x.wrapping_mul(2654435761) ^ y.wrapping_mul(40503) ^ seed) >> 28;
        image::Rgb([
            (x / 16) as u8 ^ n as u8,
            (y / 12) as u8,
            ((x + y) / 20) as u8 ^ seed as u8,
        ])
    });
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)
        .write_image(&img, w, h, image::ExtendedColorType::Rgb8)
        .unwrap();
    std::fs::write(path, out).unwrap();
}

fn main() {
    let dir = std::env::temp_dir().join(format!("rview-e2e-hold-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for i in 0..12 {
        photo_jpeg(&dir.join(format!("h{i:02}.jpg")), 4000, 3000, i);
    }
    let before = std::sync::Arc::new(std::sync::Mutex::new(0u64));
    let b2 = before.clone();
    let mut steps = vec![
        Step::WaitIdle,
        Step::Sleep(Duration::from_millis(1500)),
        Step::Check(Box::new(move |o: &Observation| {
            *b2.lock().unwrap() = o.images_shown;
            Ok(())
        })),
    ];
    for _ in 0..10 {
        steps.push(Step::Do(Action::NextFile));
        steps.push(Step::Sleep(Duration::from_millis(33)));
    }
    steps.push(Step::WaitIdle);
    steps.push(Step::Check(Box::new(move |o: &Observation| {
        let shown = o.images_shown - *before.lock().unwrap();
        println!(
            "PERF hold → (10 × Next, 33 ms): last image after {:?} from the last key, {shown} images shown on the way, final index {:?}",
            o.full_latency.unwrap_or_default(),
            o.index
        );
        (o.index == Some(10)).then_some(()).ok_or(format!("final index {:?}", o.index))
    })));
    let r = run(Config {
        files: vec![dir.join("h00.jpg")],
        driver: Some(steps),
        ..Default::default()
    });
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = r {
        eprintln!("e2e_hold_perf FAILED:\n{e:#}");
        std::process::exit(1);
    }
    println!("e2e_hold_perf: OK");
}
