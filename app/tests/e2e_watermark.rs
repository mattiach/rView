//! E2E: watermarks in Markup. The recent list (from the state file) shows a moved file as
//! unavailable; inserting an image adds it centred and selected, at half opacity, and puts it
//! first in the list (saved); dragging moves it; the preview and the PNG export show it as the
//! core renders it; Save Project embeds its pixels once; an unusable file is reported and leaves
//! the list.

use rv_core::decode::DecodeOptions;
use rv_core::edit::*;
use rview::driver::{Observation, Step};
use rview::{Config, run};
use std::path::Path;
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
        "{what}: shapes {:?}, selection {:?}, watermarks {:?}, message {:?}",
        m.map(|m| m
            .doc
            .shapes
            .iter()
            .map(|s| (s.geom.clone(), s.style.opacity))
            .collect::<Vec<_>>()),
        m.map(|m| m.selection.clone()),
        m.map(|m| m.watermarks.clone()),
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

/// A Markup button with a run-time name (the driver takes static names: leaked, once per test).
fn markup_button(name: String) -> Step {
    Step::MarkupButton(Box::leak(name.into_boxed_str()))
}

fn source() -> Vec<u8> {
    (0..W * H)
        .flat_map(|i| {
            let (x, y) = (i % W, i / W);
            [(x * 255 / W) as u8, 90, (y * 255 / H) as u8, 255]
        })
        .collect()
}

fn image_rect(o: &Observation) -> Option<RectF> {
    o.markup.as_ref()?.doc.shapes.iter().find_map(|s| match s.geom {
        Geom::Image { rect, .. } => Some(rect),
        _ => None,
    })
}

fn decoded(p: &Path) -> Result<Vec<u8>, String> {
    let b = std::fs::read(p).map_err(|e| e.to_string())?;
    let d = rv_core::decode::decode(&b, None, &DecodeOptions::default()).map_err(|e| e.to_string())?;
    Ok(rv_core::color::rgba8(d.pixels))
}

fn main() {
    let dir = std::env::temp_dir().join(format!("rview-e2e-watermark-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("photo.png");
    image::RgbaImage::from_raw(W, H, source()).unwrap().save(&file).unwrap();
    // the logo: left half red, right half white, transparent corners
    let logo = dir.join("logo.png");
    image::RgbaImage::from_fn(40, 20, |x, y| {
        if !(2..=37).contains(&x) && !(2..=17).contains(&y) {
            image::Rgba([0, 0, 0, 0])
        } else if x < 20 {
            image::Rgba([255, 0, 0, 255])
        } else {
            image::Rgba([255, 255, 255, 255])
        }
    })
    .save(&logo)
    .unwrap();
    let moved = dir.join("moved-away.png");
    let broken = dir.join("broken.png");
    std::fs::write(&broken, b"not an image").unwrap();
    let state_file = dir.join("state.toml");
    let st = rv_core::state::State {
        recent_watermarks: vec![moved.clone()],
        ..Default::default()
    };
    rv_core::state::save(&state_file, &st).unwrap();
    let out = dir.join("photo-markup.png");
    let rvm = dir.join("photo.rvm");
    let t: Arc<Mutex<[f32; 6]>> = Arc::default();

    let mut steps = vec![Step::WaitIdle, Step::Do(rview::actions::Action::Markup), Step::WaitIdle];
    let m0 = moved.clone();
    steps.push(check(move |o| {
        let m = o.markup.as_ref().ok_or("not in Markup")?;
        expect(
            m.watermarks == [(m0.clone(), false)],
            "the moved file listed as unavailable",
            o,
        )
    }));

    // ---- insert: centred, 30 % of the width (the logo is wider than tall), selected, half opacity
    steps.push(markup_button(format!("watermark:{}", logo.display())));
    steps.push(Step::WaitIdle);
    let (l1, m1, sf) = (logo.clone(), moved.clone(), state_file.clone());
    let ts = t.clone();
    steps.push(check(move |o| {
        *ts.lock().unwrap() = o.screen_from_image;
        let m = o.markup.as_ref().unwrap();
        let r = image_rect(o).ok_or("no image shape")?;
        let s = &m.doc.shapes[0];
        expect(
            (r.w - 72.0).abs() < 0.5 && (r.h - 36.0).abs() < 0.5 && (r.x + r.w / 2.0 - 120.0).abs() < 0.5,
            "centred, 30 % of the width",
            o,
        )?;
        expect(
            m.selection == [s.id.0] && (s.style.opacity - 0.5).abs() < 1e-6,
            "selected, half opacity",
            o,
        )?;
        expect(
            m.watermarks == [(l1.clone(), true), (m1.clone(), false)],
            "inserted first in the list",
            o,
        )?;
        let saved = rv_core::state::load(&sf);
        expect(saved.recent_watermarks == [l1.clone(), m1.clone()], "the list saved", o)
    }));

    // ---- drag it 40 px left
    steps.extend([
        Step::MoveToImage(120.0, 80.0),
        button(ElementState::Pressed),
        Step::MoveToImage(100.0, 80.0),
        Step::MoveToImage(80.0, 80.0),
        button(ElementState::Released),
    ]);
    steps.push(check(|o| {
        let r = image_rect(o).ok_or("no image shape")?;
        expect(
            (r.x - 44.0).abs() < 1.0 && (r.y - 62.0).abs() < 1.0,
            "moved by dragging",
            o,
        )
    }));

    // ---- the preview: half red over the image at the logo's left half
    let probe = (60u32, 80u32);
    let ts = t.clone();
    steps.push(Step::Snapshot(Box::new(move |sw, _, px| {
        let t = *ts.lock().unwrap();
        let (x, y) = (probe.0 as f32 + 0.5, probe.1 as f32 + 0.5);
        let (sx, sy) = (t[0] * x + t[1] * y + t[2], t[3] * x + t[4] * y + t[5]);
        let i = (sy as usize * sw as usize + sx as usize) * 4;
        let s = &source()[((probe.1 * W + probe.0) * 4) as usize..][..3];
        let want = [s[0] / 2 + 128, s[1] / 2, s[2] / 2];
        let got = [px[i], px[i + 1], px[i + 2]];
        if got.iter().zip(want).all(|(a, b)| a.abs_diff(b) <= 6) {
            Ok(())
        } else {
            Err(format!("preview at the logo: {got:?}, expected about {want:?}"))
        }
    })));

    // ---- export: exactly the core's render of the document
    steps.extend([
        Step::Do(rview::actions::Action::Export),
        Step::DialogText("PNG".into()),
        Step::SavePath(out.clone()),
        Step::DialogAccept,
        Step::WaitIdle,
    ]);
    let p = out.clone();
    steps.push(check(move |o| {
        let m = o.markup.as_ref().unwrap();
        let src = source();
        let (_, _, want) = render::render(&m.doc, &render::Source { w: W, h: H, rgba: &src }, &render::same);
        let got = decoded(&p)?;
        expect(got == want, "export = core render", o)?;
        let i = ((probe.1 * W + probe.0) * 4) as usize;
        expect(got[i] > src[i] && got[i + 1] < src[i + 1], "the logo in the export", o)
    }));

    // ---- Save Project: one image entry, the shape keeps its pixels when read back
    steps.push(Step::SavePath(rvm.clone()));
    steps.push(Step::Do(rview::actions::Action::SaveProject));
    steps.push(Step::WaitIdle);
    let r = rvm.clone();
    steps.push(check(move |o| {
        let p = rv_core::project::read_file(&r, &DecodeOptions::default()).map_err(|e| e.to_string())?;
        let ok =
            matches!(&p.doc.shapes[..], [s] if matches!(&s.geom, Geom::Image { image, .. } if image.data.is_some()));
        expect(ok && p.manifest.version == 2, "project with the watermark's pixels", o)
    }));

    // ---- an unusable file: reported, removed from the list, nothing inserted
    let (b2, l2) = (broken.clone(), logo.clone());
    steps.push(markup_button(format!("watermark:{}", broken.display())));
    steps.push(Step::WaitIdle);
    steps.push(check(move |o| {
        let m = o.markup.as_ref().unwrap();
        let msg = o.message.as_deref().unwrap_or("");
        expect(msg.contains("can't be used as a watermark"), "error reported", o)?;
        expect(m.doc.shapes.len() == 1, "nothing inserted", o)?;
        expect(
            !m.watermarks.iter().any(|(p, _)| *p == b2) && m.watermarks[0].0 == l2,
            "not listed",
            o,
        )
    }));

    // ---- Clear list
    steps.push(Step::MarkupButton("watermark-clear"));
    steps.push(Step::WaitIdle);
    steps.push(check(|o| {
        expect(o.markup.as_ref().unwrap().watermarks.is_empty(), "list cleared", o)
    }));

    let r = run(Config {
        files: vec![file],
        driver: Some(steps),
        auto_confirm: Some(true),
        state_file: Some(state_file.clone()),
        settings: Some(rv_core::settings::Settings {
            window_resize_mode: rv_core::settings::WindowResizeMode::Never,
            ..Default::default()
        }),
        ..Default::default()
    });
    let cleared = rv_core::state::load(&state_file).recent_watermarks.is_empty();
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = r {
        eprintln!("e2e_watermark FAILED:\n{e:#}");
        std::process::exit(1);
    }
    if !cleared {
        eprintln!("e2e_watermark FAILED: the cleared list was not saved");
        std::process::exit(1);
    }
    println!("e2e_watermark: OK");
}
