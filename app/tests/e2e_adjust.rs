//! E2E: image adjustments in Markup (exposure, saturation, sharpness). The Adjust tool's buttons change the document; the preview
//! (computed on a worker) shows the adjusted image; a Solid redaction drawn afterwards covers it
//! with its exact colour; the PNG export equals the core's adjustment of the source; undo returns
//! to the earlier preview; leaving Markup puts the original back on screen.

use rv_core::decode::{DecodeOptions, Pixels};
use rv_core::edit::Adjustments;
use rview::driver::{Observation, Step};
use rview::{Config, run};
use std::sync::{Arc, Mutex};
use winit::event::{DeviceId, ElementState, MouseButton, WindowEvent};

const W: u32 = 240;
const H: u32 = 160;
/// Where the screen is sampled (image px): left of the Inspector, outside the redaction.
const PROBE: (u32, u32) = (50, 120);

fn check(f: impl Fn(&Observation) -> Result<(), String> + 'static) -> Step {
    Step::Check(Box::new(f))
}

fn expect(cond: bool, what: &str, o: &Observation) -> Result<(), String> {
    if cond {
        return Ok(());
    }
    let m = o.markup.as_ref();
    Err(format!(
        "{what}: tool {:?}, adjust {:?}, redactions {}, dialog {:?}, message {:?}",
        m.map(|m| m.tool),
        m.map(|m| m.doc.adjust),
        m.map_or(0, |m| m.doc.redactions.len()),
        o.dialog,
        o.message
    ))
}

fn button(state: ElementState) -> Step {
    // SAFETY: no preconditions (an `unsafe fn` only in older winit versions); the id just tags a
    // synthetic event.
    #[allow(unused_unsafe)]
    let device_id = unsafe { DeviceId::dummy() };
    Step::Event(WindowEvent::MouseInput {
        device_id,
        state,
        button: MouseButton::Left,
    })
}

fn drag(a: (f32, f32), b: (f32, f32)) -> Vec<Step> {
    vec![
        Step::MoveToImage(a.0, a.1),
        button(ElementState::Pressed),
        Step::MoveToImage(b.0, b.1),
        button(ElementState::Released),
    ]
}

fn source() -> Vec<u8> {
    (0..W * H)
        .flat_map(|i| {
            let (x, y) = (i % W, i / W);
            [(x * 255 / W) as u8, (y * 255 / H) as u8, 120, 255]
        })
        .collect()
}

/// The core's adjustment of the source.
fn adjusted(a: &Adjustments) -> Vec<u8> {
    match rv_core::edit::adjust::apply(&Pixels::Rgba8(source()), W, a) {
        Pixels::Rgba8(v) => v,
        Pixels::Rgba16(_) => unreachable!(),
    }
}

fn at(v: &[u8], (x, y): (u32, u32)) -> [u8; 3] {
    let i = ((y * W + x) * 4) as usize;
    [v[i], v[i + 1], v[i + 2]]
}

/// The screen at an image point is `want` (± a few units: the view filters the texture).
fn screen_is(t: Arc<Mutex<[f32; 6]>>, p: (u32, u32), want: [u8; 3], what: &'static str) -> Step {
    Step::Snapshot(Box::new(move |sw, _, px| {
        let t = *t.lock().unwrap();
        let (x, y) = (p.0 as f32 + 0.5, p.1 as f32 + 0.5);
        let (sx, sy) = (t[0] * x + t[1] * y + t[2], t[3] * x + t[4] * y + t[5]);
        let i = (sy as usize * sw as usize + sx as usize) * 4;
        let got = [px[i], px[i + 1], px[i + 2]];
        if got.iter().zip(want).all(|(a, b)| a.abs_diff(b) <= 4) {
            Ok(())
        } else {
            Err(format!("{what}: screen {got:?}, expected {want:?}"))
        }
    }))
}

fn main() {
    let dir = std::env::temp_dir().join(format!("rview-e2e-adjust-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("photo.png");
    image::RgbaImage::from_raw(W, H, source()).unwrap().save(&file).unwrap();
    let out = dir.join("photo-markup.png");
    let t: Arc<Mutex<[f32; 6]>> = Arc::default();
    let both = Adjustments {
        exposure: 1.0,
        saturation: -100.0,
        sharpness: 60.0,
        ..Default::default()
    };
    let exposure = Adjustments {
        exposure: 1.0,
        ..Default::default()
    };

    let mut steps = vec![Step::WaitIdle, Step::Do(rview::actions::Action::Markup), Step::WaitIdle];
    steps.push(Step::MarkupButton("tool-adjust"));
    let ts = t.clone();
    steps.push(check(move |o| {
        *ts.lock().unwrap() = o.screen_from_image;
        let m = o.markup.as_ref().ok_or("not in Markup")?;
        expect(m.tool == "adjust" && m.doc.adjust.is_neutral(), "Adjust tool", o)
    }));
    steps.push(screen_is(
        t.clone(),
        PROBE,
        at(&source(), PROBE),
        "the original at first",
    ));

    // ---- two adjustments: two history entries, the preview catches up
    steps.push(Step::MarkupButton("adjust:exposure:1"));
    steps.push(Step::MarkupButton("adjust:saturation:-100"));
    steps.push(Step::MarkupButton("adjust:sharpness:60"));
    steps.push(Step::WaitIdle);
    steps.push(check(move |o| {
        let m = o.markup.as_ref().unwrap();
        expect(
            m.doc.adjust == both && m.can_undo && m.dirty,
            "adjustments in the document",
            o,
        )
    }));
    let grey = at(&adjusted(&both), PROBE);
    assert!(grey[0].abs_diff(grey[2]) <= 1, "the expected colour is grey: {grey:?}");
    steps.push(screen_is(t.clone(), PROBE, grey, "adjusted preview"));

    // ---- a Solid redaction drawn on the adjusted image: its exact colour on screen
    steps.push(Step::Key("B".into()));
    steps.extend(drag((20.0, 20.0), (90.0, 70.0)));
    steps.push(Step::WaitIdle);
    steps.push(screen_is(t.clone(), (55, 45), [0, 0, 0], "Solid cover"));
    steps.push(screen_is(t.clone(), PROBE, grey, "still adjusted around it"));

    // ---- export: the core's adjustment of the source, and the exact cover
    steps.extend([
        Step::Do(rview::actions::Action::Export),
        Step::DialogText("PNG".into()),
        Step::SavePath(out.clone()),
        Step::DialogAccept,
        Step::WaitIdle,
    ]);
    let p = out.clone();
    steps.push(check(move |o| {
        let b = std::fs::read(&p).map_err(|e| e.to_string())?;
        let d = rv_core::decode::decode(&b, None, &DecodeOptions::default()).map_err(|e| e.to_string())?;
        let px = rv_core::color::rgba8(d.pixels);
        let want = adjusted(&both);
        let outside = (0..H)
            .flat_map(|y| (0..W).map(move |x| (x, y)))
            .filter(|&(x, y)| !(19..=91).contains(&x) || !(19..=71).contains(&y))
            .all(|q| at(&px, q) == at(&want, q));
        expect(outside, "export = core adjustment outside the redaction", o)?;
        expect(at(&px, (55, 45)) == [0, 0, 0], "export: exact cover", o)
    }));

    // ---- undo the redaction, the sharpness and the saturation: exposure only on screen
    for _ in 0..3 {
        steps.push(Step::MarkupButton("undo"));
    }
    steps.push(Step::WaitIdle);
    steps.push(check(move |o| {
        let m = o.markup.as_ref().unwrap();
        expect(m.doc.adjust == exposure && m.doc.redactions.is_empty(), "undone", o)
    }));
    steps.push(screen_is(
        t.clone(),
        PROBE,
        at(&adjusted(&exposure), PROBE),
        "exposure only",
    ));
    steps.push(screen_is(
        t.clone(),
        (55, 45),
        at(&adjusted(&exposure), (55, 45)),
        "cover gone",
    ));

    // ---- leave (discard): the original on screen again
    steps.push(Step::MarkupButton("done"));
    steps.push(Step::WaitIdle);
    steps.push(check(|o| expect(o.markup.is_none(), "Markup left", o)));
    steps.push(screen_is(
        t.clone(),
        PROBE,
        at(&source(), PROBE),
        "original after leaving",
    ));

    let r = run(Config {
        files: vec![file],
        driver: Some(steps),
        auto_confirm: Some(true),
        settings: Some(rv_core::settings::Settings {
            window_resize_mode: rv_core::settings::WindowResizeMode::Never,
            ..Default::default()
        }),
        ..Default::default()
    });
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = r {
        eprintln!("e2e_adjust FAILED:\n{e:#}");
        std::process::exit(1);
    }
    println!("e2e_adjust: OK");
}
