//! Color Picker: Tools > Color Picker starts picking on the image, a click reads the colour shown
//! there and puts it first in the history, Esc stops, "Pick from image" starts again, a history
//! swatch brings its colour back, Clear empties it, closing the window ends picking.

use rview::actions::Action;
use rview::driver::{Observation, Step};
use rview::{Config, run};
use winit::event::{DeviceId, ElementState, MouseButton, WindowEvent};

const RED: [u8; 3] = [220, 30, 40];
const BLUE: [u8; 3] = [20, 60, 230];

fn check(f: impl Fn(&Observation) -> Result<(), String> + 'static) -> Step {
    Step::Check(Box::new(f))
}

fn expect(ok: bool, what: &str, o: &Observation) -> Result<(), String> {
    ok.then_some(())
        .ok_or(format!("{what}: color {:?}, tools {:?}", o.color, o.tools))
}

fn near(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 2)
}

fn click_at(x: f32, y: f32) -> Vec<Step> {
    // SAFETY: no preconditions (an `unsafe fn` only in older winit versions); the id just tags a
    // synthetic event.
    #[allow(unused_unsafe)]
    let device_id = unsafe { DeviceId::dummy() };
    let button = |state| {
        Step::Event(WindowEvent::MouseInput {
            device_id,
            state,
            button: MouseButton::Left,
        })
    };
    vec![
        Step::MoveToImage(x, y),
        button(ElementState::Pressed),
        button(ElementState::Released),
        Step::WaitIdle,
    ]
}

fn main() {
    let dir = std::env::temp_dir().join(format!("rview-e2e-pick-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let png = dir.join("halves.png");
    image::RgbImage::from_fn(200, 100, |x, _| image::Rgb(if x < 100 { RED } else { BLUE }))
        .save(&png)
        .unwrap();

    let mut steps = vec![
        Step::WaitIdle,
        Step::Do(Action::ColorPicker),
        Step::WaitIdle,
        check(|o| {
            expect(
                o.tools == ["color"] && o.color.is_some_and(|c| c.1),
                "picking on open",
                o,
            )
        }),
    ];
    steps.extend(click_at(50.0, 50.0));
    steps.push(check(|o| {
        expect(o.color.is_some_and(|c| near(c.0, RED)), "red half", o)
    }));
    steps.extend(click_at(150.0, 50.0));
    steps.push(check(|o| {
        expect(o.color.is_some_and(|c| near(c.0, BLUE) && c.1), "blue half", o)?;
        let h = &o.recent_colors;
        expect(
            h.len() == 2 && near(h[0], BLUE) && near(h[1], RED),
            &format!("history, most recent first: {h:?}"),
            o,
        )
    }));
    steps.extend([
        Step::Key("Esc".into()),
        check(|o| expect(o.color.is_some_and(|c| !c.1), "Esc stops picking", o)),
    ]);
    // a click now is a viewer click again: the colour stays
    steps.extend(click_at(50.0, 50.0));
    steps.extend([
        check(|o| expect(o.color.is_some_and(|c| near(c.0, BLUE)), "no pick after Esc", o)),
        Step::ToolButton("color", "pick"),
        check(|o| expect(o.color.is_some_and(|c| c.1), "Pick from image", o)),
    ]);
    steps.extend(click_at(50.0, 50.0));
    steps.extend([
        check(|o| expect(o.color.is_some_and(|c| near(c.0, RED)), "picked again", o)),
        check(|o| {
            let h = &o.recent_colors;
            expect(
                h.len() == 2 && near(h[0], RED),
                &format!("red moved up, not duplicated: {h:?}"),
                o,
            )
        }),
        Step::ToolButton("color", "recent:1"),
        check(|o| expect(o.color.is_some_and(|c| near(c.0, BLUE)), "swatch brings blue back", o)),
        check(|o| expect(near(o.recent_colors[0], RED), "a swatch click keeps the order", o)),
        Step::ToolButton("color", "clear"),
        check(|o| expect(o.recent_colors.is_empty(), "Clear empties the history", o)),
        Step::ToolButton("color", "close"),
        check(|o| expect(o.tools.is_empty() && o.color.is_none(), "closed", o)),
    ]);

    let r = run(Config {
        files: vec![png],
        software: true,
        driver: Some(steps),
        settings: Some(rv_core::settings::Settings {
            window_resize_mode: rv_core::settings::WindowResizeMode::Never,
            ..Default::default()
        }),
        ..Default::default()
    });
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = r {
        eprintln!("e2e_pick FAILED:\n{e:#}");
        std::process::exit(1);
    }
    println!("e2e_pick: OK");
}
