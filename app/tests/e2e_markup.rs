//! E2E: Markup and the preview-vs-export rule:
//! Markup → rectangle + arrow drawn with the pointer → move → colour change → undo ×3 → redo;
//! the document is the expected one, its render matches a document built through the API, and
//! the preview at 100 % matches the render (≥ 99 % of the pixels within ±2, shape edges excluded).
//! Then crop with the handles + Enter, rotate, Delete removes a shape (never the file), Esc twice
//! asks and leaves. Export to a file is in e2e_export: the render (`core::edit::render`) is compared instead.

use rv_core::edit::render::{Source, render, same};
use rv_core::edit::*;
use rview::actions::Action;
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
        Ok(())
    } else {
        let m = o.markup.as_ref();
        Err(format!(
            "{what}: tool {:?}, selection {:?}, shapes {:?}, crop {:?}, title {:?}",
            m.map(|m| m.tool),
            m.map(|m| m.selection.clone()),
            m.map(|m| m
                .doc
                .shapes
                .iter()
                .map(|s| (s.id.0, s.geom.clone()))
                .collect::<Vec<_>>()),
            m.map(|m| m.doc.crop),
            o.title
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

/// Press at `a`, move through the middle to `b`, release (image coordinates).
fn drag(a: (f32, f32), b: (f32, f32)) -> Vec<Step> {
    vec![
        Step::MoveToImage(a.0, a.1),
        button(ElementState::Pressed),
        Step::MoveToImage((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0),
        Step::MoveToImage(b.0, b.1),
        button(ElementState::Released),
    ]
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() <= 0.6
}

fn rect_of(o: &Observation, i: usize) -> Option<RectF> {
    match &o.markup.as_ref()?.doc.shapes.get(i)?.geom {
        Geom::Rect { rect, .. } => Some(*rect),
        _ => None,
    }
}

/// (w, h, straight RGBA8) of the document's source.
fn rgba_of(s: rv_core::export::DocSource) -> (u32, u32, std::sync::Arc<Vec<u8>>) {
    (
        s.w,
        s.h,
        std::sync::Arc::new(rv_core::color::rgba8((*s.pixels).clone())),
    )
}

fn source() -> Vec<u8> {
    let mut v = Vec::with_capacity((W * H * 4) as usize);
    for y in 0..H {
        for x in 0..W {
            v.extend_from_slice(&[(x * 255 / W) as u8, (y * 255 / H) as u8, 128, 255]);
        }
    }
    v
}

fn main() {
    let dir = std::env::temp_dir().join(format!("rview-e2e-markup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("photo.png");
    image::RgbaImage::from_raw(W, H, source()).unwrap().save(&file).unwrap();
    let file2 = file.clone();
    // the screen transform at 100 %, read by a check and used by the snapshot check after it
    let t100: Arc<Mutex<[f32; 6]>> = Arc::default();
    let t_set = t100.clone();
    let doc_at_100: Arc<Mutex<Option<Arc<DocState>>>> = Arc::default();
    let doc_set = doc_at_100.clone();

    let mut steps = vec![
        Step::WaitIdle,
        Step::Do(Action::Markup),
        Step::WaitIdle,
        check(|o| {
            let m = o.markup.as_ref().ok_or("not in Markup")?;
            expect(m.tool == "select" && m.source.is_some() && !m.dirty, "entered", o)?;
            expect(!o.title.starts_with('•'), "clean title", o)
        }),
        // rectangle
        Step::Key("R".into()),
    ];
    steps.extend(drag((20.0, 20.0), (80.0, 60.0)));
    steps.push(check(|o| {
        let r = rect_of(o, 0).ok_or("no rectangle")?;
        expect(
            near(r.x, 20.0) && near(r.y, 20.0) && near(r.w, 60.0) && near(r.h, 40.0),
            "rect drawn",
            o,
        )?;
        let m = o.markup.as_ref().unwrap();
        expect(
            m.selection == [1] && m.dirty && o.title.starts_with("• "),
            "selected, dirty, • in the title",
            o,
        )
    }));
    // arrow
    steps.push(Step::Key("A".into()));
    steps.extend(drag((100.0, 100.0), (180.0, 30.0)));
    steps.push(check(|o| {
        let m = o.markup.as_ref().unwrap();
        let ok = matches!(&m.doc.shapes.get(1).map(|s| &s.geom), Some(Geom::Arrow { a, b, .. })
            if near(a.x, 100.0) && near(a.y, 100.0) && near(b.x, 180.0) && near(b.y, 30.0));
        expect(ok && m.doc.shapes[1].id == ObjectId(2), "arrow drawn", o)
    }));
    // move the rectangle by its left edge
    steps.push(Step::Key("V".into()));
    steps.extend(drag((20.0, 40.0), (30.0, 45.0)));
    steps.push(check(|o| {
        let r = rect_of(o, 0).ok_or("no rectangle")?;
        expect(near(r.x, 30.0) && near(r.y, 25.0), "moved by (10, 5)", o)?;
        expect(
            o.markup.as_ref().unwrap().selection == [1],
            "the moved shape is selected",
            o,
        )
    }));
    // change its colour (as the Inspector does)
    steps.push(Step::MarkupStyle(Box::new(|s| s.stroke = Some(Color::rgb(0, 0, 255)))));
    steps.push(check(|o| {
        let m = o.markup.as_ref().unwrap();
        expect(m.doc.shapes[0].style.stroke == Some(Color::rgb(0, 0, 255)), "blue", o)
    }));
    // undo ×3: colour, move, arrow
    for _ in 0..3 {
        steps.push(Step::Key("Ctrl+Z".into()));
    }
    steps.push(check(|o| {
        let m = o.markup.as_ref().unwrap();
        let r = rect_of(o, 0).ok_or("no rectangle")?;
        expect(
            m.doc.shapes.len() == 1 && near(r.x, 20.0),
            "back to the first rectangle",
            o,
        )?;
        expect(
            m.doc.shapes[0].style == Style::default_for(ShapeKind::Rect),
            "original colour",
            o,
        )?;
        expect(m.can_redo && m.dirty, "redo available, still dirty", o)
    }));
    steps.push(Step::Key("Ctrl+Y".into()));
    steps.push(check(|o| {
        let m = o.markup.as_ref().unwrap();
        expect(
            m.doc.shapes.len() == 2 && m.doc.shapes[1].id == ObjectId(2),
            "arrow back, same ID",
            o,
        )
    }));
    // the render equals a document built through the API (within float noise of the pointer)
    steps.push(check(|o| {
        let m = o.markup.as_ref().unwrap();
        let (sw, sh, src) = rgba_of(m.source.clone().ok_or("no source")?);
        let mut d = DocState::new(W, H);
        for geom in [
            Geom::Rect {
                rect: RectF {
                    x: 20.0,
                    y: 20.0,
                    w: 60.0,
                    h: 40.0,
                },
                rotation: 0.0,
                corner_radius: 0.0,
            },
            Geom::Arrow {
                a: PointF { x: 100.0, y: 100.0 },
                b: PointF { x: 180.0, y: 30.0 },
                heads: ArrowHeads::End,
                bend: 0.0,
            },
        ] {
            let kind = geom.kind();
            let s = Shape {
                id: d.peek_id(),
                geom,
                style: Style::default_for(kind),
            };
            d = d.apply(&Edit::AddShape(s)).unwrap();
        }
        let s = Source {
            w: sw,
            h: sh,
            rgba: &src,
        };
        let (a, b) = (render(&m.doc, &s, &same), render(&d, &s, &same));
        let diff = a.2.iter().zip(&b.2).filter(|(x, y)| x.abs_diff(**y) > 2).count();
        expect(
            (a.0, a.1) == (W, H) && diff * 1000 < a.2.len(),
            &format!("render vs API document: {diff} samples differ"),
            o,
        )
    }));
    // preview at 100 % vs render
    steps.push(Step::Key("O".into()));
    steps.push(Step::WaitIdle);
    steps.push(check(move |o| {
        *t_set.lock().unwrap() = o.screen_from_image;
        *doc_set.lock().unwrap() = o.markup.as_ref().map(|m| m.doc.clone());
        expect((o.scale - 1.0).abs() < 1e-6, "100 %", o)
    }));
    let src = source();
    steps.push(Step::Snapshot(Box::new(move |sw, _sh, px| {
        let t = *t100.lock().unwrap();
        let doc = doc_at_100.lock().unwrap().clone().ok_or("no document")?;
        let s = Source { w: W, h: H, rgba: &src };
        let (_, _, out) = render(&doc, &s, &same);
        // shape edges: pixels with partial coverage, grown by 1 px
        let mut cover = rv_core::edit::tiny_skia::Pixmap::new(W, H).unwrap();
        rv_core::edit::render::draw_shapes(
            &mut cover.as_mut(),
            &doc.shapes,
            rv_core::edit::tiny_skia::Transform::identity(),
            1.0,
            &same,
        );
        let alpha = |x: i32, y: i32| -> u8 {
            if x < 0 || y < 0 || x >= W as i32 || y >= H as i32 {
                0
            } else {
                cover.data()[((y as u32 * W + x as u32) * 4 + 3) as usize]
            }
        };
        let edge = |x: i32, y: i32| (-1..=1).any(|dy| (-1..=1).any(|dx| !matches!(alpha(x + dx, y + dy), 0 | 255)));
        let (mut total, mut ok) = (0usize, 0usize);
        // pixels fully covered by a shape, checked on their own (the overlay must be there)
        let (mut covered, mut covered_ok) = (0usize, 0usize);
        for y in 0..H as i32 {
            for x in 0..W as i32 {
                if edge(x, y) {
                    continue;
                }
                let (scx, scy) = (
                    t[0] * x as f32 + t[1] * y as f32 + t[2],
                    t[3] * x as f32 + t[4] * y as f32 + t[5],
                );
                let (scx, scy) = (scx.round() as usize, scy.round() as usize);
                let p = &px[(scy * sw as usize + scx) * 4..][..3];
                let r = &out[((y as u32 * W + x as u32) * 4) as usize..][..3];
                total += 1;
                let same_px = p.iter().zip(r).all(|(a, b)| a.abs_diff(*b) <= 2);
                if same_px {
                    ok += 1;
                }
                if alpha(x, y) == 255 {
                    covered += 1;
                    if same_px {
                        covered_ok += 1;
                    }
                }
            }
        }
        if ok * 100 >= total * 99 && covered > 100 && covered_ok * 100 >= covered * 99 {
            Ok(())
        } else {
            Err(format!(
                "preview vs render: {ok} of {total} pixels within ±2, shape interiors {covered_ok} of {covered}"
            ))
        }
    })));
    // crop with the handles, Enter applies it
    steps.push(Step::Key("C".into()));
    steps.push(Step::WaitIdle);
    steps.extend(drag((W as f32, H as f32), (110.0, 90.0)));
    steps.extend(drag((0.0, 0.0), (10.0, 10.0)));
    steps.push(check(|o| {
        let c = o.markup.as_ref().unwrap().crop_draft.ok_or("no crop draft")?;
        expect(
            near(c.x, 10.0) && near(c.y, 10.0) && near(c.w, 100.0) && near(c.h, 80.0),
            "crop draft",
            o,
        )
    }));
    steps.push(Step::Key("Enter".into()));
    steps.push(Step::MarkupButton("rotate-right"));
    steps.push(Step::WaitIdle);
    steps.push(check(|o| {
        let m = o.markup.as_ref().unwrap();
        expect(
            m.tool == "select"
                && m.doc.crop
                    == Some(RectU {
                        x: 10,
                        y: 10,
                        w: 100,
                        h: 80,
                    }),
            "crop applied",
            o,
        )?;
        expect(m.doc.canvas_size() == (80, 100), "rotated canvas", o)?;
        let (sw, sh, src) = rgba_of(m.source.clone().unwrap());
        let out = render(
            &m.doc,
            &Source {
                w: sw,
                h: sh,
                rgba: &src,
            },
            &same,
        );
        expect((out.0, out.1) == (80, 100), "render size after crop + rotate", o)
    }));
    // Del deletes the selected shape, never the file
    steps.push(Step::Key("Tab".into()));
    steps.push(Step::Key("Del".into()));
    steps.push(check(move |o| {
        let m = o.markup.as_ref().unwrap();
        expect(m.doc.shapes.len() == 1 && file2.exists(), "shape deleted, file kept", o)
    }));
    // Esc: deselect (nothing selected now) → leave; dirty → the prompt (answered: discard)
    steps.push(Step::Key("Esc".into()));
    steps.push(Step::WaitIdle);
    steps.push(check(|o| {
        expect(o.markup.is_none() && !o.title.starts_with('•'), "left Markup", o)?;
        let ok = o.menu.iter().any(|(k, e, _)| *k == "markup" && *e);
        expect(ok, "Markup available again", o)
    }));

    let r = run(Config {
        files: vec![file],
        driver: Some(steps),
        auto_confirm: Some(true),
        ..Default::default()
    });
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = r {
        eprintln!("e2e_markup FAILED:\n{e:#}");
        std::process::exit(1);
    }
    println!("e2e_markup: OK");
}
