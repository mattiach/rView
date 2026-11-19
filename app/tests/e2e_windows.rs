//! E2E multiple windows: three files at launch → three windows
//! with their own title (Verbose mode), resized to their image, own menus (enable rules, dynamic
//! texts), MRU routing of actions, fullscreen with Alt+Enter / Esc.

use rv_core::settings::{Settings, TitleBarMode};
use rview::actions::Action;
use rview::driver::{Observation, Step};
use rview::{Config, run};
use std::path::PathBuf;
use std::time::Duration;

fn check(f: impl Fn(&Observation) -> Result<(), String> + 'static) -> Step {
    Step::Check(Box::new(f))
}

fn menu<'a>(o: &'a Observation, key: &str) -> &'a (&'static str, bool, String) {
    o.menu.iter().find(|(k, _, _)| *k == key).expect("menu item")
}

fn main() {
    let dir = std::env::temp_dir().join(format!("rview-e2e-win-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let files: Vec<(PathBuf, (u32, u32))> = [("a.png", (600, 400)), ("b.png", (300, 500)), ("c.png", (100, 100))]
        .iter()
        .map(|(n, (w, h))| {
            let p = dir.join(n);
            image::RgbaImage::from_pixel(*w, *h, image::Rgba([120, 80, 40, 255]))
                .save(&p)
                .unwrap();
            (p, (*w, *h))
        })
        .collect();
    let size_of = |i: usize| rview::layout::format_bytes(std::fs::metadata(&files[i].0).unwrap().len());
    let expected: Vec<String> = (0..3)
        .map(|i| {
            let name = files[i].0.file_name().unwrap().to_string_lossy();
            format!("rView - {name} - {}", size_of(i))
        })
        .collect();
    let c_title_in_b = format!("rView - c.png - {}", size_of(2));

    let exp = expected.clone();
    let steps = vec![
        Step::WaitIdle,
        Step::Sleep(Duration::from_millis(400)), // resize-to-image requests settle
        check(move |o| {
            (o.titles == exp)
                .then_some(())
                .ok_or(format!("titles {:?}, expected {exp:?}", o.titles))
        }),
        // a.png 600×400: the window content takes the image's aspect ratio
        Step::Focus(0),
        check(|o| {
            let (w, h) = o.inner_size.unwrap();
            let ok = (w as f64 / h as f64 - 1.5).abs() < 0.02;
            ok.then_some(()).ok_or(format!("a.png window {w}x{h}, expected 3:2"))
        }),
        // c.png 100×100 is below the minimum (20 % of the monitor on both sides, true from a
        // 500×500 monitor up, so also on the 1024×768 CI desktop): grown, still square
        Step::Focus(2),
        check(|o| {
            let (w, h) = o.inner_size.unwrap();
            (w.abs_diff(h) <= 1 && w > 100)
                .then_some(())
                .ok_or(format!("c.png window {w}x{h}"))
        }),
        // menus of the active window: enable rules and texts
        check(|o| {
            let ok = menu(o, "zoomin").1
                && menu(o, "nextfile").1
                && !menu(o, "pause").1 // not animated
                && menu(o, "rename").1 // needs an image
                && !menu(o, "undo").1 // nothing deleted
                && menu(o, "markup").1 // a static image
                && menu(o, "options").1 && menu(o, "welcome").1 // always on
                && !menu(o, "redo").1 // only in Markup
                && menu(o, "fullscreen").2 == "Enter F&ull Screen\tF11"
                && menu(o, "rename").2 == "R&ename...\tCtrl+Enter";
            ok.then_some(()).ok_or(format!("menu: {:?}", o.menu))
        }),
        // fullscreen: Alt+Enter (not Flip) and Esc back
        Step::Key("Alt+Enter".into()),
        Step::Sleep(Duration::from_millis(500)),
        check(|o| {
            let ok = o.fullscreen && menu(o, "fullscreen").2 == "Exit F&ull Screen\tF11";
            ok.then_some(())
                .ok_or(format!("fullscreen on: {} {:?}", o.fullscreen, menu(o, "fullscreen")))
        }),
        Step::Key("Esc".into()),
        Step::Sleep(Duration::from_millis(500)),
        check(|o| {
            (!o.fullscreen)
                .then_some(())
                .ok_or("Esc should leave fullscreen".into())
        }),
        // MRU: actions go to the active window only
        Step::Focus(1),
        Step::Do(Action::NextFile),
        Step::WaitIdle,
        check(move |o| {
            let ok = o.current == Some(1)
                && o.titles[1] == c_title_in_b
                && o.titles[0] == expected[0]
                && o.titles[2] == expected[2];
            ok.then_some(()).ok_or(format!("MRU routing: {:?}", o.titles))
        }),
    ];
    let settings = Settings {
        title_bar_mode: TitleBarMode::Full,
        ..Default::default()
    };
    let r = run(Config {
        files: files.iter().map(|(p, _)| p.clone()).collect(),
        software: true,
        driver: Some(steps),
        settings: Some(settings),
        ..Default::default()
    });
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = r {
        eprintln!("e2e_windows FAILED:\n{e:#}");
        std::process::exit(1);
    }
    println!("e2e_windows: OK");
}
