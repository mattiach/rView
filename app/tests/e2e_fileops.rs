//! E2E: file operations in the real app:
//! delete → AfterDelete (move forward) → undo, rename (unicode, collision, cancel), Delete
//! Permanently, File Info following navigation, Open Recent, Copy → Paste of files, Open URL
//! from a local HTTP server (temporary file removed at exit), Open With list.
//! Side effects: the clipboard content is replaced; the deleted file passes through the Recycle
//! Bin and is restored from it.

use rview::actions::Action;
use rview::driver::{Observation, Step};
use rview::{Config, run};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn png(path: &Path, w: u32, h: u32, c: [u8; 3]) {
    image::RgbImage::from_pixel(w, h, image::Rgb(c)).save(path).unwrap();
}

fn name(o: &Observation) -> String {
    o.path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn check(f: impl Fn(&Observation) -> Result<(), String> + 'static) -> Step {
    Step::Check(Box::new(f))
}

fn expect(cond: bool, what: &str, o: &Observation) -> Result<(), String> {
    if cond {
        Ok(())
    } else {
        Err(format!(
            "{what}: path {:?}, folder {:?}, index {:?}, message {:?}, dialog {:?}",
            o.path, o.folder_len, o.index, o.message, o.dialog
        ))
    }
}

/// HTTP server for one image: answers every request with `body` (Content-Type image/png).
fn serve(body: Vec<u8>) -> u16 {
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
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = s.write_all(head.as_bytes());
            let _ = s.write_all(&body);
        }
    });
    port
}

fn main() {
    if std::env::var_os("GITHUB_ACTIONS").is_some() {
        println!("e2e_fileops: SKIPPED: Recycle Bin and clipboard of the GitHub runner are unreliable, run it locally");
        return;
    }
    let _ = env_logger::try_init(); // RUST_LOG=rview::app=trace shows the driver's progress
    let dir = std::env::temp_dir().join(format!("rview-e2e-fileops-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (i, n) in ["a", "b", "c", "d", "e"].iter().enumerate() {
        png(
            &dir.join(format!("{n}.png")),
            40 + i as u32 * 10,
            30,
            [i as u8 * 50, 100, 200],
        );
    }
    let mut served = Vec::new();
    image::RgbImage::from_pixel(17, 13, image::Rgb([9, 99, 199]))
        .write_to(&mut std::io::Cursor::new(&mut served), image::ImageFormat::Png)
        .unwrap();
    let port = serve(served);
    let downloaded: Arc<Mutex<Option<PathBuf>>> = Arc::default();
    let dl = downloaded.clone();
    let d = dir.clone();
    let p = move |n: &str| d.join(n);
    let (pb, pc, pd) = (p("b.png"), p("c.png"), p("d.png"));
    let renamed = p("b è 漢字.png");
    let (r1, r2, r3) = (renamed.clone(), renamed.clone(), renamed.clone());
    let (pb1, pb2, pc1, pd1) = (pb.clone(), pb.clone(), pc.clone(), pd.clone());
    let has_apps = {
        let _com = rview::platform::ComThread::init();
        !rview::platform::open_with_handlers(&pb).is_empty()
    };

    let steps = vec![
        Step::WaitIdle,
        check(|o| {
            expect(
                name(o) == "b.png" && o.folder_len == Some(5) && o.index == Some(1),
                "opened b",
                o,
            )
        }),
        // menu state of the file actions with an image on screen (menus follow after the event batch)
        Step::Sleep(Duration::from_millis(100)),
        check(|o| {
            let on = |k: &str| o.menu.iter().any(|(key, e, _)| *key == k && *e);
            let state: Vec<_> = o
                .menu
                .iter()
                .filter(|(k, _, _)| ["rename", "delete", "copy", "paste", "undo"].contains(k))
                .collect();
            expect(
                on("rename") && on("delete") && on("copy") && on("paste") && !on("undo"),
                &format!("menu enable state {state:?}"),
                o,
            )
        }),
        // --- delete → AfterDelete = move forward → undo ---
        Step::Do(Action::Delete),
        Step::WaitIdle,
        check(move |o| {
            expect(
                name(o) == "c.png" && o.folder_len == Some(4) && o.index == Some(1),
                "moved forward to c",
                o,
            )?;
            expect(!pb1.exists() && o.has_undo, "b in the Recycle Bin, undo available", o)
        }),
        Step::Do(Action::Undo),
        Step::WaitIdle,
        check(move |o| {
            expect(
                pb2.exists() && name(o) == "b.png" && !o.has_undo,
                "b restored and shown",
                o,
            )?;
            expect(o.folder_len == Some(5) && o.index == Some(1), "folder read again", o)
        }),
        // --- rename ---
        Step::Do(Action::Rename),
        check(|o| {
            expect(
                o.dialog == Some(("rename", "b.png".into(), (0, 1))),
                "rename dialog, stem selected",
                o,
            )
        }),
        Step::Do(Action::NextFile), // ignored while the dialog is open
        check(|o| expect(name(o) == "b.png", "modal: navigation ignored", o)),
        Step::DialogText("b è 漢字.png".into()),
        Step::DialogAccept,
        Step::WaitIdle,
        check(move |o| {
            expect(
                o.dialog.is_none() && name(o) == "b è 漢字.png" && r1.exists(),
                "renamed and reopened",
                o,
            )?;
            expect(
                o.title.contains("b è 漢字.png") && o.folder_len == Some(5),
                "title and folder",
                o,
            )
        }),
        Step::Do(Action::Rename),
        Step::DialogText("a.png".into()),
        Step::DialogAccept,
        Step::WaitIdle,
        check(move |o| {
            let m = o.message.clone().unwrap_or_default();
            expect(
                m.contains("already exists") && r2.exists() && name(o) == "b è 漢字.png",
                "collision refused",
                o,
            )
        }),
        Step::Do(Action::Rename),
        Step::DialogCancel,
        check(move |o| expect(o.dialog.is_none() && r3.exists(), "cancel: nothing renamed", o)),
        // --- File Info follows the navigation ---
        Step::Do(Action::ShowFileInfo),
        Step::WaitIdle,
        check(|o| {
            let get = |k: &str| o.info_rows.iter().find(|(n, _)| *n == k).map(|(_, v)| v.clone());
            expect(get("info-name").as_deref() == Some("b è 漢字.png"), "info name", o)?;
            expect(
                get("info-dimensions").is_some_and(|d| d.starts_with("50 x 30")),
                "info dimensions",
                o,
            )?;
            expect(
                get("info-type").as_deref() == Some("PNG image") && get("info-size").is_some(),
                "info type/size",
                o,
            )
        }),
        // the details come from a worker shortly after
        Step::Sleep(std::time::Duration::from_millis(500)),
        Step::WaitIdle,
        check(|o| {
            let get = |k: &str| o.info_rows.iter().find(|(n, _)| *n == k).map(|(_, v)| v.clone());
            expect(
                get("info-channels").is_some()
                    && get("info-unique-colors").is_some()
                    && get("info-png-chunks").is_some_and(|c| c.starts_with("IHDR") && c.ends_with("IEND")),
                "info details read from the file",
                o,
            )
        }),
        Step::Do(Action::NextFile),
        Step::WaitIdle,
        check(|o| {
            let n = o
                .info_rows
                .iter()
                .find(|(n, _)| *n == "info-name")
                .map(|(_, v)| v.clone());
            expect(n == Some(name(o)), "info follows the file", o)
        }),
        Step::Do(Action::ShowFileInfo),
        check(|o| expect(o.info_rows.is_empty(), "info closed", o)),
        // --- Delete Permanently: no undo ---
        Step::Do(Action::DeletePermanent),
        Step::WaitIdle,
        check(move |o| {
            expect(!pc1.exists() && !o.has_undo, "c deleted permanently", o)?;
            expect(name(o) == "d.png" && o.folder_len == Some(4), "moved forward to d", o)
        }),
        // --- Open Recent: newest first, same in the menu ---
        check(|o| {
            expect(
                o.recents.first().map(String::as_str) == Some("d.png"),
                "d is the newest recent",
                o,
            )?;
            expect(!o.recents.iter().any(|r| r == "c.png"), "deleted file pruned", o)?;
            expect(o.recent_menu == o.recents, "menu shows the recents", o)
        }),
        // --- Copy → Paste of files ---
        Step::Do(Action::Copy),
        Step::WaitIdle,
        check(move |o| {
            let files = rview::platform::clipboard_files();
            expect(files == vec![pd1.clone()], "CF_HDROP has the file", o)?;
            let png = rview::platform::clipboard_get(rview::platform::png_format()).unwrap_or_default();
            let img = image::load_from_memory(&png).map_err(|e| e.to_string())?;
            expect((img.width(), img.height()) == (70, 30), "PNG on the clipboard", o)
        }),
        Step::Do(Action::FirstFile),
        Step::WaitIdle,
        Step::Do(Action::Paste),
        Step::WaitIdle,
        check(|o| {
            expect(
                name(o) == "d.png" && o.windows == 1,
                "paste opened the copied file here",
                o,
            )
        }),
        // --- Open URL: local server, the image opens in a new window (no empty one) ---
        Step::Do(Action::OpenUrl),
        Step::DialogText("not a url".into()),
        Step::DialogAccept,
        check(|o| {
            let m = o.message.clone().unwrap_or_default();
            expect(m.contains("not an http"), "invalid URL refused", o)
        }),
        Step::Do(Action::OpenUrl),
        Step::DialogText(format!("http://127.0.0.1:{port}/image?id=7")),
        Step::DialogAccept,
        Step::WaitIdle,
        check(move |o| {
            expect(
                o.windows == 2 && o.image_size == Some((17, 13)),
                "downloaded image in a new window",
                o,
            )?;
            let n = name(o);
            expect(
                n.starts_with("rView.") && n.ends_with(".png"),
                "temp file rView.XXXXXX.png",
                o,
            )?;
            *dl.lock().unwrap() = o.path.clone();
            Ok(())
        }),
        Step::Focus(0),
        // --- Open With: enumerated 250 ms after the file changed ---
        Step::Sleep(Duration::from_millis(800)),
        check(move |o| expect(!has_apps || !o.open_with_menu.is_empty(), "Open With entries", o)),
    ];
    let r = run(Config {
        files: vec![pb.clone()],
        driver: Some(steps),
        auto_confirm: Some(true),
        ..Default::default()
    });
    let temp = downloaded.lock().unwrap().clone();
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = r {
        eprintln!("e2e_fileops FAILED:\n{e:#}");
        std::process::exit(1);
    }
    if let Some(t) = temp
        && t.exists()
    {
        eprintln!("e2e_fileops FAILED: downloaded temp file left behind: {}", t.display());
        std::process::exit(1);
    }
    println!("e2e_fileops: OK");
}
