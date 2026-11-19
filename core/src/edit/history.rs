//! History: a sequence of immutable snapshots — undo/redo move between two stacks, no
//! inverse commands. A gesture (a drag, a slider held) replaces the current state without
//! touching `past` and becomes one entry at its end. Dirty = the current revision is not the one
//! of the last Save Project; Export/Overwrite only record `exported_revision`.

use super::apply::{Edit, EditError};
use super::model::{DocState, Revision};
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Entries kept (oldest dropped first).
pub const MAX_ENTRIES: usize = 500;
/// Estimated bytes kept (oldest dropped first).
pub const MAX_BYTES: usize = 256 << 20;

/// Session-wide revision counter: every new state gets a new number (>= 1).
static NEXT_REVISION: AtomicU64 = AtomicU64::new(1);

fn new_revision() -> Revision {
    Revision(NEXT_REVISION.fetch_add(1, Ordering::Relaxed))
}

fn stamped(mut s: DocState) -> Arc<DocState> {
    s.revision = new_revision();
    Arc::new(s)
}

pub struct History {
    past: VecDeque<Arc<DocState>>,
    current: Arc<DocState>,
    future: Vec<Arc<DocState>>,
    /// State before the running gesture.
    gesture: Option<Arc<DocState>>,
    pub saved_revision: Option<Revision>,
    pub exported_revision: Option<Revision>,
}

impl History {
    /// A new document (entering Markup from an image): clean.
    pub fn new(doc: DocState) -> History {
        let current = stamped(doc);
        History {
            saved_revision: Some(current.revision),
            past: VecDeque::new(),
            current,
            future: Vec::new(),
            gesture: None,
            exported_revision: None,
        }
    }

    /// A document read from a project (`revision` 0 after deserialisation): clean.
    pub fn from_loaded(doc: DocState) -> History {
        History::new(doc)
    }

    pub fn current(&self) -> &Arc<DocState> {
        &self.current
    }

    pub fn is_dirty(&self) -> bool {
        self.saved_revision != Some(self.current.revision)
    }

    /// The image was exported in exactly the current state (prompt text).
    pub fn exported_current(&self) -> bool {
        self.exported_revision == Some(self.current.revision)
    }

    pub fn mark_saved(&mut self) {
        self.saved_revision = Some(self.current.revision);
    }

    pub fn mark_exported(&mut self) {
        self.exported_revision = Some(self.current.revision);
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty() || self.gesture.as_ref().is_some_and(|g| !Arc::ptr_eq(g, &self.current))
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    pub fn entries(&self) -> usize {
        self.past.len()
    }

    /// One edit = one entry.
    pub fn apply(&mut self, e: &Edit) -> Result<(), EditError> {
        self.end_gesture();
        let next = self.current.apply(e)?;
        self.push(stamped(next));
        Ok(())
    }

    fn push(&mut self, next: Arc<DocState>) {
        let prev = std::mem::replace(&mut self.current, next);
        self.past.push_back(prev);
        self.future.clear();
        self.trim();
    }

    pub fn in_gesture(&self) -> bool {
        self.gesture.is_some()
    }

    pub fn begin_gesture(&mut self) {
        self.end_gesture();
        self.gesture = Some(self.current.clone());
    }

    /// The gesture's edit is always relative to the state at its start (a drag sends the total
    /// offset so far). Outside a gesture it starts one.
    pub fn update_gesture(&mut self, e: &Edit) -> Result<(), EditError> {
        if self.gesture.is_none() {
            self.begin_gesture();
        }
        let base = self.gesture.clone().expect("gesture");
        let next = base.apply(e)?;
        self.current = stamped(next);
        Ok(())
    }

    /// One history entry if the gesture changed something.
    pub fn end_gesture(&mut self) {
        let Some(base) = self.gesture.take() else { return };
        if !Arc::ptr_eq(&base, &self.current) {
            self.past.push_back(base);
            self.future.clear();
            self.trim();
        }
    }

    /// Esc during a drag: back to the state before it, no entry.
    pub fn cancel_gesture(&mut self) {
        if let Some(base) = self.gesture.take() {
            self.current = base;
        }
    }

    /// Undo closes a running gesture first.
    pub fn undo(&mut self) -> bool {
        self.end_gesture();
        let Some(prev) = self.past.pop_back() else { return false };
        self.future.push(std::mem::replace(&mut self.current, prev));
        true
    }

    pub fn redo(&mut self) -> bool {
        self.end_gesture();
        let Some(next) = self.future.pop() else { return false };
        self.past.push_back(std::mem::replace(&mut self.current, next));
        true
    }

    /// Rough size: per state its object lists, per object counted once (states share them).
    fn estimated_bytes(&self) -> usize {
        let mut seen = HashSet::new();
        let mut total = 0;
        for s in self.past.iter().chain([&self.current]).chain(self.future.iter()) {
            total += 128 + (s.shapes.len() + s.redactions.len()) * size_of::<usize>();
            for sh in &s.shapes {
                if seen.insert(Arc::as_ptr(sh) as usize) {
                    total += size_of::<super::model::Shape>()
                        + match &sh.geom {
                            super::model::Geom::Pen { points } | super::model::Geom::Highlight { points } => {
                                points.len() * size_of::<super::model::PointF>()
                            }
                            _ => 0,
                        };
                }
            }
            for r in &s.redactions {
                if seen.insert(Arc::as_ptr(r) as usize) {
                    total += size_of::<super::model::Redaction>();
                }
            }
        }
        total
    }

    fn trim(&mut self) {
        while self.past.len() > MAX_ENTRIES {
            self.past.pop_front();
        }
        while self.past.len() > 1 && self.estimated_bytes() > MAX_BYTES {
            self.past.pop_front();
        }
    }
}
