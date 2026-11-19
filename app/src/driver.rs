//! Internal E2E driver: a script of steps executed inside the real event loop.

use crate::actions::Action;
use std::collections::VecDeque;
use std::path::PathBuf;

/// Observable state of the active window (plus the titles of all windows).
#[derive(Clone, Debug)]
pub struct Observation {
    pub path: Option<PathBuf>,
    pub image_size: Option<(u32, u32)>,
    pub error: Option<String>,
    pub title: String,
    pub scale: f32,
    pub viewport: (u32, u32),
    pub windows: usize,
    pub index: Option<usize>,
    pub folder_len: Option<usize>,
    pub generation: u64,
    pub shown_generation: u64,
    pub stale_shown: usize,
    /// Request → first image on screen (preview or full), and → full image.
    pub first_latency: Option<std::time::Duration>,
    pub full_latency: Option<std::time::Duration>,
    pub showing_preview: bool,
    /// ICC description / "sRGB (assumed)" of the image on screen (Info).
    pub profile: Option<String>,
    /// Animation: (frame index, frames shown since start, paused, speed %).
    pub anim: Option<(usize, u64, bool, u32)>,
    /// Rendering on WARP (no hardware adapter): timing checks don't apply.
    pub software_adapter: bool,
    /// Titles of all windows, creation order.
    pub titles: Vec<String>,
    /// Index (creation order) of the active window.
    pub current: Option<usize>,
    pub slideshow: bool,
    pub fullscreen: bool,
    pub inner_size: Option<(u32, u32)>,
    /// Outer window rectangle (x, y, w, h), physical px.
    pub outer_rect: Option<(i32, i32, u32, u32)>,
    /// View pan offset (screen px) and scale of the active window.
    pub pan: (f32, f32),
    pub images_shown: u64,
    /// (action key, enabled, text) of the active window's menu items.
    pub menu: Vec<(&'static str, bool, String)>,
    /// Open modal dialog of the active window: (kind, text, preselected chars).
    pub dialog: Option<(&'static str, String, (usize, usize))>,
    /// Last message the app would have shown in a message box (errors of file operations).
    pub message: Option<String>,
    pub has_undo: bool,
    /// File Info rows of the active window, if its Info window is open.
    pub info_rows: Vec<(&'static str, String)>,
    /// Open Recent names (state) and as shown in the active window's menu bar.
    pub recents: Vec<String>,
    pub recent_menu: Vec<String>,
    /// Open With names in the active window's menu bar.
    pub open_with_menu: Vec<String>,
    /// Kinds of the open tool windows ("settings", "about", "welcome", "update").
    pub tools: Vec<&'static str>,
    /// Color Picker: its colour and whether clicks on the image pick.
    pub color: Option<([u8; 3], bool)>,
    /// Color Picker history (state), most recent first.
    pub recent_colors: Vec<[u8; 3]>,
    /// Settings in use by the app.
    pub settings: rv_core::settings::Settings,
    /// UI language in use.
    pub language: String,
    /// Last link the app would have opened in the browser.
    pub opened_url: Option<String>,
    pub first_launch: bool,
    /// The active window's image → screen transform (pointer events at image points).
    pub screen_from_image: [f32; 6],
    pub markup: Option<MarkupObs>,
    /// Texels of the image texture on screen (an SVG gets more when zoomed in).
    pub texture: Option<(u32, u32)>,
}

/// Markup state of the active window.
#[derive(Clone, Debug)]
pub struct MarkupObs {
    pub tool: &'static str,
    pub selection: Vec<u64>,
    pub doc: std::sync::Arc<rv_core::edit::DocState>,
    pub dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub crop_draft: Option<rv_core::edit::RectF>,
    /// The text box being edited inline.
    pub editing: Option<u64>,
    /// A point in the title bar of the Inspector panel (physical px).
    pub inspector: Option<(f32, f32)>,
    /// The document's source once decoded.
    pub source: Option<rv_core::export::DocSource>,
    /// Where Save Project writes.
    pub project_path: Option<PathBuf>,
    /// The watermark menu's recent images and whether each file is there.
    pub watermarks: Vec<(PathBuf, bool)>,
    /// Before/after: the divider's place, None = off.
    pub compare: Option<f32>,
    /// The current state was exported (Export / Overwrite Original).
    pub exported: bool,
}

pub type Check = Box<dyn Fn(&Observation) -> Result<(), String>>;
/// (width, height, RGBA8 pixels) of the window content without UI.
pub type SnapshotCheck = Box<dyn Fn(u32, u32, &[u8]) -> Result<(), String>>;

pub enum Step {
    Do(Action),
    /// Wait until no window is loading or scanning and every window has presented a frame.
    WaitIdle,
    /// Let the event loop run for this long (results arrive in between).
    Sleep(std::time::Duration),
    /// Let the event loop run until the condition holds (set by another thread, e.g. a UI
    /// Automation client); a failure after the timeout.
    Until(Box<dyn Fn() -> bool>, std::time::Duration),
    Check(Check),
    /// Make the i-th window (creation order) the active one, as a click on it would.
    Focus(usize),
    /// Press a key sequence ("Esc", "Alt+Enter") on the active window.
    Key(String),
    /// Open a file in the active window as the Open dialog or drag&drop would (manual open).
    Open(PathBuf),
    /// Feed a winit event to the active window through the normal event handler (input tests).
    Event(winit::event::WindowEvent),
    /// Move the pointer to this window position (physical px) computed from the observation.
    MoveTo(PointAt),
    Snapshot(SnapshotCheck),
    /// Type into the open dialog's text field (replaces its text).
    DialogText(String),
    /// Press OK / Cancel in the open dialog.
    DialogAccept,
    DialogCancel,
    /// Press a button of a tool window by kind ("settings", "welcome", …) and name ("ok", "apply",
    /// "cancel", "defaults", "close", "download").
    ToolButton(&'static str, &'static str),
    /// Change the draft of the open Settings window: (settings, shortcuts by action key).
    SettingsEdit(SettingsEdit),
    /// Move the pointer over this image point (image px) of the active window, as a real
    /// `CursorMoved` at the matching screen position.
    MoveToImage(f32, f32),
    /// Press a Markup toolbar/Inspector button by name ("tool-rect", "undo", "crop-apply"…).
    MarkupButton(&'static str),
    /// The next Save dialog (Save Project, Export) answers this path instead of opening.
    SavePath(PathBuf),
    /// The next unsaved-changes prompt answers this button (0 Save Project, 1 Discard, 2 Export…;
    /// None = Cancel) instead of opening; its text goes to `Observation::message`.
    Answer(Option<usize>),
    /// Type into the text box being edited (as the keyboard would; winit key events can't be built
    /// in tests).
    TypeText(String),
    /// Change the mode/shape of the selected redactions (or of the next one) as the Inspector does.
    MarkupRedact(RedactEdit),
    /// Change the style of the Markup selection as the Inspector does.
    MarkupStyle(Box<dyn Fn(&mut rv_core::edit::Style)>),
}

/// Changes a redaction's (mode, is an ellipse).
pub type RedactEdit = Box<dyn Fn(&mut rv_core::edit::RedactKind, &mut bool)>;

/// A window position (physical px) computed from the observation.
pub type PointAt = Box<dyn Fn(&Observation) -> (f32, f32)>;

pub type SettingsEdit =
    Box<dyn Fn(&mut rv_core::settings::Settings, &mut std::collections::BTreeMap<String, Vec<String>>)>;

#[derive(Default)]
pub struct Driver {
    pub steps: VecDeque<Step>,
    pub failures: Vec<String>,
    pub resume_at: Option<std::time::Instant>,
    pub until_started: Option<std::time::Instant>,
}

impl Driver {
    pub fn new(steps: Vec<Step>) -> Driver {
        Driver {
            steps: steps.into(),
            ..Default::default()
        }
    }
}
