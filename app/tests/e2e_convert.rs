//! E2E: Convert… for the file on screen: PNG with transparency → JPEG (flattened on the
//! background), → WebP (transparency kept), → PNG flattened with 300 DPI; a conversion onto the
//! source and an invalid resolution are refused with a message; resized by percentage and to a
//! print size at a DPI; off in Markup, where Export resizes the document.

use rv_core::decode::DecodeOptions;
use rview::actions::Action;
use rview::driver::{Observation, Step};
use rview::{Config, run};
use std::path::{Path, PathBuf};

const W: u32 = 64;
const H: u32 = 48;

fn check(f: impl Fn(&Observation) -> Result<(), String> + 'static) -> Step {
    Step::Check(Box::new(f))
}

fn expect(cond: bool, what: &str, o: &Observation) -> Result<(), String> {
    if cond {
        Ok(())
    } else {
        Err(format!("{what}: message {:?}, dialog {:?}", o.message, o.dialog))
    }
}

fn decoded(p: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let b = std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
    let d = rv_core::decode::decode(&b, None, &DecodeOptions::default()).map_err(|e| e.to_string())?;
    Ok((d.size.w, d.size.h, rv_core::color::rgba8(d.pixels)))
}

/// Left half transparent, right half opaque green.
fn source() -> image::RgbaImage {
    image::RgbaImage::from_fn(W, H, |x, _| {
        if x < W / 2 {
            image::Rgba([0, 0, 0, 0])
        } else {
            image::Rgba([0, 200, 0, 255])
        }
    })
}

fn convert(text: &str, dst: &Path) -> Vec<Step> {
    vec![
        Step::Do(Action::Convert),
        Step::DialogText(text.into()),
        Step::SavePath(dst.to_path_buf()),
        Step::DialogAccept,
        Step::WaitIdle,
    ]
}

fn main() {
    let dir = std::env::temp_dir().join(format!("rview-e2e-convert-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let png = dir.join("logo.png");
    source().save(&png).unwrap();
    let original = std::fs::read(&png).unwrap();
    let (jpg, webp, flat, bad) = (
        dir.join("logo.jpg"),
        dir.join("logo.webp"),
        dir.join("flat.png"),
        dir.join("bad.png"),
    );

    let mut steps = vec![Step::WaitIdle, Step::Do(Action::Convert)];
    steps.push(check(|o| {
        let ok = o.dialog.as_ref().is_some_and(|(k, t, _)| *k == "export" && t == "PNG");
        expect(ok, "Convert dialog, PNG proposed for a PNG", o)
    }));
    steps.push(Step::DialogCancel);

    // ---- PNG → JPEG: transparent half on the background
    steps.extend(convert("JPEG quality=90 background=#ff0000", &jpg));
    let p = jpg.clone();
    steps.push(check(move |o| {
        let (w, h, px) = decoded(&p)?;
        expect((w, h) == (W, H), "JPEG size", o)?;
        expect(px.chunks_exact(4).all(|q| q[3] == 255), "JPEG opaque", o)?;
        let left = &px[(10 * W as usize + 5) * 4..][..3];
        let right = &px[(10 * W as usize + 50) * 4..][..3];
        expect(
            left[0] > 230 && left[1] < 25 && left[2] < 25,
            &format!("background {left:?}"),
            o,
        )?;
        expect(right[1] > 180 && right[0] < 25, &format!("content {right:?}"), o)
    }));

    // ---- PNG → WebP: lossless, transparency kept
    steps.extend(convert("WebP", &webp));
    let p = webp.clone();
    steps.push(check(move |o| {
        let (w, h, px) = decoded(&p)?;
        expect(
            (w, h) == (W, H) && px == *source().as_raw(),
            "WebP = source with alpha",
            o,
        )
    }));

    // ---- PNG → PNG with flattening and 300 DPI: pixels flattened, size unchanged
    steps.extend(convert("PNG flatten background=#0000ff dpi=300", &flat));
    let p = flat.clone();
    steps.push(check(move |o| {
        let (w, h, px) = decoded(&p)?;
        expect((w, h) == (W, H), "same pixel size", o)?;
        expect(
            px[..4] == [0, 0, 255, 255] && px[(W as usize - 1) * 4..][..4] == [0, 200, 0, 255],
            "flattened",
            o,
        )?;
        let b = std::fs::read(&p).map_err(|e| e.to_string())?;
        let r = rv_core::details::read(&b, None, &DecodeOptions::default()).resolution;
        expect(
            r.is_some_and(|r| (r.x - 300.0).abs() < 0.02),
            &format!("300 DPI {r:?}"),
            o,
        )
    }));

    // ---- onto the source: refused, the file is untouched
    steps.extend(convert("PNG", &png));
    let (p, orig) = (png.clone(), original.clone());
    steps.push(check(move |o| {
        let msg = o.message.as_deref().unwrap_or("");
        expect(msg.contains("onto itself"), "conversion onto the source refused", o)?;
        expect(
            std::fs::read(&p).map_err(|e| e.to_string())? == orig,
            "source unchanged",
            o,
        )
    }));

    // ---- invalid resolution: the export's error, nothing written
    steps.extend(convert("PNG dpi=0", &bad));
    let (p, d) = (bad.clone(), dir.clone());
    steps.push(check(move |o| {
        let msg = o.message.as_deref().unwrap_or("");
        expect(msg.contains("Invalid resolution"), "invalid DPI reported", o)?;
        let names: Vec<String> = std::fs::read_dir(&d)
            .map_err(|e| e.to_string())?
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        expect(
            !p.exists() && names.len() == 4,
            &format!("no file or temp left {names:?}"),
            o,
        )
    }));

    // ---- resized to 50 %: transparency kept, no fringe on the edge
    let half = dir.join("half.png");
    steps.extend(convert("PNG scale=50", &half));
    let p = half.clone();
    steps.push(check(move |o| {
        let (w, h, px) = decoded(&p)?;
        expect((w, h) == (W / 2, H / 2), &format!("half size {w}x{h}"), o)?;
        expect(px[..4] == [0, 0, 0, 0], "left transparent", o)?;
        let clean = px.chunks_exact(4).filter(|q| q[3] > 0).all(|q| q[..3] == [0, 200, 0]);
        expect(clean, "no dark fringe", o)
    }));

    // ---- print size 2.54 × 2.032 cm (1 × 0.8 in) at 100 DPI: 100 × 80 pixels, 100 DPI stored
    let print = dir.join("print.png");
    steps.extend(convert("PNG dpi=100 print=2.54x2.032", &print));
    let p = print.clone();
    steps.push(check(move |o| {
        let (w, h, _) = decoded(&p)?;
        expect((w, h) == (100, 80), &format!("print pixels {w}x{h}"), o)?;
        let b = std::fs::read(&p).map_err(|e| e.to_string())?;
        let r = rv_core::details::read(&b, None, &DecodeOptions::default()).resolution;
        expect(
            r.is_some_and(|r| (r.x - 100.0).abs() < 0.02),
            &format!("100 DPI {r:?}"),
            o,
        )
    }));

    // ---- in Markup, Export is the conversion: Convert does nothing
    steps.push(Step::Do(Action::Markup));
    steps.push(Step::WaitIdle);
    steps.push(Step::Do(Action::Convert));
    steps.push(check(|o| {
        expect(o.markup.is_some() && o.dialog.is_none(), "Convert off in Markup", o)
    }));

    // ---- Markup Export resized: the document's render at 25 %
    let quarter = dir.join("quarter.png");
    steps.extend([
        Step::Do(Action::Export),
        Step::DialogText("PNG scale=25".into()),
        Step::SavePath(quarter.clone()),
        Step::DialogAccept,
        Step::WaitIdle,
    ]);
    let p = quarter.clone();
    steps.push(check(move |o| {
        let (w, h, _) = decoded(&p)?;
        expect((w, h) == (W / 4, H / 4), &format!("exported at 25 % {w}x{h}"), o)
    }));

    let r = run(Config {
        files: vec![png],
        driver: Some(steps),
        auto_confirm: Some(true),
        ..Default::default()
    });
    let left: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|d| d.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = r {
        eprintln!("e2e_convert FAILED:\n{e:#}\nfiles: {left:?}");
        std::process::exit(1);
    }
    println!("e2e_convert: OK");
}
