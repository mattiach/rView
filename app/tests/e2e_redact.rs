//! E2E: "redact Solid + export strip-all". Redact tool (B) → Solid area drawn with
//! the pointer: on screen at once (the patch in the image texture); a rectangle; Ctrl+A selects
//! both (mixed selection through the UI); a text box typed with the Text tool (T); mode change of
//! the redaction and back; Export as PNG (strip-all is the default with redactions): the area is
//! exactly the cover colour, no EXIF (the source had GPS), the text is drawn; leaving Markup puts
//! the source pixels back on screen.

use rv_core::decode::DecodeOptions;
use rv_core::edit::*;
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
        "{what}: tool {:?}, selection {:?}, redactions {:?}, shapes {:?}, editing {:?}, dialog {:?}, message {:?}",
        m.map(|m| m.tool),
        m.map(|m| m.selection.clone()),
        m.map(|m| m.doc.redactions.clone()),
        m.map(|m| m.doc.shapes.iter().map(|s| s.geom.clone()).collect::<Vec<_>>()),
        m.map(|m| m.editing),
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

/// IFD0 → GPS IFD with GPSLatitudeRef "N" (TIFF structure, big endian).
fn gps_exif() -> Vec<u8> {
    let mut e = b"MM\0*\0\0\0\x08".to_vec();
    e.extend_from_slice(&1u16.to_be_bytes());
    e.extend_from_slice(&0x8825u16.to_be_bytes()); // GPSInfoIFDPointer
    e.extend_from_slice(&4u16.to_be_bytes()); // LONG
    e.extend_from_slice(&1u32.to_be_bytes());
    e.extend_from_slice(&26u32.to_be_bytes()); // GPS IFD at 26
    e.extend_from_slice(&0u32.to_be_bytes());
    e.extend_from_slice(&1u16.to_be_bytes());
    e.extend_from_slice(&0x0001u16.to_be_bytes()); // GPSLatitudeRef
    e.extend_from_slice(&2u16.to_be_bytes()); // ASCII
    e.extend_from_slice(&2u32.to_be_bytes());
    e.extend_from_slice(b"N\0\0\0");
    e.extend_from_slice(&0u32.to_be_bytes());
    e
}

fn source() -> Vec<u8> {
    (0..W * H)
        .flat_map(|i| {
            let (x, y) = (i % W, i / W);
            [(x * 255 / W) as u8, (y * 255 / H) as u8, 200, 255]
        })
        .collect()
}

/// Inside the area (sampled): every such pixel is touched by it.
fn inside(a: &Area, x: u32, y: u32) -> bool {
    (0..4).any(|i| {
        (0..4).any(|j| {
            let p = PointF {
                x: x as f32 + (i as f32 + 0.5) / 4.0,
                y: y as f32 + (j as f32 + 0.5) / 4.0,
            };
            geom::hit_area(a, p, 0.0)
        })
    })
}

fn main() {
    let dir = std::env::temp_dir().join(format!("rview-e2e-redact-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("card.png");
    let mut png = Vec::new();
    {
        use image::ImageEncoder;
        let mut e = image::codecs::png::PngEncoder::new(&mut png);
        e.set_exif_metadata(gps_exif()).unwrap();
        e.write_image(&source(), W, H, image::ExtendedColorType::Rgba8).unwrap();
    }
    std::fs::write(&file, &png).unwrap();
    let out = dir.join("card-markup.png");
    let t_shot: Arc<Mutex<[f32; 6]>> = Arc::default();

    let mut steps = vec![Step::WaitIdle, Step::Do(rview::actions::Action::Markup), Step::WaitIdle];
    // ---- Redact tool (B): Solid black is the first mode
    steps.push(Step::Key("B".into()));
    steps.push(check(|o| {
        expect(o.markup.as_ref().is_some_and(|m| m.tool == "redact"), "Redact tool", o)
    }));
    steps.push(Step::MarkupRedact(Box::new(|k, _| {
        assert_eq!(*k, RedactKind::Solid { color: [0, 0, 0] }, "default mode");
    })));
    steps.extend(drag((40.0, 30.0), (120.0, 70.0)));
    let t_set = t_shot.clone();
    steps.push(check(move |o| {
        let m = o.markup.as_ref().ok_or("not in Markup")?;
        let ok = matches!(&m.doc.redactions[..], [r] if r.kind == RedactKind::Solid { color: [0, 0, 0] });
        expect(ok && m.selection.len() == 1, "Solid redaction drawn and selected", o)?;
        *t_set.lock().unwrap() = o.screen_from_image;
        Ok(())
    }));
    // the preview shows it: the patch is in the image texture
    let t_get = t_shot.clone();
    steps.push(Step::Snapshot(Box::new(move |sw, _, px| {
        let t = *t_get.lock().unwrap();
        let at = |x: f32, y: f32| {
            let (sx, sy) = (t[0] * x + t[1] * y + t[2], t[3] * x + t[4] * y + t[5]);
            let i = (sy.round() as usize * sw as usize + sx.round() as usize) * 4;
            [px[i], px[i + 1], px[i + 2]]
        };
        let c = at(80.0, 50.0);
        if c.iter().all(|v| *v <= 2) {
            Ok(())
        } else {
            Err(format!("preview: the redaction is not black on screen ({c:?})"))
        }
    })));
    // ---- a rectangle, then Ctrl+A: shapes and redactions together
    steps.push(Step::Key("R".into()));
    steps.extend(drag((150.0, 100.0), (200.0, 140.0)));
    // the rectangle is being adjusted: with the Rectangle tool still active the pointer resizes it
    // (bottom-right handle), moves it (drag inside), and a drag elsewhere creates nothing
    let rect_of = |o: &Observation| {
        o.markup.as_ref().and_then(|m| {
            m.doc.shapes.iter().find_map(|s| match s.geom {
                Geom::Rect { rect, .. } => Some(rect),
                _ => None,
            })
        })
    };
    steps.extend(drag((200.0, 140.0), (210.0, 150.0)));
    steps.push(check(move |o| {
        let r = rect_of(o).ok_or("no rectangle")?;
        let m = o.markup.as_ref().unwrap();
        let ok = (r.w - 60.0).abs() < 1.0 && (r.h - 50.0).abs() < 1.0 && m.doc.shapes.len() == 1 && m.tool == "rect";
        expect(ok, &format!("resized with the handle, no new shape ({r:?})"), o)
    }));
    steps.extend(drag((180.0, 125.0), (170.0, 115.0)));
    steps.push(check(move |o| {
        let r = rect_of(o).ok_or("no rectangle")?;
        expect(
            (r.x - 140.0).abs() < 1.0 && (r.y - 90.0).abs() < 1.0,
            &format!("moved by dragging inside ({r:?})"),
            o,
        )
    }));
    steps.extend(drag((20.0, 130.0), (60.0, 150.0)));
    steps.push(check(|o| {
        let m = o.markup.as_ref().unwrap();
        expect(
            m.doc.shapes.len() == 1 && m.selection.len() == 1,
            "a drag elsewhere creates nothing before the confirmation",
            o,
        )
    }));
    // a press on the Inspector, right after the pointer got there, is the Inspector's
    steps.push(Step::Sleep(std::time::Duration::from_millis(150))); // a frame lays the panels out
    steps.push(Step::MoveTo(Box::new(|o| {
        o.markup.as_ref().and_then(|m| m.inspector).unwrap_or((0.0, 0.0))
    })));
    steps.push(button(ElementState::Pressed));
    steps.push(button(ElementState::Released));
    steps.push(check(|o| {
        let m = o.markup.as_ref().unwrap();
        expect(
            m.inspector.is_some() && m.doc.shapes.len() == 1 && m.selection.len() == 1,
            "click on the Inspector: nothing drawn, selection kept",
            o,
        )
    }));
    // Enter confirms: the next drag draws a new rectangle (removed again with Del)
    steps.push(Step::Key("Enter".into()));
    steps.push(check(|o| {
        expect(o.markup.as_ref().unwrap().selection.is_empty(), "Enter confirms", o)
    }));
    steps.extend(drag((20.0, 130.0), (60.0, 150.0)));
    steps.push(check(|o| {
        expect(
            o.markup.as_ref().unwrap().doc.shapes.len() == 2,
            "after the confirmation the tool draws again",
            o,
        )
    }));
    steps.push(Step::Key("Del".into()));
    steps.push(check(|o| {
        expect(
            o.markup.as_ref().unwrap().doc.shapes.len() == 1,
            "Del removes the element being adjusted",
            o,
        )
    }));
    steps.push(Step::Key("V".into()));
    steps.push(Step::Key("Ctrl+A".into()));
    steps.push(check(|o| {
        let m = o.markup.as_ref().unwrap();
        expect(m.selection.len() == 2, "mixed selection (shape + redaction)", o)
    }));
    steps.push(Step::Key("Esc".into()));
    // ---- mode change and back (the cache keeps the Solid patch)
    steps.push(Step::Key("Tab".into()));
    steps.push(check(|o| {
        let m = o.markup.as_ref().unwrap();
        expect(
            m.selection == vec![m.doc.redactions[0].id.0],
            "redaction selected by Tab",
            o,
        )
    }));
    steps.push(Step::MarkupRedact(Box::new(|k, _| {
        *k = RedactKind::Pixelate { block: 8 }
    })));
    steps.push(check(|o| {
        let m = o.markup.as_ref().unwrap();
        expect(
            m.doc.redactions[0].kind == RedactKind::Pixelate { block: 8 },
            "pixelated",
            o,
        )
    }));
    steps.push(Step::Key("Ctrl+Z".into()));
    steps.push(check(|o| {
        let m = o.markup.as_ref().unwrap();
        expect(
            m.doc.redactions[0].kind == RedactKind::Solid { color: [0, 0, 0] },
            "undo: Solid again",
            o,
        )
    }));
    // ---- a text box (T): click, type, Esc
    steps.push(Step::Key("T".into()));
    steps.push(Step::MoveToImage(20.0, 100.0));
    steps.push(button(ElementState::Pressed));
    steps.push(button(ElementState::Released));
    steps.push(check(|o| {
        expect(
            o.markup.as_ref().unwrap().editing.is_some(),
            "editing a new text box",
            o,
        )
    }));
    steps.push(Step::TypeText("Hello".into()));
    steps.push(Step::Key("Esc".into()));
    steps.push(check(|o| {
        let m = o.markup.as_ref().unwrap();
        let ok = m
            .doc
            .shapes
            .iter()
            .any(|s| matches!(&s.geom, Geom::Text { text, .. } if text == "Hello"));
        expect(ok && m.editing.is_none(), "text committed", o)
    }));
    // the text box is still being adjusted: a click elsewhere adds nothing, a click on it edits it
    // again, Ctrl+Enter confirms
    let texts = |o: &Observation| {
        let m = o.markup.as_ref().unwrap();
        m.doc
            .shapes
            .iter()
            .filter(|s| matches!(s.geom, Geom::Text { .. }))
            .count()
    };
    steps.push(Step::MoveToImage(150.0, 20.0));
    steps.push(button(ElementState::Pressed));
    steps.push(button(ElementState::Released));
    steps.push(check(move |o| {
        let m = o.markup.as_ref().unwrap();
        expect(
            texts(o) == 1 && m.editing.is_none() && m.selection.len() == 1,
            "click elsewhere: no new text box",
            o,
        )
    }));
    steps.push(Step::MoveToImage(24.0, 104.0));
    steps.push(button(ElementState::Pressed));
    steps.push(button(ElementState::Released));
    steps.push(check(|o| {
        expect(
            o.markup.as_ref().unwrap().editing.is_some(),
            "click on the box: editing again",
            o,
        )
    }));
    steps.push(Step::TypeText("!".into()));
    steps.push(Step::MarkupButton("confirm"));
    steps.push(check(|o| {
        let m = o.markup.as_ref().unwrap();
        let ok = m
            .doc
            .shapes
            .iter()
            .any(|s| matches!(&s.geom, Geom::Text { text, .. } if text.contains('!') && text.contains("Hello")));
        expect(
            ok && m.editing.is_none() && m.selection.is_empty(),
            "Confirm: typed and confirmed",
            o,
        )
    }));
    // now a click makes a new box; left empty, it disappears
    steps.push(Step::MoveToImage(150.0, 20.0));
    steps.push(button(ElementState::Pressed));
    steps.push(button(ElementState::Released));
    steps.push(check(|o| {
        expect(
            o.markup.as_ref().unwrap().editing.is_some(),
            "new text box after the confirmation",
            o,
        )
    }));
    steps.push(Step::Key("Esc".into()));
    steps.push(check(move |o| expect(texts(o) == 1, "empty text box removed", o)));
    // ---- a loupe (toolbar), dragged from its centre: on screen it shows the image magnified
    steps.push(Step::MarkupButton("tool-loupe"));
    steps.extend(drag((120.0, 120.0), (140.0, 120.0)));
    steps.push(check(|o| {
        let m = o.markup.as_ref().unwrap();
        let ok = m.doc.shapes.iter().any(|s| {
            matches!(s.geom, Geom::Loupe { center, radius, zoom }
                if (center.x - 120.0).abs() < 0.6 && (radius - 20.0).abs() < 0.6 && zoom == 2.0)
        });
        expect(ok, "loupe drawn", o)
    }));
    let t_get = t_shot.clone();
    steps.push(Step::Snapshot(Box::new(move |sw, _, px| {
        let t = *t_get.lock().unwrap();
        let at = |x: f32, y: f32| {
            let (sx, sy) = (t[0] * x + t[1] * y + t[2], t[3] * x + t[4] * y + t[5]);
            let i = (sy.round() as usize * sw as usize + sx.round() as usize) * 4;
            px[i] as i32
        };
        // the red channel is the x gradient: 12 px right of the centre shows the image 6 px right
        let expect_r = |x: f32| (x * 255.0 / W as f32) as i32;
        let got = at(132.0, 120.0);
        if (got - expect_r(126.0)).abs() <= 3 && (got - expect_r(132.0)).abs() > 3 {
            Ok(())
        } else {
            Err(format!(
                "loupe on screen: red {got}, expected about {}",
                expect_r(126.0)
            ))
        }
    })));
    // ---- export: PNG, strip-all by default
    steps.push(Step::Key("Ctrl+Shift+S".into()));
    steps.push(check(|o| {
        let ok = o.dialog.as_ref().is_some_and(|(k, t, _)| *k == "export" && t == "PNG");
        expect(ok, "Export dialog", o)
    }));
    steps.push(Step::SavePath(out.clone()));
    steps.push(Step::DialogAccept);
    steps.push(Step::WaitIdle);
    let (p, src) = (out.clone(), source());
    steps.push(check(move |o| {
        let m = o.markup.as_ref().unwrap();
        let b = std::fs::read(&p).map_err(|e| e.to_string())?;
        let d = rv_core::decode::decode(&b, None, &DecodeOptions::default()).map_err(|e| e.to_string())?;
        expect(d.meta.exif.is_none(), "strip-all: no EXIF (the source had GPS)", o)?;
        let px = rv_core::color::rgba8(d.pixels);
        let area = &m.doc.redactions[0].area;
        let (mut covered, mut text_px) = (0, 0);
        for y in 0..H {
            for x in 0..W {
                let i = ((y * W + x) * 4) as usize;
                if inside(area, x, y) {
                    expect(px[i..i + 4] == [0, 0, 0, 255], &format!("pixel {x},{y} not covered"), o)?;
                    covered += 1;
                } else if (14..80).contains(&x) && (95..130).contains(&y) && px[i..i + 3] != src[i..i + 3] {
                    text_px += 1;
                }
            }
        }
        expect(
            covered > 3000 && text_px > 20,
            &format!("{covered} covered, {text_px} text pixels"),
            o,
        )
    }));
    // ---- leave (prompt answered: discard): the screen shows the source again
    steps.push(Step::MarkupButton("done"));
    steps.push(Step::WaitIdle);
    let t_get = t_shot.clone();
    steps.push(Step::Snapshot(Box::new(move |sw, _, px| {
        let t = *t_get.lock().unwrap();
        let (sx, sy) = (t[0] * 80.0 + t[1] * 50.0 + t[2], t[3] * 80.0 + t[4] * 50.0 + t[5]);
        let i = (sy.round() as usize * sw as usize + sx.round() as usize) * 4;
        if px[i..i + 3].iter().all(|v| *v <= 2) {
            Err("after leaving Markup the redaction is still on screen".into())
        } else {
            Ok(())
        }
    })));

    let r = run(Config {
        files: vec![file],
        driver: Some(steps),
        auto_confirm: Some(true),
        // a window of the default size: the toolbar and the Inspector leave the image free
        settings: Some(rv_core::settings::Settings {
            window_resize_mode: rv_core::settings::WindowResizeMode::Never,
            ..Default::default()
        }),
        ..Default::default()
    });
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = r {
        eprintln!("e2e_redact FAILED:\n{e:#}");
        std::process::exit(1);
    }
    println!("e2e_redact: OK");
}
