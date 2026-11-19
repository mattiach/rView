//! E2E: window geometry is restored from state.toml at start (SetWindowPlacement) and
//! saved at exit (GetWindowPlacement). Uses a temporary state file, never the user's.

use rv_core::state::{Geometry, State};
use rview::actions::Action;
use rview::driver::{Observation, Step};
use rview::{Config, run};

fn main() {
    let dir = std::env::temp_dir().join(format!("rview-e2e-geom-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("state.toml");
    let saved = Geometry {
        left: 211,
        top: 157,
        right: 1111,
        bottom: 807,
        maximized: false,
    };
    rv_core::state::save(
        &file,
        &State {
            geometry: Some(saved),
            ..Default::default()
        },
    )
    .unwrap();

    let steps = vec![
        Step::WaitIdle,
        Step::Check(Box::new(move |o: &Observation| {
            // rcNormalPosition is in workspace coordinates; with the taskbar at the bottom they equal
            // screen coordinates, as here.
            let want = (211, 157, 900, 650);
            (o.outer_rect == Some(want))
                .then_some(())
                .ok_or(format!("restored {:?}, expected {want:?}", o.outer_rect))
        })),
        Step::Do(Action::Quit), // saves the placement
    ];
    let r = run(Config {
        software: true,
        driver: Some(steps),
        state_file: Some(file.clone()),
        ..Default::default()
    });
    let after = rv_core::state::load(&file);
    let _ = std::fs::remove_dir_all(&dir);
    let ok = r.is_ok() && after.geometry == Some(saved);
    if !ok {
        eprintln!("e2e_geometry FAILED:\n{:?}\nstate after exit: {after:?}", r.err());
        std::process::exit(1);
    }
    println!("e2e_geometry: OK");
}
