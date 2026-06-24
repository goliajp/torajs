//! V0.2 P14 chunk 7.7 v2 step 12 C2 Phase C-5b — layout pass for the
//! AOT-baked DFA region.
//!
//! Each `UserBakedRegexEntry` (carried in `LinkConfig::baked_regex_entries`,
//! sourced from the SSA-side `ssa::Module::baked_regex_entries`)
//! materialises as a pair in `__DATA_CONST`:
//!
//! 1. An inner `[DfaState; N]` table (private symbol —
//!    `__torajs_baked_regex_states_<index>`, never enters the public
//!    sym table; the outer entry's `states_ptr` slot points here via
//!    the chain-LC rebase chain so dyld stamps the runtime vaddr
//!    after the ASLR slide).
//! 2. An outer `BakedDfaMeta` 32-byte struct (`__torajs_baked_regex_<index>` —
//!    public, the user-binary call to
//!    `__torajs_regex_compile_from_static_dfa(meta, pat, flag)` lands
//!    here). Inline `states_ptr` field at offset 0 is the rebase slot;
//!    `states_len + start*4` fill the remaining 24 bytes per the
//!    `BakedDfaMeta` doc comment on `torajs_regex::dfa::BakedDfaMeta`.
//!
//! Phase C-5b scaffold: defines `Layout` + the empty / zero-entry
//! input fast path of `compute_user_regex_baked_layout`. Real
//! `states[N]` + meta byte emission lands in C-5b.2 alongside the
//! payload builder; chain-LC rebase target registration lands in C-5c.
//! Until then `archive_link.rs` does not yet call into this module
//! and `baked_regex_entries` stays empty per Phase C-5a's plumbing.

use crate::exec::UserBakedRegexEntry;

// Phase C-5b scaffold — these 5 constants describe the on-disk
// `BakedDfaMeta` + `[DfaState; N]` byte layout the C-5b.2 payload
// builder will use to pack the rodata blob. They have no consumer
// yet (the non-empty `compute_user_regex_baked_layout` arm is
// `unimplemented!()`), so the workspace `warnings = "deny"` gate
// flags them dead. `#[allow(dead_code)]` until C-5b.2 lands the
// payload builder + consumes each constant.

/// On-disk size of the `BakedDfaMeta` outer struct — must match the
/// `#[repr(C)]` layout documented at `torajs_regex::dfa::BakedDfaMeta`.
/// 8-byte `states_ptr` (chain-LC rebased) + 4-byte `states_len` +
/// 4*4-byte start indices = 28 bytes, plus 4-byte tail pad to align 8 = 32.
#[allow(dead_code)]
pub(super) const OUTER_META_SIZE: u32 = 32;
/// `BakedDfaMeta` alignment (pointer-aligned native word on aarch64).
#[allow(dead_code)]
pub(super) const OUTER_META_ALIGN: u32 = 8;
/// Byte offset of the `states_ptr` slot inside `BakedDfaMeta`; this
/// is the chain-LC rebase target.
#[allow(dead_code)]
pub(super) const OUTER_STATES_PTR_OFFSET_IN_META: u32 = 0;
/// On-disk size of one `DfaState` per the `#[repr(C)]` layout doc on
/// `torajs_regex::dfa::DfaState`. 256*4 transitions + 2*1 bool +
/// 2-byte pad + 8*4 accept_before_byte = 1060 bytes.
#[allow(dead_code)]
pub(super) const INNER_DFA_STATE_SIZE: u32 = 1060;
/// `DfaState` alignment (driven by the `u32` transitions array).
#[allow(dead_code)]
pub(super) const INNER_DFA_STATE_ALIGN: u32 = 4;

/// Per-entry placement record produced by the layout pass; sym +
/// vaddr fed into `apply_user_regex_baked_overrides` once link-time
/// vaddrs are known.
#[derive(Debug, Clone)]
pub struct UserBakedRegexEntryLayout {
    /// `__torajs_baked_regex_<index>` — public outer symbol the
    /// AOT-emitted call site references.
    pub meta_sym: String,
    /// Absolute vaddr of the `BakedDfaMeta` outer struct.
    pub meta_vaddr: u64,
    /// File offset of the `BakedDfaMeta` outer struct.
    pub meta_file_offset: u32,
    /// `__torajs_baked_regex_states_<index>` — private inner symbol
    /// (rebase target only; never enters the public sym table). Reads
    /// happen through the chain-LC rebased `BakedDfaMeta::states_ptr`
    /// slot at runtime.
    pub states_sym: String,
    /// Absolute vaddr of the `[DfaState; N]` inner table.
    pub states_vaddr: u64,
    /// File offset of the `[DfaState; N]` inner table.
    pub states_file_offset: u32,
    /// Mirror of the SSA-side `states_len` — payload row count.
    pub states_len: u32,
}

/// Aggregated layout for the full baked-regex region within one
/// build. C-5b.2's payload builder reads this to byte-emit the inner
/// `[DfaState; N]` tables + outer `BakedDfaMeta` structs at the
/// recorded file offsets.
#[derive(Debug, Clone, Default)]
pub struct UserRegexBakedLayout {
    pub entries: Vec<UserBakedRegexEntryLayout>,
    /// Region's first byte file offset (= inner tables start).
    pub file_offset: u32,
    /// Region's first byte vaddr.
    pub vaddr: u64,
    /// Total cumulative byte size = inner tables + alignment pad +
    /// outer table.
    pub total_size: u32,
}

/// Compute per-entry placements for one baked-regex set, starting at
/// `(file_offset_base, vaddr_base)`. Phase C-5b scaffold returns the
/// zero-sized default on empty input; the full layout (inner tables +
/// outer table placement at aligned offsets) lands in C-5b.2 once the
/// payload builder consumes it.
pub fn compute_user_regex_baked_layout(
    entries: &[UserBakedRegexEntry],
    file_offset_base: u32,
    vaddr_base: u64,
) -> UserRegexBakedLayout {
    if entries.is_empty() {
        return UserRegexBakedLayout {
            entries: Vec::new(),
            file_offset: file_offset_base,
            vaddr: vaddr_base,
            total_size: 0,
        };
    }
    // C-5b.2 — non-empty layout will:
    //   1. pad to INNER_DFA_STATE_ALIGN (4) and lay out N inner
    //      `[DfaState; entry.states_len]` tables contiguously, each
    //      `entry.states_len * INNER_DFA_STATE_SIZE` bytes;
    //   2. pad to OUTER_META_ALIGN (8) and lay out N `BakedDfaMeta`
    //      structs at OUTER_META_SIZE each, recording per-entry
    //      `meta_vaddr` / `meta_file_offset` for sym apply +
    //      `states_vaddr` for the chain-LC rebase target;
    //   3. set `total_size` to the cumulative running offset.
    // The scaffolding leaves this branch unimplemented so the empty
    // path is exercised by every current code path (gates hold at
    // 1130/0/4); the unimplemented arm only fires once the upstream
    // Phase C-6 starts pushing entries AND C-5b.2 lands the payload
    // builder simultaneously.
    unimplemented!(
        "Phase C-5b.2 payload + sym + rebase pipeline is the next \
         landing step; current SSA Module never pushes baked_regex_entries \
         so this branch stays unreachable until C-6 trips the AOT gate."
    )
}

/// Sym names to flag as link-defined in the worklist closure so a
/// user-fn `RelocKind::Page21 / PageOff12 { target_sym }` against the
/// outer symbol isn't surfaced as `UnresolvedExterns` before emit.
/// Mirrors `user_data_globals_extra_defined_syms`. Phase C-5b scaffold
/// returns the outer symbols only; the private inner `states_sym`
/// stays internal (never reloc'd from user code — readers go through
/// the rebased `BakedDfaMeta::states_ptr` slot).
pub fn user_regex_baked_extra_defined_syms(
    entries: &[UserBakedRegexEntry],
) -> std::collections::BTreeSet<String> {
    entries
        .iter()
        .map(|e| format!("___torajs_baked_regex_{}", e.index))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_zero_size() {
        let layout = compute_user_regex_baked_layout(&[], 0x4_0000, 0x1_0001_0000);
        assert!(layout.entries.is_empty());
        assert_eq!(layout.total_size, 0);
        assert_eq!(layout.file_offset, 0x4_0000);
        assert_eq!(layout.vaddr, 0x1_0001_0000);
    }

    #[test]
    fn empty_input_empty_sym_set() {
        let syms = user_regex_baked_extra_defined_syms(&[]);
        assert!(syms.is_empty());
    }

    #[test]
    fn sym_set_uses_apple_underscore_prefix() {
        let entries = vec![
            UserBakedRegexEntry {
                index: 0,
                states_payload: Vec::new(),
                states_len: 0,
                start: 0,
                start_mid: 0,
                start_mid_word: 0,
                start_mid_nonword: 0,
            },
            UserBakedRegexEntry {
                index: 7,
                states_payload: Vec::new(),
                states_len: 0,
                start: 0,
                start_mid: 0,
                start_mid_word: 0,
                start_mid_nonword: 0,
            },
        ];
        let syms = user_regex_baked_extra_defined_syms(&entries);
        assert!(syms.contains("___torajs_baked_regex_0"));
        assert!(syms.contains("___torajs_baked_regex_7"));
        assert_eq!(syms.len(), 2);
    }

    // Note: `OUTER_META_SIZE` / `OUTER_META_ALIGN` /
    // `INNER_DFA_STATE_SIZE` / `INNER_DFA_STATE_ALIGN` are locked
    // against the runtime `#[repr(C)]` layout in
    // `torajs_regex::dfa::search::tests::{baked_dfa_meta_repr_c_layout_locked,
    // dfa_state_repr_c_layout_locked}` (Phase C-1). Adding
    // `torajs-regex` as a dev-dep here would introduce a reverse
    // crate edge (torajs-link is upstream of torajs-regex in the
    // dep graph — it materialises regex-related rodata into the
    // user binary), so we mirror the constants and rely on the
    // upstream layout-lock tests to catch any drift before the
    // workspace `cargo nextest run` reaches torajs-link.
}
