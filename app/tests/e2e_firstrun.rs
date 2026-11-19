//! E2E: first start → Welcome; closing it records the choice for update notifications
//! and `firstlaunch = false`; with notifications on, the update check reads the latest release
//! (here from a local HTTP server that answers like the GitHub API, never the real one) and a
//! higher version opens the update notice; Download opens the release page.

use rview::driver::{Observation, Step};
use rview::{Config, run};
use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

fn check(f: impl Fn(&Observation) -> Result<(), String> + 'static) -> Step {
    Step::Check(Box::new(f))
}

fn expect(cond: bool, what: &str, o: &Observation) -> Result<(), String> {
    if cond {
        Ok(())
    } else {
        Err(format!(
            "{what}: tools {:?}, status {:?}, url {:?}",
            o.tools, o.update_status, o.opened_url
        ))
    }
}

/// Answers every request with `body` as JSON; counts the requests.
fn serve(body: &'static str, hits: Arc<AtomicUsize>) -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let mut req = Vec::new();
            let mut buf = [0u8; 1024];
            while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                match s.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => req.extend_from_slice(&buf[..n]),
                }
            }
            hits.fetch_add(1, Ordering::SeqCst);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = s.write_all(head.as_bytes());
            let _ = s.write_all(body.as_bytes());
        }
    });
    port
}

fn main() {
    let dir = std::env::temp_dir().join(format!("rview-e2e-firstrun-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    image::RgbImage::from_pixel(30, 20, image::Rgb([1, 2, 3]))
        .save(dir.join("a.png"))
        .unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let release = r#"{"tag_name":"v99.0","body":"Changelog\n- faster\n- nicer","html_url":"https://example.invalid/release/99.0"}"#;
    let port = serve(release, hits.clone());
    let state_file = dir.join("state.toml"); // missing: first start
    let (st1, st2) = (state_file.clone(), state_file.clone());
    let (h1, h2) = (hits.clone(), hits.clone());

    let steps = vec![
        Step::WaitIdle,
        check(move |o| {
            expect(
                o.tools == ["welcome"] && o.first_launch,
                "Welcome at the first start",
                o,
            )?;
            expect(h1.load(Ordering::SeqCst) == 0, "no update check before the choice", o)
        }),
        // the Welcome checkbox starts from the setting (on in this test); OK keeps it
        Step::ToolButton("welcome", "ok"),
        Step::WaitIdle,
        check(move |o| {
            expect(!o.first_launch && o.settings.update_notifications, "choice recorded", o)?;
            let text = std::fs::read_to_string(&st1).unwrap_or_default();
            expect(text.contains("firstlaunch = false"), "firstlaunch saved", o)?;
            expect(h2.load(Ordering::SeqCst) == 1, "one request", o)?;
            expect(o.tools == ["update"], "newer release → update notice", o)?;
            expect(o.update_status == "Version 99.0 is available.", "status", o)
        }),
        check(move |o| {
            let text = std::fs::read_to_string(&st2).unwrap_or_default();
            expect(!text.contains("lastupdatecheck = 0"), "time of the check saved", o)
        }),
        Step::ToolButton("update", "download"),
        check(|o| {
            expect(
                o.opened_url.as_deref() == Some("https://example.invalid/release/99.0"),
                "release page",
                o,
            )?;
            expect(o.tools.is_empty(), "notice closed", o)
        }),
    ];
    let settings = rv_core::settings::Settings {
        update_notifications: true,
        ..Default::default()
    };
    let r = run(Config {
        files: vec![dir.join("a.png")],
        driver: Some(steps),
        settings: Some(settings),
        state_file: Some(state_file),
        update_url: Some(format!("http://127.0.0.1:{port}/repos/x/y/releases/latest")),
        ..Default::default()
    });
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = r {
        eprintln!("e2e_firstrun FAILED:\n{e:#}");
        std::process::exit(1);
    }
    println!("e2e_firstrun: OK");
}
