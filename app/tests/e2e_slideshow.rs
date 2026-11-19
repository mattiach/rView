//! E2E: a reversed slideshow advances on the timer and stops at the
//! edge without loop; it starts in full screen and leaving that keeps it running; a manual open
//! stops it; the menu text follows (Start/Stop Slideshow).

use rv_core::settings::{Settings, WindowResizeMode};
use rview::actions::Action;
use rview::driver::{Observation, Step};
use rview::{Config, run};
use std::time::Duration;

fn check(f: impl Fn(&Observation) -> Result<(), String> + 'static) -> Step {
    Step::Check(Box::new(f))
}

fn slideshow_text(o: &Observation) -> String {
    o.menu
        .iter()
        .find(|(k, _, _)| *k == "slideshow")
        .map(|m| m.2.clone())
        .unwrap_or_default()
}

fn main() {
    let dir = std::env::temp_dir().join(format!("rview-e2e-slide-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for i in 0..4u8 {
        image::RgbaImage::from_pixel(64, 48, image::Rgba([i * 60, 100, 100, 255]))
            .save(dir.join(format!("s{i}.png")))
            .unwrap();
    }
    let s1 = dir.join("s1.png");
    let steps = vec![
        Step::WaitIdle,
        Step::Do(Action::LastFile),
        Step::WaitIdle,
        check(|o| (o.index == Some(3)).then_some(()).ok_or(format!("start: {o:?}"))),
        Step::Do(Action::Slideshow),
        Step::Sleep(Duration::from_millis(20)), // menus are refreshed by the event loop
        check(|o| {
            (o.slideshow && o.fullscreen && slideshow_text(o) == "Stop S&lideshow")
                .then_some(())
                .ok_or(format!("started: {:?} fullscreen {}", slideshow_text(o), o.fullscreen))
        }),
        // leaving full screen keeps it running
        Step::Key("F11".into()),
        Step::Sleep(Duration::from_millis(30)),
        check(|o| {
            (o.slideshow && !o.fullscreen).then_some(()).ok_or(format!(
                "after F11: slideshow {} fullscreen {}",
                o.slideshow, o.fullscreen
            ))
        }),
        // 0.25 s per step, reversed: 3 → 2 → 1 → 0, then the edge (no loop) stops it
        Step::Sleep(Duration::from_millis(1600)),
        Step::WaitIdle,
        check(|o| {
            let ok = o.index == Some(0) && !o.slideshow && slideshow_text(o) == "Start S&lideshow";
            ok.then_some(())
                .ok_or(format!("edge: index {:?}, slideshow {}", o.index, o.slideshow))
        }),
        // a manual open stops a running slideshow
        Step::Do(Action::LastFile),
        Step::WaitIdle,
        Step::Do(Action::Slideshow),
        Step::Sleep(Duration::from_millis(100)),
        Step::Open(s1),
        Step::WaitIdle,
        check(|o| {
            (!o.slideshow && o.index == Some(1))
                .then_some(())
                .ok_or(format!("manual open: {o:?}"))
        }),
        Step::Sleep(Duration::from_millis(700)),
        check(|o| {
            (o.index == Some(1))
                .then_some(())
                .ok_or(format!("stopped: {:?}", o.index))
        }),
    ];
    let settings = Settings {
        slideshow_timer: 0.25,
        slideshow_reversed: true,
        loop_folders_enabled: false,
        window_resize_mode: WindowResizeMode::Never,
        ..Default::default()
    };
    let r = run(Config {
        files: vec![dir.join("s0.png")],
        software: true,
        driver: Some(steps),
        settings: Some(settings),
        ..Default::default()
    });
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = r {
        eprintln!("e2e_slideshow FAILED:\n{e:#}");
        std::process::exit(1);
    }
    println!("e2e_slideshow: OK");
}
