//! E2E 1: open a file from the CLI → snapshot; view actions.
//! One event loop per process, hence `harness = false`; WARP adapter.

use rview::actions::Action;
use rview::driver::{Observation, Step};
use rview::{Config, run};

const RED: [u8; 3] = [255, 0, 0];
const GREEN: [u8; 3] = [0, 255, 0];
const BLUE: [u8; 3] = [0, 0, 255];
const BG: [u8; 3] = [0x21, 0x21, 0x21];

fn px(w: u32, buf: &[u8], x: u32, y: u32) -> [u8; 3] {
    let i = ((y * w + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2]]
}

fn near(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b).all(|(&x, y)| (x as i32 - y as i32).abs() <= 3)
}

/// Around the viewport centre: (dx, dy) offset → expected colour.
fn expect(probes: &'static [(i32, i32, [u8; 3])]) -> Step {
    Step::Snapshot(Box::new(move |w, h, buf| {
        for &(dx, dy, c) in probes {
            let (x, y) = ((w as i32 / 2 + dx) as u32, (h as i32 / 2 + dy) as u32);
            let got = px(w, buf, x, y);
            if !near(got, c) {
                return Err(format!("pixel ({x},{y}) = {got:?}, want {c:?}"));
            }
        }
        if !near(px(w, buf, 1, 1), BG) {
            return Err(format!("background {:?}", px(w, buf, 1, 1)));
        }
        Ok(())
    }))
}

fn check(f: impl Fn(&Observation) -> Result<(), String> + 'static) -> Step {
    Step::Check(Box::new(f))
}

fn main() {
    let dir = std::env::temp_dir().join(format!("rview-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // 400×200: TL red, TR green, BL blue, BR white.
    let img = image::RgbaImage::from_fn(400, 200, |x, y| {
        image::Rgba(match (x < 200, y < 100) {
            (true, true) => [255, 0, 0, 255],
            (false, true) => [0, 255, 0, 255],
            (true, false) => [0, 0, 255, 255],
            _ => [255, 255, 255, 255],
        })
    });
    let png = dir.join("quadrants.png");
    img.save(&png).unwrap();

    let steps = vec![
        Step::WaitIdle,
        check(|o| {
            (o.image_size == Some((400, 200)) && o.title == "rView - quadrants.png" && o.error.is_none())
                .then_some(())
                .ok_or(format!("after open: {o:?}"))
        }),
        // Fit: the 2:1 image fills the width, quadrants around the centre, background above.
        expect(&[(-20, -20, RED), (20, -20, GREEN), (-20, 20, BLUE)]),
        Step::Do(Action::OriginalSize),
        check(|o| (o.scale == 1.0).then_some(()).ok_or(format!("original size: {o:?}"))),
        Step::Do(Action::RotateRight),
        // 90° CW: blue (was BL) is now top-left, red top-right.
        expect(&[(-20, -20, BLUE), (20, -20, RED), (20, 20, GREEN)]),
        Step::Do(Action::Mirror),
        expect(&[(-20, -20, RED), (20, -20, BLUE)]),
    ];
    // Fixed 1024×768 window: this test needs background bands around the image (resize-to-image is
    // covered by e2e_windows).
    let settings = rview_settings_no_resize();
    let r = run(Config {
        files: vec![png],
        software: true,
        driver: Some(steps),
        settings: Some(settings),
        ..Default::default()
    });

    let _ = std::fs::remove_dir_all(&dir);
    match r {
        Ok(()) => println!("e2e_open: OK"),
        Err(e) => {
            eprintln!(
                "e2e_open FAILED:
{e:#}"
            );
            std::process::exit(1);
        }
    }
}

fn rview_settings_no_resize() -> rv_core::settings::Settings {
    rv_core::settings::Settings {
        window_resize_mode: rv_core::settings::WindowResizeMode::Never,
        title_bar_mode: rv_core::settings::TitleBarMode::Name,
        ..Default::default()
    }
}
