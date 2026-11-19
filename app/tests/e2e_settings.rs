//! E2E: a setting changed in the Settings window applies to every window,
//! is saved to settings.toml, and a change made to the file from outside is picked up when a
//! window becomes active. Custom shortcuts: recorded in the draft, applied to keys and menus,
//! saved to state.toml. Cancel and Restore Defaults leave the app unchanged.
//! Files live in a temporary folder (settings.toml and state.toml included).

use rv_core::settings::TitleBarMode;
use rview::actions::Action;
use rview::driver::{Observation, Step};
use rview::{Config, run};
use std::time::Duration;

fn check(f: impl Fn(&Observation) -> Result<(), String> + 'static) -> Step {
    Step::Check(Box::new(f))
}

fn expect(cond: bool, what: &str, o: &Observation) -> Result<(), String> {
    if cond {
        Ok(())
    } else {
        Err(format!(
            "{what}: titles {:?}, tools {:?}, current {:?}",
            o.titles, o.tools, o.current
        ))
    }
}

/// Snapshot check: the corner pixel (background) has this colour.
fn corner(rgb: [u8; 3]) -> Step {
    Step::Snapshot(Box::new(move |w, _h, px| {
        let p = &px[..3];
        let ok = p.iter().zip(rgb).all(|(a, b)| a.abs_diff(b) <= 1);
        let _ = w;
        ok.then_some(()).ok_or(format!("background {p:?}, expected {rgb:?}"))
    }))
}

fn main() {
    let dir = std::env::temp_dir().join(format!("rview-e2e-settings-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for n in ["a", "b", "c"] {
        image::RgbImage::from_pixel(40, 20, image::Rgb([10, 200, 90]))
            .save(dir.join(format!("{n}.png")))
            .unwrap();
    }
    let settings_file = dir.join("settings.toml");
    let state_file = dir.join("state.toml");
    // no Welcome window in this test
    std::fs::write(&state_file, "firstlaunch = false\n").unwrap();
    let (sf1, sf2, st1) = (settings_file.clone(), settings_file.clone(), state_file.clone());
    let size: &str = rview::layout::format_bytes(std::fs::metadata(dir.join("a.png")).unwrap().len()).leak();
    let full = move |names: [&str; 2]| names.map(|n| format!("rView - {n}.png - {size}"));

    let steps = vec![
        Step::WaitIdle,
        check(move |o| expect(o.windows == 2 && o.titles == full(["a", "c"]), "two windows", o)),
        corner([0x20, 0x20, 0x20]),
        Step::Do(Action::Options),
        Step::WaitIdle,
        check(|o| expect(o.tools == ["settings"], "settings window open", o)),
        Step::Do(Action::Options), // a second request brings the same window forward
        check(|o| expect(o.tools == ["settings"], "only one settings window", o)),
        // Cancel: the draft is thrown away
        Step::SettingsEdit(Box::new(|s, _| s.title_bar_mode = TitleBarMode::Name)),
        Step::ToolButton("settings", "cancel"),
        check(move |o| {
            expect(
                o.tools.is_empty() && o.titles == full(["a", "c"]),
                "cancel changes nothing",
                o,
            )
        }),
        // Apply in the reopened window: both windows follow
        Step::Do(Action::Options),
        Step::WaitIdle,
        Step::SettingsEdit(Box::new(|s, keys| {
            s.title_bar_mode = TitleBarMode::Name;
            s.bg_color = [200, 30, 40];
            keys.insert("nextfile".into(), vec!["Space".into()]);
        })),
        Step::ToolButton("settings", "apply"),
        Step::WaitIdle,
        check(|o| {
            expect(
                o.titles == ["rView - a.png", "rView - c.png"],
                "title mode in both windows",
                o,
            )
        }),
        check(|o| expect(o.tools == ["settings"], "Apply keeps the window open", o)),
        Step::Focus(0),
        corner([200, 30, 40]),
        Step::Focus(1),
        corner([200, 30, 40]),
        check(move |o| {
            let text = std::fs::read_to_string(&sf1).unwrap_or_default();
            expect(
                text.contains("bgcolor = \"#c81e28\"") && text.contains("titlebarmode = 1"),
                "settings.toml written",
                o,
            )
        }),
        check(move |o| {
            let text = std::fs::read_to_string(&st1).unwrap_or_default();
            expect(
                text.contains("nextfile") && text.contains("Space"),
                "custom shortcut in state.toml",
                o,
            )
        }),
        // the new shortcut works and the menus show it, in every window
        check(|o| {
            let next = o
                .menu
                .iter()
                .find(|(k, _, _)| *k == "nextfile")
                .map(|(_, _, t)| t.clone());
            expect(
                next.as_deref() == Some("&Next File\tSpace"),
                "menu shows the new shortcut",
                o,
            )
        }),
        Step::Focus(0),
        Step::Key("Space".into()),
        Step::WaitIdle,
        check(|o| {
            expect(
                o.path.as_ref().is_some_and(|p| p.ends_with("b.png")),
                "Space = next file",
                o,
            )
        }),
        Step::Key("Right".into()),
        Step::WaitIdle,
        check(|o| {
            expect(
                o.path.as_ref().is_some_and(|p| p.ends_with("b.png")),
                "Right no longer bound",
                o,
            )
        }),
        // OK closes the window
        Step::ToolButton("settings", "ok"),
        check(|o| expect(o.tools.is_empty(), "OK closes settings", o)),
        // settings.toml changed by someone else: picked up when a window becomes active
        Step::Sleep(Duration::from_millis(50)),
        Step::Check(Box::new(move |_| {
            let text = std::fs::read_to_string(&sf2).unwrap_or_default();
            let text = text
                .replace("titlebarmode = 1", "titlebarmode = 3")
                .replace("#c81e28", "#102030");
            std::fs::write(&sf2, text).map_err(|e| e.to_string())
        })),
        Step::Event(winit::event::WindowEvent::Focused(true)),
        Step::WaitIdle,
        check(move |o| {
            expect(o.titles == full(["b", "c"]), "reloaded from the file", o)?;
            expect(o.settings.bg_color == [0x10, 0x20, 0x30], "colour from the file", o)
        }),
        corner([0x10, 0x20, 0x30]),
        // Restore Defaults + Apply: everything back, the custom shortcut removed
        Step::Do(Action::Options),
        Step::WaitIdle,
        Step::ToolButton("settings", "defaults"),
        Step::ToolButton("settings", "ok"),
        Step::WaitIdle,
        check(|o| {
            expect(
                o.settings == rv_core::settings::Settings::default(),
                "defaults applied",
                o,
            )?;
            let next = o
                .menu
                .iter()
                .find(|(k, _, _)| *k == "nextfile")
                .map(|(_, _, t)| t.clone());
            expect(next.as_deref() == Some("&Next File\tRight"), "default shortcut back", o)
        }),
        // Welcome opens as its own window
        Step::Do(Action::Welcome),
        Step::WaitIdle,
        check(|o| expect(o.tools == ["welcome"], "welcome", o)),
        Step::ToolButton("welcome", "ok"),
        check(|o| expect(o.tools.is_empty() && o.language == "en", "welcome closed, English", o)),
    ];
    let r = run(Config {
        files: vec![dir.join("a.png"), dir.join("c.png")],
        driver: Some(steps),
        settings_file: Some(settings_file),
        state_file: Some(state_file),
        auto_confirm: Some(true),
        ..Default::default()
    });
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = r {
        eprintln!("e2e_settings FAILED:\n{e:#}");
        std::process::exit(1);
    }
    println!("e2e_settings: OK");
}
