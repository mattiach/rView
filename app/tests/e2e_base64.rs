//! E2E: Base64.
//! - Copy as Base64 puts the PNG on screen on the clipboard as `data:image/png;base64,…`, the
//!   file's exact bytes;
//! - Paste of raw Base64 text (a JPEG, wrapped in lines) opens it in a new window from a
//!   temporary `.jpg`, removed when the app quits; plain text is ignored;
//! - Open URL takes a data URL; invalid data is reported and nothing opens;
//! - Copy as Base64 is off in Markup.
//!
//! Side effect: the clipboard content is replaced.

use rview::actions::Action;
use rview::driver::{Observation, Step};
use rview::{Config, run};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn check(f: impl Fn(&Observation) -> Result<(), String> + 'static) -> Step {
    Step::Check(Box::new(f))
}

fn expect(cond: bool, what: &str, o: &Observation) -> Result<(), String> {
    if cond {
        Ok(())
    } else {
        Err(format!(
            "{what}: path {:?}, windows {}, size {:?}, message {:?}",
            o.path, o.windows, o.image_size, o.message
        ))
    }
}

fn set_clipboard(text: String) -> Step {
    check(move |_| rview::platform::clipboard_set_text(None, &text))
}

fn encoded(img: &image::RgbaImage, f: image::ImageFormat) -> Vec<u8> {
    let mut out = Vec::new();
    let img = image::DynamicImage::ImageRgba8(img.clone());
    let img = if f == image::ImageFormat::Jpeg {
        img.to_rgb8().into()
    } else {
        img
    };
    img.write_to(&mut std::io::Cursor::new(&mut out), f).unwrap();
    out
}

fn main() {
    let dir = std::env::temp_dir().join(format!("rview-e2e-base64-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let png = dir.join("pic.png");
    let pic = image::RgbaImage::from_fn(9, 7, |x, y| {
        image::Rgba([x as u8 * 20, y as u8 * 30, 90, 128 + x as u8])
    });
    std::fs::write(&png, encoded(&pic, image::ImageFormat::Png)).unwrap();
    let png_bytes = std::fs::read(&png).unwrap();
    let jpeg = encoded(
        &image::RgbaImage::from_pixel(21, 11, image::Rgba([200, 40, 40, 255])),
        image::ImageFormat::Jpeg,
    );
    let jpeg_b64 = rv_core::dataurl::encode(&jpeg, rv_core::formats::Format::Jpeg).unwrap();
    let raw: String = jpeg_b64
        .split_once(',')
        .unwrap()
        .1
        .as_bytes()
        .chunks(76)
        .map(|l| format!("{}\r\n", std::str::from_utf8(l).unwrap()))
        .collect();
    let pasted: Arc<Mutex<Option<PathBuf>>> = Arc::default();

    let mut steps = vec![Step::WaitIdle];
    // ---- Copy as Base64: the file's bytes as a data URL
    steps.push(Step::Do(Action::CopyBase64));
    steps.push(Step::WaitIdle);
    steps.push(check(move |o| {
        let text = rview::platform::clipboard_text().unwrap_or_default();
        expect(text.starts_with("data:image/png;base64,"), "a PNG data URL", o)?;
        let (bytes, mime) = rv_core::dataurl::decode(&text).map_err(|e| e.to_string())?;
        expect(
            bytes == png_bytes && mime.as_deref() == Some("image/png"),
            "the exact file",
            o,
        )
    }));

    // ---- plain text: Paste does nothing
    steps.push(set_clipboard("just some words, not an image".into()));
    steps.push(Step::Do(Action::Paste));
    steps.push(Step::WaitIdle);
    steps.push(check(|o| {
        expect(o.windows == 1 && o.message.is_none(), "plain text ignored", o)
    }));

    // ---- Paste of raw Base64 lines: a JPEG opened from a temporary file in a new window
    steps.push(set_clipboard(raw));
    steps.push(Step::Do(Action::Paste));
    steps.push(Step::WaitIdle);
    let keep = pasted.clone();
    steps.push(check(move |o| {
        expect(o.windows == 2 && o.image_size == Some((21, 11)), "pasted JPEG shown", o)?;
        let p = o.path.clone().unwrap_or_default();
        let name = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
        expect(
            name.starts_with("rView.") && name.ends_with(".jpg"),
            "temporary .jpg",
            o,
        )?;
        *keep.lock().unwrap() = Some(p);
        Ok(())
    }));

    // ---- Open URL with a data URL, then with invalid data
    steps.push(Step::Do(Action::OpenUrl));
    steps.push(Step::DialogText(jpeg_b64));
    steps.push(Step::DialogAccept);
    steps.push(Step::WaitIdle);
    steps.push(check(|o| {
        expect(o.windows == 3 && o.image_size == Some((21, 11)), "data URL opened", o)
    }));
    steps.push(Step::Do(Action::OpenUrl));
    steps.push(Step::DialogText("data:image/png;base64,@@@@".into()));
    steps.push(Step::DialogAccept);
    steps.push(Step::WaitIdle);
    steps.push(check(|o| {
        let m = o.message.as_deref().unwrap_or("");
        expect(
            o.windows == 3 && m.contains("not valid Base64"),
            "invalid data reported",
            o,
        )
    }));

    // ---- off in Markup (it would copy the original, not the document)
    steps.push(Step::Do(Action::Markup));
    steps.push(Step::WaitIdle);
    steps.push(check(|o| {
        let on = o.menu.iter().find(|(k, ..)| *k == "copybase64").map(|(_, e, _)| *e);
        expect(
            o.markup.is_some() && on == Some(false),
            "Copy as Base64 off in Markup",
            o,
        )
    }));

    let r = run(Config {
        files: vec![png],
        driver: Some(steps),
        auto_confirm: Some(true),
        ..Default::default()
    });
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = r {
        eprintln!("e2e_base64 FAILED:\n{e:#}");
        std::process::exit(1);
    }
    let left = pasted.lock().unwrap().clone();
    if left.as_ref().is_none_or(|p| p.exists()) {
        eprintln!("e2e_base64 FAILED: temporary file not removed: {left:?}");
        std::process::exit(1);
    }
    println!("e2e_base64: OK");
}
