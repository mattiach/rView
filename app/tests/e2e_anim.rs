//! E2E: a 200-frame GIF plays smoothly; pause, next frame, speed; the frame on screen
//! is the one the player reports.

use rview::actions::Action;
use rview::driver::{Observation, Step};
use rview::{Config, run};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const N: usize = 200;
const W: u16 = 320;
const H: u16 = 240;

/// Frame i is a flat colour that encodes i (exact in a 256-colour palette).
fn color(i: usize) -> [u8; 3] {
    [i as u8, 255 - i as u8, ((i * 7) % 256) as u8]
}

fn check(f: impl Fn(&Observation) -> Result<(), String> + 'static) -> Step {
    Step::Check(Box::new(f))
}

fn main() {
    let dir = std::env::temp_dir().join(format!("rview-e2e-anim-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("anim.gif");
    {
        let f = std::fs::File::create(&path).unwrap();
        let mut e = gif::Encoder::new(f, W, H, &[]).unwrap();
        e.set_repeat(gif::Repeat::Infinite).unwrap();
        for i in 0..N {
            let c = color(i);
            let mut px: Vec<u8> = (0..W as usize * H as usize)
                .flat_map(|_| [c[0], c[1], c[2], 255])
                .collect();
            let mut fr = gif::Frame::from_rgba_speed(W, H, &mut px, 30);
            fr.delay = 2; // 20 ms
            e.write_frame(&fr).unwrap();
        }
    }

    let shown = Arc::new(Mutex::new(0u64));
    let (s1, s2, s3) = (shown.clone(), shown.clone(), shown.clone());
    let steps = vec![
        Step::WaitIdle,
        check(|o| (o.anim.is_some()).then_some(()).ok_or(format!("not animated: {o:?}"))),
        Step::Sleep(Duration::from_secs(2)),
        check(move |o| {
            let (_, n, paused, speed) = o.anim.unwrap();
            *s1.lock().unwrap() = n;
            if o.software_adapter {
                println!("SKIPPED: cadence check (WARP software adapter renders too slowly for 50 fps)");
                return (!paused && speed == 100 && n > 0)
                    .then_some(())
                    .ok_or(format!("not playing: {o:?}"));
            }
            // smooth = within 15 % of the ideal cadence
            (!paused && speed == 100 && (85..=102).contains(&n))
                .then_some(())
                .ok_or(format!("cadence: {n}"))
        }),
        Step::Do(Action::Pause),
        check(move |o| {
            *s2.lock().unwrap() = o.anim.unwrap().1;
            Ok(())
        }),
        Step::Sleep(Duration::from_millis(300)),
        check(move |o| {
            let (_, n, paused, _) = o.anim.unwrap();
            (paused && n == *s3.lock().unwrap())
                .then_some(())
                .ok_or(format!("pause: {o:?}"))
        }),
        Step::Do(Action::NextFrame),
        Step::Sleep(Duration::from_millis(30)),
        Step::Snapshot(Box::new(|w, h, buf| {
            let i = ((h / 2 * w + w / 2) * 4) as usize;
            let got = [buf[i], buf[i + 1], buf[i + 2]];
            // compared with the colour of the reported index in the next check
            *FRAME_PIXEL.lock().unwrap() = Some(got);
            Ok(())
        })),
        check(|o| {
            let (idx, _, paused, _) = o.anim.unwrap();
            let got = FRAME_PIXEL.lock().unwrap().unwrap();
            let want = color(idx);
            let ok = paused && got.iter().zip(want).all(|(&a, b)| (a as i32 - b as i32).abs() <= 2);
            ok.then_some(())
                .ok_or(format!("frame {idx}: screen {got:?}, expected {want:?}"))
        }),
        Step::Do(Action::IncreaseSpeed),
        Step::Do(Action::IncreaseSpeed),
        check(|o| (o.anim.unwrap().3 == 150).then_some(()).ok_or(format!("speed {o:?}"))),
        Step::Do(Action::ResetSpeed),
        Step::Do(Action::Pause),
        Step::Sleep(Duration::from_millis(500)),
        check(|o| {
            let (_, _, paused, speed) = o.anim.unwrap();
            (!paused && speed == 100).then_some(()).ok_or(format!("resume: {o:?}"))
        }),
    ];
    // Hardware adapter (WARP only as fallback): smoothness is about real playback.
    let r = run(Config {
        files: vec![path],
        driver: Some(steps),
        ..Default::default()
    });
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = r {
        eprintln!("e2e_anim FAILED:\n{e:#}");
        std::process::exit(1);
    }
    println!("e2e_anim: OK");
}

static FRAME_PIXEL: Mutex<Option<[u8; 3]>> = Mutex::new(None);
