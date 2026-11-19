//! Pure window logic: title text and resize-to-image. Physical pixels throughout.

use rv_core::settings::TitleBarMode;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn ltrb(l: i32, t: i32, r: i32, b: i32) -> Rect {
        Rect {
            x: l as f64,
            y: t as f64,
            w: (r - l) as f64,
            h: (b - t) as f64,
        }
    }

    pub fn center(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

/// "1.23 MiB" style (binary units, 2 decimals; bytes below 1 KiB).
pub fn format_bytes(n: u64) -> String {
    const UNITS: [&str; 4] = ["KiB", "MiB", "GiB", "TiB"];
    if n < 1024 {
        return format!("{n} bytes");
    }
    let mut v = n as f64 / 1024.0;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    format!("{v:.2} {}", UNITS[u])
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TitleInfo<'a> {
    pub name: Option<&'a str>,
    /// (0-based index, count) in the folder.
    pub index: Option<(usize, usize)>,
    pub size: Option<(u32, u32)>,
    pub bytes: Option<u64>,
}

/// TitleBarMode: 0 "rView"; 1 name; 2 "i/N - name"; 3 "i/N - name - WxH - size - rView".
/// Parts that aren't known (no folder yet, decode error) are left out.
pub fn title(mode: TitleBarMode, t: &TitleInfo) -> String {
    const APP: &str = "rView";
    let Some(name) = t.name else { return APP.into() };
    let pos = t.index.map(|(i, n)| format!("{}/{n} - ", i + 1)).unwrap_or_default();
    match mode {
        TitleBarMode::Basic => APP.into(),
        TitleBarMode::Minimal => name.into(),
        TitleBarMode::Practical => format!("{pos}{name}"),
        TitleBarMode::Verbose => {
            let mut s = format!("{pos}{name}");
            if let Some((w, h)) = t.size {
                s.push_str(&format!(" - {w}x{h}"));
            }
            if let Some(b) = t.bytes {
                s.push_str(&format!(" - {}", format_bytes(b)));
            }
            s.push_str(&format!(" - {APP}"));
            s
        }
    }
}

/// Windows double-click rule: second press within `max_ms` of the first and inside the
/// `rect` (w, h) centred on it. Physical px.
pub fn is_double_click(
    first: Option<(std::time::Instant, (f32, f32))>,
    now: std::time::Instant,
    pos: (f32, f32),
    max_ms: u128,
    rect: (f32, f32),
) -> bool {
    first.is_some_and(|(t, p)| {
        now.duration_since(t).as_millis() <= max_ms
            && (pos.0 - p.0).abs() <= rect.0 / 2.0
            && (pos.1 - p.1).abs() <= rect.1 / 2.0
    })
}

pub struct FitInput {
    /// Displayed image size (view orientation applied).
    pub image: (f64, f64),
    pub monitor: Rect,
    pub work: Rect,
    /// Current outer window rectangle (its centre is kept).
    pub outer: Rect,
    /// Outer − inner size: title bar and borders.
    pub decorations: (f64, f64),
    pub min_pct: f64,
    pub max_pct: f64,
    /// ≈ 1.5 × logical DPI, in physical px.
    pub min_width: f64,
}

/// Outer rectangle for resize-to-image: image size scaled with its aspect ratio between
/// min% and max% of the monitor, limited to the work area, centred on the old centre, title bar on
/// screen.
pub fn fit_window(f: &FitInput) -> Rect {
    let (mut w, mut h) = (f.image.0.max(1.0), f.image.1.max(1.0));
    let (min_w, min_h) = (f.monitor.w * f.min_pct / 100.0, f.monitor.h * f.min_pct / 100.0);
    let (max_w, max_h) = (f.monitor.w * f.max_pct / 100.0, f.monitor.h * f.max_pct / 100.0);
    if w < min_w && h < min_h {
        let s = (min_w / w).min(min_h / h);
        (w, h) = (w * s, h * s);
    }
    if w > max_w || h > max_h {
        let s = (max_w / w).min(max_h / h);
        (w, h) = (w * s, h * s);
    }
    let ow = (w + f.decorations.0).max(f.min_width).min(f.work.w).round();
    let oh = (h + f.decorations.1).min(f.work.h).round();
    let (cx, cy) = f.outer.center();
    // keep the old centre, but stay inside the work area (title bar never off screen)
    let x = (cx - ow / 2.0).clamp(f.work.x, f.work.x + f.work.w - ow).round();
    let y = (cy - oh / 2.0).clamp(f.work.y, f.work.y + f.work.h - oh).round();
    Rect { x, y, w: ow, h: oh }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_in_all_modes() {
        let t = TitleInfo {
            name: Some("cat.jpg"),
            index: Some((2, 10)),
            size: Some((4000, 3000)),
            bytes: Some(2_621_440),
        };
        assert_eq!(title(TitleBarMode::Basic, &t), "rView");
        assert_eq!(title(TitleBarMode::Minimal, &t), "cat.jpg");
        assert_eq!(title(TitleBarMode::Practical, &t), "3/10 - cat.jpg");
        assert_eq!(
            title(TitleBarMode::Verbose, &t),
            "3/10 - cat.jpg - 4000x3000 - 2.50 MiB - rView"
        );
        // nothing open
        assert_eq!(title(TitleBarMode::Verbose, &TitleInfo::default()), "rView");
        // decode error / folder not scanned yet: unknown parts are left out
        let e = TitleInfo {
            name: Some("bad.png"),
            ..Default::default()
        };
        assert_eq!(title(TitleBarMode::Practical, &e), "bad.png");
        assert_eq!(title(TitleBarMode::Verbose, &e), "bad.png - rView");
    }

    #[test]
    fn double_click_needs_time_and_place() {
        use std::time::{Duration, Instant};
        let t0 = Instant::now();
        let first = Some((t0, (100.0, 100.0)));
        let at = |ms| t0 + Duration::from_millis(ms);
        assert!(is_double_click(first, at(200), (101.0, 99.0), 500, (4.0, 4.0)));
        assert!(
            is_double_click(first, at(500), (102.0, 102.0), 500, (4.0, 4.0)),
            "edge of the rectangle"
        );
        assert!(
            !is_double_click(first, at(501), (100.0, 100.0), 500, (4.0, 4.0)),
            "too slow"
        );
        assert!(
            !is_double_click(first, at(100), (103.0, 100.0), 500, (4.0, 4.0)),
            "too far"
        );
        assert!(
            !is_double_click(first, at(100), (400.0, 300.0), 500, (4.0, 4.0)),
            "elsewhere"
        );
        assert!(
            !is_double_click(None, at(100), (100.0, 100.0), 500, (4.0, 4.0)),
            "no first click"
        );
    }

    #[test]
    fn bytes() {
        assert_eq!(format_bytes(512), "512 bytes");
        assert_eq!(format_bytes(1536), "1.50 KiB");
        assert_eq!(format_bytes(5 << 30), "5.00 GiB");
    }

    fn input(image: (f64, f64)) -> FitInput {
        let monitor = Rect {
            x: 0.0,
            y: 0.0,
            w: 2560.0,
            h: 1440.0,
        };
        FitInput {
            image,
            monitor,
            work: Rect { h: 1392.0, ..monitor }, // taskbar at the bottom
            outer: Rect {
                x: 780.0,
                y: 320.0,
                w: 1000.0,
                h: 800.0,
            }, // centre (1280, 720)
            decorations: (16.0, 39.0),
            min_pct: 20.0,
            max_pct: 70.0,
            min_width: 216.0,
        }
    }

    #[test]
    fn small_image_grows_to_min_big_shrinks_to_max() {
        // 100×50 < 20 % box (512×288): scaled to 512×256 with its aspect ratio
        let r = fit_window(&input((100.0, 50.0)));
        assert_eq!((r.w, r.h), (512.0 + 16.0, 256.0 + 39.0));
        // 8000×6000 > 70 % box (1792×1008): 1344×1008
        let r = fit_window(&input((8000.0, 6000.0)));
        assert_eq!((r.w, r.h), (1344.0 + 16.0, 1008.0 + 39.0));
        // in between: image size as it is
        let r = fit_window(&input((1200.0, 700.0)));
        assert_eq!((r.w, r.h), (1216.0, 739.0));
        let (cx, cy) = r.center();
        assert!(
            (cx - 1280.0).abs() <= 0.5 && (cy - 720.0).abs() <= 0.5,
            "centred on the old centre"
        );
    }

    #[test]
    fn stays_inside_the_work_area() {
        let mut f = input((1200.0, 900.0));
        f.outer = Rect {
            x: 2300.0,
            y: 1300.0,
            w: 400.0,
            h: 300.0,
        }; // centre near the bottom-right corner
        let r = fit_window(&f);
        assert!(
            r.x + r.w <= 2560.0 && r.y + r.h <= 1392.0 && r.x >= 0.0 && r.y >= 0.0,
            "{r:?}"
        );
        f.outer = Rect {
            x: -500.0,
            y: -400.0,
            w: 400.0,
            h: 300.0,
        }; // off the top-left
        let r = fit_window(&f);
        assert_eq!((r.x, r.y), (0.0, 0.0), "title bar back on screen");
        // narrow, tall image: minimum width
        let mut f = input((10.0, 1000.0));
        f.min_pct = 0.0;
        assert_eq!(fit_window(&f).w, 216.0);
        // taller than the work area even at max %: limited to the work area
        let mut f = input((1000.0, 1000.0));
        f.max_pct = 100.0;
        assert!(fit_window(&f).h <= 1392.0);
    }
}
