//! E2E: first start → Welcome; closing it saves `firstlaunch = false`.

use rview::driver::{Observation, Step};
use rview::{Config, run};

fn check(f: impl Fn(&Observation) -> Result<(), String> + 'static) -> Step {
    Step::Check(Box::new(f))
}

fn expect(cond: bool, what: &str, o: &Observation) -> Result<(), String> {
    if cond {
        Ok(())
    } else {
        Err(format!("{what}: tools {:?}, first launch {}", o.tools, o.first_launch))
    }
}

fn main() {
    let dir = std::env::temp_dir().join(format!("rview-e2e-firstrun-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    image::RgbImage::from_pixel(30, 20, image::Rgb([1, 2, 3]))
        .save(dir.join("a.png"))
        .unwrap();
    let state_file = dir.join("state.toml"); // missing: first start
    let st = state_file.clone();

    let steps = vec![
        Step::WaitIdle,
        check(|o| {
            expect(
                o.tools == ["welcome"] && o.first_launch,
                "Welcome at the first start",
                o,
            )
        }),
        Step::ToolButton("welcome", "ok"),
        Step::WaitIdle,
        check(move |o| {
            expect(!o.first_launch && o.tools.is_empty(), "Welcome closed", o)?;
            let text = std::fs::read_to_string(&st).unwrap_or_default();
            expect(text.contains("firstlaunch = false"), "firstlaunch saved", o)
        }),
    ];
    let r = run(Config {
        files: vec![dir.join("a.png")],
        driver: Some(steps),
        state_file: Some(state_file),
        ..Default::default()
    });
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = r {
        eprintln!("e2e_firstrun FAILED:\n{e:#}");
        std::process::exit(1);
    }
    println!("e2e_firstrun: OK");
}
