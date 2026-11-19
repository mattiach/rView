//! E2E: before/after in Markup. Off (and refused) while there is nothing to compare. With the
//! comparison on, left of the divider the screen shows the original (same crop and orientation),
//! right of it the document: adjusted, with a Solid redaction; moving the divider moves the
//! border; zoomed in and panned, the divider starts in the middle of the part of the image on
//! screen; the key turns it off; undoing the last edit ends it.

use rv_core::decode::Pixels;
use rv_core::edit::Adjustments;
use rview::driver::{Observation, Step};
use rview::{Config, run};
use std::sync::{Arc, Mutex};
use winit::event::{DeviceId, ElementState, MouseButton, WindowEvent};

const W: u32 = 240;
const H: u32 = 160;

fn check(f: impl Fn(&Observation) -> Result<(), String> + 'static) -> Step {
    Step::Check(Box::new(f))
}

fn expect(cond: bool, what: &str, o: &Observation) -> Result<(), String> {
    if cond {
        return Ok(());
    }
    let m = o.markup.as_ref();
    Err(format!(
        "{what}: tool {:?}, adjust {:?}, redactions {}, can_undo {:?}, message {:?}",
        m.map(|m| m.tool),
        m.map(|m| m.doc.adjust),
        m.map_or(0, |m| m.doc.redactions.len()),
        m.map(|m| m.can_undo),
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

fn middle(state: ElementState) -> Step {
    // SAFETY: as in `button`.
    #[allow(unused_unsafe)]
    let device_id = unsafe { DeviceId::dummy() };
    Step::Event(WindowEvent::MouseInput {
        device_id,
        state,
        button: MouseButton::Middle,
    })
}

fn source() -> Vec<u8> {
    (0..W * H)
        .flat_map(|i| {
            let (x, y) = (i % W, i / W);
            [(x * 255 / W) as u8, (y * 255 / H) as u8, 120, 255]
        })
        .collect()
}

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

/// The screen at these image points shows these colours (± a few units: the view filters).
fn screen(t: Arc<Mutex<[f32; 6]>>, want: Vec<((u32, u32), [u8; 3])>, what: &'static str) -> Step {
    Step::Snapshot(Box::new(move |sw, _, px| {
        let t = *t.lock().unwrap();
        for (p, c) in &want {
            let (x, y) = (p.0 as f32 + 0.5, p.1 as f32 + 0.5);
            let (sx, sy) = (t[0] * x + t[1] * y + t[2], t[3] * x + t[4] * y + t[5]);
            let i = (sy as usize * sw as usize + sx as usize) * 4;
            let got = [px[i], px[i + 1], px[i + 2]];
            if !got.iter().zip(c).all(|(a, b)| a.abs_diff(*b) <= 4) {
                return Err(format!("{what}: at {p:?} the screen is {got:?}, expected {c:?}"));
            }
        }
        Ok(())
    }))
}

fn main() {
    let dir = std::env::temp_dir().join(format!("rview-e2e-compare-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("photo.png");
    image::RgbaImage::from_raw(W, H, source()).unwrap().save(&file).unwrap();
    let t: Arc<Mutex<[f32; 6]>> = Arc::default();
    let a = Adjustments {
        exposure: 1.0,
        saturation: -100.0,
        ..Default::default()
    };
    let (src, adj) = (source(), adjusted(&a));
    // left and right of the middle, low (clear of the redaction drawn above)
    let (left, right) = ((60, 130), (180, 130));
    // inside the redaction, each side of the middle
    let (red_l, red_r) = ((60, 40), (180, 40));

    let mut steps = vec![Step::WaitIdle, Step::Do(rview::actions::Action::Markup), Step::WaitIdle];
    let ts = t.clone();
    steps.push(check(move |o| {
        *ts.lock().unwrap() = o.screen_from_image;
        expect(o.markup.is_some(), "in Markup", o)
    }));
    // ---- nothing edited yet: nothing to compare, the button and the key do nothing
    steps.push(Step::MarkupButton("compare"));
    steps.push(Step::Key("\\".into()));
    steps.push(Step::WaitIdle);
    steps.push(check(|o| {
        expect(o.markup.as_ref().unwrap().compare.is_none(), "no edits: stays off", o)
    }));
    // ---- the document: adjusted, a Solid redaction across the middle
    steps.push(Step::MarkupButton("adjust:exposure:1"));
    steps.push(Step::MarkupButton("adjust:saturation:-100"));
    steps.push(Step::Key("B".into()));
    steps.extend([
        Step::MoveToImage(30.0, 20.0),
        button(ElementState::Pressed),
        Step::MoveToImage(210.0, 60.0),
        button(ElementState::Released),
    ]);
    steps.push(Step::MarkupButton("tool-select"));
    steps.push(Step::WaitIdle);
    steps.push(screen(
        t.clone(),
        vec![(left, at(&adj, left)), (right, at(&adj, right)), (red_l, [0, 0, 0])],
        "the document everywhere before comparing",
    ));

    // ---- compare: the original on the left, the document on the right
    steps.push(Step::MarkupButton("compare"));
    steps.push(Step::WaitIdle);
    steps.push(screen(
        t.clone(),
        vec![
            (left, at(&src, left)),
            (right, at(&adj, right)),
            (red_l, at(&src, red_l)),
            (red_r, [0, 0, 0]),
        ],
        "split at the middle",
    ));
    steps.push(check(|o| {
        let m = o.markup.as_ref().unwrap();
        expect(
            m.doc.redactions.len() == 1 && m.doc.adjust.exposure == 1.0,
            "the document unchanged",
            o,
        )
    }));

    // ---- the divider near the right edge: the right probe shows the original too
    steps.push(Step::MarkupButton("compare-at:0.9"));
    steps.push(Step::WaitIdle);
    steps.push(screen(
        t.clone(),
        vec![
            (right, at(&src, right)),
            (red_r, at(&src, red_r)),
            ((230, 130), at(&adj, (230, 130))),
        ],
        "divider moved",
    ));

    // ---- the key turns it off: the document everywhere again
    steps.push(Step::Key("\\".into()));
    steps.push(Step::WaitIdle);
    steps.push(screen(
        t.clone(),
        vec![(left, at(&adj, left)), (red_l, [0, 0, 0]), (red_r, [0, 0, 0])],
        "comparison off",
    ));

    // ---- zoomed in and panned to one side: the divider starts in the middle of what is on screen
    steps.push(Step::MarkupButton("compare"));
    for _ in 0..8 {
        steps.push(Step::Do(rview::actions::Action::ZoomIn));
    }
    steps.push(Step::WaitIdle);
    steps.extend([
        Step::MoveToImage(120.0, 100.0),
        middle(ElementState::Pressed),
        Step::MoveToImage(100.0, 100.0),
        middle(ElementState::Released),
    ]);
    steps.push(Step::MarkupButton("compare"));
    steps.push(Step::MarkupButton("compare"));
    steps.push(Step::WaitIdle);
    let ts = t.clone();
    steps.push(check(move |o| {
        *ts.lock().unwrap() = o.screen_from_image;
        expect(
            o.markup.as_ref().unwrap().compare == Some(0.5),
            "on again, in the middle",
            o,
        )
    }));
    let (ts, s2, a2) = (t.clone(), src.clone(), adj.clone());
    steps.push(Step::Snapshot(Box::new(move |sw, sh, px| {
        let t = *ts.lock().unwrap();
        // the image's horizontal extent on screen, and the part of it the window shows
        let (x0, x1) = (t[2], t[0] * W as f32 + t[2]);
        let (v0, v1) = (x0.max(0.0), x1.min(sw as f32));
        if x0 >= 0.0 && x1 <= sw as f32 {
            return Err(format!("the image should go past the window: {x0}..{x1} of {sw}"));
        }
        let split = (v0 + v1) / 2.0;
        if (split - (x0 + x1) / 2.0).abs() < 20.0 {
            return Err(format!("panned too little to tell the middles apart ({split})"));
        }
        // the screen's middle row, clear of the redaction (image rows 20…60)
        let sy = sh as f32 / 2.0;
        let iy = ((sy - t[5]) / t[4]) as u32;
        if !(61..H).contains(&iy) {
            return Err(format!("probe row {iy} not clear of the redaction"));
        }
        for (dx, want, side) in [(-10.0, &s2, "before"), (10.0, &a2, "after")] {
            let sx = split + dx;
            let ix = ((sx - t[2]) / t[0]) as u32;
            let i = (sy as usize * sw as usize + sx as usize) * 4;
            let got = [px[i], px[i + 1], px[i + 2]];
            let c = at(want, (ix, iy));
            if !got.iter().zip(c).all(|(a, b)| a.abs_diff(b) <= 4) {
                return Err(format!("{side} side at screen x {sx}: {got:?}, expected {c:?}"));
            }
        }
        Ok(())
    })));

    // ---- undo removes the redaction, not the comparison toggles; undoing every edit ends it
    steps.push(Step::MarkupButton("undo"));
    steps.push(Step::WaitIdle);
    steps.push(check(|o| {
        let m = o.markup.as_ref().unwrap();
        expect(
            m.doc.redactions.is_empty() && m.doc.adjust.saturation == -100.0,
            "undo: the redaction",
            o,
        )?;
        expect(m.compare.is_some(), "still comparing (adjusted)", o)
    }));
    steps.push(Step::MarkupButton("undo"));
    steps.push(Step::MarkupButton("undo"));
    steps.push(Step::WaitIdle);
    steps.push(check(|o| {
        let m = o.markup.as_ref().unwrap();
        expect(
            m.doc.adjust.is_neutral() && m.compare.is_none(),
            "nothing left: comparison ended",
            o,
        )
    }));

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
        eprintln!("e2e_compare FAILED:\n{e:#}");
        std::process::exit(1);
    }
    println!("e2e_compare: OK");
}
