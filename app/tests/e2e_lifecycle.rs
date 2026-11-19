//! E2E: the Markup document's life cycle.
//! - a Solid redaction and a rectangle drawn with the pointer, Ctrl+A (mixed selection) → nudge
//!   (Translate) → Delete → Undo: both come back with the same IDs, moved;
//! - navigating away with a dirty document → prompt → Cancel keeps file, document and Markup;
//! - Export → title still has `•` → Done → the prompt says "exported but not saved" (no Export
//!   button) → Save Project → `.rvm` written, Markup left;
//! - the `.rvm` reopened in Markup: same IDs, an export bit-identical to the first one; Copy puts
//!   the rendered image on the clipboard (no file, no metadata); the shape and the redaction are
//!   moved and the redaction's mode is changed (still editable objects).
//!
//! Side effect: the clipboard content is replaced.

use rv_core::edit::*;
use rview::driver::{Observation, Step};
use rview::{Config, run};
use std::path::Path;
use winit::event::{DeviceId, ElementState, MouseButton, WindowEvent};

const W: u32 = 200;
const H: u32 = 140;

fn check(f: impl Fn(&Observation) -> Result<(), String> + 'static) -> Step {
    Step::Check(Box::new(f))
}

fn expect(cond: bool, what: &str, o: &Observation) -> Result<(), String> {
    if cond {
        return Ok(());
    }
    let m = o.markup.as_ref();
    Err(format!(
        "{what}: path {:?}, title {:?}, message {:?}, selection {:?}, dirty {:?}, shapes {:?}, redactions {:?}",
        o.path,
        o.title,
        o.message,
        m.map(|m| m.selection.clone()),
        m.map(|m| m.dirty),
        m.map(|m| m.doc.shapes.clone()),
        m.map(|m| m.doc.redactions.clone()),
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
        Step::MoveToImage((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0),
        Step::MoveToImage(b.0, b.1),
        button(ElementState::Released),
    ]
}

/// (id, top-left) of every shape and redaction.
fn objects(o: &Observation) -> Vec<(u64, f32, f32)> {
    let Some(m) = &o.markup else { return Vec::new() };
    let shapes = m.doc.shapes.iter().map(|s| match &s.geom {
        Geom::Rect { rect, .. } => (s.id.0, rect.x, rect.y),
        _ => (s.id.0, f32::NAN, f32::NAN),
    });
    let reds = m.doc.redactions.iter().map(|r| match &r.area {
        Area::Rect { rect, .. } | Area::Ellipse { rect, .. } => (r.id.0, rect.x, rect.y),
    });
    shapes.chain(reds).collect()
}

fn export(steps: &mut Vec<Step>, to: &Path) {
    steps.push(Step::Key("Ctrl+Shift+S".into()));
    steps.push(Step::DialogText("PNG".into()));
    steps.push(Step::SavePath(to.to_path_buf()));
    steps.push(Step::DialogAccept);
    steps.push(Step::WaitIdle);
}

fn main() {
    if std::env::var_os("GITHUB_ACTIONS").is_some() {
        println!("e2e_lifecycle: SKIPPED: the drag checks are unreliable on the GitHub runner, run it locally");
        return;
    }
    let dir = std::env::temp_dir().join(format!("rview-e2e-lifecycle-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (n, c) in [("a", 60u8), ("b", 160)] {
        image::RgbImage::from_fn(W, H, |x, y| image::Rgb([(x * 255 / W) as u8, (y * 255 / H) as u8, c]))
            .save(dir.join(format!("{n}.png")))
            .unwrap();
    }
    let a = dir.join("a.png");
    let (out1, out2, rvm) = (dir.join("out1.png"), dir.join("out2.png"), dir.join("a.rvm"));

    let mut steps = vec![Step::WaitIdle, Step::Do(rview::actions::Action::Markup), Step::WaitIdle];
    // ---- mixed selection → Translate → Delete → Undo
    steps.push(Step::Key("B".into()));
    steps.extend(drag((20.0, 20.0), (80.0, 60.0)));
    steps.push(Step::Key("Enter".into()));
    steps.push(Step::Key("R".into()));
    steps.extend(drag((110.0, 70.0), (170.0, 120.0)));
    steps.push(Step::Key("Enter".into()));
    steps.push(Step::Key("V".into()));
    steps.push(Step::Key("Ctrl+A".into()));
    steps.push(check(|o| {
        let m = o.markup.as_ref().ok_or("not in Markup")?;
        let ok = m.doc.shapes.len() == 1
            && m.doc.redactions.len() == 1
            && matches!(m.doc.redactions[0].kind, RedactKind::Solid { .. })
            && m.selection.len() == 2;
        expect(ok, "a Solid redaction and a rectangle, both selected", o)
    }));
    steps.push(Step::Key("Shift+Right".into())); // nudge 10 px
    steps.push(check(|o| {
        let v = objects(o);
        let ok = v.len() == 2 && (v[0].1 - 120.0).abs() < 0.01 && (v[1].1 - 30.0).abs() < 0.01;
        expect(ok, &format!("both moved by 10 px {v:?}"), o)
    }));
    let before_delete = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let bd = before_delete.clone();
    steps.push(check(move |o| {
        *bd.lock().unwrap() = objects(o);
        Ok(())
    }));
    steps.push(Step::Key("Del".into()));
    steps.push(check(|o| {
        let m = o.markup.as_ref().unwrap();
        expect(
            m.doc.shapes.is_empty() && m.doc.redactions.is_empty() && a_exists(o),
            "Delete removed both (never the file)",
            o,
        )
    }));
    steps.push(Step::Key("Ctrl+Z".into()));
    let bd = before_delete.clone();
    steps.push(check(move |o| {
        let want = bd.lock().unwrap().clone();
        expect(objects(o) == want, &format!("undo: same IDs and positions {want:?}"), o)
    }));

    // ---- navigation with a dirty document → prompt → Cancel keeps everything
    // select all, then Esc deselects (with nothing selected Esc would leave): arrows navigate again
    steps.push(Step::Key("Ctrl+A".into()));
    steps.push(Step::Key("Esc".into()));
    steps.push(check(|o| {
        let m = o.markup.as_ref().ok_or("left Markup")?;
        expect(m.selection.is_empty(), "nothing selected", o)
    }));
    steps.push(Step::Answer(None));
    steps.push(Step::Key("Right".into()));
    steps.push(Step::WaitIdle);
    let bd = before_delete.clone();
    steps.push(check(move |o| {
        let m = o.markup.as_ref().ok_or("left Markup")?;
        let asked = o
            .message
            .as_deref()
            .is_some_and(|t| t.starts_with("There are unsaved changes."));
        expect(asked, "the prompt was shown", o)?;
        expect(
            o.path.as_ref().is_some_and(|p| p.ends_with("a.png")) && m.dirty && objects(o) == *bd.lock().unwrap(),
            "Cancel: same file, same document, still dirty",
            o,
        )
    }));

    // ---- Export → still dirty → Done → "exported but not saved" → Save Project
    export(&mut steps, &out1);
    steps.push(check(|o| {
        let m = o.markup.as_ref().ok_or("not in Markup")?;
        expect(
            m.exported && m.dirty && o.title.starts_with("• "),
            "exported, title still •",
            o,
        )
    }));
    steps.push(Step::SavePath(rvm.clone()));
    steps.push(Step::Answer(Some(0)));
    steps.push(Step::MarkupButton("done"));
    steps.push(Step::WaitIdle);
    let r = rvm.clone();
    steps.push(check(move |o| {
        let text = o.message.as_deref().unwrap_or("");
        expect(
            text.contains("The image was exported, but the changes are not saved as a project")
                && !text.contains("Export…"),
            "prompt: exported but not saved, no Export button",
            o,
        )?;
        expect(
            o.markup.is_none() && r.exists(),
            "Save Project from the prompt, Markup left",
            o,
        )
    }));

    // ---- reopen the project: same IDs, bit-identical export, objects still editable
    steps.push(Step::Open(rvm.clone()));
    steps.push(Step::WaitIdle);
    steps.push(Step::Do(rview::actions::Action::Markup));
    steps.push(Step::WaitIdle);
    let bd = before_delete.clone();
    steps.push(check(move |o| {
        let m = o.markup.as_ref().ok_or("project not in Markup")?;
        expect(
            !m.dirty && objects(o) == *bd.lock().unwrap(),
            "reopened: clean, same IDs and positions",
            o,
        )
    }));
    export(&mut steps, &out2);
    let (o1, o2) = (out1.clone(), out2.clone());
    steps.push(check(move |o| {
        let same = std::fs::read(&o1)
            .ok()
            .is_some_and(|a| std::fs::read(&o2).is_ok_and(|b| a == b));
        expect(same, "export after reopening = export before, bit for bit", o)
    }));
    // Copy in Markup: the rendered image (as the export), no file, no metadata
    steps.push(Step::Key("Ctrl+C".into()));
    steps.push(Step::WaitIdle);
    let o1 = out1.clone();
    steps.push(check(move |o| {
        use rview::platform::{CF_DIBV5, clipboard_files, clipboard_get, png_format};
        let png = clipboard_get(png_format()).unwrap_or_default();
        let d = rv_core::decode::decode(&png, None, &Default::default()).map_err(|e| e.to_string())?;
        let e = rv_core::decode::decode(&std::fs::read(&o1).unwrap(), None, &Default::default()).unwrap();
        let same = rv_core::color::rgba8(d.pixels) == rv_core::color::rgba8(e.pixels);
        expect(
            same && d.meta.exif.is_none() && clipboard_get(CF_DIBV5).is_some() && clipboard_files().is_empty(),
            "Copy: rendered PNG + DIBV5, no CF_HDROP, no EXIF",
            o,
        )
    }));
    // move the rectangle and the redaction (pointer drags), then change the redaction's mode
    // the rectangle (outline only) dragged by its left edge at x = 120; the redaction selected with
    // Tab (the Inspector now covers the top-left corner) and nudged 10 px left
    steps.extend(drag((120.0, 95.0), (125.0, 99.0)));
    steps.push(Step::Key("Tab".into()));
    steps.push(check(|o| {
        let m = o.markup.as_ref().ok_or("not in Markup")?;
        expect(m.selection == [1], "Tab selects the redaction", o)
    }));
    steps.push(Step::Key("Shift+Left".into()));
    steps.push(Step::MarkupRedact(Box::new(|k, _| {
        *k = RedactKind::Pixelate { block: 8 }
    })));
    let bd = before_delete.clone();
    steps.push(check(move |o| {
        let m = o.markup.as_ref().unwrap();
        let b = bd.lock().unwrap().clone();
        let v = objects(o);
        let moved = v.len() == 2
            && v.iter().zip(&b).all(|(x, y)| x.0 == y.0)
            && (v[0].1 - b[0].1 - 5.0).abs() < 0.01
            && (v[1].1 - b[1].1 + 10.0).abs() < 0.01;
        expect(moved, &format!("shape and redaction moved {v:?}"), o)?;
        expect(
            m.doc.redactions[0].kind == RedactKind::Pixelate { block: 8 } && m.dirty,
            "mode changed",
            o,
        )
    }));

    let r = run(Config {
        files: vec![a.clone()],
        driver: Some(steps),
        auto_confirm: Some(true),
        ..Default::default()
    });
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = r {
        eprintln!("e2e_lifecycle FAILED:\n{e:#}");
        std::process::exit(1);
    }
    println!("e2e_lifecycle: OK");
}

fn a_exists(o: &Observation) -> bool {
    o.path.as_ref().is_some_and(|p| p.exists())
}
