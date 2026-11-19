//! Loader + cache on real files.

use image::ImageEncoder;
use rv_core::loader::{Loader, Options, Outcome, Preload};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp(name: &str) -> TempDir {
    let d = std::env::temp_dir().join(format!("rview-loader-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    TempDir(d)
}

/// PNG of w×h with a gradient (cheap to encode, real work to decode + premultiply).
fn png(dir: &Path, name: &str, w: u32, h: u32) -> PathBuf {
    let img = image::RgbaImage::from_fn(w, h, |x, y| image::Rgba([x as u8, y as u8, (x ^ y) as u8, 200]));
    let p = dir.join(name);
    img.save(&p).unwrap();
    p
}

fn loader(budget: u64) -> (Loader, mpsc::Receiver<Outcome>) {
    let (tx, rx) = mpsc::channel();
    let tx = std::sync::Mutex::new(tx);
    let l = Loader::new(2, budget, Options::default(), move |o| {
        let _ = tx.lock().unwrap().send(o);
    });
    (l, rx)
}

fn wait_idle(l: &Loader) {
    let t = Instant::now();
    while l.pending() > 0 {
        assert!(t.elapsed() < Duration::from_secs(60), "loader never idle");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn recv(rx: &mpsc::Receiver<Outcome>) -> Outcome {
    rx.recv_timeout(Duration::from_secs(60)).expect("no outcome")
}

fn pre(p: &Path, distance: u8) -> Preload {
    Preload {
        path: p.to_path_buf(),
        distance,
        behind: false,
    }
}

#[test]
fn newer_request_cancels_older_never_delivered() {
    let t = temp("stale");
    let big = png(&t.0, "big.png", 3000, 3000);
    let small = png(&t.0, "small.png", 16, 16);
    let (l, rx) = loader(256 << 20);
    l.request(1, 1, big, false, 2048, vec![]);
    l.request(1, 2, small.clone(), false, 2048, vec![]);
    let o = recv(&rx);
    assert_eq!((o.generation, &o.path), (2, &small));
    wait_idle(&l);
    assert!(rx.try_recv().is_err(), "obsolete result delivered");
}

#[test]
fn preloads_never_block_the_current_request() {
    let t = temp("reserved");
    let bigs: Vec<_> = (0..4).map(|i| png(&t.0, &format!("p{i}.png"), 2500, 2500)).collect();
    let cur = png(&t.0, "cur.png", 16, 16);
    let cur2 = png(&t.0, "cur2.png", 16, 16);
    let (l, rx) = loader(1 << 30);
    let preloads: Vec<_> = bigs.iter().enumerate().map(|(i, p)| pre(p, i as u8 + 1)).collect();
    l.request(1, 1, cur, false, 2048, preloads.clone());
    assert_eq!(recv(&rx).generation, 1);
    std::thread::sleep(Duration::from_millis(20)); // preloads are running now
    let t0 = Instant::now();
    l.request(1, 2, cur2.clone(), false, 2048, preloads); // same window: preloads kept
    let o = recv(&rx);
    assert_eq!((o.generation, &o.path), (2, &cur2));
    assert!(l.pending() > 0, "preloads should still be busy");
    assert!(
        t0.elapsed() < Duration::from_millis(500),
        "current waited for preloads: {:?}",
        t0.elapsed()
    );
    wait_idle(&l);
    assert!(bigs.iter().all(|b| l.is_cached(b)), "kept preloads end up in the cache");
}

#[test]
fn preloads_outside_new_window_are_cancelled() {
    let t = temp("cancel");
    let bigs: Vec<_> = (0..3).map(|i| png(&t.0, &format!("q{i}.png"), 2500, 2500)).collect();
    let a = png(&t.0, "a.png", 16, 16);
    let b = png(&t.0, "b.png", 16, 16);
    let (l, rx) = loader(1 << 30);
    l.request(1, 1, a, false, 2048, bigs.iter().map(|p| pre(p, 1)).collect());
    l.request(1, 2, b, false, 2048, vec![]);
    wait_idle(&l);
    assert!(
        bigs.iter().all(|p| !l.is_cached(p)),
        "cancelled preloads must not be cached"
    );
    let gens: Vec<u64> = rx.try_iter().map(|o| o.generation).collect();
    assert_eq!(gens, [2]);
}

#[test]
fn preload_hit_and_reload_bypasses_cache() {
    let t = temp("hit");
    let a = png(&t.0, "a.png", 64, 64);
    let b = png(&t.0, "b.png", 640, 480);
    let (l, rx) = loader(256 << 20);
    l.request(1, 1, a, false, 2048, vec![pre(&b, 1)]);
    recv(&rx);
    wait_idle(&l);
    assert!(l.is_cached(&b));
    let t0 = Instant::now();
    l.request(1, 2, b.clone(), false, 2048, vec![]);
    let hit = recv(&rx).result.unwrap();
    assert!(
        t0.elapsed() < Duration::from_millis(50),
        "cache hit took {:?}",
        t0.elapsed()
    );
    assert_eq!((hit.w, hit.h, hit.preview), (640, 480, false));
    l.request(1, 3, b, true, 2048, vec![]); // Reload: decoded again
    let again = recv(&rx).result.unwrap();
    assert!(!std::sync::Arc::ptr_eq(&hit, &again));
}

#[test]
fn no_preload_above_half_budget_and_budget_eviction() {
    let t = temp("budget");
    let a = png(&t.0, "a.png", 16, 16);
    let big = png(&t.0, "big.png", 1000, 1000); // 4 MB decoded
    let (l, rx) = loader(6 << 20);
    l.request(1, 1, a, false, 2048, vec![pre(&big, 1)]);
    recv(&rx);
    wait_idle(&l);
    assert!(!l.is_cached(&big), "preload above half the budget");
    // as current it is decoded (and cached, it fits the budget)
    l.request(1, 2, big.clone(), false, 2048, vec![]);
    recv(&rx);
    wait_idle(&l);
    assert!(l.is_cached(&big));
    l.set_cache_budget(1 << 20);
    assert!(!l.is_cached(&big), "evicted when the budget shrinks");
}

#[test]
fn missing_file_reports_not_found() {
    let t = temp("missing");
    let (l, rx) = loader(1 << 20);
    l.request(1, 1, t.0.join("gone.png"), false, 2048, vec![]);
    let e = recv(&rx).result.unwrap_err();
    assert!(e.not_found && e.code == 5, "{e:?}");
}

#[test]
fn big_jpeg_preview_then_full_same_geometry() {
    let t = temp("preview");
    let (w, h) = (6000u32, 4200u32); // 25.2 MP > 24 MP
    let img = image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x / 24) as u8, (y / 17) as u8, 90]));
    let p = t.0.join("big.jpg");
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80)
        .write_image(&img, w, h, image::ExtendedColorType::Rgb8)
        .unwrap();
    std::fs::write(&p, out).unwrap();
    let (l, rx) = loader(1 << 30);
    let t0 = Instant::now();
    l.request(1, 1, p, false, 2048, vec![]);
    let first = recv(&rx).result.unwrap();
    let t_preview = t0.elapsed();
    let second = recv(&rx).result.unwrap();
    let t_full = t0.elapsed();
    println!(
        "LOADER preview {}x{} after {t_preview:?}, full after {t_full:?}",
        first.w, first.h
    );
    assert!(first.preview && first.w < w && first.full == (w, h), "{first:?}");
    assert!(!second.preview && (second.w, second.h) == (w, h) && second.full == (w, h));
    assert!(t_preview < t_full);
}
