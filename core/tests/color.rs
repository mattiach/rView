//! Colour management (ICC and alpha). Expected values are computed with the
//! standard matrices in f64 and compared to qcms (±1).

mod common;
use common::*;
use rv_core::color::{ViewSpace, ViewTransform, icc_description, into_view};
use rv_core::decode::{self, DecodeOptions};
use rv_core::loader::{ImageInfo, Loader, Options};

type M = [[f64; 3]; 3];
const P3_TO_SRGB: M = [
    [1.2249401762805598, -0.22494017628055996, 0.0],
    [-0.04205695470968816, 1.0420569547096882, 0.0],
    [-0.019637554590334432, -0.07863604555063188, 1.0982736001409663],
];
const SRGB_TO_P3: M = [
    [0.8224621209, 0.1775378791, 0.0],
    [0.0331941989, 0.9668058011, 0.0],
    [0.0170826307, 0.0723974407, 0.9105199286],
];
const ADOBE_TO_SRGB: M = [
    [1.39835574, -0.39835574, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, -0.04292898, 1.04292898],
];

fn expected(rgb: [u8; 3], dec: impl Fn(f64) -> f64, m: M) -> [u8; 3] {
    let lin = rgb.map(|c| dec(c as f64 / 255.0));
    let mut out = [0u8; 3];
    for (i, row) in m.iter().enumerate() {
        let v = row[0] * lin[0] + row[1] * lin[1] + row[2] * lin[2];
        out[i] = (srgb_enc(v.clamp(0.0, 1.0)) * 255.0).round() as u8;
    }
    out
}

fn convert(icc: Option<&[u8]>, space: ViewSpace, rgb: [u8; 3]) -> [u8; 3] {
    let mut px = [rgb[0], rgb[1], rgb[2], 255];
    ViewTransform::new(icc, space).apply(&mut px);
    [px[0], px[1], px[2]]
}

fn assert_close(got: [u8; 3], want: [u8; 3], tol: i32, what: &str) {
    assert!(close(&got, &want, tol), "{what}: got {got:?}, want {want:?}");
}

// In-gamut samples (no clipping in either space).
const SAMPLES: [[u8; 3]; 6] = [
    [180, 150, 130],
    [90, 120, 160],
    [200, 200, 200],
    [60, 60, 60],
    [128, 100, 90],
    [120, 160, 110],
];

#[test]
fn p3_to_srgb_known_pixels() {
    let icc = icc_rgb(P3_D50[0], P3_D50[1], P3_D50[2], curv_srgb());
    for s in SAMPLES {
        assert_close(
            convert(Some(&icc), ViewSpace::Srgb, s),
            expected(s, srgb_dec, P3_TO_SRGB),
            1,
            &format!("P3 {s:?}"),
        );
    }
}

#[test]
fn adobe_rgb_to_srgb_known_pixels() {
    let gamma = 563.0 / 256.0;
    let icc = icc_rgb(ADOBE_D50[0], ADOBE_D50[1], ADOBE_D50[2], curv_gamma(gamma));
    for s in SAMPLES {
        let want = expected(s, |e| e.powf(gamma), ADOBE_TO_SRGB);
        assert_close(
            convert(Some(&icc), ViewSpace::Srgb, s),
            want,
            1,
            &format!("Adobe {s:?}"),
        );
    }
}

#[test]
fn gray_profiles_are_converted() {
    for gamma in [1.0, 1.8, 2.2] {
        let icc = icc_gray(curv_gamma(gamma));
        for g in [0u8, 40, 128, 200, 255] {
            let want = (srgb_enc((g as f64 / 255.0).powf(gamma)) * 255.0).round() as u8;
            assert_close(
                convert(Some(&icc), ViewSpace::Srgb, [g; 3]),
                [want; 3],
                1,
                &format!("gray γ{gamma} {g}"),
            );
        }
    }
}

#[test]
fn srgb_identity_and_fallbacks() {
    for s in SAMPLES {
        // no profile: untouched, bit-exact
        assert_eq!(convert(None, ViewSpace::Srgb, s), s);
        // invalid profile → treated as sRGB
        assert_eq!(convert(Some(b"not an icc profile at all"), ViewSpace::Srgb, s), s);
        // Raw view space: never converted, whatever the profile says
        let p3 = icc_rgb(P3_D50[0], P3_D50[1], P3_D50[2], curv_srgb());
        assert_eq!(convert(Some(&p3), ViewSpace::Raw, s), s);
    }
    let sys = std::path::Path::new(r"C:\Windows\System32\spool\drivers\color\sRGB Color Space Profile.icm");
    match std::fs::read(sys) {
        Ok(icc) => {
            for s in SAMPLES {
                assert_close(convert(Some(&icc), ViewSpace::Srgb, s), s, 1, "Windows sRGB profile");
            }
        }
        Err(_) => println!("SKIPPED: Windows sRGB profile not found"),
    }
}

#[test]
fn display_p3_view_space() {
    for s in SAMPLES {
        assert_close(
            convert(None, ViewSpace::DisplayP3, s),
            expected(s, srgb_dec, SRGB_TO_P3),
            1,
            &format!("→P3 {s:?}"),
        );
    }
    // P3 source shown in a P3 view: (near) identity
    let p3 = icc_rgb(P3_D50[0], P3_D50[1], P3_D50[2], curv_srgb());
    for s in SAMPLES {
        assert_close(convert(Some(&p3), ViewSpace::DisplayP3, s), s, 1, "P3→P3");
    }
}

#[test]
fn whole_pipeline_and_info() {
    let px = image::RgbaImage::from_pixel(2, 2, image::Rgba([180, 150, 130, 255]));
    let icc = icc_rgb(P3_D50[0], P3_D50[1], P3_D50[2], curv_srgb());
    let d = decode::decode(
        &encode(&px, Enc::Png, Some(icc.clone()), None),
        None,
        &DecodeOptions::default(),
    )
    .unwrap();
    let info = ImageInfo::of(&d);
    assert_eq!(
        (info.profile.as_str(), info.profile_assumed, info.hdr),
        ("test", false, false)
    );
    let view = into_view(d, ViewSpace::Srgb);
    assert_close(
        [view[0], view[1], view[2]],
        expected([180, 150, 130], srgb_dec, P3_TO_SRGB),
        1,
        "pipeline",
    );
    assert_eq!(icc_description(&icc).as_deref(), Some("test"));

    let d = decode::decode(&encode(&px, Enc::Png, None, None), None, &DecodeOptions::default()).unwrap();
    assert_eq!(ImageInfo::of(&d).profile, "sRGB (assumed)");
}

#[test]
fn alpha_premultiplied_in_linear_light_after_conversion() {
    // 50 % alpha: the colour is converted first (straight), then premultiplied in linear light.
    let icc = icc_rgb(P3_D50[0], P3_D50[1], P3_D50[2], curv_srgb());
    let px = image::RgbaImage::from_pixel(1, 1, image::Rgba([180, 150, 130, 128]));
    let d = decode::decode(&encode(&px, Enc::Png, Some(icc), None), None, &DecodeOptions::default()).unwrap();
    let v = into_view(d, ViewSpace::Srgb);
    let straight = expected([180, 150, 130], srgb_dec, P3_TO_SRGB);
    for c in 0..3 {
        let want = (srgb_enc(srgb_dec(straight[c] as f64 / 255.0) * 128.0 / 255.0) * 255.0).round() as i32;
        assert!((v[c] as i32 - want).abs() <= 1, "channel {c}: {} vs {want}", v[c]);
    }
    assert_eq!(v[3], 128);
    // fully transparent pixels carry no colour (no halo when filtered)
    let t = image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 255, 0, 0]));
    let d = decode::decode(&encode(&t, Enc::Png, None, None), None, &DecodeOptions::default()).unwrap();
    assert_eq!(into_view(d, ViewSpace::Srgb), vec![0, 0, 0, 0]);
}

#[test]
fn cache_key_depends_on_view_space() {
    let dir = std::env::temp_dir().join(format!("rview-color-cache-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("a.png");
    image::RgbaImage::from_pixel(8, 8, image::Rgba([1, 2, 3, 255]))
        .save(&p)
        .unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let tx = std::sync::Mutex::new(tx);
    let l = Loader::new(2, 64 << 20, Options::default(), move |o| {
        let _ = tx.lock().unwrap().send(o);
    });
    l.request(1, 1, p.clone(), false, 2048, vec![]);
    rx.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
    while l.pending() > 0 {
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(l.is_cached(&p));
    l.set_options(Options {
        view_space: ViewSpace::DisplayP3,
        ..Default::default()
    });
    assert!(!l.is_cached(&p), "another view space must not hit the sRGB entry");
    l.set_options(Options::default());
    assert!(l.is_cached(&p), "the sRGB entry is still there");
    let _ = std::fs::remove_dir_all(&dir);
}
