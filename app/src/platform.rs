//! Windows-only helpers used by the app. Free functions, no abstraction.

use windows::Win32::Foundation::HWND;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

pub fn hwnd(window: &Window) -> Option<HWND> {
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(HWND(h.hwnd.get() as *mut _)),
        _ => None,
    }
}

/// STA on the UI thread (dialogs, shell, clipboard).
pub fn init_com_ui() {
    // SAFETY: first COM call on this thread; failure only means COM is already initialised.
    unsafe {
        let _ =
            windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
    }
}

/// System window background colour (used when BgColorEnabled = false).
pub fn system_window_color() -> [u8; 3] {
    // SAFETY: GetSysColor has no preconditions.
    let c = unsafe { windows::Win32::Graphics::Gdi::GetSysColor(windows::Win32::Graphics::Gdi::COLOR_WINDOW) };
    [(c & 0xFF) as u8, ((c >> 8) & 0xFF) as u8, ((c >> 16) & 0xFF) as u8]
}

/// Double-click interval in milliseconds.
pub fn double_click_ms() -> u128 {
    // SAFETY: no preconditions.
    unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime() as u128 }
}

/// Size of the rectangle, centred on the first click, the second click must fall in (px).
pub fn double_click_rect() -> (f32, f32) {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXDOUBLECLK, SM_CYDOUBLECLK};
    // SAFETY: no preconditions.
    let (x, y) = unsafe { (GetSystemMetrics(SM_CXDOUBLECLK), GetSystemMetrics(SM_CYDOUBLECLK)) };
    (x.max(1) as f32, y.max(1) as f32)
}

/// Class background brush: until the first GPU frame the system paints the window with the
/// viewer background (WM_ERASEBKGND through DefWindowProc), so it can be shown immediately.
pub fn set_background_brush(window: &Window, rgb: [u8; 3]) {
    use windows::Win32::Foundation::COLORREF;
    use windows::Win32::Graphics::Gdi::CreateSolidBrush;
    use windows::Win32::Graphics::Gdi::{DeleteObject, HBRUSH};
    use windows::Win32::UI::WindowsAndMessaging::{GCLP_HBRBACKGROUND, SetClassLongPtrW};
    // The class is shared by all windows: one brush for the process, replaced when the colour
    // changes (Settings).
    static CURRENT: std::sync::Mutex<Option<([u8; 3], isize)>> = std::sync::Mutex::new(None);
    let Some(h) = hwnd(window) else { return };
    let mut cur = CURRENT.lock().unwrap();
    if cur.is_some_and(|(c, _)| c == rgb) {
        return;
    }
    // SAFETY: valid hwnd; the old brush is deleted only after the class no longer uses it.
    unsafe {
        let brush = CreateSolidBrush(COLORREF(
            rgb[0] as u32 | ((rgb[1] as u32) << 8) | ((rgb[2] as u32) << 16),
        ));
        SetClassLongPtrW(h, GCLP_HBRBACKGROUND, brush.0 as isize);
        if let Some((_, old)) = cur.replace((rgb, brush.0 as isize)) {
            let _ = DeleteObject(HBRUSH(old as *mut _).into());
        }
    }
}

/// Hide a window from the screen, taskbar and Alt+Tab while it keeps working as a visible one
/// (painting, input, focus): the E2E tests run this way.
pub fn cloak(window: &Window) {
    use windows::Win32::Graphics::Dwm::{DWMWA_CLOAK, DwmSetWindowAttribute};
    let Some(h) = hwnd(window) else { return };
    let value = windows::core::BOOL::from(true);
    // SAFETY: valid hwnd, pointer to a BOOL of the declared size.
    unsafe {
        let _ = DwmSetWindowAttribute(
            h,
            DWMWA_CLOAK,
            &value as *const _ as *const core::ffi::c_void,
            size_of::<windows::core::BOOL>() as u32,
        );
    }
}

/// Dark/light title bar following the system theme: the documented DWM attribute, set at
/// creation and on every `ThemeChanged` (winit raises it on WM_SETTINGCHANGE).
pub fn set_dark_titlebar(window: &Window, dark: bool) {
    use windows::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
    let Some(h) = hwnd(window) else { return };
    let value = windows::core::BOOL::from(dark);
    // SAFETY: valid hwnd, pointer to a BOOL of the declared size.
    unsafe {
        let _ = DwmSetWindowAttribute(
            h,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &value as *const _ as *const core::ffi::c_void,
            size_of::<windows::core::BOOL>() as u32,
        );
    }
}

/// Move and size the whole window (physical px, frame and menu bar included).
pub fn set_outer_rect(window: &Window, r: &crate::layout::Rect) {
    use windows::Win32::UI::WindowsAndMessaging::{SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos};
    let Some(h) = hwnd(window) else { return };
    // SAFETY: h is a live top-level window owned by this thread.
    unsafe {
        let _ = SetWindowPos(
            h,
            None,
            r.x as i32,
            r.y as i32,
            r.w as i32,
            r.h as i32,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

/// (monitor rectangle, work area) of the monitor the window is on, physical px (`rcWork`).
pub fn monitor_rects(window: &Window) -> Option<(crate::layout::Rect, crate::layout::Rect)> {
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow};
    let h = hwnd(window)?;
    let mut mi = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: valid hwnd; cbSize set as required.
    let ok = unsafe { GetMonitorInfoW(MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST), &mut mi) };
    if !ok.as_bool() {
        return None;
    }
    let r = |r: windows::Win32::Foundation::RECT| crate::layout::Rect::ltrb(r.left, r.top, r.right, r.bottom);
    Some((r(mi.rcMonitor), r(mi.rcWork)))
}

/// `GetWindowPlacement`: normal (restored) rectangle and maximized flag.
pub fn placement(window: &Window) -> Option<rv_core::state::Geometry> {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowPlacement, SW_SHOWMAXIMIZED, WINDOWPLACEMENT};
    let h = hwnd(window)?;
    let mut wp = WINDOWPLACEMENT {
        length: size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    // SAFETY: valid hwnd; length set as required.
    unsafe { GetWindowPlacement(h, &mut wp).ok()? };
    let r = wp.rcNormalPosition;
    Some(rv_core::state::Geometry {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
        maximized: wp.showCmd == SW_SHOWMAXIMIZED.0 as u32,
    })
}

/// `SetWindowPlacement`; Windows moves it back on screen if the monitor is gone.
pub fn set_placement(window: &Window, g: &rv_core::state::Geometry) {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{
        SW_SHOWMAXIMIZED, SW_SHOWNORMAL, SetWindowPlacement, WINDOWPLACEMENT,
    };
    let Some(h) = hwnd(window) else { return };
    let wp = WINDOWPLACEMENT {
        length: size_of::<WINDOWPLACEMENT>() as u32,
        showCmd: if g.maximized {
            SW_SHOWMAXIMIZED.0 as u32
        } else {
            SW_SHOWNORMAL.0 as u32
        },
        rcNormalPosition: RECT {
            left: g.left,
            top: g.top,
            right: g.right,
            bottom: g.bottom,
        },
        ..Default::default()
    };
    // SAFETY: valid hwnd and a fully initialised WINDOWPLACEMENT.
    unsafe {
        let _ = SetWindowPlacement(h, &wp);
    }
}

// ---------------------------------------------------------------------------------------------
// file operations and Windows integrations
// ---------------------------------------------------------------------------------------------

use crate::i18n::{t, tf};
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use windows::core::{HSTRING, PCWSTR, PWSTR};

/// Path for direct Win32 calls: `\\?\` form above MAX_PATH (std does the same internally), so
/// long paths work without the long-path manifest (test executables have none).
fn win_path(p: &Path) -> HSTRING {
    let w: Vec<u16> = p.as_os_str().encode_wide().collect();
    let bs = b'\\' as u16;
    if w.len() < 248 || w.starts_with(&[bs, bs, b'?' as u16]) {
        return HSTRING::from(p.as_os_str());
    }
    let mut v: Vec<u16> = if w.starts_with(&[bs, bs]) {
        r"\\?\UNC".encode_utf16().chain(w[1..].iter().copied()).collect()
    } else {
        r"\\?\".encode_utf16().chain(w.iter().copied()).collect()
    };
    for c in &mut v {
        if *c == b'/' as u16 {
            *c = bs;
        }
    }
    HSTRING::from_wide(&v)
}

/// The same path without the `\?\` (or `\?\UNC\`) prefix: the Recycle Bin records long
/// original paths in that form.
pub fn plain_path(p: &Path) -> PathBuf {
    let w: Vec<u16> = p.as_os_str().encode_wide().collect();
    let unc: Vec<u16> = r"\\?\UNC\".encode_utf16().collect();
    let verbatim: Vec<u16> = r"\\?\".encode_utf16().collect();
    let rest = if w.starts_with(&unc) {
        [r"\\".encode_utf16().collect::<Vec<u16>>(), w[unc.len()..].to_vec()].concat()
    } else if w.starts_with(&verbatim) {
        w[verbatim.len()..].to_vec()
    } else {
        return p.to_path_buf();
    };
    PathBuf::from(OsString::from_wide(&rest))
}

/// Rename without ever replacing an existing file (`MoveFileExW` without
/// `MOVEFILE_REPLACE_EXISTING`; `std::fs::rename` would overwrite). A case-only rename works.
pub fn rename_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use windows::Win32::Storage::FileSystem::{MOVE_FILE_FLAGS, MoveFileExW};
    // SAFETY: valid NUL-terminated paths.
    unsafe { MoveFileExW(&win_path(from), &win_path(to), MOVE_FILE_FLAGS(0)) }
        .map_err(|e| std::io::Error::from_raw_os_error(e.code().0 & 0xFFFF))
}

/// Open a web link in the default browser (`ShellExecuteW` "open").
pub fn open_url(url: &str) {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    // SAFETY: NUL-terminated strings that outlive the call.
    unsafe {
        ShellExecuteW(
            None,
            windows::core::w!("open"),
            &HSTRING::from(url),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

/// The Windows Settings page where the user makes rView their default app, when the installer
/// registered it (per user or for all users).
pub fn default_apps_page() -> Option<String> {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
    [
        (HKEY_CURRENT_USER, "registeredAppUser"),
        (HKEY_LOCAL_MACHINE, "registeredAppMachine"),
    ]
    .into_iter()
    .find(|(root, _)| {
        // SAFETY: constant NUL-terminated names; no data is read back.
        unsafe {
            RegGetValueW(
                *root,
                windows::core::w!(r"Software\RegisteredApplications"),
                windows::core::w!("rView"),
                RRF_RT_REG_SZ,
                None,
                None,
                None,
            )
        }
        .is_ok()
    })
    .map(|(_, query)| format!("ms-settings:defaultapps?{query}=rView"))
}

/// The drive has a Recycle Bin (network shares and some removable drives don't: a "recycle"
/// there would delete permanently).
pub fn has_recycle_bin(path: &Path) -> bool {
    use windows::Win32::UI::Shell::{SHQUERYRBINFO, SHQueryRecycleBinW};
    let root: PathBuf = path.components().take(2).collect();
    let mut info = SHQUERYRBINFO {
        cbSize: size_of::<SHQUERYRBINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: NUL-terminated root path, cbSize set.
    unsafe { SHQueryRecycleBinW(&HSTRING::from(root.as_os_str()), &mut info).is_ok() }
}

/// Move to the Recycle Bin. Ok(Some(id)) = the bin's item, for Undo Delete; Ok(None) =
/// the file is gone but not found in the bin (cannot be undone). Call on a worker thread.
pub fn recycle(path: &Path) -> Result<Option<OsString>, String> {
    // The shell can't record a full path of MAX_PATH (260) characters or more in the bin: Windows
    // 11 25H2 then deletes the file for good and reports success (8.3 names don't help, the shell
    // expands them). Refused, so a "move to the Recycle Bin" never loses a file.
    let full = rv_core::folder::long_path(path);
    if full.as_os_str().encode_wide().count() >= 260 {
        return Err(tf("bin-path-too-long", &[("path", &full.display().to_string())]));
    }
    trash::delete(path).map_err(|e| e.to_string())?;
    if path.exists() {
        return Err(tf("bin-not-moved", &[("path", &path.display().to_string())]));
    }
    // the bin records the long form of the folder: a path given with 8.3 names (RUNNER~1) must be
    // expanded to be found (the folder still exists, only the file is gone)
    let (name, parent) = (path.file_name(), path.parent().map(rv_core::folder::long_path));
    let parent = parent.as_deref();
    let items = trash::os_limited::list().map_err(|e| e.to_string())?;
    Ok(items
        .into_iter()
        .filter(|i| {
            Some(i.name.as_os_str()) == name
                && parent.is_some_and(|p| rv_core::folder::same_path_ordinal(&plain_path(&i.original_parent), p))
        })
        .max_by_key(|i| i.time_deleted)
        .map(|i| i.id))
}

/// Delete Permanently: the file's bytes are overwritten in 3 passes (zeros, ones, random), each
/// flushed to the disk, then the file is deleted. A file with other hard links is only unlinked:
/// its data still belongs to them. SSD wear levelling and compressed or copy-on-write volumes can
/// keep old copies of the blocks elsewhere. Call on a worker thread.
pub fn delete_overwritten(path: &Path) -> Result<(), String> {
    overwrite(path).map_err(|e| e.to_string())?;
    std::fs::remove_file(path).map_err(|e| e.to_string())
}

fn overwrite(path: &Path) -> std::io::Result<()> {
    use std::hash::BuildHasher;
    use std::io::{Seek, Write};
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle};
    let mut f = std::fs::OpenOptions::new().write(true).open(path)?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: open file handle, pointer to a struct of the declared type.
    unsafe { GetFileInformationByHandle(windows::Win32::Foundation::HANDLE(f.as_raw_handle()), &mut info) }
        .map_err(std::io::Error::other)?;
    if info.nNumberOfLinks > 1 {
        return Ok(());
    }
    let len = f.metadata()?.len();
    let mut buf = vec![0u8; 1 << 20];
    let mut seed = std::hash::RandomState::new().hash_one(path) | 1;
    for fill in [Some(0x00), Some(0xFF), None] {
        if let Some(b) = fill {
            buf.fill(b);
        }
        f.rewind()?;
        let mut left = len;
        while left > 0 {
            let n = left.min(buf.len() as u64) as usize;
            if fill.is_none() {
                for chunk in buf[..n].chunks_mut(8) {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    chunk.copy_from_slice(&seed.to_le_bytes()[..chunk.len()]);
                }
            }
            f.write_all(&buf[..n])?;
            left -= n as u64;
        }
        f.sync_all()?;
    }
    Ok(())
}

/// The grab cursor over the title bar too, as over the movable panels (Windows draws grab and
/// grabbing as its move cursor).
pub fn grab_cursor_on_caption(window: &Window) {
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{HTCAPTION, IDC_SIZEALL, LoadCursorW, SetCursor, WM_SETCURSOR};
    unsafe extern "system" fn proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _: usize, _: usize) -> LRESULT {
        // SAFETY: called by the system with this window's message; a shared system cursor.
        unsafe {
            // Opening a menu sends HTCAPTION with no mouse message (high word 0) to reset the
            // cursor to the arrow for the whole menu loop: leave that one to the system.
            if msg == WM_SETCURSOR
                && (lp.0 & 0xFFFF) as u32 == HTCAPTION
                && (lp.0 >> 16) & 0xFFFF != 0
                && let Ok(c) = LoadCursorW(None, IDC_SIZEALL)
            {
                SetCursor(Some(c));
                return LRESULT(1);
            }
            DefSubclassProc(h, msg, wp, lp)
        }
    }
    let Some(h) = hwnd(window) else { return };
    // SAFETY: valid hwnd on its own thread; the procedure is a plain function with no state.
    unsafe {
        let _ = SetWindowSubclass(h, Some(proc), 0x7276_6361, 0);
    }
}

/// Undo Delete: restore the bin item `id` to its original place. Fails (nothing changes)
/// if a file with that name exists again. Call on a worker thread.
pub fn restore(id: &OsStr) -> Result<PathBuf, String> {
    let items = trash::os_limited::list().map_err(|e| e.to_string())?;
    let item = items.into_iter().find(|i| i.id == id).ok_or_else(|| t("bin-gone"))?;
    let path = plain_path(&item.original_path());
    trash::os_limited::restore_all([item]).map_err(|e| match e {
        trash::Error::RestoreCollision { path, .. } => tf("file-exists", &[("path", &path.display().to_string())]),
        e => e.to_string(),
    })?;
    Ok(path)
}

// --- clipboard: own implementation, image + files in one transaction ---

const CF_UNICODETEXT: u32 = 13;
const CF_HDROP: u32 = 15;
pub const CF_DIBV5: u32 = 17;

/// Registered "PNG" clipboard format.
pub fn png_format() -> u32 {
    use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
    // SAFETY: constant NUL-terminated name.
    unsafe { RegisterClipboardFormatW(windows::core::w!("PNG")) }
}

/// The clipboard, open for this scope (retried: another process may hold it briefly).
struct Clipboard;

impl Clipboard {
    fn open(owner: Option<HWND>) -> Result<Clipboard, String> {
        use windows::Win32::System::DataExchange::OpenClipboard;
        for _ in 0..20 {
            // SAFETY: owner is a window of this thread or None.
            if unsafe { OpenClipboard(owner) }.is_ok() {
                return Ok(Clipboard);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Err(t("clipboard-busy"))
    }
}

impl Drop for Clipboard {
    fn drop(&mut self) {
        // SAFETY: opened by this thread.
        unsafe {
            let _ = windows::Win32::System::DataExchange::CloseClipboard();
        }
    }
}

fn global_from(bytes: &[u8]) -> Result<windows::Win32::Foundation::HGLOBAL, String> {
    use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
    // SAFETY: the block is allocated with the requested size and written within it.
    unsafe {
        let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)).map_err(|e| e.message())?;
        let p = GlobalLock(h) as *mut u8;
        if p.is_null() {
            return Err("GlobalLock failed".into());
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
        let _ = GlobalUnlock(h);
        Ok(h)
    }
}

/// `DROPFILES` + NUL-terminated UTF-16 paths + final NUL (CF_HDROP).
pub fn hdrop_bytes(files: &[PathBuf]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&20u32.to_le_bytes()); // pFiles: offset of the list
    out.extend_from_slice(&[0u8; 8]); // pt
    out.extend_from_slice(&0u32.to_le_bytes()); // fNC
    out.extend_from_slice(&1u32.to_le_bytes()); // fWide
    for f in files {
        for c in f.as_os_str().encode_wide().chain([0]) {
            out.extend_from_slice(&c.to_le_bytes());
        }
    }
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

/// CF_DIBV5: 32-bit BGRA bottom-up with an alpha mask, sRGB colour space (`LCS_sRGB`).
pub fn dibv5_bytes(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(124 + rgba.len());
    let mut le = |v: u32| out.extend_from_slice(&v.to_le_bytes());
    le(124); // bV5Size
    le(w); // bV5Width
    le(h); // bV5Height > 0: bottom-up, what most readers expect
    le(1 | (32 << 16)); // planes = 1, bit count = 32
    le(3); // BI_BITFIELDS
    le(w * h * 4); // size image
    le(2835); // 72 dpi
    le(2835);
    le(0); // colours used
    le(0); // important
    le(0x00FF_0000); // red mask
    le(0x0000_FF00); // green
    le(0x0000_00FF); // blue
    le(0xFF00_0000); // alpha
    le(0x7352_4742); // LCS_sRGB ('sRGB')
    for _ in 0..9 {
        le(0); // endpoints
    }
    le(0); // gamma red, green, blue
    le(0);
    le(0);
    le(4); // LCS_GM_IMAGES
    le(0); // profile data
    le(0); // profile size
    le(0); // reserved
    debug_assert_eq!(out.len(), 124);
    for row in rgba.chunks_exact(w as usize * 4).rev() {
        for p in row.chunks_exact(4) {
            out.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
        }
    }
    out
}

/// Copy: PNG + CF_DIBV5 + CF_HDROP in one clipboard transaction.
pub fn clipboard_set(owner: Option<HWND>, png: &[u8], dibv5: &[u8], files: &[PathBuf]) -> Result<(), String> {
    use windows::Win32::Foundation::GlobalFree;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::DataExchange::{EmptyClipboard, SetClipboardData};
    let _c = Clipboard::open(owner)?;
    let hdrop = hdrop_bytes(files);
    // SAFETY: clipboard open by this thread; on success the system owns each handle.
    unsafe {
        EmptyClipboard().map_err(|e| e.message())?;
        for (fmt, data) in [(png_format(), png), (CF_DIBV5, dibv5), (CF_HDROP, &hdrop[..])] {
            let h = global_from(data)?;
            if SetClipboardData(fmt, Some(HANDLE(h.0))).is_err() {
                let _ = GlobalFree(Some(h));
                return Err(tf("clipboard-refused", &[("format", &fmt.to_string())]));
            }
        }
    }
    Ok(())
}

/// Raw bytes of a clipboard format (None if absent).
pub fn clipboard_get(format: u32) -> Option<Vec<u8>> {
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::DataExchange::{GetClipboardData, IsClipboardFormatAvailable};
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    // SAFETY: the handle belongs to the clipboard and is only read while it is open.
    unsafe {
        IsClipboardFormatAvailable(format).ok()?;
        let _c = Clipboard::open(None).ok()?;
        let h = HGLOBAL(GetClipboardData(format).ok()?.0);
        let p = GlobalLock(h) as *const u8;
        if p.is_null() {
            return None;
        }
        let v = std::slice::from_raw_parts(p, GlobalSize(h)).to_vec();
        let _ = GlobalUnlock(h);
        Some(v)
    }
}

/// Files on the clipboard (CF_HDROP, e.g. from Explorer's Copy).
pub fn clipboard_files() -> Vec<PathBuf> {
    let Some(b) = clipboard_get(CF_HDROP) else {
        return vec![];
    };
    if b.len() < 20 {
        return vec![];
    }
    let off = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
    let wide = u32::from_le_bytes([b[16], b[17], b[18], b[19]]) != 0;
    let list = b.get(off..).unwrap_or_default();
    let names: Vec<OsString> = if wide {
        let u: Vec<u16> = list.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        u.split(|c| *c == 0)
            .take_while(|s| !s.is_empty())
            .map(OsString::from_wide)
            .collect()
    } else {
        list.split(|c| *c == 0)
            .take_while(|s| !s.is_empty())
            .map(|s| OsString::from(String::from_utf8_lossy(s).into_owned()))
            .collect()
    };
    names.into_iter().map(PathBuf::from).collect()
}

/// Text on the clipboard (CF_UNICODETEXT).
pub fn clipboard_text() -> Option<String> {
    let b = clipboard_get(CF_UNICODETEXT)?;
    let u: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    let end = u.iter().position(|c| *c == 0).unwrap_or(u.len());
    Some(String::from_utf16_lossy(&u[..end]))
}

// --- shell ---

/// COM for a worker thread that calls the shell (STA), released at the end of the scope.
pub struct ComThread;

impl ComThread {
    pub fn init() -> ComThread {
        init_com_ui();
        ComThread
    }
}

impl Drop for ComThread {
    fn drop(&mut self) {
        // SAFETY: paired with the CoInitializeEx of `init` (also when that returned S_FALSE).
        unsafe { windows::Win32::System::Com::CoUninitialize() }
    }
}

/// Show in Explorer: the folder with the file selected (`SHOpenFolderAndSelectItems`).
pub fn show_in_explorer(path: &Path) -> Result<(), String> {
    use windows::Win32::UI::Shell::{ILCreateFromPathW, ILFree, SHOpenFolderAndSelectItems};
    // SAFETY: the PIDL is created from a NUL-terminated path and freed after use.
    unsafe {
        let pidl = ILCreateFromPathW(&HSTRING::from(path.as_os_str()));
        if pidl.is_null() {
            return Err(tf("file-not-found", &[("path", &path.display().to_string())]));
        }
        let r = SHOpenFolderAndSelectItems(pidl, None, 0);
        ILFree(Some(pidl));
        r.map_err(|e| e.message())
    }
}

/// One "Open With" entry: name shown, and an RGBA icon if the app has one.
#[derive(Clone, Debug)]
pub struct Handler {
    pub ui_name: String,
    pub icon: Option<(u32, u32, Vec<u8>)>,
}

fn take_pwstr(p: PWSTR) -> String {
    // SAFETY: shell-allocated string, freed with CoTaskMemFree after copying.
    unsafe {
        let s = p.to_string().unwrap_or_default();
        windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
        s
    }
}

fn assoc_handlers(ext: &str) -> Vec<windows::Win32::UI::Shell::IAssocHandler> {
    use windows::Win32::UI::Shell::{ASSOC_FILTER_RECOMMENDED, IAssocHandler, SHAssocEnumHandlers};
    let mut out = Vec::new();
    // SAFETY: COM calls on an initialised thread; the out array has one slot.
    unsafe {
        let Ok(list) = SHAssocEnumHandlers(&HSTRING::from(ext), ASSOC_FILTER_RECOMMENDED) else {
            return out;
        };
        loop {
            let mut slot: [Option<IAssocHandler>; 1] = [None];
            let mut n = 0;
            if list.Next(&mut slot, Some(&mut n)).is_err() || n == 0 {
                break;
            }
            out.extend(slot[0].take());
        }
    }
    out
}

/// Friendly name of the default app for an extension (".png").
pub fn default_app_name(ext: &str) -> Option<String> {
    use windows::Win32::UI::Shell::{ASSOCF_NONE, ASSOCSTR_FRIENDLYAPPNAME, AssocQueryStringW};
    let mut buf = [0u16; 512];
    let mut len = buf.len() as u32;
    // SAFETY: buffer and its length in characters.
    unsafe {
        AssocQueryStringW(
            ASSOCF_NONE,
            ASSOCSTR_FRIENDLYAPPNAME,
            &HSTRING::from(ext),
            PCWSTR::null(),
            Some(PWSTR(buf.as_mut_ptr())),
            &mut len,
        )
        .ok()
        .ok()?;
    }
    let end = buf.iter().position(|c| *c == 0).unwrap_or(0);
    Some(String::from_utf16_lossy(&buf[..end]))
}

fn ext_of(path: &Path) -> String {
    format!(".{}", path.extension().map(|e| e.to_string_lossy()).unwrap_or_default())
}

/// Recommended apps for the file type: the default one first, the others in natural order.
/// Call on a worker thread holding a `ComThread`.
pub fn open_with_handlers(path: &Path) -> Vec<Handler> {
    let ext = ext_of(path);
    let default = default_app_name(&ext);
    let mut list: Vec<Handler> = assoc_handlers(&ext)
        .iter()
        .filter_map(|h| {
            // SAFETY: COM calls on a live handler.
            let ui_name = take_pwstr(unsafe { h.GetUIName() }.ok()?);
            let mut file = PWSTR::null();
            let mut index = 0;
            // SAFETY: as above; out parameters are local.
            let icon = unsafe { h.GetIconLocation(&mut file, &mut index) }
                .ok()
                .and_then(|_| icon_rgba(&take_pwstr(file), index));
            Some(Handler { ui_name, icon })
        })
        .collect();
    list.sort_by(|a, b| {
        let (da, db) = (
            Some(&a.ui_name) == default.as_ref(),
            Some(&b.ui_name) == default.as_ref(),
        );
        db.cmp(&da)
            .then_with(|| rv_core::folder::natural_cmp(OsStr::new(&a.ui_name), OsStr::new(&b.ui_name)))
    });
    list.dedup_by(|a, b| a.ui_name == b.ui_name);
    list
}

/// Open the file with the recommended app called `ui_name` (`IAssocHandler::Invoke`). Worker
/// thread holding a `ComThread`.
pub fn open_with(path: &Path, ui_name: &str) -> Result<(), String> {
    use windows::Win32::System::Com::IDataObject;
    use windows::Win32::UI::Shell::{BHID_DataObject, IShellItem, SHCreateItemFromParsingName};
    let h = assoc_handlers(&ext_of(path))
        .into_iter()
        // SAFETY: COM call on a live handler.
        .find(|h| unsafe { h.GetUIName() }.ok().map(take_pwstr).as_deref() == Some(ui_name))
        .ok_or_else(|| tf("openwith-gone", &[("app", ui_name)]))?;
    // SAFETY: COM calls with a NUL-terminated path.
    unsafe {
        let item: IShellItem =
            SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None).map_err(|e| e.message())?;
        let data: IDataObject = item.BindToHandler(None, &BHID_DataObject).map_err(|e| e.message())?;
        h.Invoke(&data).map_err(|e| e.message())
    }
}

/// "Choose another app": the system Open With dialog, then the file opens (`SHOpenWithDialog`).
pub fn open_with_dialog(owner: Option<HWND>, path: &Path) {
    before_modal(owner);
    use windows::Win32::UI::Shell::{OAIF_EXEC, OPENASINFO, SHOpenWithDialog};
    let file = HSTRING::from(path.as_os_str());
    let info = OPENASINFO {
        pcszFile: PCWSTR(file.as_ptr()),
        pcszClass: PCWSTR::null(),
        oaifInFlags: OAIF_EXEC,
    };
    // SAFETY: the strings outlive the modal call.
    unsafe {
        let _ = SHOpenWithDialog(owner, &info);
    }
}

/// Small icon (system small-icon size) of `file,index` as straight RGBA.
fn icon_rgba(file: &str, index: i32) -> Option<(u32, u32, Vec<u8>)> {
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::UI::WindowsAndMessaging::{
        DestroyIcon, GetIconInfo, GetSystemMetrics, HICON, ICONINFO, PrivateExtractIconsW, SM_CXSMICON,
    };
    let mut name = [0u16; 260];
    let w: Vec<u16> = file.encode_utf16().collect();
    if w.is_empty() || w.len() >= name.len() {
        return None;
    }
    name[..w.len()].copy_from_slice(&w);
    // SAFETY: GDI calls on handles created here and released before returning.
    unsafe {
        let size = GetSystemMetrics(SM_CXSMICON).max(16);
        let mut icon = [HICON::default()];
        if PrivateExtractIconsW(&name, index, size, size, Some(&mut icon), None, 0) == 0 || icon[0].is_invalid() {
            return None;
        }
        let mut ii = ICONINFO::default();
        let mut out = None;
        if GetIconInfo(icon[0], &mut ii).is_ok() {
            let dc = CreateCompatibleDC(None);
            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: size,
                    biHeight: -size,
                    biPlanes: 1,
                    biBitCount: 32,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut px = vec![0u8; (size * size * 4) as usize];
            let rows = GetDIBits(
                dc,
                ii.hbmColor,
                0,
                size as u32,
                Some(px.as_mut_ptr() as *mut _),
                &mut bmi,
                DIB_RGB_COLORS,
            );
            if rows == size {
                let no_alpha = px.chunks_exact(4).all(|p| p[3] == 0);
                for p in px.chunks_exact_mut(4) {
                    p.swap(0, 2);
                    if no_alpha {
                        p[3] = 255;
                    }
                }
                out = Some((size as u32, size as u32, px));
            }
            let _ = DeleteDC(dc);
            let _ = DeleteObject(ii.hbmColor.into());
            let _ = DeleteObject(ii.hbmMask.into());
        }
        let _ = DestroyIcon(icon[0]);
        out
    }
}

// --- native dialogs ---

/// Call before any modal dialog owned by a window: a pending repaint of the owner would be sent
/// again and again during the dialog's loop (winit can't paint while its handler waits for the
/// dialog), the queue never gets empty and Windows never shows the dialog — the app hangs.
pub fn before_modal(owner: Option<HWND>) {
    if let Some(h) = owner {
        use windows::Win32::Graphics::Gdi::{RDW_NOINTERNALPAINT, RDW_VALIDATE, RedrawWindow};
        // SAFETY: a window of this thread. Validate the whole client area and drop the "internal
        // paint" flag winit's request_redraw sets (ValidateRect alone keeps it).
        unsafe {
            let _ = RedrawWindow(Some(h), None, None, RDW_VALIDATE | RDW_NOINTERNALPAINT);
        }
    }
}

/// Yes/No TaskDialog with an optional verification checkbox.
/// Returns (yes, checkbox ticked). Loaded at run time: `TaskDialogIndirect` exists only in
/// Common Controls 6, which a process without the app manifest (tests) doesn't get; there a plain
/// message box is used, without the checkbox.
pub fn confirm(
    owner: Option<HWND>,
    title: &str,
    instruction: &str,
    content: &str,
    check: Option<&str>,
) -> (bool, bool) {
    use windows::Win32::Foundation::HINSTANCE;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows::Win32::UI::Controls::*;
    use windows::Win32::UI::WindowsAndMessaging::IDYES;
    use windows::core::HRESULT;
    type Indirect =
        unsafe extern "system" fn(*const TASKDIALOGCONFIG, *mut i32, *mut i32, *mut windows::core::BOOL) -> HRESULT;
    // SAFETY: the function pointer has TaskDialogIndirect's documented signature; all strings
    // outlive the modal call.
    unsafe {
        let f = LoadLibraryW(windows::core::w!("comctl32.dll"))
            .ok()
            .and_then(|m| GetProcAddress(m, windows::core::s!("TaskDialogIndirect")));
        let Some(f) = f else {
            before_modal(owner);
            let yes = rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Warning)
                .set_title(title)
                .set_description(format!("{instruction}\n\n{content}"))
                .set_buttons(rfd::MessageButtons::YesNo)
                .show()
                == rfd::MessageDialogResult::Yes;
            return (yes, false);
        };
        let f: Indirect = std::mem::transmute(f);
        before_modal(owner);
        let (t, i, c) = (HSTRING::from(title), HSTRING::from(instruction), HSTRING::from(content));
        let v = check.map(HSTRING::from);
        let cfg = TASKDIALOGCONFIG {
            cbSize: size_of::<TASKDIALOGCONFIG>() as u32,
            hwndParent: owner.unwrap_or_default(),
            hInstance: HINSTANCE::default(),
            dwFlags: TDF_ALLOW_DIALOG_CANCELLATION | TDF_POSITION_RELATIVE_TO_WINDOW,
            dwCommonButtons: TDCBF_YES_BUTTON | TDCBF_NO_BUTTON,
            pszWindowTitle: PCWSTR(t.as_ptr()),
            Anonymous1: TASKDIALOGCONFIG_0 {
                pszMainIcon: TD_WARNING_ICON,
            },
            pszMainInstruction: PCWSTR(i.as_ptr()),
            pszContent: PCWSTR(c.as_ptr()),
            pszVerificationText: v.as_ref().map_or(PCWSTR::null(), |v| PCWSTR(v.as_ptr())),
            nDefaultButton: IDYES.0,
            ..Default::default()
        };
        let (mut button, mut checked) = (0i32, windows::core::BOOL(0));
        if f(&cfg, &mut button, std::ptr::null_mut(), &mut checked).is_err() {
            return (false, false);
        }
        (button == IDYES.0, checked.as_bool())
    }
}

/// TaskDialog with custom buttons (the Markup "unsaved changes" prompt). Returns the index
/// of the button pressed, None for Cancel / Esc / the window's close box. Without Common Controls 6
/// (test executables) a message box with OK = first button, Cancel = None.
pub fn ask(owner: Option<HWND>, title: &str, instruction: &str, content: &str, buttons: &[String]) -> Option<usize> {
    use windows::Win32::Foundation::HINSTANCE;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows::Win32::UI::Controls::*;
    use windows::core::HRESULT;
    type Indirect =
        unsafe extern "system" fn(*const TASKDIALOGCONFIG, *mut i32, *mut i32, *mut windows::core::BOOL) -> HRESULT;
    // SAFETY: the function pointer has TaskDialogIndirect's documented signature; all strings and
    // the button array outlive the modal call.
    unsafe {
        let f = LoadLibraryW(windows::core::w!("comctl32.dll"))
            .ok()
            .and_then(|m| GetProcAddress(m, windows::core::s!("TaskDialogIndirect")));
        let Some(f) = f else {
            before_modal(owner);
            let ok = rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Warning)
                .set_title(title)
                .set_description(format!("{instruction}\n\n{content}"))
                .set_buttons(rfd::MessageButtons::OkCancel)
                .show()
                == rfd::MessageDialogResult::Ok;
            return ok.then_some(0);
        };
        let f: Indirect = std::mem::transmute(f);
        before_modal(owner);
        let (t, i, c) = (HSTRING::from(title), HSTRING::from(instruction), HSTRING::from(content));
        let labels: Vec<HSTRING> = buttons.iter().map(|b| HSTRING::from(b.as_str())).collect();
        let list: Vec<TASKDIALOG_BUTTON> = labels
            .iter()
            .enumerate()
            .map(|(k, l)| TASKDIALOG_BUTTON {
                nButtonID: 100 + k as i32,
                pszButtonText: PCWSTR(l.as_ptr()),
            })
            .collect();
        let cfg = TASKDIALOGCONFIG {
            cbSize: size_of::<TASKDIALOGCONFIG>() as u32,
            hwndParent: owner.unwrap_or_default(),
            hInstance: HINSTANCE::default(),
            dwFlags: TDF_ALLOW_DIALOG_CANCELLATION | TDF_POSITION_RELATIVE_TO_WINDOW,
            dwCommonButtons: TDCBF_CANCEL_BUTTON,
            pszWindowTitle: PCWSTR(t.as_ptr()),
            Anonymous1: TASKDIALOGCONFIG_0 {
                pszMainIcon: TD_WARNING_ICON,
            },
            pszMainInstruction: PCWSTR(i.as_ptr()),
            pszContent: PCWSTR(c.as_ptr()),
            cButtons: list.len() as u32,
            pButtons: list.as_ptr(),
            nDefaultButton: 100,
            ..Default::default()
        };
        let mut pressed = 0i32;
        if f(&cfg, &mut pressed, std::ptr::null_mut(), std::ptr::null_mut()).is_err() {
            return None;
        }
        (pressed >= 100 && ((pressed - 100) as usize) < buttons.len()).then(|| (pressed - 100) as usize)
    }
}

// --- locale formatting (Info) and UI language ---

/// The user's display languages in order of preference ("it-IT", "en-US"), from
/// `GetUserPreferredUILanguages`.
pub fn preferred_ui_languages() -> Vec<String> {
    use windows::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};
    let (mut n, mut len) = (0u32, 0u32);
    // SAFETY: first call asks for the buffer size, the second fills a buffer of that size.
    unsafe {
        if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut n, None, &mut len).is_err() || len == 0 {
            return vec![];
        }
        let mut buf = vec![0u16; len as usize];
        if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut n, Some(PWSTR(buf.as_mut_ptr())), &mut len).is_err() {
            return vec![];
        }
        buf.split(|c| *c == 0)
            .filter(|s| !s.is_empty())
            .map(String::from_utf16_lossy)
            .collect()
    }
}

fn locale_info(lctype: u32) -> String {
    use windows::Win32::Globalization::GetLocaleInfoEx;
    let mut buf = [0u16; 16];
    // SAFETY: buffer with its length; a null locale name = the user default.
    let n = unsafe { GetLocaleInfoEx(PCWSTR::null(), lctype, Some(&mut buf)) };
    String::from_utf16_lossy(&buf[..(n.max(1) - 1) as usize])
}

/// (decimal separator, thousands separator, digit grouping such as "3;0" or "3;2;0") of the
/// user's locale.
pub fn number_format() -> (String, String, String) {
    use windows::Win32::Globalization::{LOCALE_SDECIMAL, LOCALE_SGROUPING, LOCALE_STHOUSAND};
    (
        locale_info(LOCALE_SDECIMAL),
        locale_info(LOCALE_STHOUSAND),
        locale_info(LOCALE_SGROUPING),
    )
}

/// Date and time in the user's short formats, local time zone.
pub fn local_date_time(t: std::time::SystemTime) -> String {
    use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows::Win32::Globalization::{DATE_SHORTDATE, GetDateFormatEx, GetTimeFormatEx, TIME_FORMAT_FLAGS};
    use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    let since_1601 = t
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() / 100 + 116_444_736_000_000_000)
        .unwrap_or(0) as u64;
    let ft = FILETIME {
        dwLowDateTime: since_1601 as u32,
        dwHighDateTime: (since_1601 >> 32) as u32,
    };
    let (mut utc, mut local) = (SYSTEMTIME::default(), SYSTEMTIME::default());
    let (mut d, mut h) = ([0u16; 64], [0u16; 64]);
    // SAFETY: plain conversions into local buffers.
    unsafe {
        if FileTimeToSystemTime(&ft, &mut utc).is_err()
            || SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).is_err()
        {
            return String::new();
        }
        let nd = GetDateFormatEx(
            PCWSTR::null(),
            DATE_SHORTDATE,
            Some(&local),
            PCWSTR::null(),
            Some(&mut d),
            PCWSTR::null(),
        );
        let nh = GetTimeFormatEx(
            PCWSTR::null(),
            TIME_FORMAT_FLAGS(0),
            Some(&local),
            PCWSTR::null(),
            Some(&mut h),
        );
        format!(
            "{} {}",
            String::from_utf16_lossy(&d[..(nd.max(1) - 1) as usize]),
            String::from_utf16_lossy(&h[..(nh.max(1) - 1) as usize])
        )
    }
}

/// Gives a window the exe's icon resource (`assets/icon/rview.ico`, embedded by build.rs):
/// title bar at the small icon size, taskbar and Alt+Tab at the large one.
pub fn with_app_icon(attrs: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSMICON};
    use winit::platform::windows::{IconExtWindows, WindowAttributesExtWindows};
    use winit::window::Icon;
    // SAFETY: GetSystemMetrics has no preconditions.
    let small = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16) as u32;
    attrs
        .with_window_icon(Icon::from_resource(1, Some(winit::dpi::PhysicalSize::new(small, small))).ok())
        .with_taskbar_icon(Icon::from_resource(1, None).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_overwritten_wipes_the_bytes_but_not_other_hard_links() {
        let dir = std::env::temp_dir().join(format!("rview-shred-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let secret: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8 | 1).collect();
        let a = dir.join("a.bin");
        std::fs::write(&a, &secret).unwrap();
        overwrite(&a).unwrap();
        let after = std::fs::read(&a).unwrap();
        assert_eq!(after.len(), secret.len());
        assert!(after.iter().zip(&secret).filter(|(x, y)| x == y).count() < secret.len() / 100);
        delete_overwritten(&a).unwrap();
        assert!(!a.exists());

        let (b, c) = (dir.join("b.bin"), dir.join("c.bin"));
        std::fs::write(&b, &secret).unwrap();
        std::fs::hard_link(&b, &c).unwrap();
        delete_overwritten(&b).unwrap();
        assert!(!b.exists());
        assert_eq!(std::fs::read(&c).unwrap(), secret);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
