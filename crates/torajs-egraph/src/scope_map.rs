//! Domtree-aware scoped hash map — the core data structure backing
//! GVN / CSE / LICM in Cranelift's aegraph (see `wasmtime/cranelift/
//! codegen/src/scoped_hash_map.rs`).
//!
//! Each entry is associated with a depth in a logical stack of scopes.
//! On `push_scope`, a new layer begins; on `pop_scope`, every entry
//! inserted while that layer was top gets removed. Lookups walk the
//! whole stack newest-first so a value seen in an outer (dominating)
//! scope is reused while still in that scope, then "forgotten" when
//! the scope pops — modelling SSA dominance exactly: a value computed
//! in block B is available to every descendant of B in the dominator
//! tree but invisible to siblings.
//!
//! Implementation choice: single `HashMap<K, Vec<(depth, V)>>` with a
//! current depth counter. `push_scope` is O(1) (depth++); `pop_scope`
//! is O(entries_at_this_depth) via a sidecar `inserted` log. This
//! matches cranelift's design and avoids the O(stack-depth) lookup
//! cost of a layered-HashMap-stack alternative.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::hash::Hash;

/// Scoped hash map. Generic over key and value.
///
/// Invariant: `entries[k]` is a stack of `(depth, value)` entries with
/// strictly increasing depth values (every push at depth N appears
/// after every push at depth < N for the same key, and pops remove
/// the most recent push).
#[derive(Debug)]
pub struct ScopedHashMap<K: Eq + Hash + Clone, V> {
    /// Current scope depth; bumped by `push_scope`, decremented by
    /// `pop_scope`. Starts at 0 (the always-present root scope).
    depth: u32,
    /// For each key, the stack of (scope_depth, value) entries shadowing
    /// older values. Top of the stack is the currently-visible value
    /// (or absent → key has no binding visible from the current scope).
    entries: HashMap<K, Vec<(u32, V)>>,
    /// Per-scope log of keys inserted at that depth. Indexed by scope
    /// depth: `inserted[d]` holds every key whose top-of-stack entry
    /// was created while at depth `d`. `pop_scope` pops the entry for
    /// each such key, then truncates this log.
    inserted: Vec<Vec<K>>,
}

impl<K: Eq + Hash + Clone, V> Default for ScopedHashMap<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Eq + Hash + Clone, V> ScopedHashMap<K, V> {
    /// Build an empty scoped map. The root scope (depth 0) is always
    /// present; no `push_scope` call is required before first insert.
    pub fn new() -> Self {
        Self {
            depth: 0,
            entries: HashMap::new(),
            inserted: vec![Vec::new()],
        }
    }

    /// Current scope depth. 0 means only the root scope is active.
    pub fn depth(&self) -> u32 {
        self.depth
    }

    /// Begin a new scope. Subsequent inserts shadow older bindings;
    /// `pop_scope` removes them, restoring the prior visible value.
    pub fn push_scope(&mut self) {
        self.depth += 1;
        self.inserted.push(Vec::new());
    }

    /// End the current scope, removing every binding inserted while
    /// at this depth. Restores the previous value (from any enclosing
    /// scope) for each key. Panics if called at depth 0 (no scope to
    /// pop — root scope is permanent).
    pub fn pop_scope(&mut self) {
        assert!(self.depth > 0, "cannot pop the root scope");
        let popped = self.inserted.pop().expect("inserted log invariant");
        for key in popped {
            if let Entry::Occupied(mut e) = self.entries.entry(key) {
                let stack = e.get_mut();
                debug_assert_eq!(
                    stack.last().map(|(d, _)| *d),
                    Some(self.depth),
                    "inserted log must match top of entries stack"
                );
                stack.pop();
                if stack.is_empty() {
                    e.remove();
                }
            }
        }
        self.depth -= 1;
    }

    /// Insert (or shadow) a key at the current scope depth. Returns the
    /// previous value visible at the current scope, if any. If the
    /// existing top-of-stack entry was already at the current depth,
    /// it is replaced in-place (no double-stack); otherwise a new
    /// stack frame is pushed.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        let stack = self.entries.entry(key.clone()).or_default();
        if let Some((top_depth, _)) = stack.last()
            && *top_depth == self.depth
        {
            // Replace in place — already shadowed at this depth.
            let prev = stack.pop().map(|(_, v)| v);
            stack.push((self.depth, value));
            return prev;
        }
        // Fresh entry at this depth — push + log so pop_scope can find it.
        stack.push((self.depth, value));
        self.inserted[self.depth as usize].push(key);
        None
    }

    /// Look up the currently-visible value for a key. Walks the stack
    /// for that key once; O(1) amortized because shadowing replaces in
    /// place at the same depth (insert above) — stack length is
    /// bounded by the number of distinct depths at which the key was
    /// written, not by the number of writes.
    pub fn get(&self, key: &K) -> Option<&V> {
        self.entries.get(key).and_then(|s| s.last().map(|(_, v)| v))
    }

    /// True if `key` has a binding visible in the current scope.
    pub fn contains_key(&self, key: &K) -> bool {
        self.get(key).is_some()
    }

    /// Remove every binding and reset to the empty root-scope state.
    /// Useful between functions so the same allocator is reused.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.inserted.clear();
        self.inserted.push(Vec::new());
        self.depth = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_scope_insert_get() {
        let mut m: ScopedHashMap<&str, i32> = ScopedHashMap::new();
        assert_eq!(m.depth(), 0);
        assert_eq!(m.insert("a", 1), None);
        assert_eq!(m.get(&"a"), Some(&1));
        assert_eq!(m.get(&"b"), None);
    }

    #[test]
    fn shadowing_then_pop_restores() {
        let mut m: ScopedHashMap<&str, i32> = ScopedHashMap::new();
        m.insert("a", 1);
        m.push_scope();
        assert_eq!(m.insert("a", 2), None); // first write at new depth
        assert_eq!(m.get(&"a"), Some(&2));
        m.pop_scope();
        assert_eq!(m.get(&"a"), Some(&1));
    }

    #[test]
    fn same_depth_insert_replaces_in_place() {
        let mut m: ScopedHashMap<&str, i32> = ScopedHashMap::new();
        m.insert("a", 1);
        assert_eq!(m.insert("a", 2), Some(1));
        assert_eq!(m.get(&"a"), Some(&2));
        // entries stack must have length 1 → pop_scope at depth 0
        // would remove it cleanly if depth were >0; here we just
        // check that no spurious extra frame piles up.
        let stack = m.entries.get(&"a").unwrap();
        assert_eq!(stack.len(), 1);
    }

    #[test]
    fn sibling_scopes_do_not_leak() {
        let mut m: ScopedHashMap<&str, i32> = ScopedHashMap::new();
        m.insert("root", 0);
        // Enter scope A
        m.push_scope();
        m.insert("a_only", 100);
        assert_eq!(m.get(&"a_only"), Some(&100));
        m.pop_scope();
        // a_only must be gone, root must still be visible
        assert_eq!(m.get(&"a_only"), None);
        assert_eq!(m.get(&"root"), Some(&0));
        // Enter sibling scope B
        m.push_scope();
        assert_eq!(m.get(&"a_only"), None, "sibling must not see A");
        m.insert("b_only", 200);
        assert_eq!(m.get(&"b_only"), Some(&200));
        m.pop_scope();
        assert_eq!(m.get(&"b_only"), None);
    }

    #[test]
    fn nested_scopes_chain() {
        let mut m: ScopedHashMap<&str, i32> = ScopedHashMap::new();
        m.insert("x", 0);
        for d in 1..=4 {
            m.push_scope();
            m.insert("x", d);
            assert_eq!(m.get(&"x"), Some(&d));
        }
        // Unwind
        for d in (0..=3).rev() {
            m.pop_scope();
            assert_eq!(m.get(&"x"), Some(&d));
        }
        assert_eq!(m.depth(), 0);
    }

    #[test]
    fn contains_key_tracks_scope() {
        let mut m: ScopedHashMap<i32, &str> = ScopedHashMap::new();
        m.push_scope();
        m.insert(42, "hello");
        assert!(m.contains_key(&42));
        m.pop_scope();
        assert!(!m.contains_key(&42));
    }

    #[test]
    fn clear_resets_to_root() {
        let mut m: ScopedHashMap<&str, i32> = ScopedHashMap::new();
        m.push_scope();
        m.insert("a", 1);
        m.push_scope();
        m.insert("a", 2);
        m.clear();
        assert_eq!(m.depth(), 0);
        assert_eq!(m.get(&"a"), None);
    }

    #[test]
    #[should_panic(expected = "cannot pop the root scope")]
    fn pop_at_root_panics() {
        let mut m: ScopedHashMap<&str, i32> = ScopedHashMap::new();
        m.pop_scope();
    }
}
