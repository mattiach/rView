//! E2E 2: fast navigation, 200 × Next → the last requested image is on screen and no
//! obsolete result was ever shown; a file deleted during navigation is skipped; First/Last/Random.

use rview::actions::Action;
use rview::driver::{Observation, Step};
use rview::{Config, run};
use std::path::PathBuf;
use std::time::Duration;

const N: u32 = 23; // 200 % 23 = 16: the end position differs from the start

fn size(i: u32) -> (u32, u32) {
    (300 + i * 7, 200 + i * 3)
}

fn check(f: impl Fn(&Observation) -> Result<(), String> + 'static) -> Step {
    Step::Check(Box::new(f))
}

fn expect_file(dir: PathBuf, i: u32, index: usize, len: usize) -> Step {
    check(move |o| {
        let want = dir.join(format!("f{i:02}.png"));
        let ok = o.path.as_ref() == Some(&want)
            && o.index == Some(index)
            && o.folder_len == Some(len)
            && o.image_size == Some(size(i))
            && o.shown_generation == o.generation
            && o.stale_shown == 0
            && o.error.is_none();
        ok.then_some(())
            .ok_or(format!("expected f{i:02} at index {index} of {len}: {o:?}"))
    })
}

fn main() {
    let dir = std::env::temp_dir().join(format!("rview-e2e-nav-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for i in 0..N {
        let (w, h) = size(i);
        image::RgbaImage::from_fn(w, h, |x, y| image::Rgba([(x + i) as u8, y as u8, (i * 10) as u8, 255]))
            .save(dir.join(format!("f{i:02}.png")))
            .unwrap();
    }

    let mut steps = vec![Step::WaitIdle, expect_file(dir.clone(), 0, 0, N as usize)];
    for _ in 0..200 {
        steps.push(Step::Do(Action::NextFile));
        steps.push(Step::Sleep(Duration::from_millis(1)));
    }
    steps.push(Step::WaitIdle);
    steps.push(check(|o| {
        (o.generation == 201)
            .then_some(())
            .ok_or(format!("generation {}", o.generation))
    }));
    steps.push(expect_file(dir.clone(), 16, 16, N as usize));
    // f17 disappears from disk: Next skips it (the list is not rescanned within 3 s).
    let gone = dir.join("f17.png");
    steps.push(check(move |_| std::fs::remove_file(&gone).map_err(|e| e.to_string())));
    steps.push(Step::Do(Action::NextFile));
    steps.push(Step::WaitIdle);
    steps.push(expect_file(dir.clone(), 18, 17, N as usize - 1));
    steps.push(Step::Do(Action::PreviousFile));
    steps.push(Step::WaitIdle);
    steps.push(expect_file(dir.clone(), 16, 16, N as usize - 1));
    steps.push(Step::Do(Action::LastFile));
    steps.push(Step::WaitIdle);
    steps.push(expect_file(dir.clone(), N - 1, N as usize - 2, N as usize - 1));
    steps.push(Step::Do(Action::FirstFile));
    steps.push(Step::WaitIdle);
    steps.push(expect_file(dir.clone(), 0, 0, N as usize - 1));
    steps.push(Step::Do(Action::RandomFile));
    steps.push(Step::WaitIdle);
    steps.push(check(|o| match o.index {
        Some(i) if i != 0 && o.error.is_none() && o.stale_shown == 0 => Ok(()),
        _ => Err(format!("random: {o:?}")),
    }));

    let r = run(Config {
        files: vec![dir.join("f00.png")],
        software: true,
        driver: Some(steps),
        ..Default::default()
    });
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = r {
        eprintln!("e2e_navigate FAILED:\n{e:#}");
        std::process::exit(1);
    }
    println!("e2e_navigate: OK");
}
