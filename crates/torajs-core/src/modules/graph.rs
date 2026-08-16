//! The module graph the resolver builds as it walks.
//!
//! `modules.rs` answers "what does each module contribute"; this file
//! answers the one question that has to be settled before those
//! contributions can be spliced back together — in what order do the
//! modules evaluate (ES §16.2.1.5).

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;

use super::WorkItem;

/// Which module requested which, in source order — enough to replay
/// §16.2.1.5's rule that a module body runs only after every module it
/// requests has run.
///
/// The resolver drains its worklist breadth-first, so the order it
/// walks modules in is the opposite of the order they must evaluate
/// in. Rather than reshape the walk, each module's injected statements
/// are held aside and reassembled here in post-order.
#[derive(Default)]
pub(super) struct ModuleGraph {
    /// Entry-file requests, in the order their clauses appear.
    pub(super) roots: Vec<PathBuf>,
    /// Requester → requested, in clause order, first walk wins.
    edges: HashMap<PathBuf, Vec<PathBuf>>,
    /// Insertion order of `edges`, so a module the walk reached but
    /// the entry file cannot (only possible through a request the
    /// filter deduplicated away) still gets emitted.
    walked: Vec<PathBuf>,
}

impl ModuleGraph {
    /// The paths a walk just pushed onto the worklist. Everything past
    /// `mark` was appended by the walk that ran after the mark was
    /// taken, which is exactly that module's request list — so no
    /// caller has to thread its own identity down into the helpers.
    pub(super) fn edges_since(&self, work: &VecDeque<WorkItem>, mark: usize) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = Vec::new();
        for (path, ..) in work.iter().skip(mark) {
            if !out.contains(path) {
                out.push(path.clone());
            }
        }
        out
    }

    pub(super) fn record(&mut self, path: PathBuf, deps: Vec<PathBuf>) {
        if !self.edges.contains_key(&path) {
            self.walked.push(path.clone());
        }
        // A second walk of the same module asks for names the first did
        // not, and can therefore reach modules the first never named.
        let slot = self.edges.entry(path).or_default();
        for d in deps {
            if !slot.contains(&d) {
                slot.push(d);
            }
        }
    }

    /// Depth-first post-order from the entry's requests: dependencies
    /// first, each module once. A cycle stops at the member already on
    /// the stack — the spec does the same, evaluating whichever member
    /// the cycle was entered through only once.
    pub(super) fn evaluation_order(&self) -> Vec<PathBuf> {
        let mut order: Vec<PathBuf> = Vec::new();
        let mut seen: HashSet<PathBuf> = HashSet::new();
        for root in self.roots.iter().chain(self.walked.iter()) {
            self.visit(root, &mut seen, &mut order);
        }
        order
    }

    fn visit(&self, node: &PathBuf, seen: &mut HashSet<PathBuf>, order: &mut Vec<PathBuf>) {
        if !seen.insert(node.clone()) {
            return;
        }
        if let Some(deps) = self.edges.get(node) {
            for d in deps {
                self.visit(d, seen, order);
            }
        }
        order.push(node.clone());
    }
}
