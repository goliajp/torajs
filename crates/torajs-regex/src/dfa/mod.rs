//! NFA → DFA conversion substrate for backref-free patterns.
//!
//! Layered chunks:
//! - **Eligibility** (`analyze` + `DfaEligibility`) — pre-order AST
//!   walker reporting whether the pattern uses only DFA-representable
//!   opcodes. Result cached on `Program.can_dfa` at compile time.
//! - **ε-closure** (`epsilon_closure`) — given a `Program` and a seed
//!   PC list, return the set of PCs reachable via pure-ε transitions.
//!   Building block for subset construction.
//! - **byte step** (`byte_step`) — given a PC set already closed under
//!   ε, return the PCs reached by consuming one input byte at each
//!   byte-matching op (`Char`, `AnyChar`, `Class`). Feed back into
//!   `epsilon_closure` for the next state.
//!
//! - **subset construction** (`build_dfa`) — BFS subset construction
//!   driver that composes `epsilon_closure` + `byte_step` into a
//!   [`DfaProgram`]: dense 256-way transitions per state, deduped via
//!   `BTreeMap<BTreeSet<usize>, u32>`, state 0 reserved as the dead
//!   state (empty PC set). Builder presumes caller already verified
//!   `prog.can_dfa` (see [`analyze`]); SAVE / Anchor / WBound are
//!   currently treated as terminal in [`epsilon_closure`], so the
//!   resulting DFA does not correctly handle those features yet —
//!   future chunks will revisit them.
//! - **executor** (`dfa_search`) — straight-line byte walk that drives
//!   a built [`DfaProgram`] over a haystack and returns the longest
//!   match-end position seen (per-byte, leftmost-longest semantics).
//!   Single multiply-free index per step; on dead-state (idx 0) the
//!   walk stops since no extension can ever accept.
//! - **per-call build**, no cache yet. The chunk 6 lazy cache
//!   (`Program::dfa_cache: UnsafeCell<Option<DfaProgram>>` +
//!   `get_or_build_dfa`) shipped first but was deleted after the
//!   chunk 7.5 OnceCell audit: both `UnsafeCell` and `OnceCell`
//!   variants triggered a flaky SIGBUS in `regex-021-test-lastindex`
//!   under hot-path consumption. Root cause family (RegExp lifetime
//!   vs cached `&DfaProgram` across the SSA-lower ABI boundary) is
//!   left to a future chunk 7.6 deep audit; chunk 7 v3 wire bypasses
//!   the cache and `build_dfa(prog, flags)` runs per `search_from_with_ws`
//!   call instead.
//!
//! - **position-aware closure** ([`ctx::PositionCtx`] +
//!   [`ctx::epsilon_closure_with_ctx`]) — chunk 8 substrate. Threads
//!   byte-position context (left-byte, text-start, text-end) through
//!   the ε-closure so `Op::AnchorB` / `Op::AnchorE` advance instead of
//!   staying terminal. Subset construction itself still uses the
//!   legacy closure; subsequent chunks thread the ctx through the
//!   builder and executor.
//!
//! Future chunks: `u`-flag code-point step (chunk 10). The full set
//! of zero-width / capture opcodes is now DFA-resident:
//! - `^` (chunk 8.5) — position-aware start states + line-break entry
//!   selection in the wire.
//! - `$` (chunk 8.6a) — per-state `is_accept_at_end` precomputed by
//!   re-closing under an at-end ctx.
//! - `\b` / `\B` (chunk 8.6b) — state key upgraded to `(PC set,
//!   LeftByteAttr)` so the BFS step can re-close with `right_byte =
//!   b` and resolve WBound/NWBound under the cursor's left/right
//!   class pair.
//! - capture group SAVE (chunk 9) — DFA is capture-blind (Save is a
//!   no-op ε in the closure) and `vm::search_from_with_ws` runs a
//!   second-pass Pike VM on the DFA-found `[start..end]` window with
//!   `end_target = end` to extract captures.
//! Tracking RFC: `.claude/rfcs/20260622-pike-vm-dfa/design.md`.

pub mod ctx;
pub use ctx::{PositionCtx, epsilon_closure_full, epsilon_closure_with_ctx};

mod build;
mod build_helpers;
mod pending_class;
mod program;
mod search;
mod state;
mod step;

pub use build::build_dfa;
pub use pending_class::PendingClass;
pub use program::{BakedDfaMeta, DfaProgram, DfaStates};
pub use search::{
    DfaState, TX_ACCEPT_BIT, TX_MONOTONE_BIT, TX_STATE_MASK, dfa_search, dfa_search_mid,
    dfa_search_mid_nonword, dfa_search_mid_word, first_viable_start,
};
pub use step::{byte_step, byte_step_full};

// chunk 8.6b — closure-API role split:
// - `epsilon_closure_with_ctx` is used for the BFS pre-step / post-
//   step closure (WBound stays terminal here; it'll be resolved by
//   the next BFS step when `right_byte` becomes known).
// - `epsilon_closure_full(.., right_byte = Some(b))` is used inside
//   the per-byte BFS step to resolve WBound/NWBound under the
//   correct left/right class pair.
// - `epsilon_closure_full(.., right_byte = None)` is used only for
//   `pc_set_is_accept_at_end`, where `None` truly means "no upcoming
//   byte" (cursor at haystack end) — WBound resolves against
//   `(left, non-word)`.
// Using `_full(.., None)` for BFS initial/post-step (right unknown,
// not "at end") would let WBound eagerly advance whenever the left
// is word-class, which is wrong.

use alloc::collections::BTreeSet;
use alloc::vec::Vec;

use crate::node::{Node, NodeKind};
use crate::program::{Op, Program};

/// Outcome of DFA eligibility analysis.
///
/// `Eligible` means the pattern's AST uses only opcodes representable
/// in a classical Thompson → DFA subset construction. The blocker
/// variants name the specific feature that forces NFA simulation —
/// useful for future logging / heuristics ("how often do we lose DFA
/// fast path to backref vs lookaround?").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DfaEligibility {
    Eligible,
    HasBackref,
    HasLookahead,
    HasNegLookahead,
    HasLookbehind,
    HasNegLookbehind,
    /// RFC 20260712 chunk D — a lazy quantifier's match END depends
    /// on thread priority, which the DFA powerset erases (it always
    /// answers the greedy/longest boundary: `/.*?/.exec("x")` came
    /// back `"x"` instead of `""`). Pike-VM-only until a
    /// DFA-existence + Pike-boundary split lands (L3b).
    HasLazyQuantifier,
    /// A prefix-overlapping alternation — a higher-priority branch is a
    /// fixed-length atom prefix of a lower-priority branch (`1|12`,
    /// `a|ab`, `\d|\d\d`), so ECMAScript §22.2 leftmost-first and the
    /// DFA's leftmost-longest disagree (`/1|12/.exec("123")` came back
    /// "12" instead of "1"). Same DFA-powerset-erases-thread-priority
    /// root as [`HasLazyQuantifier`]; the Pike VM is leftmost-first
    /// correct, so Pike-VM-only until a priority-ordered DFA lands
    /// (L3b — the unified "DFA priority awareness" item that also
    /// reclaims the lazy + multiline-`$` faces).
    HasPrefixAlt,
}

impl DfaEligibility {
    pub fn is_eligible(self) -> bool {
        matches!(self, DfaEligibility::Eligible)
    }
}

/// A single fixed-position matcher at the head of an alternation branch
/// — as far as a positional prefix comparison can reach.
enum Atom {
    Ch(u8),
    Cls(crate::charclass::CharClass),
    /// `.` — overlaps every other atom, so treated as compatible with
    /// anything (over-approximates: only ever costs the DFA fast path,
    /// never correctness).
    Any,
}

impl Atom {
    /// Whether the two atoms can match the SAME input char at a given
    /// position — the condition under which branch priority becomes
    /// observable. Conservative: unequal classes are treated as
    /// disjoint (a rare `[ab]|[abc]`-style overlap keeps the DFA;
    /// recorded L3b).
    fn overlaps(&self, other: &Atom) -> bool {
        match (self, other) {
            (Atom::Any, _) | (_, Atom::Any) => true,
            (Atom::Ch(a), Atom::Ch(b)) => a == b,
            (Atom::Cls(a), Atom::Cls(b)) => a == b,
            _ => false,
        }
    }
}

/// The leading fixed-position atoms of an alternation branch, stopping
/// at the first non-atomic node (quantifier / group / anchor / nested
/// alt / backref). `complete` is true when EVERY node of the branch was
/// an atom — i.e. the branch matches exactly this atom string, which is
/// what lets a shorter complete branch be a genuine prefix of a longer
/// one.
struct LeadingAtoms {
    atoms: alloc::vec::Vec<Atom>,
    complete: bool,
}

fn atom_of(node: &Node) -> Option<Atom> {
    match node.kind {
        NodeKind::Char => Some(Atom::Ch(node.ch)),
        NodeKind::Class => Some(Atom::Cls(node.cc.clone())),
        NodeKind::Any => Some(Atom::Any),
        _ => None,
    }
}

fn leading_atoms(node: &Node) -> LeadingAtoms {
    if let Some(a) = atom_of(node) {
        return LeadingAtoms {
            atoms: alloc::vec![a],
            complete: true,
        };
    }
    if matches!(node.kind, NodeKind::Concat) {
        let mut atoms = alloc::vec::Vec::new();
        for kid in node.kids.iter() {
            match atom_of(kid) {
                Some(a) => atoms.push(a),
                None => {
                    return LeadingAtoms {
                        atoms,
                        complete: false,
                    };
                }
            }
        }
        return LeadingAtoms {
            atoms,
            complete: true,
        };
    }
    LeadingAtoms {
        atoms: alloc::vec::Vec::new(),
        complete: false,
    }
}

/// True iff some higher-priority branch (earlier in source order) is a
/// complete atom prefix of a lower-priority branch — the exact case
/// where leftmost-first (ES §22.2) and the DFA's leftmost-longest
/// disagree. A complete shorter branch that positionally overlaps the
/// head of a longer branch will win under leftmost-first (the DFA would
/// wrongly extend to the longer match). Prefix-unrelated alternations
/// (`cat|dog`, `POST|PUT`) are left on the DFA fast path.
fn alt_forces_leftmost_first(alt: &Node) -> bool {
    let branches: alloc::vec::Vec<LeadingAtoms> =
        alt.kids.iter().map(|k| leading_atoms(k)).collect();
    for i in 0..branches.len() {
        let hi = &branches[i];
        if !hi.complete || hi.atoms.is_empty() {
            continue;
        }
        for lo in branches.iter().skip(i + 1) {
            if hi.atoms.len() <= lo.atoms.len()
                && hi
                    .atoms
                    .iter()
                    .zip(lo.atoms.iter())
                    .all(|(x, y)| x.overlaps(y))
            {
                return true;
            }
        }
    }
    false
}

/// Walk `root` recursively, returning the first blocker encountered or
/// `Eligible` if the entire tree is DFA-compatible.
///
/// Order: self node → `child` (used by Repeat / Group / lookaround) →
/// `kids` (used by Concat / Alt). Pre-order so a blocker at the root is
/// reported without descending. Reentrant / stack depth follows the AST
/// nesting depth, which the parser already bounds via standard regex
/// grammar — no separate guard needed.
pub fn analyze(root: &Node) -> DfaEligibility {
    match root.kind {
        NodeKind::Backref => return DfaEligibility::HasBackref,
        NodeKind::Lookahead => return DfaEligibility::HasLookahead,
        NodeKind::NegLookahead => return DfaEligibility::HasNegLookahead,
        NodeKind::Lookbehind => return DfaEligibility::HasLookbehind,
        NodeKind::NegLookbehind => return DfaEligibility::HasNegLookbehind,
        _ => {}
    }
    if root.lazy {
        return DfaEligibility::HasLazyQuantifier;
    }
    if matches!(root.kind, NodeKind::Alt) && alt_forces_leftmost_first(root) {
        return DfaEligibility::HasPrefixAlt;
    }
    if let Some(child) = root.child.as_ref() {
        let r = analyze(child);
        if !r.is_eligible() {
            return r;
        }
    }
    for kid in root.kids.iter() {
        let r = analyze(kid);
        if !r.is_eligible() {
            return r;
        }
    }
    DfaEligibility::Eligible
}

/// RC-4 multiline-`$` face — true iff the tree contains an
/// `AnchorEnd` (`$`) node whose effective m-bit is set (the global
/// `m` flag or an enclosing `(?m:…)` modifier group merged into
/// `Node::eff_ims` at parse time). The DFA closure resolves
/// `Op::AnchorE` only at text end (`PositionCtx` carries no right
/// byte), so a multiline `$` before `\n` silently missed; callers
/// gate `can_dfa` off for such patterns and the Pike VM
/// (multiline-aware AnchorE) takes over. DFA support needs a
/// right-byte-aware closure or RE2-style `(?=\n|$)` folding —
/// roadmap item.
pub fn tree_contains_ml_anchor_end(root: &Node) -> bool {
    if matches!(root.kind, NodeKind::AnchorEnd) && root.eff_ims & crate::parser::RE_FLAG_M != 0 {
        return true;
    }
    if let Some(child) = root.child.as_ref()
        && tree_contains_ml_anchor_end(child)
    {
        return true;
    }
    root.kids.iter().any(|k| tree_contains_ml_anchor_end(k))
}

/// Compute the ε-closure of `seeds` under `prog` — the set of all PCs
/// reachable from any `seeds[i]` via pure-ε transitions.
///
/// ε ops handled in this chunk: [`Op::Jmp`] (unconditional), [`Op::Split`]
/// (both branches). Everything else (`Save`, the anchors, `WBound` /
/// `NWBound`, the lookaround variants, every byte-consuming op) is
/// terminal — its PC is recorded in the closure if it was reached, but
/// the walk does not follow its successor.
///
/// Save / Anchor / WBound are kept terminal here because they carry
/// state that a flat PC-set cannot represent: `Save` mutates a capture
/// slot (future chunk: track per-state save-mask), the anchors depend on
/// `pos` relative to text start / line boundaries (future chunk:
/// position-context DFA states tracking left-byte / at-start / at-end),
/// `WBound` / `NWBound` depend on the surrounding bytes. The subset
/// construction will revisit these once the per-state context is added.
///
/// PCs are deduped via the returned `BTreeSet`, so a circular `JMP`
/// chain terminates in O(n) where n = `prog.len()`. Out-of-range or
/// unknown-opcode PCs are silently dropped (defensive — mirrors
/// `vm/dispatch.rs::add_thread`).

// chunk 10c split — `dfa/build.rs` owns the subset-construction
// builder + `LeftByteAttr` / `pc_set_is_accept_at_end` / `intern_state`
// machinery; `dfa/search.rs` owns the executor + `DfaState` /
// `DfaProgram` data types; chunk 10d follow-up split — `dfa/step.rs`
// owns the byte-step transitions + case-fold helper. Tests stay inline
// below since `#[cfg(test)] mod tests` doesn't count toward the
// file-size limit.

/// Stricter than [`crate::program::Program::can_dfa`]: chunks 8.5
/// (`^`), 8.6a (`$`), 8.6b (`\b` / `\B`), and 9 (capture group SAVE)
/// have all cleared their respective opcodes from the blocker list.
/// What remains rejected here is just byte-step / op-set combinations
/// the DFA substrate doesn't model — sub-programs (lookaround) are
/// already rejected upstream by `can_dfa`.
///
/// Cleared blockers (full chain):
/// - `Op::AnchorB` (`^`, chunk 8.5) — position-aware builder resolves
///   it via `start` / `start_mid*` start states; the executor picks
///   per the cursor position (byte 0 — or a line break under
///   `RE_FLAG_M`, chunk 8.8 — selects `start`).
/// - `Op::AnchorE` (`$`, chunk 8.6a) — every DFA state precomputes
///   `is_accept_at_end` (PC set re-closed under an at-end ctx); the
///   executor consults the flag after the byte walk so `$` can fire
///   at the haystack end.
/// - `Op::WBound` / `Op::NWBound` (chunk 8.6b) — state key upgraded
///   to `(PC set, LeftByteAttr)`; BFS re-closes the set with
///   `right_byte = b` on each step, so `\b` / `\B` resolve with the
///   true left/right class pair at the cursor.
/// - `Op::Save` (chunk 9) — DFA is capture-blind (Save is a no-op ε in
///   the closure) and [`crate::vm::search_from_with_ws`] runs a
///   second-pass Pike VM on the DFA-found `[start..end]` window with
///   `end_target = end` so the winning thread's saves come out.
///
/// Sub-programs (lookaround bodies) cannot appear when `can_dfa` is
/// true (lookaround is itself a blocker in [`analyze`]) — the loop
/// over `sub_progs` is a belt-and-suspenders defensive check (it
/// always returns true for `can_dfa`-eligible programs but stays so
/// future opcode additions don't quietly slip past).
pub fn prog_ops_dfa_safe(prog: &Program) -> bool {
    // No opcode is rejected at this layer any more — `can_dfa`
    // upstream rules out backref / lookaround which are the only
    // truly DFA-incompatible ops the AST can emit.
    //
    // RFC 20260711 chunk B — class-shape gate: property-table-bearing
    // classes that are neither byte-steppable (`byte_only` expansion
    // leaves) nor pending-serveable (pure positive property) reach the
    // Program as single cp-aware `Op::Class` ops (negated `\P{...}`,
    // property + explicit non-ASCII bits — `expand_unsafe_class`
    // declines them since the full UCD tables explode the byte-level
    // expansion). `byte_step`'s `class.test(byte)` would evaluate the
    // negation at byte level and silently mis-match multi-byte cps,
    // so these programs are Pike-VM-only (`dispatch_class` decodes
    // the cp and `test_cp` applies the negation after the table
    // union). DFA residency for them is the L3b follow-up.
    prog.classes
        .iter()
        .all(|c| c.byte_only || c.u_prop_tables.is_empty() || c.is_uflag_property_only())
}

pub fn epsilon_closure(prog: &Program, seeds: &[usize]) -> BTreeSet<usize> {
    let mut closure: BTreeSet<usize> = BTreeSet::new();
    let mut work: Vec<usize> = Vec::new();
    for &seed in seeds {
        if seed < prog.len() && closure.insert(seed) {
            work.push(seed);
        }
    }
    while let Some(pc) = work.pop() {
        let ins = prog.insts[pc];
        let op = match Op::from_u8(ins.op) {
            Some(o) => o,
            None => continue,
        };
        match op {
            Op::Jmp => {
                let t = ins.a as usize;
                if t < prog.len() && closure.insert(t) {
                    work.push(t);
                }
            }
            Op::Split => {
                let t1 = ins.a as usize;
                let t2 = ins.b as usize;
                if t1 < prog.len() && closure.insert(t1) {
                    work.push(t1);
                }
                if t2 < prog.len() && closure.insert(t2) {
                    work.push(t2);
                }
            }
            _ => {}
        }
    }
    closure
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;

    fn char_node(b: u8) -> alloc::boxed::Box<Node> {
        let mut n = Node::new(NodeKind::Char);
        n.ch = b;
        n
    }

    /// Round 5 attack #9 — transitions are packed words (destination
    /// index | folded accept/monotone bits); tests that dereference a
    /// target as a state index strip the flag bits first.
    fn tgt(packed: u32) -> usize {
        (packed & TX_STATE_MASK) as usize
    }

    #[test]
    fn single_char_is_eligible() {
        let n = char_node(b'a');
        assert_eq!(analyze(&n), DfaEligibility::Eligible);
        assert!(analyze(&n).is_eligible());
    }

    #[test]
    fn anychar_class_anchors_are_eligible() {
        for k in [
            NodeKind::Any,
            NodeKind::Class,
            NodeKind::AnchorBeg,
            NodeKind::AnchorEnd,
            NodeKind::WBound,
            NodeKind::NWBound,
        ] {
            let n = Node::new(k);
            assert_eq!(analyze(&n), DfaEligibility::Eligible, "{k:?}");
        }
    }

    #[test]
    fn concat_of_chars_is_eligible() {
        let mut concat = Node::new(NodeKind::Concat);
        concat.push_kid(char_node(b'a'));
        concat.push_kid(char_node(b'b'));
        concat.push_kid(char_node(b'c'));
        assert_eq!(analyze(&concat), DfaEligibility::Eligible);
    }

    #[test]
    fn alt_of_concats_is_eligible() {
        let mut alt = Node::new(NodeKind::Alt);
        for c in [b'a', b'b'] {
            let mut concat = Node::new(NodeKind::Concat);
            concat.push_kid(char_node(c));
            alt.push_kid(concat);
        }
        assert_eq!(analyze(&alt), DfaEligibility::Eligible);
    }

    #[test]
    fn repeat_over_class_is_eligible() {
        let mut rep = Node::new(NodeKind::Repeat);
        rep.min = 0;
        rep.max = -1;
        rep.child = Some(Node::new(NodeKind::Class));
        assert_eq!(analyze(&rep), DfaEligibility::Eligible);
    }

    #[test]
    fn group_around_char_is_eligible() {
        let mut g = Node::new(NodeKind::Group);
        g.capture_idx = 1;
        g.child = Some(char_node(b'x'));
        assert_eq!(analyze(&g), DfaEligibility::Eligible);
    }

    #[test]
    fn root_backref_is_blocker() {
        let n = Node::new(NodeKind::Backref);
        assert_eq!(analyze(&n), DfaEligibility::HasBackref);
        assert!(!analyze(&n).is_eligible());
    }

    #[test]
    fn nested_backref_is_blocker() {
        let mut concat = Node::new(NodeKind::Concat);
        concat.push_kid(char_node(b'a'));
        concat.push_kid(Node::new(NodeKind::Backref));
        assert_eq!(analyze(&concat), DfaEligibility::HasBackref);
    }

    #[test]
    fn lookahead_variants_are_blockers() {
        for (k, want) in [
            (NodeKind::Lookahead, DfaEligibility::HasLookahead),
            (NodeKind::NegLookahead, DfaEligibility::HasNegLookahead),
            (NodeKind::Lookbehind, DfaEligibility::HasLookbehind),
            (NodeKind::NegLookbehind, DfaEligibility::HasNegLookbehind),
        ] {
            let mut n = Node::new(k);
            n.child = Some(char_node(b'x'));
            assert_eq!(analyze(&n), want, "{k:?}");
        }
    }

    #[test]
    fn lookahead_deeply_nested_in_group_is_blocker() {
        let mut outer = Node::new(NodeKind::Group);
        outer.capture_idx = 1;
        let mut rep = Node::new(NodeKind::Repeat);
        rep.min = 1;
        rep.max = -1;
        let mut la = Node::new(NodeKind::Lookahead);
        la.child = Some(char_node(b'z'));
        rep.child = Some(la);
        outer.child = Some(rep);
        assert_eq!(analyze(&outer), DfaEligibility::HasLookahead);
    }

    #[test]
    fn first_blocker_wins_pre_order() {
        let mut alt = Node::new(NodeKind::Alt);
        alt.push_kid(Node::new(NodeKind::Backref));
        alt.push_kid(Node::new(NodeKind::Lookahead));
        assert_eq!(analyze(&alt), DfaEligibility::HasBackref);
    }

    #[test]
    fn empty_concat_is_eligible() {
        let n = Node::new(NodeKind::Concat);
        assert_eq!(analyze(&n), DfaEligibility::Eligible);
    }

    // Integration: parse real regex source through the parser pipeline
    // (including `resolve_backrefs` for named `\k<name>`) and check the
    // eligibility verdict against a curated truth table.
    fn parse_and_analyze(pattern: &[u8], flags: u8) -> DfaEligibility {
        use crate::parser::Parser;
        use crate::resolve::resolve_backrefs;
        let mut parser = Parser::new(pattern, flags);
        let mut root = parser.parse().expect("parse should succeed");
        let names = parser.names.clone();
        let n_captures = parser.n_captures;
        assert!(
            resolve_backrefs(&mut root, &names, n_captures, flags),
            "resolve_backrefs should succeed for {pattern:?}"
        );
        analyze(&root)
    }

    #[test]
    fn parsed_literal_pattern_is_eligible() {
        assert_eq!(parse_and_analyze(b"abc", 0), DfaEligibility::Eligible);
    }

    #[test]
    fn parsed_alt_repeat_class_pattern_is_eligible() {
        assert_eq!(
            parse_and_analyze(b"(foo|bar)+[0-9]*\\b", 0),
            DfaEligibility::Eligible
        );
    }

    #[test]
    fn parsed_anchor_dotall_pattern_is_eligible() {
        assert_eq!(parse_and_analyze(b"^.*$", 0), DfaEligibility::Eligible);
    }

    #[test]
    fn parsed_backref_pattern_is_blocked() {
        // `(a)\1` — capture + decimal backref to it.
        assert_eq!(parse_and_analyze(b"(a)\\1", 0), DfaEligibility::HasBackref);
    }

    #[test]
    fn parsed_named_backref_pattern_is_blocked() {
        // `(?<x>a)\k<x>` — named capture + `\k<name>` backref.
        // `resolve_backrefs` rewrites this into a `Backref` node with
        // the resolved capture index; without it the walker would not
        // see the blocker.
        assert_eq!(
            parse_and_analyze(b"(?<x>a)\\k<x>", 0),
            DfaEligibility::HasBackref
        );
    }

    #[test]
    fn parsed_lookahead_pattern_is_blocked() {
        assert_eq!(
            parse_and_analyze(b"foo(?=bar)", 0),
            DfaEligibility::HasLookahead
        );
    }

    #[test]
    fn parsed_neg_lookahead_pattern_is_blocked() {
        assert_eq!(
            parse_and_analyze(b"foo(?!bar)", 0),
            DfaEligibility::HasNegLookahead
        );
    }

    #[test]
    fn parsed_lookbehind_pattern_is_blocked() {
        assert_eq!(
            parse_and_analyze(b"(?<=foo)bar", 0),
            DfaEligibility::HasLookbehind
        );
    }

    #[test]
    fn parsed_neg_lookbehind_pattern_is_blocked() {
        assert_eq!(
            parse_and_analyze(b"(?<!foo)bar", 0),
            DfaEligibility::HasNegLookbehind
        );
    }

    // ε-closure tests — synthetic mini-Programs that exercise the JMP /
    // SPLIT walk plus the "terminal" treatment of every other op.

    use crate::program::Inst;

    fn into_set(v: &[usize]) -> BTreeSet<usize> {
        v.iter().copied().collect()
    }

    #[test]
    fn epsilon_closure_of_empty_seed_list_is_empty() {
        let prog = Program::new();
        assert!(epsilon_closure(&prog, &[]).is_empty());
    }

    #[test]
    fn seed_out_of_range_is_dropped() {
        let prog = Program::new();
        assert!(epsilon_closure(&prog, &[0, 999]).is_empty());
    }

    #[test]
    fn seed_at_char_op_only_contains_self() {
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        assert_eq!(epsilon_closure(&prog, &[0]), into_set(&[0]));
    }

    #[test]
    fn jmp_chain_walks_to_terminal_op() {
        // 0: JMP 2 → 2: JMP 4 → 4: CHAR a → 5: MATCH
        let mut prog = Program::new();
        prog.emit(Inst::jmp(2));
        prog.emit(Inst::char_lit(b'x')); // unreachable filler
        prog.emit(Inst::jmp(4));
        prog.emit(Inst::char_lit(b'y')); // unreachable filler
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        assert_eq!(epsilon_closure(&prog, &[0]), into_set(&[0, 2, 4]));
    }

    #[test]
    fn split_forks_both_targets() {
        // 0: SPLIT 1, 3 — both targets are char ops (terminal)
        let mut prog = Program::new();
        prog.emit(Inst::split(1, 3));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept()); // unreachable filler
        prog.emit(Inst::char_lit(b'b'));
        prog.emit(Inst::match_accept());
        assert_eq!(epsilon_closure(&prog, &[0]), into_set(&[0, 1, 3]));
    }

    #[test]
    fn split_to_split_walks_transitively() {
        // 0: SPLIT 1,2; 1: SPLIT 3,4; 2: char; 3: char; 4: char
        let mut prog = Program::new();
        prog.emit(Inst::split(1, 2));
        prog.emit(Inst::split(3, 4));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::char_lit(b'b'));
        prog.emit(Inst::char_lit(b'c'));
        prog.emit(Inst::match_accept());
        assert_eq!(epsilon_closure(&prog, &[0]), into_set(&[0, 1, 2, 3, 4]));
    }

    #[test]
    fn circular_jmp_terminates() {
        // 0: JMP 1; 1: JMP 0 — must not loop forever
        let mut prog = Program::new();
        prog.emit(Inst::jmp(1));
        prog.emit(Inst::jmp(0));
        assert_eq!(epsilon_closure(&prog, &[0]), into_set(&[0, 1]));
    }

    #[test]
    fn save_op_is_terminal_in_this_chunk() {
        // 0: SAVE 0; 1: CHAR a — closure of {0} = {0} only (not {0, 1});
        // SAVE walk is deferred to a future save-mask-aware chunk.
        let mut prog = Program::new();
        prog.emit(Inst::save(0));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        assert_eq!(epsilon_closure(&prog, &[0]), into_set(&[0]));
    }

    #[test]
    fn anchor_b_is_terminal_in_this_chunk() {
        // 0: ANCHOR_B; 1: CHAR a — closure of {0} = {0}; anchor walk
        // deferred to a future position-context-aware chunk.
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnchorB));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        assert_eq!(epsilon_closure(&prog, &[0]), into_set(&[0]));
    }

    #[test]
    fn multi_seed_unions_per_seed_closures() {
        // 0: JMP 2; 2: CHAR a; 3: JMP 5; 5: CHAR b
        let mut prog = Program::new();
        prog.emit(Inst::jmp(2));
        prog.emit(Inst::char_lit(b'x')); // filler
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::jmp(5));
        prog.emit(Inst::char_lit(b'y')); // filler
        prog.emit(Inst::char_lit(b'b'));
        prog.emit(Inst::match_accept());
        assert_eq!(epsilon_closure(&prog, &[0, 3]), into_set(&[0, 2, 3, 5]));
    }

    // byte_step tests — single-byte transition over an already-ε-closed
    // PC set.

    use crate::charclass::CharClass;

    fn set(v: &[usize]) -> BTreeSet<usize> {
        v.iter().copied().collect()
    }

    #[test]
    fn byte_step_empty_states_returns_empty() {
        let prog = Program::new();
        assert!(byte_step(&prog, &BTreeSet::new(), b'a').is_empty());
    }

    #[test]
    fn byte_step_char_hit_advances_to_next_pc() {
        // 0: CHAR a; 1: MATCH
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        assert_eq!(byte_step(&prog, &set(&[0]), b'a'), set(&[1]));
    }

    #[test]
    fn byte_step_char_miss_drops_pc() {
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        assert!(byte_step(&prog, &set(&[0]), b'b').is_empty());
    }

    #[test]
    fn byte_step_anychar_advances_on_any_byte_under_s_flag() {
        // chunk 10a — under `s` (per-inst pad s-bit since
        // regexp-modifiers), `.` matches every byte (dot-all).
        let mut prog = Program::new();
        let mut any = Inst::simple(Op::AnyChar);
        any.pad = crate::parser::RE_FLAG_S as u16;
        prog.emit(any);
        prog.emit(Inst::match_accept());
        for b in [b'a', b'\n', 0u8, 0xff] {
            assert_eq!(byte_step(&prog, &set(&[0]), b), set(&[1]), "byte 0x{b:02x}");
        }
    }

    #[test]
    fn byte_step_anychar_skips_newline_without_s_flag() {
        // chunk 10a — without `s`, `.` advances on every byte except
        // `\n` (0x0A). This honours the JS spec at the DFA level so the
        // hot-path gate no longer needs to require the `s` flag.
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnyChar));
        prog.emit(Inst::match_accept());
        for b in [b'a', 0u8, 0xff, b'\r'] {
            assert_eq!(byte_step(&prog, &set(&[0]), b), set(&[1]), "byte 0x{b:02x}");
        }
        assert!(byte_step(&prog, &set(&[0]), b'\n').is_empty());
    }

    #[test]
    fn byte_step_class_hit_and_miss() {
        // 0: CLASS [a-c]; 1: MATCH
        let mut prog = Program::new();
        let mut cc = CharClass::new();
        cc.add_range(b'a', b'c');
        let idx = prog.intern_class(&cc);
        prog.emit(Inst::class_ref(idx));
        prog.emit(Inst::match_accept());
        assert_eq!(byte_step(&prog, &set(&[0]), b'a'), set(&[1]));
        assert_eq!(byte_step(&prog, &set(&[0]), b'c'), set(&[1]));
        assert!(byte_step(&prog, &set(&[0]), b'd').is_empty());
    }

    #[test]
    fn byte_step_jmp_and_split_are_inert() {
        // 0: JMP 2; 1: SPLIT 0,2; 2: MATCH — none of these consume bytes.
        let mut prog = Program::new();
        prog.emit(Inst::jmp(2));
        prog.emit(Inst::split(0, 2));
        prog.emit(Inst::match_accept());
        assert!(byte_step(&prog, &set(&[0, 1]), b'a').is_empty());
    }

    #[test]
    fn byte_step_save_anchor_are_inert() {
        // 0: SAVE 0; 1: ANCHOR_B; 2: WBOUND; 3: MATCH
        let mut prog = Program::new();
        prog.emit(Inst::save(0));
        prog.emit(Inst::simple(Op::AnchorB));
        prog.emit(Inst::simple(Op::WBound));
        prog.emit(Inst::match_accept());
        assert!(byte_step(&prog, &set(&[0, 1, 2]), b'a').is_empty());
    }

    #[test]
    fn byte_step_unions_advances_across_state_set() {
        // 0: CHAR a; 1: MATCH; 2: CHAR b; 3: MATCH
        // states {0, 2}, byte 'a' → only pc 0 advances → {1}
        // states {0, 2}, byte 'b' → only pc 2 advances → {3}
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        prog.emit(Inst::char_lit(b'b'));
        prog.emit(Inst::match_accept());
        assert_eq!(byte_step(&prog, &set(&[0, 2]), b'a'), set(&[1]));
        assert_eq!(byte_step(&prog, &set(&[0, 2]), b'b'), set(&[3]));
    }

    #[test]
    fn byte_step_next_pc_past_end_is_dropped() {
        // 0: CHAR a — pc 0's successor 1 is past end of (1-inst) program
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        // No MATCH terminator: pc 1 is out of range, must be dropped.
        assert!(byte_step(&prog, &set(&[0]), b'a').is_empty());
    }

    // build_dfa tests — composes epsilon_closure + byte_step into a
    // deterministic table. Each test asserts state count + key
    // transitions rather than the full 256-byte table for readability.

    #[test]
    fn build_dfa_empty_program_has_only_dead_state() {
        let prog = Program::new();
        let dfa = build_dfa(&prog, 0);
        assert_eq!(dfa.states.len(), 1);
        assert_eq!(dfa.start, 0);
        assert!(!dfa.states[0].is_accept);
    }

    #[test]
    fn build_dfa_dead_state_self_loops_to_zero() {
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        for b in 0u16..=255 {
            assert_eq!(dfa.states[0].transitions[b as usize], 0);
        }
        assert!(!dfa.states[0].is_accept);
    }

    #[test]
    fn build_dfa_match_only_is_accept_immediately() {
        // 0: MATCH — accepts empty string. ε-closure of {0} = {0}, has MATCH.
        let mut prog = Program::new();
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        assert!(dfa.states[dfa.start as usize].is_accept);
    }

    /// Round 3 Phase B sub-batch 6 attack #R-G — `compute_monotone_
    /// accept` invariants. A single-char literal `/a/` has three
    /// states: dead (0), start (non-accept, only `a` advances), and
    /// accept (no outgoing transitions). The accept state qualifies
    /// for `monotone_accept = true` — it is `is_accept` and every
    /// transition either goes to dead (= valid exit) or to an
    /// accepting state (none exist here, so the for-loop never finds
    /// a non-accept target). The start state stays `false` because
    /// `is_accept == false` short-circuits before the transition
    /// scan.
    #[test]
    fn monotone_accept_single_char_literal() {
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        let start = dfa.start as usize;
        assert!(!dfa.states[start].monotone_accept);
        let accept = tgt(dfa.states[start].transitions[b'a' as usize]);
        assert!(dfa.states[accept].is_accept);
        assert!(dfa.states[accept].monotone_accept);
        // Dead state stays `false` — the `is_accept` short-circuit
        // skips it before the transition scan.
        assert!(!dfa.states[0].monotone_accept);
    }

    /// Round 3 Phase B sub-batch 6 attack #R-G — Kleene-plus over a
    /// single byte (`/a+/`) has a self-looping accept state whose
    /// `transitions[b'a']` returns to itself and all other
    /// `transitions[b]` go to dead. Both target classes (self =
    /// accept, dead = exit) qualify, so `monotone_accept` is `true`.
    /// This is the substrate shape `\p{L}+/u` collapses to once the
    /// K-PROPERTY pending-class fast path is folded in.
    #[test]
    fn monotone_accept_kleene_plus_single_byte() {
        // /a+/ encoded by hand: SPLIT(1, 3); CHAR a; JMP 0; MATCH.
        let mut prog = Program::new();
        prog.emit(Inst::split(1, 3));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::jmp(0));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        let start = dfa.start as usize;
        // The `a`-loop accept state is reached via a single `a` from
        // start; it self-loops on `a` and dies on any other byte.
        let after_a = tgt(dfa.states[start].transitions[b'a' as usize]);
        assert!(dfa.states[after_a].is_accept);
        assert!(dfa.states[after_a].monotone_accept);
        // Self-loop on `a` is the only non-dead transition.
        assert_eq!(tgt(dfa.states[after_a].transitions[b'a' as usize]), after_a);
        for b in 0u16..=255 {
            if b == b'a' as u16 {
                continue;
            }
            assert_eq!(dfa.states[after_a].transitions[b as usize], 0);
        }
    }

    /// Round 5 attack #9 — every packed transition word's top two
    /// bits must mirror the destination state's `is_accept` /
    /// `monotone_accept` fields exactly, and the masked index must be
    /// in bounds. Sweeps all 256 slots of every state across pattern
    /// shapes that exercise accept self-loops (`a+`), plain literals,
    /// and the mid-walk accept/non-accept alternation the dotall
    /// executor path hits (`a.+c` under `s`).
    #[test]
    fn fold_accept_bits_mirrors_target_flags() {
        let progs: [(&dyn Fn(&mut Program), u8); 3] = [
            (
                &|p: &mut Program| {
                    // /a+/: CHAR a; SPLIT 0, 2; MATCH
                    p.emit(Inst::char_lit(b'a'));
                    p.emit(Inst::split(0, 2));
                    p.emit(Inst::match_accept());
                },
                0,
            ),
            (
                &|p: &mut Program| {
                    // /abc/
                    p.emit(Inst::char_lit(b'a'));
                    p.emit(Inst::char_lit(b'b'));
                    p.emit(Inst::char_lit(b'c'));
                    p.emit(Inst::match_accept());
                },
                0,
            ),
            (
                &|p: &mut Program| {
                    // /a.+c/s: CHAR a; ANY(s); SPLIT 1, 4; CHAR c; MATCH
                    p.emit(Inst::char_lit(b'a'));
                    let mut any = Inst::simple(Op::AnyChar);
                    any.pad = crate::parser::RE_FLAG_S as u16;
                    p.emit(any);
                    p.emit(Inst::split(1, 4));
                    p.emit(Inst::char_lit(b'c'));
                    p.emit(Inst::match_accept());
                },
                0,
            ),
        ];
        for (emit, flags) in progs {
            let mut prog = Program::new();
            emit(&mut prog);
            let dfa = build_dfa(&prog, flags);
            for (si, s) in dfa.states.iter().enumerate() {
                for b in 0..256 {
                    let packed = s.transitions[b];
                    let idx = tgt(packed);
                    assert!(idx < dfa.states.len(), "state {si} byte {b}: oob");
                    assert_eq!(
                        packed & TX_ACCEPT_BIT != 0,
                        dfa.states[idx].is_accept,
                        "state {si} byte {b}: accept bit != target field"
                    );
                    assert_eq!(
                        packed & TX_MONOTONE_BIT != 0,
                        dfa.states[idx].monotone_accept,
                        "state {si} byte {b}: monotone bit != target field"
                    );
                }
            }
        }
    }

    #[test]
    fn build_dfa_single_char_literal() {
        // 0: CHAR a; 1: MATCH
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        // dead + start({0}) + accept({1}) = 3 states.
        assert_eq!(dfa.states.len(), 3);
        assert_eq!(dfa.start, 1);
        assert!(!dfa.states[dfa.start as usize].is_accept);
        let accept = tgt(dfa.states[dfa.start as usize].transitions[b'a' as usize]);
        assert_ne!(accept, 0);
        assert!(dfa.states[accept].is_accept);
        assert_eq!(dfa.states[dfa.start as usize].transitions[b'b' as usize], 0);
    }

    #[test]
    fn build_dfa_literal_abc_walks_to_accept() {
        // 0: CHAR a; 1: CHAR b; 2: CHAR c; 3: MATCH
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::char_lit(b'b'));
        prog.emit(Inst::char_lit(b'c'));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        let s1 = dfa.start as usize;
        let s2 = tgt(dfa.states[s1].transitions[b'a' as usize]);
        assert_ne!(s2, 0);
        let s3 = tgt(dfa.states[s2].transitions[b'b' as usize]);
        assert_ne!(s3, 0);
        let s4 = tgt(dfa.states[s3].transitions[b'c' as usize]);
        assert_ne!(s4, 0);
        assert!(dfa.states[s4].is_accept);
        assert!(!dfa.states[s2].is_accept);
        assert!(!dfa.states[s3].is_accept);
        // wrong-byte transitions all route to dead.
        assert_eq!(dfa.states[s1].transitions[b'z' as usize], 0);
        assert_eq!(dfa.states[s2].transitions[b'z' as usize], 0);
        assert_eq!(dfa.states[s3].transitions[b'z' as usize], 0);
    }

    #[test]
    fn build_dfa_anychar_routes_every_byte_to_accept_under_s_flag() {
        // 0: ANY(s); 1: MATCH — under `s` (per-inst pad s-bit), `.`
        // matches every byte.
        let mut prog = Program::new();
        let mut any = Inst::simple(Op::AnyChar);
        any.pad = crate::parser::RE_FLAG_S as u16;
        prog.emit(any);
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        let start = dfa.start as usize;
        let target = dfa.states[start].transitions[0];
        assert_ne!(target, 0);
        assert!(dfa.states[tgt(target)].is_accept);
        for b in 0u16..=255 {
            // A UTF-8 lead byte is not a character yet, so `.` waits
            // for the tail rather than accepting mid-character — in
            // either mode, which is why this walks past the `u` flag
            // without asking for it.
            if matches!(b, 0xC2..=0xF4) {
                assert_ne!(
                    dfa.states[start].transitions[b as usize], target,
                    "lead byte 0x{b:02x} accepted before its tail"
                );
                continue;
            }
            assert_eq!(
                dfa.states[start].transitions[b as usize], target,
                "byte 0x{b:02x}"
            );
        }
    }

    #[test]
    fn build_dfa_anychar_skips_newline_without_s_flag() {
        // chunk 10a — without `s`, `.` routes `\n` to dead state.
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnyChar));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        let start = dfa.start as usize;
        let target = tgt(dfa.states[start].transitions[b'a' as usize]);
        assert_ne!(target, 0);
        assert!(dfa.states[target].is_accept);
        assert_eq!(dfa.states[start].transitions[b'\n' as usize], 0);
    }

    #[test]
    fn build_dfa_class_only_in_range_advances() {
        // 0: CLASS [a-c]; 1: MATCH
        let mut prog = Program::new();
        let mut cc = CharClass::new();
        cc.add_range(b'a', b'c');
        let idx = prog.intern_class(&cc);
        prog.emit(Inst::class_ref(idx));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        let start = dfa.start as usize;
        let target = dfa.states[start].transitions[b'a' as usize];
        assert_ne!(target, 0);
        assert!(dfa.states[tgt(target)].is_accept);
        // all in-range bytes route to the same accept state.
        assert_eq!(dfa.states[start].transitions[b'b' as usize], target);
        assert_eq!(dfa.states[start].transitions[b'c' as usize], target);
        // out-of-range bytes route to dead.
        assert_eq!(dfa.states[start].transitions[b'd' as usize], 0);
        assert_eq!(dfa.states[start].transitions[b'`' as usize], 0);
    }

    #[test]
    fn build_dfa_alternation_both_branches_accept() {
        // 0: SPLIT 1, 3; 1: CHAR a; 2: MATCH; 3: CHAR b; 4: MATCH
        let mut prog = Program::new();
        prog.emit(Inst::split(1, 3));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        prog.emit(Inst::char_lit(b'b'));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        let start = dfa.start as usize;
        let via_a = tgt(dfa.states[start].transitions[b'a' as usize]);
        let via_b = tgt(dfa.states[start].transitions[b'b' as usize]);
        assert_ne!(via_a, 0);
        assert_ne!(via_b, 0);
        assert!(dfa.states[via_a].is_accept);
        assert!(dfa.states[via_b].is_accept);
        // 'c' from start routes to dead.
        assert_eq!(dfa.states[start].transitions[b'c' as usize], 0);
    }

    #[test]
    fn build_dfa_kleene_star_self_loops_and_accepts_empty() {
        // a*: 0: SPLIT 1, 3; 1: CHAR a; 2: JMP 0; 3: MATCH
        let mut prog = Program::new();
        prog.emit(Inst::split(1, 3));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::jmp(0));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        // ε-closure({0}) walks SPLIT to {1, 3}; 3 = MATCH → start accepts.
        assert!(dfa.states[dfa.start as usize].is_accept);
        let via_a = tgt(dfa.states[dfa.start as usize].transitions[b'a' as usize]);
        assert_ne!(via_a, 0);
        // After consuming 'a' we are back at an equivalent ε-closed set.
        assert!(dfa.states[via_a].is_accept);
        assert_eq!(dfa.states[dfa.start as usize].transitions[b'b' as usize], 0);
    }

    #[test]
    fn build_dfa_kleene_plus_self_loops_but_not_empty() {
        // a+: 0: CHAR a; 1: SPLIT 0, 2; 2: MATCH
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::split(0, 2));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        // Start ε-closure = {0} — no MATCH yet, so start does NOT accept.
        assert!(!dfa.states[dfa.start as usize].is_accept);
        let via_a = tgt(dfa.states[dfa.start as usize].transitions[b'a' as usize]);
        assert_ne!(via_a, 0);
        assert!(dfa.states[via_a].is_accept);
        // Repeated 'a' from via_a must land on the same state (dedup).
        let via_aa = tgt(dfa.states[via_a].transitions[b'a' as usize]);
        assert_eq!(via_aa, via_a, "kleene-plus loops back to itself");
        assert_eq!(dfa.states[via_a].transitions[b'b' as usize], 0);
    }

    #[test]
    fn build_dfa_dedup_equivalent_pc_sets() {
        // (a|a)b — both alternatives converge on the same PC set after 'a';
        // dedup must collapse them so the total state count stays minimal.
        // 0: SPLIT 1, 3; 1: CHAR a; 2: JMP 4; 3: CHAR a; 4: CHAR b; 5: MATCH
        let mut prog = Program::new();
        prog.emit(Inst::split(1, 3));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::jmp(4));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::char_lit(b'b'));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        let start = dfa.start as usize;
        let after_a = tgt(dfa.states[start].transitions[b'a' as usize]);
        assert_ne!(after_a, 0);
        let accept = tgt(dfa.states[after_a].transitions[b'b' as usize]);
        assert_ne!(accept, 0);
        assert!(dfa.states[accept].is_accept);
        // dead + start + after_a + accept = 4 states (no duplicate after_a).
        assert_eq!(dfa.states.len(), 4);
    }

    #[test]
    fn build_dfa_class_after_class_routes_only_via_lower_then_digit() {
        // [a-z][0-9]: CLASS lower; CLASS digit; MATCH
        let mut prog = Program::new();
        let mut lower = CharClass::new();
        lower.add_range(b'a', b'z');
        let li = prog.intern_class(&lower);
        let mut digit = CharClass::new();
        digit.add_range(b'0', b'9');
        let di = prog.intern_class(&digit);
        prog.emit(Inst::class_ref(li));
        prog.emit(Inst::class_ref(di));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        let start = dfa.start as usize;
        let s1 = tgt(dfa.states[start].transitions[b'a' as usize]);
        assert_ne!(s1, 0);
        let s2 = tgt(dfa.states[s1].transitions[b'5' as usize]);
        assert_ne!(s2, 0);
        assert!(dfa.states[s2].is_accept);
        // Uppercase from start = dead.
        assert_eq!(dfa.states[start].transitions[b'A' as usize], 0);
        // Digit from start = dead (must consume lower first).
        assert_eq!(dfa.states[start].transitions[b'5' as usize], 0);
        // Letter from s1 = dead (need a digit, not another letter).
        assert_eq!(dfa.states[s1].transitions[b'x' as usize], 0);
    }

    #[test]
    fn build_dfa_miss_routes_to_dead_then_self_loops() {
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        // miss from start lands on dead (idx 0)…
        let dead = dfa.states[dfa.start as usize].transitions[b'b' as usize];
        assert_eq!(dead, 0);
        // …and dead self-loops on every byte.
        for b in 0u16..=255 {
            assert_eq!(dfa.states[0].transitions[b as usize], 0);
        }
    }

    // dfa_search tests — anchored leftmost-longest driver.

    /// Round 3 Phase B sub-batch 4 attack #R-J v2 — `dfa_search` now
    /// takes `prog: &Program` to look up K-PROPERTY class tables. This
    /// helper used to discard `prog` after `build_dfa`; it now returns
    /// the `(Program, DfaProgram)` pair so callers can pass `&prog`
    /// to `dfa_search` / `dfa_search_mid*`.
    fn build_dfa_for(insts: &[Inst]) -> (Program, DfaProgram) {
        let mut prog = Program::new();
        for ins in insts {
            prog.emit(*ins);
        }
        let dfa = build_dfa(&prog, 0);
        (prog, dfa)
    }

    #[test]
    fn dfa_search_literal_matches_at_exact_length() {
        // 0: CHAR a; 1: CHAR b; 2: CHAR c; 3: MATCH
        let (prog, dfa) = build_dfa_for(&[
            Inst::char_lit(b'a'),
            Inst::char_lit(b'b'),
            Inst::char_lit(b'c'),
            Inst::match_accept(),
        ]);
        assert_eq!(dfa_search(&dfa, &prog, b"abc"), Some(3));
        assert_eq!(dfa_search(&dfa, &prog, b"abcd"), Some(3)); // trailing byte ignored
    }

    #[test]
    fn dfa_search_literal_misses_on_first_byte_mismatch() {
        let (prog, dfa) = build_dfa_for(&[Inst::char_lit(b'a'), Inst::match_accept()]);
        assert_eq!(dfa_search(&dfa, &prog, b"b"), None);
        assert_eq!(dfa_search(&dfa, &prog, b""), None);
    }

    #[test]
    fn dfa_search_match_only_accepts_empty() {
        let (prog, dfa) = build_dfa_for(&[Inst::match_accept()]);
        assert_eq!(dfa_search(&dfa, &prog, b""), Some(0));
        // accepts empty prefix even with trailing bytes — dead-state
        // halts the walk but the seeded `Some(0)` survives.
        assert_eq!(dfa_search(&dfa, &prog, b"anything"), Some(0));
    }

    #[test]
    fn dfa_search_kleene_star_matches_empty_and_extends() {
        // a*: 0: SPLIT 1, 3; 1: CHAR a; 2: JMP 0; 3: MATCH
        let (prog, dfa) = build_dfa_for(&[
            Inst::split(1, 3),
            Inst::char_lit(b'a'),
            Inst::jmp(0),
            Inst::match_accept(),
        ]);
        assert_eq!(dfa_search(&dfa, &prog, b""), Some(0));
        assert_eq!(dfa_search(&dfa, &prog, b"a"), Some(1));
        assert_eq!(dfa_search(&dfa, &prog, b"aaaa"), Some(4));
        // hits dead on 'b' after consuming a's, returns longest seen.
        assert_eq!(dfa_search(&dfa, &prog, b"aab"), Some(2));
        assert_eq!(dfa_search(&dfa, &prog, b"b"), Some(0));
    }

    #[test]
    fn dfa_search_kleene_plus_requires_one_byte() {
        // a+: 0: CHAR a; 1: SPLIT 0, 2; 2: MATCH
        let (prog, dfa) = build_dfa_for(&[
            Inst::char_lit(b'a'),
            Inst::split(0, 2),
            Inst::match_accept(),
        ]);
        assert_eq!(dfa_search(&dfa, &prog, b""), None);
        assert_eq!(dfa_search(&dfa, &prog, b"a"), Some(1));
        assert_eq!(dfa_search(&dfa, &prog, b"aaaa"), Some(4));
        assert_eq!(dfa_search(&dfa, &prog, b"b"), None);
    }

    #[test]
    fn dfa_search_alternation_takes_either_branch() {
        // 0: SPLIT 1, 3; 1: CHAR a; 2: MATCH; 3: CHAR b; 4: MATCH
        let (prog, dfa) = build_dfa_for(&[
            Inst::split(1, 3),
            Inst::char_lit(b'a'),
            Inst::match_accept(),
            Inst::char_lit(b'b'),
            Inst::match_accept(),
        ]);
        assert_eq!(dfa_search(&dfa, &prog, b"a"), Some(1));
        assert_eq!(dfa_search(&dfa, &prog, b"b"), Some(1));
        assert_eq!(dfa_search(&dfa, &prog, b"c"), None);
    }

    #[test]
    fn dfa_search_leftmost_longest_prefers_extended_accept() {
        // a*b: 0: SPLIT 1, 3; 1: CHAR a; 2: JMP 0; 3: CHAR b; 4: MATCH
        // Start does not accept (no MATCH in ε-closure of {0,1,3}).
        // 'aaab' should match 4 bytes.
        let (prog, dfa) = build_dfa_for(&[
            Inst::split(1, 3),
            Inst::char_lit(b'a'),
            Inst::jmp(0),
            Inst::char_lit(b'b'),
            Inst::match_accept(),
        ]);
        assert_eq!(dfa_search(&dfa, &prog, b"b"), Some(1));
        assert_eq!(dfa_search(&dfa, &prog, b"ab"), Some(2));
        assert_eq!(dfa_search(&dfa, &prog, b"aaab"), Some(4));
        // No 'b' tail → no match (the executor is anchored leftmost-longest,
        // doesn't yet retry suffixes — that's the search-from-position chunk).
        assert_eq!(dfa_search(&dfa, &prog, b"aaa"), None);
    }

    #[test]
    fn dfa_search_dead_state_short_circuits_after_miss() {
        // Pattern `ab`: walking "ac…" should stop at index 1 (state hits
        // dead) and return None regardless of tail content.
        let (prog, dfa) = build_dfa_for(&[
            Inst::char_lit(b'a'),
            Inst::char_lit(b'b'),
            Inst::match_accept(),
        ]);
        // Even with a megabyte of garbage, the executor only reads up to
        // the first mismatch — but we just sanity-check the answer.
        assert_eq!(dfa_search(&dfa, &prog, b"ac"), None);
        assert_eq!(dfa_search(&dfa, &prog, b"axxxxxxxxxx"), None);
    }

    #[test]
    fn dfa_search_class_range_matches_in_range_bytes() {
        // [0-9]+: CLASS [0-9]; SPLIT 0, 2; MATCH
        let mut prog = Program::new();
        let mut digit = CharClass::new();
        digit.add_range(b'0', b'9');
        let di = prog.intern_class(&digit);
        prog.emit(Inst::class_ref(di));
        prog.emit(Inst::split(0, 2));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        assert_eq!(dfa_search(&dfa, &prog, b"42"), Some(2));
        assert_eq!(dfa_search(&dfa, &prog, b"42x"), Some(2));
        assert_eq!(dfa_search(&dfa, &prog, b"x"), None);
    }

    #[test]
    fn dfa_search_anychar_consumes_exactly_one_byte() {
        // 0: ANY; 1: MATCH — chunk 10a `.` excludes `\n` without `s`.
        let (prog, dfa) = build_dfa_for(&[Inst::simple(Op::AnyChar), Inst::match_accept()]);
        // Start does not accept (no MATCH in {0}); first non-`\n` byte
        // takes us to an accepting state — match length 1.
        assert_eq!(dfa_search(&dfa, &prog, b""), None);
        assert_eq!(dfa_search(&dfa, &prog, b"a"), Some(1));
        // `\n` is the one byte `.` rejects without the `s` flag.
        assert_eq!(dfa_search(&dfa, &prog, b"\n"), None);
        // After the accept, the DFA stays in an accepting state for one
        // more byte (the build queues the post-accept set), so longer
        // input still reports the longest accepting position — which is
        // length 1 because the post-accept set has no further MATCH.
        assert_eq!(dfa_search(&dfa, &prog, b"ab"), Some(1));
    }

    #[test]
    fn dfa_search_anychar_matches_newline_under_s_flag() {
        // chunk 10a — with `s` (per-inst pad s-bit), `.` covers `\n`.
        let mut prog = Program::new();
        let mut any = Inst::simple(Op::AnyChar);
        any.pad = crate::parser::RE_FLAG_S as u16;
        prog.emit(any);
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        assert_eq!(dfa_search(&dfa, &prog, b"\n"), Some(1));
        assert_eq!(dfa_search(&dfa, &prog, b"a"), Some(1));
    }

    // chunk 8.5 — position-aware build_dfa with `start` / `start_mid`.
    // `^` (AnchorB) advances through `start` (text_start=true closure)
    // but stays terminal in `start_mid` (text_start=false closure).

    #[test]
    fn build_dfa_pattern_without_anchor_dedups_start_states() {
        // Plain `a` — no AnchorB, so the text_start=true and
        // text_start=false closures coincide; the dedup map collapses
        // them to the same DFA state.
        let (_prog, dfa) = build_dfa_for(&[Inst::char_lit(b'a'), Inst::match_accept()]);
        assert_eq!(dfa.start, dfa.start_mid);
        assert_ne!(dfa.start, 0);
    }

    #[test]
    fn build_dfa_anchor_b_distinct_start_states() {
        // `^a`: 0: ANCHOR_B; 1: CHAR a; 2: MATCH
        // start (text_start=true): closure = {0, 1} (ANCHOR_B advances)
        // start_mid (text_start=false): closure = {0} (ANCHOR_B blocks),
        // with no byte-step out of pc 0 (it's an inert ε op).
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnchorB));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        assert_ne!(dfa.start, dfa.start_mid);
        // From `start`, byte 'a' advances to an accepting state.
        let via_a = tgt(dfa.states[dfa.start as usize].transitions[b'a' as usize]);
        assert_ne!(via_a, 0);
        assert!(dfa.states[via_a].is_accept);
        // From `start_mid`, byte 'a' dead-ends — `^` blocked the closure.
        assert_eq!(
            dfa.states[dfa.start_mid as usize].transitions[b'a' as usize],
            0
        );
    }

    #[test]
    fn dfa_search_anchor_b_matches_at_text_start() {
        // `^abc`: 0: ANCHOR_B; 1: CHAR a; 2: CHAR b; 3: CHAR c; 4: MATCH
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnchorB));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::char_lit(b'b'));
        prog.emit(Inst::char_lit(b'c'));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        // anchored entry: `^abc` matches the start of "abc..." and
        // returns the match length (3 bytes consumed past anchor).
        assert_eq!(dfa_search(&dfa, &prog, b"abc"), Some(3));
        assert_eq!(dfa_search(&dfa, &prog, b"abcdef"), Some(3));
        assert_eq!(dfa_search(&dfa, &prog, b"xabc"), None);
    }

    #[test]
    fn dfa_search_mid_anchor_b_always_misses() {
        // Same `^abc`: from start_mid the AnchorB closure blocks, so
        // no input ever matches via dfa_search_mid.
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnchorB));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::char_lit(b'b'));
        prog.emit(Inst::char_lit(b'c'));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        assert_eq!(dfa_search_mid(&dfa, &prog, b"abc"), None);
        assert_eq!(dfa_search_mid(&dfa, &prog, b"abcdef"), None);
        assert_eq!(dfa_search_mid(&dfa, &prog, b""), None);
    }

    #[test]
    fn dfa_search_pattern_without_anchor_both_entries_equivalent() {
        // Plain `abc` — start and start_mid coincide, so both entries
        // return identical results.
        let (prog, dfa) = build_dfa_for(&[
            Inst::char_lit(b'a'),
            Inst::char_lit(b'b'),
            Inst::char_lit(b'c'),
            Inst::match_accept(),
        ]);
        for hay in [&b""[..], b"abc", b"abcd", b"axc"] {
            assert_eq!(
                dfa_search(&dfa, &prog, hay),
                dfa_search_mid(&dfa, &prog, hay),
                "hay={hay:?}"
            );
        }
    }

    #[test]
    fn dfa_search_anchor_b_alternation_anchored_branch_only() {
        // `^a|b`: SPLIT 1, 4; 1: ANCHOR_B; 2: CHAR a; 3: MATCH;
        //         4: CHAR b; 5: MATCH
        // start (text_start=true): both branches alive — accepts "a" or "b".
        // start_mid (text_start=false): anchored branch dies, only "b" path
        // remains.
        let mut prog = Program::new();
        prog.emit(Inst::split(1, 4));
        prog.emit(Inst::simple(Op::AnchorB));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        prog.emit(Inst::char_lit(b'b'));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        assert_ne!(dfa.start, dfa.start_mid);
        assert_eq!(dfa_search(&dfa, &prog, b"a"), Some(1));
        assert_eq!(dfa_search(&dfa, &prog, b"b"), Some(1));
        // From start_mid, 'a' branch is dead — only 'b' matches.
        assert_eq!(dfa_search_mid(&dfa, &prog, b"a"), None);
        assert_eq!(dfa_search_mid(&dfa, &prog, b"b"), Some(1));
    }

    #[test]
    fn build_dfa_anchor_b_then_match_accepts_empty_at_start() {
        // `^`: 0: ANCHOR_B; 1: MATCH
        // start: closure = {0, 1} (anchor advances, match in set) →
        // start state already accepts (empty match at text-start).
        // start_mid: closure = {0} (anchor blocks) → start_mid does
        // not accept.
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnchorB));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        assert!(dfa.states[dfa.start as usize].is_accept);
        assert!(!dfa.states[dfa.start_mid as usize].is_accept);
        assert_eq!(dfa_search(&dfa, &prog, b""), Some(0));
        assert_eq!(dfa_search(&dfa, &prog, b"x"), Some(0));
        assert_eq!(dfa_search_mid(&dfa, &prog, b""), None);
        assert_eq!(dfa_search_mid(&dfa, &prog, b"x"), None);
    }

    #[test]
    fn prog_ops_dfa_safe_accepts_every_can_dfa_op_after_chunk_8_6b() {
        // All zero-width and capture opcodes are DFA-resident now:
        // - AnchorB (chunk 8.5) via dual start states
        // - Save (chunk 9) via second-pass NFA at DFA boundary
        // - AnchorE (chunk 8.6a) via per-state is_accept_at_end
        // - WBound / NWBound (chunk 8.6b) via LeftByteAttr state key
        for op in [Op::AnchorB, Op::AnchorE, Op::WBound, Op::NWBound, Op::Save] {
            let mut p = Program::new();
            if matches!(op, Op::Save) {
                p.emit(Inst::save(0));
            } else {
                p.emit(Inst::simple(op));
            }
            p.emit(Inst::char_lit(b'a'));
            p.emit(Inst::match_accept());
            assert!(prog_ops_dfa_safe(&p), "{op:?} should now be safe");
        }
    }

    #[test]
    fn build_dfa_anchor_e_at_haystack_end_accepts() {
        // chunk 8.6a: `a$` — DFA must accept "a" only when the
        // haystack ends right after the `a`. Mid-byte `is_accept`
        // never fires on its own (Match PC is behind AnchorE).
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::simple(Op::AnchorE));
        prog.emit(Inst::match_accept());
        assert!(prog_ops_dfa_safe(&prog));
        let dfa = build_dfa(&prog, 0);
        // Standalone "a": consume one byte, then at-end accept.
        assert_eq!(dfa_search(&dfa, &prog, b"a"), Some(1));
        // "ab": consume `a`, then byte `b` lives in a state whose
        // PC set has the Match-after-AnchorE reachable only at end —
        // but the haystack has not ended, so no accept here either.
        assert_eq!(dfa_search(&dfa, &prog, b"ab"), None);
        // Empty hay: start state has no path to Match via at-end
        // closure (the `Op::Char` is still required).
        assert_eq!(dfa_search(&dfa, &prog, b""), None);
    }

    #[test]
    fn build_dfa_wbound_then_word_only_matches_at_boundary() {
        // chunk 8.6b — `/\bfoo/` style: WBound at cursor=0 needs
        // left=None|non-word + right=word. Standalone "foo" or
        // " foo" hits; "xfoo" misses at offset 0 (word-word, no
        // boundary). We test directly via dfa_search/dfa_search_mid*.
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::WBound));
        prog.emit(Inst::char_lit(b'f'));
        prog.emit(Inst::char_lit(b'o'));
        prog.emit(Inst::char_lit(b'o'));
        prog.emit(Inst::match_accept());
        assert!(prog_ops_dfa_safe(&prog));
        let dfa = build_dfa(&prog, 0);
        // Anchored at text-start: left=None (non-word), right='f'
        // (word) → boundary → match "foo".
        assert_eq!(dfa_search(&dfa, &prog, b"foo"), Some(3));
        // Mid-nonword entry: left=NonWord, right='f' (word) →
        // boundary → match.
        assert_eq!(dfa_search_mid_nonword(&dfa, &prog, b"foo"), Some(3));
        // Mid-word entry: left=Word, right='f' (word) → no boundary
        // → no match at offset 0.
        assert_eq!(dfa_search_mid_word(&dfa, &prog, b"foo"), None);
    }

    #[test]
    fn build_dfa_nwbound_only_matches_inside_word_run() {
        // chunk 8.6b — `/\Bfoo/`: NWBound advances when there is no
        // boundary. So "foo" at offset 0 (left=None / non-word, right
        // ='f' / word — boundary) does *not* match here.
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::NWBound));
        prog.emit(Inst::char_lit(b'f'));
        prog.emit(Inst::char_lit(b'o'));
        prog.emit(Inst::char_lit(b'o'));
        prog.emit(Inst::match_accept());
        assert!(prog_ops_dfa_safe(&prog));
        let dfa = build_dfa(&prog, 0);
        assert_eq!(dfa_search(&dfa, &prog, b"foo"), None);
        assert_eq!(dfa_search_mid_nonword(&dfa, &prog, b"foo"), None);
        // Mid-word entry: left=Word, right='f' (word) — no boundary,
        // NWBound advances → match.
        assert_eq!(dfa_search_mid_word(&dfa, &prog, b"foo"), Some(3));
    }

    #[test]
    fn build_dfa_anchor_e_only_pattern_accepts_empty_hay_at_zero() {
        // `$` alone — anchored DFA at offset 0 accepts only when
        // the haystack is already empty. Mid-haystack ends are
        // handled by the outer `search_from` loop (it advances `st`
        // and re-anchors); a single `dfa_search(b"x")` call walks
        // the byte 'x', finds no consumer (start state has no
        // byte-step out), goes dead, and returns None.
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnchorE));
        prog.emit(Inst::match_accept());
        assert!(prog_ops_dfa_safe(&prog));
        let dfa = build_dfa(&prog, 0);
        // Empty hay: start state's at-end closure reaches Match.
        assert_eq!(dfa_search(&dfa, &prog, b""), Some(0));
        // Non-empty hay: anchored DFA can't `consume` the byte (no
        // Char/AnyChar in the program), state goes dead → None.
        // The outer `search_from` is what runs dfa_search again at
        // `st=hay.len()` to find the zero-width end accept.
        assert_eq!(dfa_search(&dfa, &prog, b"x"), None);
    }

    #[test]
    fn build_dfa_save_walks_through_to_accept() {
        // chunk 9: build_dfa walks `Op::Save` as a no-op ε. The
        // capture group `(a)` compiles to `Save(0) Char(a) Save(1)
        // Match` (after the implicit whole-match Save 0/1 in
        // production; here we test the closure shape directly): the
        // resulting DFA must reach `Match` after consuming one `a`,
        // exactly like a non-captured `/a/` would.
        let mut prog = Program::new();
        prog.emit(Inst::save(0));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::save(1));
        prog.emit(Inst::match_accept());
        assert!(prog_ops_dfa_safe(&prog));
        let dfa = build_dfa(&prog, 0);
        // Start state is not accept (need to consume `a` first).
        assert!(!dfa.states[dfa.start as usize].is_accept);
        assert_eq!(dfa_search(&dfa, &prog, b"a"), Some(1));
        assert_eq!(dfa_search(&dfa, &prog, b"axyz"), Some(1));
        assert_eq!(dfa_search(&dfa, &prog, b""), None);
        assert_eq!(dfa_search(&dfa, &prog, b"x"), None);
    }

    // chunk 8.7 — ASCII case-fold under `RE_FLAG_I`. `byte_step` and
    // `build_dfa` now thread the flag and resolve `Op::Char` /
    // `Op::Class` against the case-paired byte when `i` is set.

    use crate::parser::RE_FLAG_I;

    fn char_ci(ch: u8) -> Inst {
        let mut i = Inst::char_lit(ch);
        i.pad = RE_FLAG_I as u16;
        i
    }

    fn class_ref_ci(idx: i32) -> Inst {
        let mut i = Inst::class_ref(idx);
        i.pad = RE_FLAG_I as u16;
        i
    }

    #[test]
    fn byte_step_i_flag_char_advances_on_both_cases() {
        // Under the per-inst i-bit both 'a' and 'A' advance; a plain
        // char inst stays case-sensitive.
        let mut prog = Program::new();
        prog.emit(char_ci(b'a'));
        prog.emit(Inst::match_accept());
        let mut plain = Program::new();
        plain.emit(Inst::char_lit(b'a'));
        plain.emit(Inst::match_accept());
        // Plain (no i-bit): only 'a' advances.
        assert_eq!(byte_step(&plain, &set(&[0]), b'a'), set(&[1]));
        assert!(byte_step(&plain, &set(&[0]), b'A').is_empty());
        // i-bit: both 'a' and 'A' advance.
        assert_eq!(byte_step(&prog, &set(&[0]), b'a'), set(&[1]));
        assert_eq!(byte_step(&prog, &set(&[0]), b'A'), set(&[1]));
        // Non-alpha bytes still respect literal compare.
        assert!(byte_step(&prog, &set(&[0]), b'0').is_empty());
    }

    #[test]
    fn byte_step_i_flag_class_matches_case_paired_byte() {
        // 0: CLASS [a-c](i); 1: MATCH — 'A' / 'B' / 'C' also match.
        let mut cc = CharClass::new();
        cc.add_range(b'a', b'c');
        let mut prog = Program::new();
        let idx = prog.intern_class(&cc);
        prog.emit(class_ref_ci(idx));
        prog.emit(Inst::match_accept());
        let mut plain = Program::new();
        let pidx = plain.intern_class(&cc);
        plain.emit(Inst::class_ref(pidx));
        plain.emit(Inst::match_accept());
        // Plain: only lowercase.
        assert_eq!(byte_step(&plain, &set(&[0]), b'a'), set(&[1]));
        assert!(byte_step(&plain, &set(&[0]), b'A').is_empty());
        // i-bit: uppercase pair matches via CharClass::test_fold.
        assert_eq!(byte_step(&prog, &set(&[0]), b'A'), set(&[1]));
        assert_eq!(byte_step(&prog, &set(&[0]), b'C'), set(&[1]));
        // Out-of-class bytes still miss.
        assert!(byte_step(&prog, &set(&[0]), b'D').is_empty());
        assert!(byte_step(&prog, &set(&[0]), b'1').is_empty());
    }

    #[test]
    fn build_dfa_i_flag_literal_accepts_both_cases() {
        // /abc/i: 0: CHAR a(i); 1: CHAR b(i); 2: CHAR c(i); 3: MATCH
        let mut prog = Program::new();
        prog.emit(char_ci(b'a'));
        prog.emit(char_ci(b'b'));
        prog.emit(char_ci(b'c'));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        // All eight case combinations of "abc" should accept under i.
        for hay in [
            &b"abc"[..],
            b"ABC",
            b"Abc",
            b"aBc",
            b"abC",
            b"AbC",
            b"aBC",
            b"ABc",
        ] {
            assert_eq!(dfa_search(&dfa, &prog, hay), Some(3), "hay={hay:?}");
        }
        // Non-alpha mismatch still misses.
        assert_eq!(dfa_search(&dfa, &prog, b"axc"), None);
        assert_eq!(dfa_search(&dfa, &prog, b"abd"), None);
    }

    #[test]
    fn build_dfa_no_i_flag_does_not_case_fold() {
        // Same /abc/ without flag — only "abc" matches.
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::char_lit(b'b'));
        prog.emit(Inst::char_lit(b'c'));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        assert_eq!(dfa_search(&dfa, &prog, b"abc"), Some(3));
        assert_eq!(dfa_search(&dfa, &prog, b"ABC"), None);
        assert_eq!(dfa_search(&dfa, &prog, b"Abc"), None);
    }

    #[test]
    fn build_dfa_i_flag_class_range_case_folds() {
        // /[a-z]/i: under the per-inst i-bit also matches A-Z.
        let mut prog = Program::new();
        let mut lower = CharClass::new();
        lower.add_range(b'a', b'z');
        let li = prog.intern_class(&lower);
        prog.emit(class_ref_ci(li));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        for hay in [&b"a"[..], b"z", b"M", b"Z"] {
            assert_eq!(dfa_search(&dfa, &prog, hay), Some(1), "hay={hay:?}");
        }
        // Digits stay outside the (folded) class.
        assert_eq!(dfa_search(&dfa, &prog, b"7"), None);
    }

    // chunk 8.8 (per-inst since regexp-modifiers) — multiline `^`.
    // `Op::AnchorB` advances when the ctx is at text-start *or* the
    // instruction's baked `pad` m-bit is set and `left_byte ==
    // Some(b'\n')`. The `vm::search_from_with_ws` wire picks
    // `dfa.start` at line-start cursor positions (gated on
    // `Program::has_ml_anchor_b`) and `dfa.start_mid` elsewhere.

    use crate::parser::RE_FLAG_M;

    fn anchor_b_ml() -> Inst {
        let mut i = Inst::simple(Op::AnchorB);
        i.pad = RE_FLAG_M as u16;
        i
    }

    #[test]
    fn build_dfa_mflag_for_anchor_b_pattern_compiles() {
        // `^a` with the m-bit baked on the anchor inst:
        // 0: ANCHOR_B(m); 1: CHAR a; 2: MATCH
        // Multiline `^` resolution is wire-level (see
        // `vm::search_from_with_ws`: line-start positions re-enter via
        // `dfa.start`). The BFS just has to produce a valid DFA — the
        // `start` state mirrors the no-flag DFA (AnchorB advances under
        // `is_text_start = true` regardless of the m-bit) and
        // `start_mid` still blocks AnchorB (left_byte = None in
        // ctx_mid_default).
        let mut prog = Program::new();
        prog.emit(anchor_b_ml());
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        // start (text_start=true closure) accepts 'a'.
        let via_a = tgt(dfa.states[dfa.start as usize].transitions[b'a' as usize]);
        assert!(
            dfa.states[via_a].is_accept,
            "start + 'a' must accept under the m-bit (text_start ctx)"
        );
        // start_mid (no left_byte) blocks AnchorB — all bytes dead.
        for b in [b'a', b'\n', b'x'] {
            assert_eq!(
                dfa.states[dfa.start_mid as usize].transitions[b as usize], 0,
                "byte {b:02x} from start_mid must be dead (wire re-enters at line-start)"
            );
        }
    }

    #[test]
    fn epsilon_closure_full_anchor_b_advances_under_mflag_after_newline() {
        // Direct ε-closure test against the per-inst m-bit semantic.
        let mut prog = Program::new();
        prog.emit(anchor_b_ml());
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        // m-bit set, left = '\n': AnchorB advances.
        let ctx_after_nl = PositionCtx {
            left_byte: Some(b'\n'),
            is_text_start: false,
            is_text_end: false,
        };
        let cl = crate::dfa::epsilon_closure_full(&prog, &[0], ctx_after_nl, Some(b'a'));
        assert!(
            cl.contains(&1),
            "AnchorB must advance after \\n under the m-bit"
        );
        // m-bit set, left = 'a': AnchorB stays terminal.
        let ctx_after_a = PositionCtx {
            left_byte: Some(b'a'),
            is_text_start: false,
            is_text_end: false,
        };
        let cl = crate::dfa::epsilon_closure_full(&prog, &[0], ctx_after_a, Some(b'b'));
        assert!(!cl.contains(&1));
        // m-bit clear, left = '\n': AnchorB stays terminal (plain `^`
        // inside a `(?-m:…)` scope or without the m flag).
        let mut plain = Program::new();
        plain.emit(Inst::simple(Op::AnchorB));
        plain.emit(Inst::char_lit(b'a'));
        plain.emit(Inst::match_accept());
        let cl = crate::dfa::epsilon_closure_full(&plain, &[0], ctx_after_nl, Some(b'a'));
        assert!(!cl.contains(&1));
    }

    // chunk 10b — u-flag `.` matches one code point (1-4 UTF-8 bytes)
    // via the BFS deferred[u_skip] buckets. byte_step_full splits the
    // PC into the right bucket on the first byte; subsequent
    // continuation bytes promote it forward until it lands in `ready`
    // exactly `cp_len` bytes after the first.

    #[test]
    fn byte_step_full_anychar_routes_multi_byte_to_deferred() {
        // 0: ANY(s); 1: MATCH — dotAll per-inst.
        let mut prog = Program::new();
        let mut any = Inst::simple(Op::AnyChar);
        any.pad = crate::parser::RE_FLAG_S as u16;
        prog.emit(any);
        prog.emit(Inst::match_accept());
        // ASCII first byte → ready advance (u_skip = 0).
        let (ready, def) = byte_step_full(&prog, &set(&[0]), b'a');
        assert_eq!(ready, set(&[1]));
        assert!(def[0].is_empty() && def[1].is_empty() && def[2].is_empty());
        // 2-byte first (0xCE) → deferred[0] (u_skip = 1).
        let (ready, def) = byte_step_full(&prog, &set(&[0]), 0xCE);
        assert!(ready.is_empty());
        assert_eq!(def[0], set(&[1]));
        // 3-byte first (0xE6) → deferred[1] (u_skip = 2).
        let (ready, def) = byte_step_full(&prog, &set(&[0]), 0xE6);
        assert!(ready.is_empty());
        assert_eq!(def[1], set(&[1]));
        // 4-byte first (0xF0) → deferred[2] (u_skip = 3).
        let (ready, def) = byte_step_full(&prog, &set(&[0]), 0xF0);
        assert!(ready.is_empty());
        assert_eq!(def[2], set(&[1]));
        // Continuation byte alone (no prior multi-byte context) is an
        // unpaired tail — defensive 1-byte advance per
        // `utf8_len_for`.
        let (ready, def) = byte_step_full(&prog, &set(&[0]), 0x80);
        assert_eq!(ready, set(&[1]));
        assert!(def.iter().all(|d| d.is_empty()));
    }

    /// `.` means one character in either mode — the deferral is not
    /// the u flag's doing. Stopping after a multi-byte character's
    /// first byte put the match boundary inside the character, which
    /// the string layer cannot slice.
    #[test]
    fn byte_step_full_defers_multi_byte_leads_without_the_u_flag_too() {
        let mut prog = Program::new();
        let mut any = Inst::simple(Op::AnyChar);
        any.pad = crate::parser::RE_FLAG_S as u16;
        prog.emit(any);
        prog.emit(Inst::match_accept());
        for (b, want) in [(0xCEu8, 0usize), (0xE6, 1), (0xF0, 2)] {
            let (ready, def) = byte_step_full(&prog, &set(&[0]), b);
            assert!(ready.is_empty(), "byte 0x{b:02x} advanced too early");
            assert_eq!(def[want], set(&[1]), "byte 0x{b:02x}");
        }
        for b in [b'a', 0x80u8] {
            let (ready, def) = byte_step_full(&prog, &set(&[0]), b);
            assert_eq!(ready, set(&[1]), "byte 0x{b:02x}");
            assert!(def.iter().all(|d| d.is_empty()), "byte 0x{b:02x}");
        }
    }

    #[test]
    fn dfa_search_u_flag_dot_consumes_four_byte_code_point() {
        // /^.$/u — under u-flag `.` matches one code point; the
        // anchored DFA walks 4 bytes and accepts at the haystack end
        // via `is_accept_at_end` (AnchorE closes through Match).
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnchorB));
        prog.emit(Inst::simple(Op::AnyChar));
        prog.emit(Inst::simple(Op::AnchorE));
        prog.emit(Inst::match_accept());
        let u = crate::parser::RE_FLAG_U;
        let dfa = build_dfa(&prog, u);
        // 😀 = U+1F600 = F0 9F 98 80 (4 bytes).
        let smile: &[u8] = b"\xF0\x9F\x98\x80";
        assert_eq!(dfa_search(&dfa, &prog, smile), Some(4));
        // ASCII "a" — 1 byte cp, also matches.
        assert_eq!(dfa_search(&dfa, &prog, b"a"), Some(1));
        // Two cps — `^.$` doesn't match.
        assert_eq!(dfa_search(&dfa, &prog, b"ab"), None);
        // 4-byte cp followed by extra ASCII — `$` fails after cp end.
        let mut smile_plus = smile.to_vec();
        smile_plus.push(b'x');
        assert_eq!(dfa_search(&dfa, &prog, &smile_plus), None);
    }

    #[test]
    fn dfa_search_u_flag_dot_two_byte_code_point_round_trip() {
        // /^.$/u over "Ω" = U+03A9 = CE A9 (2 bytes).
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnchorB));
        prog.emit(Inst::simple(Op::AnyChar));
        prog.emit(Inst::simple(Op::AnchorE));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, crate::parser::RE_FLAG_U);
        let omega: &[u8] = b"\xCE\xA9";
        assert_eq!(dfa_search(&dfa, &prog, omega), Some(2));
    }

    #[test]
    fn dfa_search_u_flag_dot_rejects_truncated_multi_byte_at_end() {
        // /^.$/u over a 3-byte first byte with only 1 continuation —
        // incomplete cp at hay end must not accept (deferred PCs are
        // lost; `is_accept_at_end` operates on ready only).
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnchorB));
        prog.emit(Inst::simple(Op::AnyChar));
        prog.emit(Inst::simple(Op::AnchorE));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, crate::parser::RE_FLAG_U);
        let truncated: &[u8] = b"\xE6\x88"; // first 2 of 3 bytes
        assert_eq!(dfa_search(&dfa, &prog, truncated), None);
    }

    // -------------------------------------------------------------------
    // Round 3 Phase B sub-batch 4 attack #R-J v2 (§2.5.E)
    //
    // K-PROPERTY pending-class executor handler tests. Compile-time
    // entry: `compile_uflag_pattern` builds a Program with K-PROPERTY
    // routing (no chunk-10d expansion); `build_dfa(..., RE_FLAG_U)`
    // emits the pending state. The state-count checkpoint
    // (`path_a_v2_letter_state_count_bound`) is the §3.4.E v2 STOP
    // gate — if it fails (> 10 states for `\p{L}+/u`), the §2.5.E
    // mechanism is unsound and the impl must NOT proceed to bench /
    // conformance.
    // -------------------------------------------------------------------

    fn compile_uflag_pattern(pat: &str) -> Program {
        use crate::compiler::compile;
        use crate::parser::Parser;
        use crate::resolve::resolve_backrefs;
        let flags = crate::parser::RE_FLAG_U;
        let mut p = Parser::new(pat.as_bytes(), flags);
        let mut root = p.parse().expect("Path A v2 test pattern must parse");
        let names = p.names.clone();
        resolve_backrefs(&mut root, &names, p.n_captures, flags);
        let mut prog = Program::new();
        compile(&mut prog, &root, flags);
        prog.emit(Inst::match_accept());
        prog
    }

    /// §4.3 test A — `\p{L}/u` (no `+`) compiles to a single Op::Class.
    #[test]
    fn path_a_v2_property_letter_compiles_to_single_op_class() {
        let prog = compile_uflag_pattern("\\p{L}");
        // [Op::Class, Op::Match]
        assert_eq!(prog.insts.len(), 2);
        assert_eq!(prog.insts[0].op, Op::Class as u8);
        assert_eq!(prog.insts[1].op, Op::Match as u8);
        assert_eq!(prog.classes.len(), 1);
        assert!(prog.classes[0].is_uflag_property_only());
    }

    /// §4.3 test B — §3.4.E v2 STOP gate. `\p{L}+/u` must yield ≤ 10
    /// DFA states (expected actual: 4-5). If this fails, the v2 §2.5.E
    /// mechanism is unsound — impl must NOT proceed to bench/conformance
    /// and must surface to main session for redesign.
    #[test]
    fn path_a_v2_letter_state_count_bound() {
        let prog = compile_uflag_pattern("\\p{L}+");
        let dfa = build_dfa(&prog, crate::parser::RE_FLAG_U);
        let n = dfa.states.len();
        assert!(
            n <= 10,
            "Path A v2 §3.4.E — \\p{{L}}+/u should yield ≤ 10 DFA \
             states (expected 4-5), got {n}. If > 10, the v2 §2.5.E \
             pending_class mechanism is unsound — STOP commit.",
        );
    }

    /// §4.3 test C — ASCII letters accepted via the K-PROPERTY pending
    /// handler. Start state is pending(\p{L}); first ASCII byte goes
    /// through the cp handler with utf8_len=1 → matches → yes_target.
    #[test]
    fn path_a_v2_property_letter_matches_ascii() {
        let prog = compile_uflag_pattern("\\p{L}+");
        let dfa = build_dfa(&prog, crate::parser::RE_FLAG_U);
        assert_eq!(dfa_search(&dfa, &prog, b"Hello"), Some(5));
        // "abc123" — stops at '1' which is K-NUMBER not K-LETTER.
        assert_eq!(dfa_search(&dfa, &prog, b"abc123"), Some(3));
        // Pure digits — first char already a miss → None.
        assert_eq!(dfa_search(&dfa, &prog, b"123"), None);
        assert_eq!(dfa_search(&dfa, &prog, b""), None);
    }

    /// §4.3 test D — non-ASCII letters accepted via multi-byte UTF-8
    /// decoding in the pending handler. Greek α (U+03B1 = 0xCE 0xB1)
    /// and CJK 中 (U+4E2D = 0xE4 0xB8 0xAD) are both K-LETTER cps.
    #[test]
    fn path_a_v2_property_letter_matches_non_ascii_first_cp() {
        let prog = compile_uflag_pattern("\\p{L}+");
        let dfa = build_dfa(&prog, crate::parser::RE_FLAG_U);
        // Greek α + non-letter '!' — pending handler consumes the
        // 2-byte cp, then post-pending byte-step on '!' dies. Match
        // length = 2 bytes (one cp).
        assert_eq!(dfa_search(&dfa, &prog, &[0xCE, 0xB1, b'!']), Some(2));
        // CJK 中 — 3-byte cp, single iteration. Match length = 3.
        assert_eq!(dfa_search(&dfa, &prog, &[0xE4, 0xB8, 0xAD, b'!']), Some(3));
    }

    /// §4.3 test E — non-letters rejected (cp-miss routes to no_target
    /// = dead). `'5'` (ASCII digit, not K-LETTER) and Arabic-Indic 4
    /// (U+0664, K-NUMBER but not K-LETTER) both miss.
    #[test]
    fn path_a_v2_property_letter_rejects_non_letters() {
        let prog = compile_uflag_pattern("\\p{L}+");
        let dfa = build_dfa(&prog, crate::parser::RE_FLAG_U);
        assert_eq!(dfa_search(&dfa, &prog, b"5!"), None);
        // Arabic-Indic 4 (0xD9 0xA4) — K-NUMBER, not K-LETTER.
        assert_eq!(dfa_search(&dfa, &prog, &[0xD9, 0xA4]), None);
    }

    /// §4.3 test F.1 — invalid UTF-8 lead byte (0xFF) routes to dead
    /// via the executor's `_ => return no_target` arm.
    #[test]
    fn path_a_v2_invalid_lead_drops_to_dead() {
        let prog = compile_uflag_pattern("\\p{L}+");
        let dfa = build_dfa(&prog, crate::parser::RE_FLAG_U);
        // 0xFF is an invalid UTF-8 lead → no_target = dead → None.
        assert_eq!(dfa_search(&dfa, &prog, &[0xFF]), None);
    }

    /// §4.3 test F.2 — truncated multi-byte sequence at hay end. Lead
    /// 0xE4 expects 3 total bytes; only 2 given → no_target = dead.
    #[test]
    fn path_a_v2_truncated_sequence_drops_to_dead() {
        let prog = compile_uflag_pattern("\\p{L}+");
        let dfa = build_dfa(&prog, crate::parser::RE_FLAG_U);
        assert_eq!(dfa_search(&dfa, &prog, &[0xE4, 0xB8]), None);
        // Lone continuation byte 0xBF — invalid lead path.
        assert_eq!(dfa_search(&dfa, &prog, &[0xBF]), None);
    }

    /// §4.3 test F.3 — invalid continuation byte. Lead 0xCE (2-byte
    /// lead) followed by another lead 0xCE (NOT a valid continuation
    /// `0x80..=0xBF`) → cp-miss → no_target.
    #[test]
    fn path_a_v2_invalid_continuation_drops_to_dead() {
        let prog = compile_uflag_pattern("\\p{L}+");
        let dfa = build_dfa(&prog, crate::parser::RE_FLAG_U);
        assert_eq!(dfa_search(&dfa, &prog, &[0xCE, 0xCE]), None);
    }

    /// RFC 20260711 chunk B — content-distinct property classes in one
    /// ready set (`\p{L}|\p{N}` disjunction) cannot be served by the
    /// single-class pending slot; the build must poison so consumers
    /// fall to the cp-aware Pike VM (was a silent byte-step fallback
    /// that returned `false` for `/\p{L}|\p{N}/u.test("α")`).
    /// Content-EQUAL duplicates (two `\p{L}` occurrences interned at
    /// distinct indices) stay serveable.
    #[test]
    fn multi_property_disjunction_poisons_dfa() {
        let prog = compile_uflag_pattern("\\p{L}|\\p{N}");
        let dfa = build_dfa(&prog, crate::parser::RE_FLAG_U);
        assert!(dfa.poisoned, "\\p{{L}}|\\p{{N}} must poison the DFA");

        let prog = compile_uflag_pattern("\\p{L}x|\\p{L}y");
        let dfa = build_dfa(&prog, crate::parser::RE_FLAG_U);
        assert!(
            !dfa.poisoned,
            "content-equal \\p{{L}} duplicates stay pending-serveable"
        );

        let prog = compile_uflag_pattern("\\p{L}+");
        let dfa = build_dfa(&prog, crate::parser::RE_FLAG_U);
        assert!(!dfa.poisoned, "single-class loop must not poison");
    }

    /// §4.3 test G — cp-boundary edge: 2-byte plane minimum letter
    /// U+00C0 (À) = 0xC3 0x80. 4-byte plane letters and non-letters
    /// resolve through the full UCD 16.0.0 gc L table (RFC 20260711
    /// chunk B — the pre-chunk-B curated table missed all of plane 1+,
    /// so U+10000 used to pin as a reject).
    #[test]
    fn path_a_v2_cp_boundary_decoding() {
        let prog = compile_uflag_pattern("\\p{L}+");
        let dfa = build_dfa(&prog, crate::parser::RE_FLAG_U);
        // U+00C0 À — 2-byte K-LETTER → match.
        assert_eq!(dfa_search(&dfa, &prog, &[0xC3, 0x80]), Some(2));
        // U+10000 Linear B Aa — 4-byte cp, gc Lo → match.
        assert_eq!(dfa_search(&dfa, &prog, &[0xF0, 0x90, 0x80, 0x80]), Some(4));
        // U+1F600 😀 — 4-byte cp, gc So (not a letter) → reject
        // (handler decodes cp + class.test_cp(cp) returns false).
        assert_eq!(dfa_search(&dfa, &prog, &[0xF0, 0x9F, 0x98, 0x80]), None);
    }

    /// §4.3 test I — nested K-PROPERTY under repeat
    /// (`(?:\p{L}+){2,}`). Must compile + match without state-count
    /// explosion. Risk R4 mitigation.
    #[test]
    fn path_a_v2_nested_kproperty_under_repeat() {
        let prog = compile_uflag_pattern("(?:\\p{L}+){2,}");
        let dfa = build_dfa(&prog, crate::parser::RE_FLAG_U);
        let n = dfa.states.len();
        assert!(
            n <= 20,
            "Path A v2 — nested K-PROPERTY under repeat state count: \
             {n} (≤ 20 expected)"
        );
        // Match the first \p{L}+ — second iteration of {2,} can match
        // against the same letters (no space between iterations
        // required since (?:X+){2,} ≡ X+).
        assert_eq!(dfa_search(&dfa, &prog, b"abc def"), Some(3));
    }

    /// Round 3 Phase B sub-batch 4 attack #R-J v3 option A — mid-match
    /// non-ASCII letter correctness. Under v2 (singleton-only pending
    /// detection) the post-pending state {1, 2, 4} fell back to
    /// `byte_step_full`'s ASCII-only `class.test(byte)`, silently
    /// dropping every non-ASCII cp after position 0. v3-A extends
    /// `compute_pending_class_for` to also handle multi-PC sets where
    /// every byte-consuming PC is the same K-PROPERTY class, so the
    /// loop-body state's pending_class drives the cp handler on EVERY
    /// iteration of `\p{L}+/u`, not just the first.
    ///
    /// This is the v3-A acceptance gate; failure means the multi-PC
    /// extension is unsound and v3-A must STOP commit.
    #[test]
    fn path_a_v3_letter_multi_byte_mid_match() {
        let prog = compile_uflag_pattern("\\p{L}+");
        let dfa = build_dfa(&prog, crate::parser::RE_FLAG_U);

        // ASCII-only baselines (must not regress vs v2).
        assert_eq!(dfa_search(&dfa, &prog, b"Hello"), Some(5));
        assert_eq!(dfa_search(&dfa, &prog, b"12345"), None);

        // Mid-match Greek α (U+03B1 = 0xCE 0xB1): "Hellα!" must
        // consume all 6 bytes (4 ASCII letters + 2-byte α). v2
        // returned Some(4) — α was dropped at the post-pending
        // boundary because the multi-PC ready set fell back to
        // `class.test(byte)` (ASCII bitmap only). v3-A's multi-PC
        // pending_class handler reads the cp via the executor handler
        // on every loop iteration → Some(6).
        let hay_hell_alpha = b"Hell\xCE\xB1!";
        assert_eq!(
            dfa_search(&dfa, &prog, hay_hell_alpha),
            Some(6),
            "v3-A — Greek α mid-match must be consumed (v2 regression \
             returned Some(4))"
        );

        // Two CJK letters: 漢 (U+6F22 = 0xE6 0xBC 0xA2) + 字
        // (U+5B57 = 0xE5 0xAD 0x97) = 6 bytes letter run.
        let kanji = b"\xE6\xBC\xA2\xE5\xAD\x97!";
        assert_eq!(
            dfa_search(&dfa, &prog, kanji),
            Some(6),
            "v3-A — second CJK 字 must be consumed (v2 regression \
             returned Some(3))"
        );

        // Mixed ASCII + Greek + space terminator: 3 ASCII + 2-byte α
        // = 5 bytes letter run.
        let mixed = b"abc\xCE\xB1 def";
        assert_eq!(
            dfa_search(&dfa, &prog, mixed),
            Some(5),
            "v3-A — ASCII+Greek mixed run must reach the space"
        );
    }

    /// §4.3 test J — K-MIXED (\p{L} plus explicit non-ASCII bit) must
    /// still flow through `utf8_class_expand`. Compiles to MULTIPLE
    /// Op::Class instructions (chunk-10d byte-step path). Risk R5
    /// mitigation.
    #[test]
    fn path_a_v2_kmixed_still_expands() {
        // `\d` is K-SAFE (ASCII-only), but `\d|\p{L}` would coalesce
        // into a single class via OR-union, blocked by negate-only.
        // Use a K-NEG (`[^a]`) for confirmation: it negates → not
        // K-PROPERTY → still expands.
        let prog = compile_uflag_pattern("[^a]");
        let class_count = prog
            .insts
            .iter()
            .filter(|i| i.op == Op::Class as u8)
            .count();
        assert!(
            class_count > 1,
            "K-NEG ([^a]/u) should expand to > 1 Op::Class — Path A \
             v2 should NOT take this path through pending_class. Got \
             class_count = {class_count}"
        );
    }
}
