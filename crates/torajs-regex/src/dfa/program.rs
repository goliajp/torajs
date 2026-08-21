//! The DFA a compiled pattern carries, in the two shapes it arrives
//! in — `DfaProgram`, built at RegExp construction, and
//! `BakedDfaMeta`, its `.rodata` twin the AOT pipeline emits and the
//! linker mirrors (`torajs-link/src/user_regex_baked_layout.rs`).
//!
//! Split out of `search.rs` in rotation 470, when the dead-start scan
//! pushed that file past the 500-line cap: `search.rs` answers how
//! long a match starting at a position is, and this file only
//! describes what the matcher is handed.

pub use super::state::DfaState;

/// A built DFA — dense transition table + four anchored start state
/// indices. `states[0]` = dead (empty PC set, self-loops, never
/// accepts).
///
/// Start-state selection (chunk 8.6b):
/// - `start` — cursor at text-start (offset 0) or — under `RE_FLAG_M`
///   — immediately after `\n`. `is_text_start = true` so `^`
///   advances; left attr folds in as `TextStart` (no preceding byte
///   for WBound — `at_word_boundary` treats `None` as non-word).
/// - `start_mid_word` — cursor at offset > 0 with the just-consumed
///   byte being a word-class byte (`[A-Za-z0-9_]`). `^` blocks; WBound
///   sees `left = Word`.
/// - `start_mid_nonword` — cursor at offset > 0 with the just-
///   consumed byte being non-word and not a line-start trigger. `^`
///   blocks; WBound sees `left = NonWord`.
/// - `start_mid` — back-compat alias, equals `start_mid_nonword` (the
///   no-WBound case where `left_byte = None` had been the BFS seed).
///
/// Patterns without `Op::AnchorB` / `Op::WBound` / `Op::NWBound` dedup
/// all four start states down. Caller must gate via `prog.can_dfa`
/// + [`super::prog_ops_dfa_safe`].
/// V0.2 P14 chunk 7.7 v2 step 12 C2 Phase B (2026-06-24) — prep for
/// AOT-precompile DFA. Wraps `DfaProgram::states` so the same struct
/// supports both:
/// - runtime `build_dfa` returns `DfaStates::Owned(Vec<DfaState>)`
/// - tr build pipeline emits `DfaStates::Static(&'static [DfaState])`
///   from .rodata-baked tables (Phase C)
///
/// Read sites stay `dfa.states[i]` via `Deref<Target = [DfaState]>` +
/// auto-deref-and-index; no per-call-site changes needed beyond the
/// `build_dfa` wrap site. `DfaState` does NOT need `Clone` (vs Cow's
/// `ToOwned` requirement), so the slice-ification has zero cost on
/// the cold path.
pub enum DfaStates {
    Owned(alloc::vec::Vec<DfaState>),
    /// Reserved for Phase C — currently no producer; the variant
    /// lives so `dfa.states.deref()` lowers identically for both
    /// shapes today and stays stable when Phase C lands.
    Static(&'static [DfaState]),
}

impl core::ops::Deref for DfaStates {
    type Target = [DfaState];
    #[inline]
    fn deref(&self) -> &[DfaState] {
        match self {
            Self::Owned(v) => v.as_slice(),
            Self::Static(s) => s,
        }
    }
}

pub struct DfaProgram {
    pub states: DfaStates,
    pub start: u32,
    pub start_mid: u32,
    pub start_mid_word: u32,
    pub start_mid_nonword: u32,
    /// Round 3 Phase B attack #R-A2 — `true` iff
    /// `start == start_mid == start_mid_word == start_mid_nonword`.
    /// Patterns without `^` / `\b` / `\B` / multiline-`^` dedup the
    /// four start indices during BFS (`needs_attr_split == false`); the
    /// runtime `at_line_start` + `prev_is_word` selection branch is
    /// then wasted work. The wire (`vm::search_from_with_ws`) reads
    /// this flag and short-circuits to a single `dfa_search` when set.
    pub all_starts_equal: bool,
    /// Round 3 Phase B attack #R-E — `true` iff at least one
    /// `DfaState` in `states` has a non-zero `accept_before_byte`
    /// mask (i.e. the pattern contains `\b` / `\B` / multiline-`$` /
    /// other zero-width accepts). When `false`, the per-byte
    /// `mask_get` call in `dfa_search_from` is wasted work and the
    /// executor gates it off, saving ~35 ns/iter on no-`\b` patterns
    /// (every fixture without word-boundary or other zero-width
    /// accept site benefits).
    pub any_accept_before_byte: bool,
    /// RFC 20260711 chunk B — `true` when the BFS met a K-PROPERTY
    /// state shape the single-class pending mechanism cannot serve
    /// (content-distinct property classes in one ready set, e.g.
    /// `\p{L}|\p{N}`, or K-PROPERTY mixed with deferred bytes). A
    /// poisoned DFA would silently drop non-ASCII cps on those
    /// states, so every consumer (eager `dfa_runtime`, `resolve_dfa`
    /// local build, AOT bake) discards it and the cp-aware Pike VM
    /// serves the program. Never baked — `baked_dfa_view` hardcodes
    /// `false` because `try_bake_regex_dfa` refuses poisoned builds.
    pub poisoned: bool,
}

/// V0.2 P14 chunk 7.7 v2 step 12 C2 Phase C-1 (2026-06-24) — C-ABI
/// metadata struct emitted by the AOT pipeline (ssa_lower) alongside a
/// `[DfaState; N]` static into the user binary's `.rodata`. The
/// runtime constructor `__torajs_regex_compile_from_static_dfa`
/// (Phase C-2) reads one of these to build a `DfaProgram` whose
/// `states` is `DfaStates::Static(...)` over the baked slice, then
/// pre-fills `RegExp::dfa_cache` so the surface match path never
/// touches the (single-mutator UB-flaky) lazy build path for
/// AOT-eligible literal regexes — the central reason Phase C exists.
///
/// Layout on aarch64-apple-darwin (Round 3 Phase B sub-batch 1
/// extension — `any_accept_before_byte` baked into existing tail pad
/// at offset 28; `OUTER_META_SIZE` stays 32):
///
/// | offset | size | field |
/// | ------ | ---- | ----- |
/// | 0      | 8    | states_ptr (chain-LC rebased at link) |
/// | 8      | 4    | states_len |
/// | 12     | 4    | start |
/// | 16     | 4    | start_mid |
/// | 20     | 4    | start_mid_word |
/// | 24     | 4    | start_mid_nonword |
/// | 28     | 1    | any_accept_before_byte (attack #R-E) |
/// | 29     | 3    | (tail padding to align 8) |
///
/// total = 32 bytes, align 8 (pointer-aligned, native word size). The
/// unit test `baked_dfa_meta_repr_c_layout_locked` enforces this
/// layout — accidental field-reorder or type-change is caught before
/// the AOT byte emitter rather than corrupting linker output.
///
/// Note `all_starts_equal` (attack #R-A2) is NOT baked — the four
/// `start*` indices already live in this struct, so the runtime
/// `baked_dfa_view` constructor derives the flag locally for ~5 ns
/// once per call (amortised across all 100k iters of a hot loop).
#[repr(C)]
pub struct BakedDfaMeta {
    /// Pointer to a `.rodata`-backed `[DfaState; states_len]` table.
    /// Rebased by the chain-LC dyld fixup chain at load time (same
    /// mechanism vtable + class_layouts ride on) so ASLR slide is
    /// honoured. Always non-null in well-formed input.
    pub states_ptr: *const DfaState,
    /// Number of `DfaState` entries at `states_ptr`. Stored as `u32`
    /// not `usize` since DFA state count is bounded by the BFS subset
    /// construction's state cap (well under 4 billion); locking the
    /// width keeps the C-ABI struct word-portable across host
    /// architectures should that ever matter.
    pub states_len: u32,
    /// Four anchored start state indices — exact mirror of
    /// `DfaProgram::{start, start_mid, start_mid_word, start_mid_nonword}`.
    pub start: u32,
    pub start_mid: u32,
    pub start_mid_word: u32,
    pub start_mid_nonword: u32,
    /// Round 3 Phase B attack #R-E — host-pre-computed mirror of
    /// `DfaProgram::any_accept_before_byte`. Lives in the 4-byte tail
    /// pad of the original layout so the struct size stays 32 bytes
    /// (`OUTER_META_SIZE` unchanged) and the chain-LC rebase slot at
    /// offset 0 doesn't move. ssa_lower computes this from the
    /// built DFA's per-state masks before serialising the entry.
    pub any_accept_before_byte: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::offset_of;

    /// Phase C-1 layout lock — `BakedDfaMeta` mirrors the
    /// 32-byte AOT metadata struct ssa_lower emits per literal regex.
    /// See doc comment on the struct for the offset table.
    #[test]
    fn baked_dfa_meta_repr_c_layout_locked() {
        assert_eq!(offset_of!(BakedDfaMeta, states_ptr), 0);
        assert_eq!(offset_of!(BakedDfaMeta, states_len), 8);
        assert_eq!(offset_of!(BakedDfaMeta, start), 12);
        assert_eq!(offset_of!(BakedDfaMeta, start_mid), 16);
        assert_eq!(offset_of!(BakedDfaMeta, start_mid_word), 20);
        assert_eq!(offset_of!(BakedDfaMeta, start_mid_nonword), 24);
        // Round 3 Phase B attack #R-E — `any_accept_before_byte` sits
        // in the original 4-byte tail pad; struct stays 32 bytes /
        // align 8, so `OUTER_META_SIZE` in user_regex_baked_layout is
        // unchanged.
        assert_eq!(offset_of!(BakedDfaMeta, any_accept_before_byte), 28);
        assert_eq!(size_of::<BakedDfaMeta>(), 32);
        assert_eq!(align_of::<BakedDfaMeta>(), 8);
    }
}
