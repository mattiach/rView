//! Markup editor core: document model, edits, history, geometry, rendering. No UI.

pub mod adjust;
pub mod apply;
pub mod geom;
pub mod history;
pub mod model;
pub mod picture;
pub mod redact;
pub mod render;
pub mod text;

pub use adjust::Adjustments;
pub use apply::{Edit, EditError};
pub use history::History;
pub use model::*;
/// The app rasterises the preview overlay with the same crate and version.
pub use tiny_skia;
