//! E2E: images wider than the texture limit (16384 on D3D12/WARP) are split
//! into tiles, each with its own mip chain. Checks on the GPU snapshot:
//! 1. downscaled (fit, mip ≈ 4): the horizontal gradient stays continuous across the tile boundary;
//! 2. at 100 %, with the boundary in the middle of the window: exact pixel values on both sides.

use rview::actions::Action;
use rview::driver::{Observation, Step};
use rview::{Config, run};
use winit::dpi::PhysicalPosition;
use winit::event::{DeviceId, ElementState, MouseButton, WindowEvent};

const W: u32 = 20_000;
const H: u32 = 300;
const SEAM: u32 = 16_384;

/// Red channel of image column x (smooth ramp 0..255).
fn ramp(x: u32) -> u8 {
    (x as u64 * 255 / (W as u64 - 1)) as u8
}

fn ev(e: WindowEvent) -> Step {
    Step::Event(e)
}

/// Drag the image by dx (negative = image moves left), in steps the window can contain.
fn drag(mut dx: f64) -> Vec<Step> {
    let mut v = Vec::new();
    while dx.abs() > 0.5 {
        let d = dx.clamp(-800.0, 800.0);
        let (x0, x1) = if d < 0.0 {
            (1000.0, 1000.0 + d)
        } else {
            (100.0, 100.0 + d)
        };
        let at = |x: f64| WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(x, 300.0),
        };
        let b = |s| WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state: s,
            button: MouseButton::Left,
        };
        v.extend([
            ev(at(x0)),
            ev(b(ElementState::Pressed)),
            ev(at(x1)),
            ev(b(ElementState::Released)),
        ]);
        dx -= d;
        // keep presses apart in time so they never pair into a double click
        v.push(Step::Sleep(std::time::Duration::from_millis(
            unsafe_double_click_ms() as u64 + 50,
        )));
    }
    v
}

fn unsafe_double_click_ms() -> u32 {
    // SAFETY: no preconditions.
    unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime() }
}

fn main() {
    let _ = env_logger::try_init(); // RUST_LOG=rview::app=trace shows the driver's progress
    let dir = std::env::temp_dir().join(format!("rview-e2e-tile-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("wide.png");
    image::RgbaImage::from_fn(W, H, |x, _| image::Rgba([ramp(x), 128, 64, 255]))
        .save(&p)
        .unwrap();

    let mut steps = vec![
        Step::WaitIdle,
        Step::Check(Box::new(|o: &Observation| {
            (o.image_size == Some((W, H)))
                .then_some(())
                .ok_or(format!("not loaded: {o:?}"))
        })),
        // 1. fit: one row through the middle of the image, continuous ramp
        Step::Snapshot(Box::new(|w, h, buf| {
            let y = h / 2;
            let row: Vec<i32> = (0..w).map(|x| buf[((y * w + x) * 4) as usize] as i32).collect();
            // columns covered by the image (background is 0x21 grey, G would be 0x21 there)
            let img: Vec<usize> = (0..w as usize)
                .filter(|&x| buf[((y * w) as usize + x) * 4 + 1] > 100)
                .collect();
            let (a, b) = (img[2], img[img.len() - 3]);
            let max_jump = (a..b).map(|x| (row[x + 1] - row[x]).abs()).max().unwrap();
            let backwards = (a..b).filter(|&x| row[x + 1] < row[x] - 1).count();
            (max_jump <= 3 && backwards == 0).then_some(()).ok_or(format!(
                "discontinuity at fit: max step {max_jump}, decreasing {backwards}"
            ))
        })),
        // 2. 100 %: bring image column SEAM to the window centre
        Step::Do(Action::OriginalSize),
    ];
    // at 100 % the image centre (x = W/2) is at the window centre; move it by W/2 − SEAM
    steps.extend(drag(W as f64 / 2.0 - SEAM as f64));
    steps.push(Step::Snapshot(Box::new(|w, h, buf| {
        let (cx, y) = (w / 2, h / 2);
        // find the image column under cx from the ramp itself, then compare a ±40 px band
        let px = |x: u32| buf[((y * w + x) * 4) as usize] as i32;
        let mut worst = 0;
        let mut first_bad = None;
        for sx in cx - 40..cx + 40 {
            // expected value: the snapshot column sx shows image column SEAM + (sx − cx)
            let ix = (SEAM as i64 + sx as i64 - cx as i64) as u32;
            let d = (px(sx) - ramp(ix) as i32).abs();
            if d > worst {
                worst = d;
                first_bad = Some((sx, px(sx), ramp(ix)));
            }
        }
        (worst <= 1)
            .then_some(())
            .ok_or(format!("seam at 100 %: worst {worst} at {first_bad:?}"))
    })));

    let settings = rv_core::settings::Settings {
        window_resize_mode: rv_core::settings::WindowResizeMode::Never,
        ..Default::default()
    };
    let r = run(Config {
        files: vec![p],
        software: true,
        driver: Some(steps),
        settings: Some(settings),
        ..Default::default()
    });
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = r {
        eprintln!("e2e_tiling FAILED:\n{e:#}");
        std::process::exit(1);
    }
    println!("e2e_tiling: OK");
}
