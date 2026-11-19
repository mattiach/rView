//! E2E input: winit events fed to the real window event handler.
//! Double click = time AND place (Windows rule); a pan drag is not half of a double click; mouse
//! Back/Forward navigate; a multi-file drop opens the first file here and the others in new windows.
//! OS-level input (real mouse, touchpad) is covered by the manual checklist.

use rview::actions::Action;
use rview::driver::{Observation, Step};
use rview::{Config, run};
use std::time::Duration;
use winit::dpi::PhysicalPosition;
use winit::event::{DeviceId, ElementState, MouseButton, WindowEvent};

fn dev() -> DeviceId {
    DeviceId::dummy()
}

fn move_to(x: f64, y: f64) -> Step {
    Step::Event(WindowEvent::CursorMoved {
        device_id: dev(),
        position: PhysicalPosition::new(x, y),
    })
}

fn button(b: MouseButton, state: ElementState) -> Step {
    Step::Event(WindowEvent::MouseInput {
        device_id: dev(),
        state,
        button: b,
    })
}

fn click() -> Vec<Step> {
    vec![
        button(MouseButton::Left, ElementState::Pressed),
        button(MouseButton::Left, ElementState::Released),
    ]
}

fn check(f: impl Fn(&Observation) -> Result<(), String> + 'static) -> Step {
    Step::Check(Box::new(f))
}

fn fullscreen_is(want: bool, what: &'static str) -> Step {
    check(move |o| {
        (o.fullscreen == want)
            .then_some(())
            .ok_or(format!("{what}: fullscreen = {}", o.fullscreen))
    })
}

fn main() {
    let dir = std::env::temp_dir().join(format!("rview-e2e-input-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (i, n) in ["a.png", "b.png", "c.png"].iter().enumerate() {
        image::RgbaImage::from_pixel(2000, 1500, image::Rgba([40 * i as u8, 90, 90, 255]))
            .save(dir.join(n))
            .unwrap();
    }
    let (b, c) = (dir.join("b.png"), dir.join("c.png"));
    let settle = || Step::Sleep(Duration::from_millis(500));
    let slow = Duration::from_millis(rview_double_click_ms() as u64 + 150);

    let mut steps = vec![Step::WaitIdle, move_to(300.0, 300.0)];
    // 1. real double click: same place, quick → fullscreen; Esc back
    steps.extend(click());
    steps.extend(click());
    steps.extend([
        settle(),
        fullscreen_is(true, "double click"),
        Step::Key("Esc".into()),
        settle(),
    ]);
    steps.push(fullscreen_is(false, "after Esc"));
    // 2. two quick clicks far apart → not a double click
    steps.push(move_to(300.0, 300.0));
    steps.extend(click());
    steps.push(move_to(500.0, 420.0));
    steps.extend(click());
    steps.extend([settle(), fullscreen_is(false, "clicks 230 px apart")]);
    // 3. two clicks at the same place but slower than the system double-click time
    steps.push(move_to(300.0, 300.0));
    steps.extend(click());
    steps.push(Step::Sleep(slow));
    steps.extend(click());
    steps.extend([settle(), fullscreen_is(false, "slow clicks")]);
    // 4. pan drag, back to the start and click again: the drag cancels the pending double click.
    // Wait first: the previous click at this spot must not pair with the press that starts the drag.
    steps.push(Step::Sleep(slow));
    steps.push(Step::Do(Action::ZoomIn)); // zoomed in: there is room to pan
    steps.push(Step::Do(Action::ZoomIn));
    steps.push(move_to(300.0, 300.0));
    steps.push(button(MouseButton::Left, ElementState::Pressed));
    steps.push(move_to(200.0, 250.0));
    steps.push(check(|o| {
        (o.pan != (0.0, 0.0))
            .then_some(())
            .ok_or(format!("the drag should pan: {:?}", o.pan))
    }));
    steps.push(move_to(300.0, 300.0));
    steps.push(button(MouseButton::Left, ElementState::Released));
    steps.extend(click());
    steps.extend([settle(), fullscreen_is(false, "click after a pan drag")]);
    // 5. mouse Back / Forward
    steps.push(button(MouseButton::Forward, ElementState::Pressed));
    steps.push(Step::WaitIdle);
    steps.push(check(|o| {
        (o.index == Some(1))
            .then_some(())
            .ok_or(format!("Forward: {:?}", o.index))
    }));
    steps.push(button(MouseButton::Back, ElementState::Pressed));
    steps.push(Step::WaitIdle);
    steps.push(check(|o| {
        (o.index == Some(0)).then_some(()).ok_or(format!("Back: {:?}", o.index))
    }));
    // 5b. the same in fullscreen
    steps.extend([Step::Key("F11".into()), settle(), fullscreen_is(true, "F11")]);
    steps.push(button(MouseButton::Forward, ElementState::Pressed));
    steps.push(Step::WaitIdle);
    steps.push(check(|o| {
        (o.index == Some(1))
            .then_some(())
            .ok_or(format!("Forward in fullscreen: {:?}", o.index))
    }));
    steps.push(button(MouseButton::Back, ElementState::Pressed));
    steps.push(Step::WaitIdle);
    steps.push(check(|o| {
        (o.index == Some(0))
            .then_some(())
            .ok_or(format!("Back in fullscreen: {:?}", o.index))
    }));
    steps.extend([Step::Key("Esc".into()), settle(), fullscreen_is(false, "Esc")]);
    // 6. drop of two files (winit: HoveredFile × n, then DroppedFile × n): first here, second in a new window
    steps.push(Step::Event(WindowEvent::HoveredFile(b.clone())));
    steps.push(Step::Event(WindowEvent::HoveredFile(c.clone())));
    steps.push(Step::Event(WindowEvent::DroppedFile(b.clone())));
    steps.push(Step::Event(WindowEvent::DroppedFile(c.clone())));
    steps.push(Step::WaitIdle);
    steps.push(check(|o| {
        let ok = o.windows == 2 && o.titles == ["b.png", "c.png"];
        ok.then_some(())
            .ok_or(format!("drop of 2 files: {} windows {:?}", o.windows, o.titles))
    }));
    // a later single-file drop on the first window replaces its image (new drag, counter reset)
    steps.push(Step::Focus(0));
    steps.push(Step::Event(WindowEvent::HoveredFile(c.clone())));
    steps.push(Step::Event(WindowEvent::DroppedFile(c.clone())));
    steps.push(Step::WaitIdle);
    steps.push(check(|o| {
        let ok = o.windows == 2 && o.titles == ["c.png", "c.png"];
        ok.then_some(())
            .ok_or(format!("second drop: {} windows {:?}", o.windows, o.titles))
    }));

    let settings = rv_core::settings::Settings {
        window_resize_mode: rv_core::settings::WindowResizeMode::Never,
        ..Default::default()
    };
    let r = run(Config {
        files: vec![dir.join("a.png")],
        software: true,
        driver: Some(steps),
        settings: Some(settings),
        ..Default::default()
    });
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = r {
        eprintln!("e2e_input FAILED:\n{e:#}");
        std::process::exit(1);
    }
    println!("e2e_input: OK");
}

fn rview_double_click_ms() -> u32 {
    // SAFETY: no preconditions.
    unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime() }
}
