//! E2E: an SVG zoomed in beyond its first raster (made at the monitor size) gets a sharper
//! one: more texels, same logical size, the view doesn't move.

use rview::actions::Action;
use rview::driver::{Observation, Step};
use rview::{Config, run};

fn check(f: impl Fn(&Observation) -> Result<(), String> + 'static) -> Step {
    Step::Check(Box::new(f))
}

fn main() {
    let dir = std::env::temp_dir().join(format!("rview-e2e-svg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("lines.svg");
    std::fs::write(
        &file,
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 100"><rect width="200" height="100" fill="#fff"/><path d="M0 50 L200 50" stroke="#000" stroke-width="0.3"/></svg>"##,
    )
    .unwrap();
    let first = std::sync::Arc::new(std::sync::Mutex::new((0u32, 0u32, (0u32, 0u32), 0f32)));
    let (f1, f2) = (first.clone(), first.clone());
    let steps = vec![
        Step::WaitIdle,
        check(move |o| {
            let t = o.texture.ok_or("no texture")?;
            let size = o.image_size.ok_or("no image")?;
            if t != size {
                return Err(format!("first raster {t:?} vs logical {size:?}"));
            }
            *f1.lock().unwrap() = (t.0, t.1, size, o.scale);
            Ok(())
        }),
        Step::Do(Action::OriginalSize),
        Step::Do(Action::ZoomIn),
        Step::Do(Action::ZoomIn),
        Step::WaitIdle,
        Step::Sleep(std::time::Duration::from_millis(300)),
        Step::WaitIdle,
        check(move |o| {
            let (w0, _, size, _) = *f2.lock().unwrap();
            let t = o.texture.ok_or("no texture")?;
            let need = (o.scale * size.0.max(size.1) as f32) as u32;
            if o.image_size != Some(size) {
                return Err(format!("logical size changed: {:?} → {:?}", size, o.image_size));
            }
            if t.0 <= w0 || t.0 < need.min(8192) {
                return Err(format!(
                    "no sharper raster: texture {t:?} (first {w0}), view needs {need} at scale {}",
                    o.scale
                ));
            }
            Ok(())
        }),
    ];
    let r = run(Config {
        files: vec![file],
        driver: Some(steps),
        ..Default::default()
    });
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = r {
        eprintln!("e2e_svg FAILED:\n{e:#}");
        std::process::exit(1);
    }
    println!("e2e_svg: OK");
}
