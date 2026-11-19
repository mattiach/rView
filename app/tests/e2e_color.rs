//! E2E: colour and alpha as they reach the screen.
//! 1. Downscaled stripes of opaque red / fully transparent "green garbage": filtering must not leak
//!    the hidden green (premultiplied in linear light) → no halo.
//! 2. 50 % white over the background: linear-light composition, exact value.
//! 3. P3-tagged image: converted to the sRGB view space (provisional Auto), Info shows the profile.

#[path = "../../core/tests/common/mod.rs"]
mod common;

use rview::actions::Action;
use rview::driver::{Observation, Step};
use rview::{Config, run};

const BG: f64 = 0x21 as f64 / 255.0;

fn srgb_dec(e: f64) -> f64 {
    common::srgb_dec(e)
}

fn to8(l: f64) -> i32 {
    (common::srgb_enc(l.clamp(0.0, 1.0)) * 255.0).round() as i32
}

fn center(w: u32, h: u32, buf: &[u8]) -> [i32; 3] {
    let i = ((h / 2 * w + w / 2) * 4) as usize;
    [buf[i] as i32, buf[i + 1] as i32, buf[i + 2] as i32]
}

fn near(got: [i32; 3], want: [i32; 3], tol: i32) -> bool {
    got.iter().zip(want).all(|(a, b)| (a - b).abs() <= tol)
}

fn main() {
    let dir = std::env::temp_dir().join(format!("rview-e2e-color-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let stripes = image::RgbaImage::from_fn(1600, 1600, |x, _| {
        image::Rgba(if x % 2 == 0 { [255, 0, 0, 255] } else { [0, 255, 0, 0] })
    });
    stripes.save(dir.join("a_stripes.png")).unwrap();
    image::RgbaImage::from_pixel(64, 64, image::Rgba([255, 255, 255, 128]))
        .save(dir.join("b_half.png"))
        .unwrap();
    let p3 = common::icc_rgb(
        common::P3_D50[0],
        common::P3_D50[1],
        common::P3_D50[2],
        common::curv_srgb(),
    );
    let px = image::RgbaImage::from_pixel(64, 64, image::Rgba([180, 150, 130, 255]));
    std::fs::write(
        dir.join("c_p3.png"),
        common::encode(&px, common::Enc::Png, Some(p3), None),
    )
    .unwrap();

    let bg_lin = srgb_dec(BG);
    let steps = vec![
        Step::WaitIdle,
        Step::Snapshot(Box::new(move |w, h, buf| {
            // central 200×200 region (inside the downscaled image)
            let (mut max_g, mut max_b, mut max_r) = (0u8, 0u8, 0u8);
            for y in h / 2 - 100..h / 2 + 100 {
                for x in w / 2 - 100..w / 2 + 100 {
                    let i = ((y * w + x) * 4) as usize;
                    max_r = max_r.max(buf[i]);
                    max_g = max_g.max(buf[i + 1]);
                    max_b = max_b.max(buf[i + 2]);
                }
            }
            let bg8 = to8(bg_lin) as u8;
            // G/B can only come from the background (≤ bg); straight-alpha filtering would leak green.
            (max_g <= bg8 + 2 && max_b <= bg8 + 2 && max_r > 150)
                .then_some(())
                .ok_or(format!("halo: max G {max_g}, max B {max_b}, max R {max_r}"))
        })),
        Step::Do(Action::NextFile),
        Step::WaitIdle,
        Step::Snapshot(Box::new(move |w, h, buf| {
            let a = 128.0 / 255.0;
            let stored = (common::srgb_enc(a) * 255.0).round() / 255.0; // white premultiplied, 8 bit
            let out = srgb_dec(stored) + bg_lin * (1.0 - a);
            let want = [to8(out); 3];
            let got = center(w, h, buf);
            near(got, want, 2)
                .then_some(())
                .ok_or(format!("alpha: {got:?} vs {want:?}"))
        })),
        Step::Do(Action::NextFile),
        Step::WaitIdle,
        Step::Check(Box::new(|o: &Observation| {
            (o.profile.as_deref() == Some("test"))
                .then_some(())
                .ok_or(format!("profile: {o:?}"))
        })),
        Step::Snapshot(Box::new(|w, h, buf| {
            // P3 → sRGB with the standard matrix
            const M: [[f64; 3]; 3] = [
                [1.2249401762805598, -0.22494017628055996, 0.0],
                [-0.04205695470968816, 1.0420569547096882, 0.0],
                [-0.019637554590334432, -0.07863604555063188, 1.0982736001409663],
            ];
            let lin = [180.0, 150.0, 130.0].map(|c: f64| srgb_dec(c / 255.0));
            let want = M.map(|r| to8(r[0] * lin[0] + r[1] * lin[1] + r[2] * lin[2]));
            let got = center(w, h, buf);
            near(got, want, 2)
                .then_some(())
                .ok_or(format!("P3: {got:?} vs {want:?}"))
        })),
    ];
    let r = run(Config {
        files: vec![dir.join("a_stripes.png")],
        software: true,
        driver: Some(steps),
        ..Default::default()
    });
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = r {
        eprintln!("e2e_color FAILED:\n{e:#}");
        std::process::exit(1);
    }
    println!("e2e_color: OK");
}
