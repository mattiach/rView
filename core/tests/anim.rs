//! Animations: frame count, delays, composited frames, playback clock,
//! speed/pause/next frame, finite loops, streaming mode. Animated WebP / JXL: no encoder → SKIPPED.

use gif::Repeat;
use image::codecs::gif::GifEncoder;
use image::{Frame, Rgba, RgbaImage};
use rv_core::anim::{Player, ViewFrame, is_animated, source_frame};
use rv_core::color::ViewSpace;
use rv_core::formats::Format;
use std::sync::Arc;
use std::time::{Duration, Instant};

const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];

/// 8×8, 3 frames: red 30 ms; blue 4×4 patch at (4,4) over it (disposal Keep) 50 ms; green 70 ms.
/// Written with the `gif` crate: image's encoder ignores sub-frame offsets.
fn gif(repeat: Repeat) -> Arc<[u8]> {
    let mut out = Vec::new();
    {
        let mut e = gif::Encoder::new(&mut out, 8, 8, &[]).unwrap();
        e.set_repeat(repeat).unwrap();
        let mut frame = |w: u16, h: u16, at: u16, c: [u8; 4], cs: u16| {
            let mut px: Vec<u8> = (0..w as usize * h as usize).flat_map(|_| c).collect();
            let mut f = gif::Frame::from_rgba(w, h, &mut px);
            (f.left, f.top, f.delay, f.dispose) = (at, at, cs, gif::DisposalMethod::Keep);
            e.write_frame(&f).unwrap();
        };
        frame(8, 8, 0, RED, 3);
        frame(4, 4, 4, BLUE, 5);
        frame(8, 8, 0, GREEN, 7);
    }
    out.into()
}

fn px(f: &ViewFrame, x: usize, y: usize) -> [u8; 4] {
    let i = (y * 8 + x) * 4;
    f.rgba[i..i + 4].try_into().unwrap()
}

fn player(bytes: Arc<[u8]>, format: Format, t0: Instant, limit: Option<u64>) -> Player {
    let wake = Arc::new(|| {});
    match limit {
        None => Player::spawn(bytes, format, ViewSpace::Srgb, t0, wake),
        Some(l) => Player::spawn_with_limit(bytes, format, ViewSpace::Srgb, t0, wake, l),
    }
}

fn wait_complete(p: &mut Player) -> usize {
    let t = Instant::now();
    loop {
        if let Some(n) = p.frame_count() {
            return n;
        }
        assert!(t.elapsed() < Duration::from_secs(20), "frames never complete");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn detection() {
    assert!(is_animated(&gif(Repeat::Infinite), Format::Gif));
    let mut one = Vec::new();
    GifEncoder::new(&mut one)
        .encode_frame(Frame::new(RgbaImage::from_pixel(4, 4, Rgba(RED))))
        .unwrap();
    assert!(!is_animated(&one, Format::Gif));
    let mut png = Vec::new();
    RgbaImage::from_pixel(4, 4, Rgba(RED))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    assert!(!is_animated(&png, Format::Png));
    println!(
        "SKIPPED here: animated WebP and JPEG XL (no encoder for a synthetic fixture); covered by real files in core/tests/fixtures.rs"
    );
}

#[test]
fn composited_frames_and_delays() {
    let (f1, icc) = source_frame(&gif(Repeat::Infinite), Format::Gif, 1).unwrap();
    assert!(icc.is_none());
    assert_eq!((f1.w, f1.h, f1.delay), (8, 8, Duration::from_millis(50)));
    assert_eq!(&f1.rgba[0..4], &RED); // untouched by the 4×4 patch
    assert_eq!(&f1.rgba[(6 * 8 + 6) * 4..][..4], &BLUE);
    assert!(source_frame(&gif(Repeat::Infinite), Format::Gif, 3).is_err());
}

#[test]
fn clock_follows_delays_and_loops() {
    let t0 = Instant::now();
    let ms = |n| t0 + Duration::from_millis(n);
    let mut p = player(gif(Repeat::Infinite), Format::Gif, t0, None);
    assert_eq!(wait_complete(&mut p), 3);
    assert!(p.tick(ms(29)).is_none(), "frame 0 lasts 30 ms");
    let f = p.tick(ms(30)).expect("frame 1");
    assert_eq!((p.index(), px(&f, 0, 0), px(&f, 6, 6)), (1, RED, BLUE));
    assert!(p.tick(ms(79)).is_none());
    assert_eq!(px(&p.tick(ms(80)).expect("frame 2"), 0, 0), GREEN);
    assert_eq!(p.next_due(), Some(ms(150)));
    let f = p.tick(ms(150)).expect("loops back to frame 0");
    assert_eq!((p.index(), px(&f, 0, 0)), (0, RED));
}

#[test]
fn speed_pause_and_next_frame() {
    let t0 = Instant::now();
    let ms = |n| t0 + Duration::from_millis(n);
    let mut p = player(gif(Repeat::Infinite), Format::Gif, t0, None);
    wait_complete(&mut p);
    p.tick(ms(30)).unwrap(); // frame 1, 50 ms at 100 %
    p.change_speed(100); // 200 %
    p.tick(ms(80)).unwrap(); // frame 2 (scheduled before the change), now 70 ms / 2
    assert_eq!(p.next_due(), Some(ms(115)));
    p.toggle_pause(ms(90));
    assert!(p.tick(ms(10_000)).is_none() && p.next_due().is_none());
    assert_eq!(
        px(&p.next_frame(ms(10_000)).unwrap(), 0, 0),
        RED,
        "N steps while paused"
    );
    p.change_speed(2000);
    assert_eq!(p.speed, 1000);
    p.change_speed(-5000);
    assert_eq!(p.speed, 0);
    p.reset_speed();
    assert_eq!(p.speed, 100);
}

#[test]
fn finite_loop_count_stops_on_last_frame() {
    let t0 = Instant::now();
    let mut p = player(gif(Repeat::Finite(1)), Format::Gif, t0, None);
    wait_complete(&mut p);
    let s = |n| t0 + Duration::from_secs(n);
    assert!(p.tick(s(1)).is_some());
    assert!(p.tick(s(2)).is_some());
    assert!(p.tick(s(3)).is_none(), "played once: stays on the last frame");
    assert!(p.next_due().is_none());
}

#[test]
fn streaming_keeps_order_across_loops() {
    let t0 = Instant::now();
    // limit of 1 byte: nothing can be stored, the decoder loops and streams
    let mut p = player(gif(Repeat::Infinite), Format::Gif, t0, Some(1));
    let mut seen = Vec::new();
    let mut now = t0;
    let deadline = Instant::now() + Duration::from_secs(20);
    while seen.len() < 7 {
        assert!(Instant::now() < deadline, "streaming stalled after {seen:?}");
        now += Duration::from_secs(1);
        match p.tick(now) {
            Some(f) => seen.push(if px(&f, 6, 6) == BLUE {
                'b'
            } else if px(&f, 0, 0) == GREEN {
                'g'
            } else {
                'r'
            }),
            None => std::thread::sleep(Duration::from_millis(2)),
        }
    }
    assert!(p.is_streaming());
    assert_eq!(seen.iter().collect::<String>(), "bgrbgrb");
    assert_eq!(p.frame_count(), None, "nothing stored");
}

#[test]
fn apng_frames() {
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, 8, 8);
        e.set_color(png::ColorType::Rgba);
        e.set_animated(2, 0).unwrap();
        let mut w = e.write_header().unwrap();
        w.set_frame_delay(1, 10).unwrap();
        w.write_image_data(&RgbaImage::from_pixel(8, 8, Rgba(RED)).into_raw())
            .unwrap();
        w.set_frame_delay(2, 10).unwrap();
        w.write_image_data(&RgbaImage::from_pixel(8, 8, Rgba(GREEN)).into_raw())
            .unwrap();
        w.finish().unwrap();
    }
    let bytes: Arc<[u8]> = out.into();
    assert!(is_animated(&bytes, Format::Png));
    let (f, _) = source_frame(&bytes, Format::Png, 1).unwrap();
    assert_eq!((&f.rgba[0..4], f.delay), (&GREEN[..], Duration::from_millis(200)));
    let t0 = Instant::now();
    let mut p = player(bytes, Format::Png, t0, None);
    assert_eq!(wait_complete(&mut p), 2);
    assert!(p.tick(t0 + Duration::from_millis(99)).is_none());
    assert_eq!(px(&p.tick(t0 + Duration::from_millis(100)).unwrap(), 0, 0), GREEN);
}

#[test]
fn save_frame_as_png_and_jpeg() {
    use rv_core::export::{ExportFormat, ExportOptions, verify};
    let opts = |format| ExportOptions {
        format,
        jpeg_quality: 95,
        background: [255, 255, 255],
        preset: rv_core::settings::MetadataPreset::Keep,
    };
    let bytes = gif(Repeat::Infinite);
    let (png, w, h) = rv_core::anim::export_frame(&bytes, Format::Gif, 1, &opts(ExportFormat::Png)).unwrap();
    assert!(verify(&png, ExportFormat::Png, w, h));
    let img = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
        .unwrap()
        .into_rgba8();
    assert_eq!(img.dimensions(), (8, 8));
    assert_eq!((img.get_pixel(0, 0).0, img.get_pixel(6, 6).0), (RED, BLUE));
    let (jpg, w, h) = rv_core::anim::export_frame(&bytes, Format::Gif, 1, &opts(ExportFormat::Jpeg)).unwrap();
    assert!(verify(&jpg, ExportFormat::Jpeg, w, h), "the chosen format is used");
}

#[test]
fn previous_frame_steps_back_and_wraps() {
    let t0 = Instant::now();
    let mut p = player(gif(Repeat::Infinite), Format::Gif, t0, None);
    let n = wait_complete(&mut p);
    assert!(n >= 2);
    assert_eq!(p.index(), 0);
    p.previous_frame(t0).unwrap();
    assert_eq!(p.index(), n - 1, "from the first frame to the last");
    p.previous_frame(t0).unwrap();
    assert_eq!(p.index(), n - 2);
    p.next_frame(t0).unwrap();
    assert_eq!(p.index(), n - 1);
}
