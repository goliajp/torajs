//! The facts in force while the walk goes down a body.
//!
//! A nested statement list sees a copy of the facts it starts with,
//! changes it, and drops it at its end. Each change is logged instead,
//! and the copy dropped by undoing the log back to where it began.

use super::super::names::HashMap;

use super::Key;

/// By binding: the discriminant fields and the constants they differ
/// from. A binding is present only with at least one entry.
#[derive(Default)]
pub(super) struct Facts<'a> {
    by_name: HashMap<&'a str, Vec<(&'a str, Key<'a>)>>,
    undo: Vec<Undo<'a>>,
}

enum Undo<'a> {
    Pushed(&'a str),
    Removed(&'a str, Vec<(&'a str, Key<'a>)>),
}

impl<'a> Facts<'a> {
    pub(super) fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    pub(super) fn get(&self, x: &str) -> Option<&Vec<(&'a str, Key<'a>)>> {
        self.by_name.get(x)
    }

    pub(super) fn push(&mut self, x: &'a str, d: &'a str, k: Key<'a>) {
        self.by_name.entry(x).or_default().push((d, k));
        self.undo.push(Undo::Pushed(x));
    }

    pub(super) fn remove(&mut self, x: &'a str) {
        if let Some(v) = self.by_name.remove(x) {
            self.undo.push(Undo::Removed(x, v));
        }
    }

    /// Where a copy taken now begins.
    pub(super) fn mark(&self) -> usize {
        self.undo.len()
    }

    /// Drop the copy that began at `mark`.
    pub(super) fn restore(&mut self, mark: usize) {
        let by_name = &mut self.by_name;
        for u in self.undo.drain(mark..).rev() {
            match u {
                Undo::Pushed(x) => {
                    let v = by_name.get_mut(x).expect("a logged push is in place");
                    v.pop();
                    if v.is_empty() {
                        by_name.remove(x);
                    }
                }
                Undo::Removed(x, v) => {
                    by_name.insert(x, v);
                }
            }
        }
    }
}
