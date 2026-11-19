//! Native E2E through UI Automation: the app runs its normal event loop while a UI
//! Automation client in a second process (this test executable in client mode, driven over
//! stdin/stdout) reads and operates the native UI, the way a screen reader would (patterns only: no
//! keyboard or mouse input is injected). Out of process as Microsoft recommends for UI Automation
//! clients: the app's UI thread is blocked in modal loops (menus, TaskDialog, file dialogs) while the
//! client works.
//! - menu bar (muda): the top-level menus are exposed with their names;
//! - context menu (muda): opened by a right click, its items and their enabled state;
//! - Delete confirmation (TaskDialog): "Do not ask again" ticked, Yes → Recycle Bin, AskDelete off;
//! - Open dialog (rfd / IFileDialog): file name typed through the Value pattern, Open invoked;
//! - Settings window (egui through AccessKit): tabs and buttons exposed, Cancel invoked.
//!
//! Unlike the other E2E tests the windows are not cloaked (UI Automation leaves cloaked windows and
//! everything they own out of its tree): the app, its menus and dialogs appear on screen for a
//! moment. Without an interactive desktop (a service session) the suite prints `SKIPPED`; a failed
//! run is retried once (UI Automation timing on CI runners).

use rview::actions::Action;
use rview::driver::{Observation, Step};
use rview::{Config, run};
use std::io::{BufRead, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use uiautomation::controls::ControlType;
use uiautomation::patterns::{UIInvokePattern, UITogglePattern, UIValuePattern};
use uiautomation::types::{Handle, TreeScope};
use uiautomation::{UIAutomation, UIElement};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::*;

const TIMEOUT: Duration = Duration::from_secs(20);

fn main() {
    if let Some(pid) = std::env::var("RVIEW_UIA_CLIENT").ok().and_then(|p| p.parse().ok()) {
        let open = std::env::var_os("RVIEW_UIA_OPEN").unwrap_or_default();
        return client_process(pid, Path::new(&open));
    }
    if !interactive_desktop() {
        println!("uia_native: SKIPPED: no interactive desktop session (UI Automation needs one)");
        return;
    }
    let first = attempt();
    let result = match first {
        Err(e) if std::env::var_os("RVIEW_UIA_RETRIED").is_none() => {
            println!("uia_native: first run failed, retrying once:\n{e}");
            let ok = std::process::Command::new(std::env::current_exe().unwrap())
                .env("RVIEW_UIA_RETRIED", "1")
                .status()
                .is_ok_and(|s| s.success());
            if ok {
                Ok(())
            } else {
                Err("retry failed too".to_string())
            }
        }
        r => r,
    };
    match result {
        Ok(()) => println!("uia_native: OK"),
        Err(e) => {
            eprintln!("uia_native FAILED:\n{e}");
            std::process::exit(1);
        }
    }
}

/// Phases of the UI Automation thread; the app's script waits for each one to finish.
const MENUBAR: u8 = 1;
const CONTEXT: u8 = 2;
const DELETE: u8 = 3;
const OPEN: u8 = 4;
const SETTINGS: u8 = 5;
const DONE: u8 = 100;

struct Shared {
    /// Phase requested by the script; the client sets it to DONE + phase when finished.
    phase: AtomicU8,
    failures: Mutex<Vec<String>>,
}

impl Shared {
    fn request(self: &Arc<Self>, p: u8) -> Step {
        let s = self.clone();
        Step::Check(Box::new(move |_| {
            s.phase.store(p, Ordering::SeqCst);
            Ok(())
        }))
    }

    fn wait(self: &Arc<Self>, p: u8) -> Step {
        let s = self.clone();
        // longer than any wait of the client, which always answers
        Step::Until(
            Box::new(move || s.phase.load(Ordering::SeqCst) == DONE + p),
            TIMEOUT * 4,
        )
    }

    fn fail(&self, what: String) {
        self.failures.lock().unwrap().push(what);
    }
}

fn check(f: impl Fn(&Observation) -> Result<(), String> + 'static) -> Step {
    Step::Check(Box::new(f))
}

fn attempt() -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!("rview-uia-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (n, c) in [("a", 40u8), ("b", 120), ("c", 200)] {
        image::RgbImage::from_pixel(64, 48, image::Rgb([c, 90, 30]))
            .save(dir.join(format!("{n}.png")))
            .unwrap();
    }
    let shared = Arc::new(Shared {
        phase: AtomicU8::new(0),
        failures: Mutex::new(Vec::new()),
    });
    let client = {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .env("RVIEW_UIA_CLIENT", std::process::id().to_string())
            .env("RVIEW_UIA_OPEN", dir.join("c.png"))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("client process: {e}"))?;
        let s = shared.clone();
        std::thread::spawn(move || bridge(&s, child))
    };
    let a = dir.join("a.png");
    let right = |state| {
        Step::Event(winit::event::WindowEvent::MouseInput {
            // SAFETY: a dummy device id is only compared, never dereferenced (winit's own tests).
            #[allow(unused_unsafe)]
            device_id: unsafe { winit::event::DeviceId::dummy() },
            state,
            button: winit::event::MouseButton::Right,
        })
    };
    let steps = vec![
        Step::WaitIdle,
        shared.request(MENUBAR),
        shared.wait(MENUBAR),
        // the context menu loop runs inside the right-click handler until the client closes it
        Step::MoveTo(Box::new(|o| (o.viewport.0 as f32 / 2.0, o.viewport.1 as f32 / 2.0))),
        shared.request(CONTEXT),
        right(winit::event::ElementState::Pressed),
        right(winit::event::ElementState::Released),
        shared.wait(CONTEXT),
        // Delete asks through the TaskDialog (no auto_confirm in this test)
        shared.request(DELETE),
        Step::Do(Action::Delete),
        shared.wait(DELETE),
        Step::WaitIdle,
        check(move |o| {
            if a.exists() || !o.has_undo || o.settings.ask_delete {
                return Err(format!(
                    "after Delete: a.png exists {}, undo {}, askdelete {}",
                    a.exists(),
                    o.has_undo,
                    o.settings.ask_delete
                ));
            }
            Ok(())
        }),
        Step::Do(Action::Undo),
        Step::WaitIdle,
        check(|o| match &o.path {
            Some(p) if p.ends_with("a.png") && p.exists() => Ok(()),
            p => Err(format!("Undo Delete: showing {p:?}")),
        }),
        // Open dialog
        shared.request(OPEN),
        Step::Do(Action::Open),
        shared.wait(OPEN),
        Step::WaitIdle,
        check(|o| match &o.path {
            Some(p) if p.ends_with("c.png") => Ok(()),
            p => Err(format!(
                "Open dialog: showing {p:?}, windows {:?}, error {:?}, message {:?}",
                o.titles, o.error, o.message
            )),
        }),
        // Settings window through AccessKit
        Step::Do(Action::Options),
        Step::WaitIdle,
        shared.request(SETTINGS),
        shared.wait(SETTINGS),
        Step::WaitIdle,
        check(|o| {
            o.tools
                .is_empty()
                .then_some(())
                .ok_or(format!("Cancel through UI Automation left {:?} open", o.tools))
        }),
    ];
    let r = run(Config {
        files: vec![dir.join("a.png")],
        driver: Some(steps),
        visible: true,
        ..Default::default()
    });
    shared.phase.store(DONE, Ordering::SeqCst);
    let _ = client.join();
    let _ = std::fs::remove_dir_all(&dir);
    let mut failures = shared.failures.lock().unwrap().clone();
    if let Err(e) = r {
        failures.push(format!("{e:#}"));
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}

// ------------------------------------------------------------------ the UI Automation client

/// App side: forwards each requested phase to the client process (one line: its number) and
/// records its answer (`ok` or `err <reason>`). A dead client fails the phase, never hangs it.
fn bridge(s: &Shared, mut child: std::process::Child) {
    let mut input = child.stdin.take().unwrap();
    let mut output = std::io::BufReader::new(child.stdout.take().unwrap());
    loop {
        let p = s.phase.load(Ordering::SeqCst);
        if p == DONE {
            let _ = writeln!(input, "{DONE}");
            let _ = child.wait();
            return;
        }
        // 0 = nothing asked yet, DONE + n = phase n finished: wait for the next request
        if p == 0 || p > DONE {
            std::thread::sleep(Duration::from_millis(20));
            continue;
        }
        let mut line = String::new();
        let sent = writeln!(input, "{p}").and_then(|_| input.flush()).is_ok();
        if !sent || output.read_line(&mut line).unwrap_or(0) == 0 {
            s.fail(format!("phase {p}: the UI Automation client process is gone"));
        } else if let Some(e) = line.trim_end().strip_prefix("err ") {
            s.fail(format!("phase {p}: {e}"));
        }
        s.phase.store(DONE + p, Ordering::SeqCst);
    }
}

/// The app's process id (the client looks only at its windows).
static TARGET: OnceLock<u32> = OnceLock::new();

/// Client mode: run the phases the app asks for, answer each on stdout.
fn client_process(pid: u32, open_path: &Path) {
    TARGET.set(pid).unwrap();
    let uia = UIAutomation::new().map_err(err);
    let mut out = std::io::stdout();
    for line in std::io::stdin().lock().lines() {
        let Ok(p) = line.unwrap_or_default().trim().parse::<u8>() else {
            continue;
        };
        if p == DONE {
            return;
        }
        let r = match &uia {
            Err(e) => Err(format!("UI Automation unavailable: {e}")),
            Ok(uia) => match p {
                MENUBAR => menubar(uia),
                CONTEXT => context_menu(uia),
                DELETE => delete_dialog(uia),
                OPEN => open_dialog(uia, open_path),
                SETTINGS => settings(uia),
                _ => Ok(()),
            },
        };
        let answer = match r {
            Ok(()) => "ok".to_string(),
            Err(e) => {
                close_dialogs();
                format!("err {}", e.replace(['\r', '\n'], " "))
            }
        };
        let _ = writeln!(out, "{answer}").and_then(|_| out.flush());
    }
}

type R = Result<(), String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Top-level windows of the app's process: (hwnd, class, title).
fn windows() -> Vec<(HWND, String, String)> {
    unsafe extern "system" fn each(h: HWND, out: LPARAM) -> windows::core::BOOL {
        // SAFETY: `out` is the Vec passed by `windows()` for the duration of EnumWindows.
        let out = unsafe { &mut *(out.0 as *mut Vec<HWND>) };
        out.push(h);
        true.into()
    }
    let mut all: Vec<HWND> = Vec::new();
    // SAFETY: the callback only pushes into the Vec, which outlives the call.
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut all as *mut _ as isize));
    }
    let me = TARGET.get().copied().unwrap_or(0);
    all.into_iter()
        .filter_map(|h| {
            let mut pid = 0u32;
            let (mut class, mut title) = ([0u16; 256], [0u16; 512]);
            // SAFETY: valid buffers of the given sizes.
            unsafe {
                GetWindowThreadProcessId(h, Some(&mut pid));
                if pid != me {
                    return None;
                }
                let n = GetClassNameW(h, &mut class) as usize;
                let m = GetWindowTextW(h, &mut title) as usize;
                Some((
                    h,
                    String::from_utf16_lossy(&class[..n]),
                    String::from_utf16_lossy(&title[..m]),
                ))
            }
        })
        .collect()
}

/// Wait for a window of this process.
fn wait_window(what: &str, f: impl Fn(&str, &str) -> bool) -> Result<HWND, String> {
    let t = Instant::now();
    while t.elapsed() < TIMEOUT {
        if let Some((h, ..)) = windows().into_iter().find(|(_, c, t)| f(c, t)) {
            return Ok(h);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(format!("no {what} window after {TIMEOUT:?}; windows: {:?}", windows()))
}

fn element(uia: &UIAutomation, h: HWND) -> Result<UIElement, String> {
    uia.element_from_handle(Handle::from(h)).map_err(err)
}

/// Descendants of `root` of a control type, retried until `ok` accepts them (AccessKit and the
/// shell dialogs build their trees asynchronously).
fn find(
    uia: &UIAutomation,
    root: &UIElement,
    ty: Option<ControlType>,
    ok: impl Fn(&[UIElement]) -> bool,
) -> Result<Vec<UIElement>, String> {
    let t = Instant::now();
    let cond = uia.create_true_condition().map_err(err)?;
    loop {
        let all = root.find_all(TreeScope::Descendants, &cond).unwrap_or_default();
        let v: Vec<UIElement> = all
            .into_iter()
            .filter(|e| ty.is_none_or(|ty| e.get_control_type().is_ok_and(|c| c == ty)))
            .collect();
        if ok(&v) || t.elapsed() > TIMEOUT {
            return Ok(v);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn names(v: &[UIElement]) -> Vec<String> {
    v.iter().filter_map(|e| e.get_name().ok()).collect()
}

fn named<'a>(v: &'a [UIElement], name: &str) -> Option<&'a UIElement> {
    v.iter().find(|e| e.get_name().is_ok_and(|n| n == name))
}

fn invoke(e: &UIElement) -> R {
    e.get_pattern::<UIInvokePattern>().map_err(err)?.invoke().map_err(err)
}

fn main_window() -> Result<HWND, String> {
    wait_window("main", |c, t| c == "Window Class" && t.ends_with(".png"))
}

fn menubar(uia: &UIAutomation) -> R {
    let root = element(uia, main_window()?)?;
    let want = ["File", "Edit", "View", "Go", "Markup", "Tools", "Help"];
    let items = find(uia, &root, Some(ControlType::MenuItem), |v| {
        want.iter().all(|w| named(v, w).is_some())
    })?;
    let got = names(&items);
    for w in want {
        let Some(e) = named(&items, w) else {
            return Err(format!("menu bar: no '{w}' in {got:?}"));
        };
        if !e.is_enabled().map_err(err)? {
            return Err(format!("menu bar: '{w}' disabled"));
        }
    }
    eprintln!("UIA menu bar: {got:?}");
    Ok(())
}

fn context_menu(uia: &UIAutomation) -> R {
    let popup = wait_window("context menu", |c, _| c == "#32768")?;
    let root = element(uia, popup)?;
    let want = [
        "Open...",
        "Show in Explorer",
        "Markup",
        "Rename...",
        "Delete",
        "Next File",
        "View",
    ];
    let got: Result<Vec<(String, bool)>, String> = find(uia, &root, Some(ControlType::MenuItem), |v| {
        want.iter().all(|w| names(v).iter().any(|n| n.starts_with(w)))
    })
    .map(|items| {
        items
            .iter()
            .filter_map(|e| Some((e.get_name().ok()?, e.is_enabled().ok()?)))
            .collect()
    });
    // read before closing; then Esc posted to the menu window (not global input)
    // SAFETY: plain message post to a window handle (invalid handles are ignored).
    unsafe {
        let _ = PostMessageW(Some(popup), WM_KEYDOWN, WPARAM(VK_ESCAPE_CODE), LPARAM(0));
    }
    let got = got?;
    for w in want {
        match got.iter().find(|(n, _)| n.starts_with(w)) {
            Some((_, true)) => {}
            Some((n, false)) => return Err(format!("context menu: '{n}' disabled with an image open")),
            None => return Err(format!("context menu: no '{w}' in {got:?}")),
        }
    }
    // no folder history yet: Undo Delete isn't in the context menu, Next File needs a folder
    eprintln!("UIA context menu: {} items, all expected ones enabled", got.len());
    Ok(())
}

const VK_ESCAPE_CODE: usize = 0x1B;

fn delete_dialog(uia: &UIAutomation) -> R {
    let h = wait_window("Delete confirmation", |c, t| c == "#32770" && t == "Delete")?;
    let root = element(uia, h)?;
    let boxes = find(uia, &root, Some(ControlType::CheckBox), |v| !v.is_empty())?;
    let check = named(&boxes, "Do not ask again").ok_or(format!(
        "TaskDialog: no 'Do not ask again' checkbox in {:?}",
        names(&boxes)
    ))?;
    check
        .get_pattern::<UITogglePattern>()
        .map_err(err)?
        .toggle()
        .map_err(err)?;
    let buttons = find(uia, &root, Some(ControlType::Button), |v| named(v, "Yes").is_some())?;
    named(&buttons, "No").ok_or("TaskDialog: no 'No' button")?;
    invoke(named(&buttons, "Yes").ok_or(format!("TaskDialog: no 'Yes' in {:?}", names(&buttons)))?)?;
    eprintln!("UIA TaskDialog: checkbox ticked, Yes invoked");
    Ok(())
}

fn open_dialog(uia: &UIAutomation, path: &Path) -> R {
    // titled with the action's name, "Open..."
    let h = wait_window("Open", |c, t| c == "#32770" && t.starts_with("Open"))?;
    let root = element(uia, h)?;
    // the "File name" box (automation id 1148, an edit inside a combo box)
    let edits = find(uia, &root, Some(ControlType::Edit), |v| {
        v.iter().any(|e| e.get_automation_id().is_ok_and(|id| id == "1148"))
    })?;
    let name = edits
        .iter()
        .find(|e| e.get_automation_id().is_ok_and(|id| id == "1148"))
        .ok_or("Open dialog: no file name box")?;
    name.get_pattern::<UIValuePattern>()
        .map_err(err)?
        .set_value(&path.display().to_string())
        .map_err(err)?;
    // Open: a split button with automation id 1 (the navigation pane has an "Apps" item with the
    // same id)
    let is_open = |e: &UIElement| {
        e.get_automation_id().is_ok_and(|id| id == "1")
            && e.get_control_type()
                .is_ok_and(|c| matches!(c, ControlType::SplitButton | ControlType::Button))
    };
    let buttons = find(uia, &root, None, |v| v.iter().any(is_open))?;
    invoke(
        buttons
            .iter()
            .find(|e| is_open(e))
            .ok_or("Open dialog: no Open button")?,
    )?;
    // the dialog closes once the app has the path
    let t = Instant::now();
    // SAFETY: plain query on a window handle.
    while unsafe { IsWindow(Some(h)) }.as_bool() {
        if t.elapsed() > TIMEOUT {
            return Err("Open dialog: still open after Open was invoked".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    eprintln!("UIA Open dialog: path typed, Open invoked, dialog closed");
    Ok(())
}

fn settings(uia: &UIAutomation) -> R {
    let h = wait_window("Settings", |_, t| t == "Settings")?;
    let root = element(uia, h)?;
    let want = [
        "Window",
        "Image",
        "Miscellaneous",
        "Shortcuts",
        "OK",
        "Cancel",
        "Apply",
        "Restore Defaults",
    ];
    let all = find(uia, &root, None, |v| want.iter().all(|w| named(v, w).is_some()))?;
    if let Some(w) = want.iter().find(|w| named(&all, w).is_none()) {
        return Err(format!("Settings (AccessKit): no '{w}' in {:?}", names(&all)));
    }
    invoke(named(&all, "Cancel").unwrap())?;
    // the click reaches egui on its next frame
    let t = Instant::now();
    while windows().iter().any(|(_, _, title)| title == "Settings") {
        if t.elapsed() > TIMEOUT {
            return Err("Settings: still open after Cancel was invoked".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    eprintln!("UIA Settings: tabs and buttons exposed by AccessKit, Cancel invoked");
    Ok(())
}

/// After a failure: close any dialog of this process so the app's modal loop returns (a menu
/// loop ends on WM_CANCELMODE to its owner).
fn close_dialogs() {
    for (h, class, _) in windows() {
        let msg = match class.as_str() {
            "#32770" => WM_CLOSE,
            "Window Class" => WM_CANCELMODE,
            _ => continue,
        };
        // SAFETY: plain message post to a window of this process.
        unsafe {
            let _ = PostMessageW(Some(h), msg, WPARAM(0), LPARAM(0));
        }
    }
}

/// An interactive desktop receives input (not the case for services and some CI agents).
fn interactive_desktop() -> bool {
    use windows::Win32::System::StationsAndDesktops::{CloseDesktop, DESKTOP_ACCESS_FLAGS, OpenInputDesktop};
    // SAFETY: the handle is closed right away.
    unsafe {
        match OpenInputDesktop(Default::default(), false, DESKTOP_ACCESS_FLAGS(0x0001)) {
            Ok(d) => {
                let _ = CloseDesktop(d);
                true
            }
            Err(_) => false,
        }
    }
}
