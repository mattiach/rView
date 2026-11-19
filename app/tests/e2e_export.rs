//! E2E: "export", "export without Save Project",
//! "overwrite with a locked file"; then Overwrite Original, Save Project after it (the project keeps
//! the true original), Restore Original, and the `.rvm` opened in the viewer and in Markup.

use rv_core::decode::DecodeOptions;
use rv_core::edit::*;
use rview::actions::Action;
use rview::driver::{Observation, Step};
use rview::{Config, run};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use winit::event::{DeviceId, ElementState, MouseButton, WindowEvent};

const W: u32 = 240;
const H: u32 = 160;

fn check(f: impl Fn(&Observation) -> Result<(), String> + 'static) -> Step {
    Step::Check(Box::new(f))
}

fn expect(cond: bool, what: &str, o: &Observation) -> Result<(), String> {
    if cond {
        Ok(())
    } else {
        let m = o.markup.as_ref();
        Err(format!(
            "{what}: dirty {:?}, exported {:?}, project {:?}, message {:?}, title {:?}, dialog {:?}",
            m.map(|m| m.dirty),
            m.map(|m| m.exported),
            m.map(|m| m.project_path.clone()),
            o.message,
            o.title,
            o.dialog
        ))
    }
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

fn rect(a: (f32, f32), b: (f32, f32)) -> Vec<Step> {
    vec![
        Step::Key("R".into()),
        Step::MoveToImage(a.0, a.1),
        button(ElementState::Pressed),
        Step::MoveToImage(b.0, b.1),
        button(ElementState::Released),
        Step::Key("V".into()),
    ]
}

/// The flattened document of the observation, as the export must produce it (8-bit).
fn render(o: &Observation) -> (u32, u32, Vec<u8>) {
    let m = o.markup.as_ref().expect("in Markup");
    let f = rv_core::export::flatten(&m.doc, m.source.as_ref().expect("source"));
    (f.w, f.h, rv_core::color::rgba8(f.pixels))
}

fn decoded(p: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let b = std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
    let d = rv_core::decode::decode(&b, None, &DecodeOptions::default()).map_err(|e| e.to_string())?;
    Ok((d.size.w, d.size.h, rv_core::color::rgba8(d.pixels)))
}

fn mean_diff(a: &[u8], b: &[u8]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (*x as i32 - *y as i32).abs() as f64)
        .sum::<f64>()
        / a.len().max(1) as f64
}

/// Files in `dir` other than the expected ones (temp files, backups).
fn extra_files(dir: &Path, expected: &[&str]) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !expected.contains(&n.as_str()))
        .collect()
}

fn main() {
    let dir = std::env::temp_dir().join(format!("rview-e2e-export-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let photo = dir.join("photo.jpg");
    let img = image::RgbImage::from_fn(W, H, |x, y| image::Rgb([(x * 255 / W) as u8, (y * 255 / H) as u8, 128]));
    img.save(&photo).unwrap();
    let original = std::fs::read(&photo).unwrap();
    let (out_png, rvm) = (dir.join("out.png"), dir.join("photo.rvm"));
    let lock: Arc<Mutex<Option<std::fs::File>>> = Arc::default();
    let saved_doc: Arc<Mutex<Option<Arc<DocState>>>> = Arc::default();

    let mut steps = vec![Step::WaitIdle, Step::Do(Action::Markup), Step::WaitIdle];
    steps.extend(rect((30.0, 30.0), (120.0, 90.0)));

    // ---- export (Ctrl+Shift+S): dialog → PNG → file = render, document still dirty
    steps.push(Step::Key("Ctrl+Shift+S".into()));
    steps.push(check(|o| {
        let ok = o.dialog.as_ref().is_some_and(|(k, t, _)| *k == "export" && t == "JPEG");
        expect(ok, "Export dialog, JPEG proposed for a JPEG", o)
    }));
    steps.push(Step::DialogText("PNG".into()));
    steps.push(Step::SavePath(out_png.clone()));
    steps.push(Step::DialogAccept);
    steps.push(Step::WaitIdle);
    let p = out_png.clone();
    steps.push(check(move |o| {
        let (w, h, px) = decoded(&p)?;
        let (ew, eh, exp) = render(o);
        expect((w, h) == (ew, eh) && px == exp, "exported PNG = render", o)?;
        let m = o.markup.as_ref().unwrap();
        expect(
            m.exported && m.dirty && o.title.starts_with("• "),
            "exported but still dirty",
            o,
        )
    }));

    // ---- overwrite with the original locked by another program: nothing changes
    let (l, ph, d) = (lock.clone(), photo.clone(), dir.clone());
    steps.push(check(move |_| {
        use std::os::windows::fs::OpenOptionsExt;
        let f = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&ph)
            .map_err(|e| e.to_string())?;
        *l.lock().unwrap() = Some(f);
        let _ = &d;
        Ok(())
    }));
    steps.push(Step::Do(Action::OverwriteOriginal));
    steps.push(Step::WaitIdle);
    let (l, ph, d, orig) = (lock.clone(), photo.clone(), dir.clone(), original.clone());
    steps.push(check(move |o| {
        l.lock().unwrap().take();
        let same = std::fs::read(&ph).map_err(|e| e.to_string())? == orig;
        let msg = o.message.as_deref().unwrap_or("");
        expect(msg.contains("in use"), "locked: the user is told", o)?;
        expect(same, "locked: original byte-identical", o)?;
        let extra = extra_files(&d, &["photo.jpg", "out.png"]);
        expect(extra.is_empty(), &format!("locked: no temp/backup left {extra:?}"), o)
    }));

    // ---- overwrite (unlocked): the file is the render as JPEG, still dirty, Restore Original
    steps.push(Step::Do(Action::OverwriteOriginal));
    steps.push(Step::WaitIdle);
    let (ph, d) = (photo.clone(), dir.clone());
    steps.push(check(move |o| {
        let (w, h, px) = decoded(&ph)?;
        let (ew, eh, exp) = render(o);
        let diff = mean_diff(&px, &exp);
        expect(
            (w, h) == (ew, eh) && diff < 3.0,
            &format!("overwritten = render (mean diff {diff:.2})"),
            o,
        )?;
        let m = o.markup.as_ref().unwrap();
        expect(m.exported && m.dirty, "overwrite leaves the document dirty", o)?;
        let restore = o.menu.iter().any(|(k, _, t)| *k == "undo" && t.contains("Restore"));
        expect(o.has_undo && restore, "Restore Original registered", o)?;
        let extra = extra_files(&d, &["photo.jpg", "out.png"]);
        expect(
            extra.is_empty(),
            &format!("backup moved to the Recycle Bin {extra:?}"),
            o,
        )
    }));

    // ---- Save Project after the overwrite: clean, and the project holds the true original
    steps.push(Step::SavePath(rvm.clone()));
    steps.push(Step::Key("Ctrl+S".into()));
    steps.push(Step::WaitIdle);
    let (r, orig, sd) = (rvm.clone(), original.clone(), saved_doc.clone());
    steps.push(check(move |o| {
        let m = o.markup.as_ref().unwrap();
        expect(!m.dirty && !o.title.starts_with('•'), "Save Project makes it clean", o)?;
        expect(m.project_path.as_deref() == Some(r.as_path()), "project path", o)?;
        let p = rv_core::project::read_file(&r, &DecodeOptions::default()).map_err(|e| e.to_string())?;
        expect(
            p.original == orig,
            "the project embeds the original bytes, not the overwritten file",
            o,
        )?;
        let mut d = p.doc.clone();
        d.revision = m.doc.revision;
        expect(d == *m.doc, "the project's document", o)?;
        *sd.lock().unwrap() = Some(m.doc.clone());
        Ok(())
    }));

    // ---- leave (clean: no prompt), then Restore Original
    steps.push(Step::MarkupButton("done"));
    steps.push(Step::WaitIdle);
    steps.push(check(|o| expect(o.markup.is_none(), "left Markup", o)));
    steps.push(Step::Do(Action::Undo));
    steps.push(Step::WaitIdle);
    let (ph, orig) = (photo.clone(), original.clone());
    steps.push(check(move |o| {
        let same = std::fs::read(&ph).map_err(|e| e.to_string())? == orig;
        expect(same && !o.has_undo, "Restore Original brought the file back", o)
    }));

    // ---- the project in the viewer (its render) and in Markup (its document, clean)
    steps.push(Step::Open(rvm.clone()));
    steps.push(Step::WaitIdle);
    let sd = saved_doc.clone();
    steps.push(check(move |o| {
        let doc = sd.lock().unwrap().clone().ok_or("no saved doc")?;
        let (cw, ch) = doc.canvas_size();
        expect(
            o.error.is_none() && o.image_size == Some((cw, ch)),
            "project shown as its render",
            o,
        )
    }));
    steps.push(Step::Do(Action::Markup));
    steps.push(Step::WaitIdle);
    let (sd, r) = (saved_doc.clone(), rvm.clone());
    steps.push(check(move |o| {
        let saved = sd.lock().unwrap().clone().ok_or("no saved doc")?;
        let m = o.markup.as_ref().ok_or("not in Markup")?;
        let mut d = (*m.doc).clone();
        d.revision = saved.revision;
        expect(d == *saved, "the project's document is loaded", o)?;
        expect(
            !m.dirty && m.project_path.as_deref() == Some(r.as_path()),
            "clean, saving to the project",
            o,
        )?;
        expect(
            o.image_size == Some((W, H)),
            "the view shows the original under the document",
            o,
        )
    }));
    // an edit, then the prompt (auto: discard); the view goes back to the project's render
    steps.extend(rect((10.0, 10.0), (40.0, 40.0)));
    steps.push(Step::MarkupButton("done"));
    steps.push(Step::WaitIdle);
    let sd = saved_doc.clone();
    steps.push(check(move |o| {
        let doc = sd.lock().unwrap().clone().ok_or("no saved doc")?;
        expect(
            o.markup.is_none() && o.image_size == Some(doc.canvas_size()),
            "discarded, render again",
            o,
        )
    }));

    let r = run(Config {
        files: vec![photo],
        driver: Some(steps),
        auto_confirm: Some(true),
        ..Default::default()
    });
    let left: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|d| d.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = r {
        eprintln!("e2e_export FAILED:\n{e:#}\nfiles: {left:?}");
        std::process::exit(1);
    }
    println!("e2e_export: OK");
}
