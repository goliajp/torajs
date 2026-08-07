//! `DfaState` struct + Default + layout doc. Co-located with
//! [`super::search`]'s executor pre-#R-G but split out as the file
//! crossed the 500 LOC HARD limit when `monotone_accept` (and its
//! companion inner-loop fast path in `search.rs`) landed.
//!
//! The layout-lock unit test lives in `super::search::tests::
//! dfa_state_repr_c_layout_locked` so all `#[repr(C)]` offset
//! assertions sit alongside the AOT byte-emitter `INNER_DFA_STATE_SIZE`
//! constant they mirror.

use super::pending_class::PendingClass;

/// One state of a [`super::search::DfaProgram`].
///
/// `transitions[byte]` is a packed word: destination state index in
/// the low 30 bits (0 = dead state) plus the destination's
/// `is_accept` / `monotone_accept` flags in bits 31/30, folded in at
/// build time (Round 5 attack #9 — see
/// [`super::search::TX_ACCEPT_BIT`]). Dead slots stay exactly 0.
/// `is_accept` is true iff the PC set this state represents contains an
/// [`crate::program::Op::Match`] PC (i.e. the NFA can accept at this
/// byte position).
/// `is_accept_at_end` (chunk 8.6a) is true iff the PC set, when
/// re-closed under an at-end ctx (`is_text_end = true`), reaches an
/// [`crate::program::Op::Match`] — so the executor can accept a match
/// that depends on `$` (`Op::AnchorE`) firing at the hay end.
///
/// The 256-way transition table is dense — every byte slot is filled at
/// build time, so the executor is a single
/// `state = states[state].transitions[byte]` step per input byte.
/// Memory cost is `256 * 4 = 1024` bytes per state plus 1 byte for the
/// flag (padded to 4); future sparse map can replace it when state
/// counts blow past a hot-cache budget. Capture writes are handled by
/// the wire's second-pass Pike VM, not in the DFA itself (chunk 9).
///
/// V0.2 P14 chunk 7.7 v2 step 12 C2 Phase C-1 (2026-06-24) — `#[repr(C)]`
/// locks declared field order + C alignment so the AOT pipeline
/// (ssa_lower) can emit a byte-identical `[DfaState; N]` static into
/// `.rodata` and the `DfaStates::Static(&'static [...])` reader (Phase
/// B) deserialises it byte-for-byte.
///
/// Round 3 Phase B sub-batch 4 attack #R-J v2 (§2.5.E, 2026-06-24) —
/// appended a 12-byte [`PendingClass`] triple for K-PROPERTY cp-step
/// support, growing the layout to 1072 bytes. The new field is
/// [`PendingClass::INERT`] for every state that is NOT a K-PROPERTY
/// pending state, so the executor's hot path adds only one byte read +
/// one branch on `pending_class.active != 0`. Round 3 Phase B
/// sub-batch 6 attack #R-G (2026-06-25) — filled 1 of the 2 padding
/// bytes between `is_accept_at_end` and `accept_before_byte` with a
/// `monotone_accept` bool; the layout stays 1072 bytes / align 4.
/// Layout on aarch64-apple-darwin:
///
/// | offset | size | field |
/// | ------ | ---- | ----- |
/// | 0      | 1024 | transitions[256] |
/// | 1024   | 1    | is_accept |
/// | 1025   | 1    | is_accept_at_end |
/// | 1026   | 1    | monotone_accept (Round 3 Phase B sub-batch 6 #R-G) |
/// | 1027   | 1    | (padding to align 4) |
/// | 1028   | 32   | accept_before_byte[8] |
/// | 1060   | 12   | pending_class (Round 3 Path A v2) |
///
/// total = 1072 bytes, align 4. The unit test
/// `dfa_state_repr_c_layout_locked` enforces this layout so accidental
/// field-reorder / type-change breaks `cargo test` before reaching the
/// AOT byte emitter (where the same miscalculation would corrupt
/// `.rodata` silently).
#[repr(C)]
pub struct DfaState {
    /// `transitions[byte]` = destination state index. 0 means dead.
    pub transitions: [u32; 256],
    /// True iff the NFA PC set behind this state contains [`crate::program::Op::Match`].
    pub is_accept: bool,
    /// chunk 8.6a — true iff re-closing the PC set under an at-end ctx
    /// (`is_text_end = true`) reaches [`crate::program::Op::Match`].
    /// Set offline at build time; consumed by [`dfa_search_from`] after
    /// the byte walk to honour `Op::AnchorE` (`$`) at the haystack end.
    pub is_accept_at_end: bool,
    /// Round 3 Phase B sub-batch 6 attack #R-G — true iff this state
    /// is `is_accept == true` AND every non-dead `transitions[b]` lands
    /// on a state with `is_accept == true`, AND when
    /// `pending_class.active != 0` both `yes_target` and `no_target`
    /// either equal 0 (dead) or land on an `is_accept` state. Set
    /// offline at build time by `compute_monotone_accept` after the
    /// BFS completes; consumed by [`dfa_search_from`] to skip the
    /// per-byte `last_accept = Some(cursor)` write on the `\p{L}+/u`-
    /// class self-loop hot path. Saves ~2-8 ns/iter on letter-heavy
    /// haystacks. The exit boundary (transition out of monotone
    /// region, dead, or haystack end) writes `last_accept` once.
    pub monotone_accept: bool,
    /// Explicit 1-byte padding so `accept_before_byte` lands
    /// deterministically at offset 1028, mirroring
    /// [`PendingClass::_pad`]. The AOT bake path serialises a
    /// `DfaState` by reading its bytes through `*const u8`
    /// (`ssa_lower_regex_bake::try_bake_regex_dfa`), and a compiler-
    /// inserted padding hole would be read there as uninitialised
    /// memory — whatever the allocator last left in that byte lands
    /// verbatim in `__DATA_CONST`, so the same source built twice
    /// produced two different binaries. Declaring the byte gives it a
    /// defined value and keeps the serialised image total.
    pub _pad: u8,
    /// chunk 8.6b — 256-bit packed mask. Bit `b` is set iff re-closing
    /// the PC set at this state with `right_byte = Some(b)` reaches
    /// `Op::Match`. Lets the executor record a zero-width accept at
    /// cursor `i` *before* stepping byte `hay[i]` — required for
    /// patterns like `/\bword\b/` where the trailing `\b` resolves
    /// against the byte the cursor is about to consume, but the
    /// resulting `Op::Match` is zero-width (it never survives a
    /// `byte_step` that doesn't consume).
    pub accept_before_byte: [u32; 8],
    /// Round 3 Phase B sub-batch 4 attack #R-J v2 (§2.5.E) — fixed-size
    /// per-state K-PROPERTY cp-step handler triple. When
    /// `pending_class.active != 0` the executor short-circuits the
    /// 256-way `transitions[byte]` lookup, decodes 1-4 UTF-8 bytes as
    /// one cp, calls
    /// `prog.classes[pending_class.class_idx].test_cp(cp)`, then
    /// transitions to `yes_target` (matched, advances by utf8_len) or
    /// `no_target` (not matched / invalid UTF-8). Default
    /// [`PendingClass::INERT`] (all-zero) keeps non-K-PROPERTY states on
    /// the existing dense-transitions fast path.
    pub pending_class: PendingClass,
}

impl Default for DfaState {
    fn default() -> Self {
        Self {
            transitions: [0u32; 256],
            is_accept: false,
            is_accept_at_end: false,
            monotone_accept: false,
            _pad: 0,
            accept_before_byte: [0u32; 8],
            pending_class: PendingClass::INERT,
        }
    }
}
