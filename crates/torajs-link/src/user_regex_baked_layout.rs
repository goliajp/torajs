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
//! Phase C-5b.2 lands the full layout pass + `build_user_regex_baked_payload`
//! byte emitter + `apply_user_regex_baked_overrides` sym registration +
//! `build_user_regex_baked_region` pipeline helper. Chain-LC rebase target
//! registration (states_ptr slot → states_vaddr) lands in C-5c. AOT gate
//! that pushes entries from ssa_lower lands in C-6. Until C-5c wires the
//! region into archive_link, `baked_regex_entries` stays empty per Phase
//! C-5a's plumbing and the non-empty arms here are exercised only by unit
//! tests.

use crate::chained_fixups_starts::RebaseTarget;
use crate::exec::UserBakedRegexEntry;
use crate::resolve::SymTable;

/// On-disk size of the `BakedDfaMeta` outer struct — must match the
/// `#[repr(C)]` layout documented at `torajs_regex::dfa::BakedDfaMeta`.
/// 8-byte `states_ptr` (chain-LC rebased) + 4-byte `states_len` +
/// 4*4-byte start indices = 28 bytes, plus 4-byte tail pad to align 8 = 32.
pub(super) const OUTER_META_SIZE: u32 = 32;
/// `BakedDfaMeta` alignment (pointer-aligned native word on aarch64).
pub(super) const OUTER_META_ALIGN: u32 = 8;
/// Byte offset of the `states_ptr` slot inside `BakedDfaMeta`; this
/// is the chain-LC rebase target.
pub(super) const OUTER_STATES_PTR_OFFSET_IN_META: u32 = 0;
/// On-disk size of one `DfaState` per the `#[repr(C)]` layout doc on
/// `torajs_regex::dfa::DfaState`. 256*4 transitions + 2*1 bool +
/// 2-byte pad + 8*4 accept_before_byte = 1060 bytes.
pub(super) const INNER_DFA_STATE_SIZE: u32 = 1060;
/// `DfaState` alignment (driven by the `u32` transitions array).
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
/// `(file_offset_base, vaddr_base)`. Pure function — no allocations
/// beyond the returned `entries` Vec. Layout discipline:
///
///   1. Inner block — pad to `INNER_DFA_STATE_ALIGN (4)`, then lay out
///      N `[DfaState; entry.states_len]` tables contiguously, each
///      `entry.states_len * INNER_DFA_STATE_SIZE` bytes. Records each
///      entry's `states_vaddr` / `states_file_offset`.
///   2. Outer block — pad to `OUTER_META_ALIGN (8)`, then lay out N
///      `BakedDfaMeta` structs at `OUTER_META_SIZE` each. Records each
///      entry's `meta_vaddr` / `meta_file_offset`.
///   3. `total_size` = final running offset (= cumulative byte size).
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
    let mut placements: Vec<UserBakedRegexEntryLayout> = Vec::with_capacity(entries.len());
    let mut running: u32 = 0;
    // Phase 1: inner [DfaState; N] tables, contiguous. region_start
    // is already DfaState-aligned (8-byte segment alignment is a
    // superset of 4-byte INNER_DFA_STATE_ALIGN), so the per-entry
    // inner tables sit at running=0 + multiples of INNER_DFA_STATE_SIZE
    // (1060 — itself a multiple of 4). No intra-phase pad required.
    let mut inner_vaddrs: Vec<(u64, u32)> = Vec::with_capacity(entries.len());
    for e in entries {
        running = align_up(running, INNER_DFA_STATE_ALIGN);
        let states_vaddr = vaddr_base + u64::from(running);
        let states_file_offset = file_offset_base + running;
        inner_vaddrs.push((states_vaddr, states_file_offset));
        running += e.states_len * INNER_DFA_STATE_SIZE;
    }
    // Phase 2: outer BakedDfaMeta structs, aligned to OUTER_META_ALIGN.
    running = align_up(running, OUTER_META_ALIGN);
    for (e, (states_vaddr, states_file_offset)) in entries.iter().zip(inner_vaddrs.into_iter()) {
        let meta_vaddr = vaddr_base + u64::from(running);
        let meta_file_offset = file_offset_base + running;
        placements.push(UserBakedRegexEntryLayout {
            meta_sym: format!("___torajs_baked_regex_{}", e.index),
            meta_vaddr,
            meta_file_offset,
            states_sym: format!("___torajs_baked_regex_states_{}", e.index),
            states_vaddr,
            states_file_offset,
            states_len: e.states_len,
        });
        running += OUTER_META_SIZE;
    }
    UserRegexBakedLayout {
        entries: placements,
        file_offset: file_offset_base,
        vaddr: vaddr_base,
        total_size: running,
    }
}

/// Byte-emit the baked-regex region: inner `[DfaState; N]` tables
/// followed by aligned outer `BakedDfaMeta` structs. `states_ptr`
/// slots are zero-filled — chain-LC rebase (C-5c) will stamp them in
/// place at link time with the runtime `states_vaddr`.
pub fn build_user_regex_baked_payload(
    layout: &UserRegexBakedLayout,
    entries: &[UserBakedRegexEntry],
) -> Vec<u8> {
    let mut buf: Vec<u8> = Vec::with_capacity(layout.total_size as usize);
    if layout.total_size == 0 {
        debug_assert!(entries.is_empty());
        return buf;
    }
    debug_assert_eq!(layout.entries.len(), entries.len());
    let region_start = layout.file_offset;
    // Phase 1: inner DfaState arrays. Each entry's payload is the
    // pre-serialised `#[repr(C)] DfaState` byte image authored by
    // ssa_lower; len must equal states_len * INNER_DFA_STATE_SIZE.
    for (placement, entry) in layout.entries.iter().zip(entries.iter()) {
        pad_buf_to(&mut buf, region_start, placement.states_file_offset);
        debug_assert_eq!(
            entry.states_payload.len(),
            (entry.states_len * INNER_DFA_STATE_SIZE) as usize,
            "baked regex entry {}: states_payload len {} != states_len {} * {}",
            entry.index,
            entry.states_payload.len(),
            entry.states_len,
            INNER_DFA_STATE_SIZE,
        );
        buf.extend_from_slice(&entry.states_payload);
    }
    // Phase 2: outer BakedDfaMeta structs.
    for (placement, entry) in layout.entries.iter().zip(entries.iter()) {
        pad_buf_to(&mut buf, region_start, placement.meta_file_offset);
        let meta_start = buf.len();
        // states_ptr u64 @ OUTER_STATES_PTR_OFFSET_IN_META — zero now,
        // chain-LC rebase will overwrite with the load-time vaddr.
        debug_assert_eq!(OUTER_STATES_PTR_OFFSET_IN_META, 0);
        buf.extend_from_slice(&0u64.to_le_bytes());
        debug_assert_eq!(buf.len() - meta_start, 8);
        // states_len u32 + 4 start indices u32 = 5 * 4 bytes.
        buf.extend_from_slice(&entry.states_len.to_le_bytes());
        buf.extend_from_slice(&entry.start.to_le_bytes());
        buf.extend_from_slice(&entry.start_mid.to_le_bytes());
        buf.extend_from_slice(&entry.start_mid_word.to_le_bytes());
        buf.extend_from_slice(&entry.start_mid_nonword.to_le_bytes());
        debug_assert_eq!(buf.len() - meta_start, 28);
        // Tail pad — keeps the next BakedDfaMeta at OUTER_META_ALIGN
        // and locks the struct stride at OUTER_META_SIZE.
        buf.extend_from_slice(&[0u8; 4]);
        debug_assert_eq!(buf.len() - meta_start, OUTER_META_SIZE as usize);
    }
    debug_assert_eq!(buf.len() as u32, layout.total_size);
    buf
}

/// Register each entry's outer `meta_sym → meta_vaddr` into the
/// effective sym table. The private inner `states_sym` is **not**
/// inserted — user code never reloc's against it; runtime reads flow
/// through the chain-LC rebased `BakedDfaMeta::states_ptr` slot.
pub fn apply_user_regex_baked_overrides(layout: &UserRegexBakedLayout, sym_table: &mut SymTable) {
    for entry in &layout.entries {
        sym_table.insert(entry.meta_sym.clone(), entry.meta_vaddr);
    }
}

/// Derive the chain-fixup rebase target list for the baked-regex
/// region. One target per entry: the `BakedDfaMeta::states_ptr` slot
/// (= meta_vaddr + OUTER_STATES_PTR_OFFSET_IN_META) → `states_vaddr`.
/// Mirrors `compute_class_layouts_rebase_targets`'s `(slot_vaddr -
/// seg_vmaddr_base, target_vaddr - image_vmaddr_base)` convention.
pub fn compute_user_regex_baked_rebase_targets(
    layout: &UserRegexBakedLayout,
    seg_vmaddr_base: u64,
    image_vmaddr_base: u64,
) -> Vec<RebaseTarget> {
    layout
        .entries
        .iter()
        .map(|entry| {
            let slot_vaddr = entry.meta_vaddr + u64::from(OUTER_STATES_PTR_OFFSET_IN_META);
            debug_assert!(
                slot_vaddr >= seg_vmaddr_base,
                "baked_regex states_ptr slot vaddr {slot_vaddr:#x} cannot precede segment base {seg_vmaddr_base:#x}",
            );
            debug_assert!(
                entry.states_vaddr >= image_vmaddr_base,
                "baked_regex states target {:#x} cannot precede image base {image_vmaddr_base:#x}",
                entry.states_vaddr,
            );
            (
                slot_vaddr - seg_vmaddr_base,
                entry.states_vaddr - image_vmaddr_base,
            )
        })
        .collect()
}

/// Pipeline helper validating the C-5c chain-LC precondition + layout
/// build in one call. Mirrors `build_user_data_globals_region`. Empty
/// input skips computation entirely. Caller must pass `has_dyld=true`
/// before non-empty input — chain-LC rebase requires a dyld region.
pub fn build_user_regex_baked_region(
    entries: &[UserBakedRegexEntry],
    has_dyld: bool,
    file_offset_base: u32,
    vaddr_base: u64,
) -> Result<UserRegexBakedLayout, usize> {
    if entries.is_empty() {
        return Ok(UserRegexBakedLayout {
            entries: Vec::new(),
            file_offset: file_offset_base,
            vaddr: vaddr_base,
            total_size: 0,
        });
    }
    if !has_dyld {
        return Err(entries.len());
    }
    Ok(compute_user_regex_baked_layout(
        entries,
        file_offset_base,
        vaddr_base,
    ))
}

/// Sym names to flag as link-defined in the worklist closure so a
/// user-fn `RelocKind::Page21 / PageOff12 { target_sym }` against the
/// outer symbol isn't surfaced as `UnresolvedExterns` before emit.
/// Mirrors `user_data_globals_extra_defined_syms`. Outer symbols only;
/// the private inner `states_sym` stays internal (never reloc'd from
/// user code — readers go through the rebased
/// `BakedDfaMeta::states_ptr` slot).
pub fn user_regex_baked_extra_defined_syms(
    entries: &[UserBakedRegexEntry],
) -> std::collections::BTreeSet<String> {
    entries
        .iter()
        .map(|e| format!("___torajs_baked_regex_{}", e.index))
        .collect()
}

fn align_up(running: u32, align: u32) -> u32 {
    debug_assert!(align.is_power_of_two());
    let mask = align - 1;
    (running + mask) & !mask
}

fn pad_buf_to(buf: &mut Vec<u8>, region_start: u32, target_file_offset: u32) {
    debug_assert!(
        target_file_offset >= region_start,
        "target offset {target_file_offset:#x} precedes region start {region_start:#x}",
    );
    let target = (target_file_offset - region_start) as usize;
    while buf.len() < target {
        buf.push(0);
    }
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

    fn make_entry(index: u32, states_len: u32) -> UserBakedRegexEntry {
        UserBakedRegexEntry {
            index,
            states_payload: vec![0u8; (states_len * INNER_DFA_STATE_SIZE) as usize],
            states_len,
            start: 0,
            start_mid: 0,
            start_mid_word: 0,
            start_mid_nonword: 0,
        }
    }

    #[test]
    fn single_entry_one_state_layout() {
        let entries = vec![make_entry(0, 1)];
        let layout = compute_user_regex_baked_layout(&entries, 0x4_0000, 0x1_0001_0000);
        assert_eq!(layout.entries.len(), 1);
        let p = &layout.entries[0];
        assert_eq!(p.meta_sym, "___torajs_baked_regex_0");
        assert_eq!(p.states_sym, "___torajs_baked_regex_states_0");
        assert_eq!(p.states_file_offset, 0x4_0000);
        assert_eq!(p.states_vaddr, 0x1_0001_0000);
        assert_eq!(p.states_len, 1);
        // Inner block: 1 * 1060 = 1060 bytes. align_up(1060, 8) = 1064.
        assert_eq!(p.meta_file_offset, 0x4_0000 + 1064);
        assert_eq!(p.meta_vaddr, 0x1_0001_0000 + 1064);
        // total = 1064 + 32 = 1096.
        assert_eq!(layout.total_size, 1096);
    }

    #[test]
    fn multi_entry_layout_alignment() {
        // entry0 states_len=1 → inner @ 0, span 1060
        // entry1 states_len=2 → inner @ 1060 (already 4-aligned), span 2120
        // running = 3180; align_up(3180, 8) = 3184
        // meta0 @ 3184, meta1 @ 3216, total = 3248
        let entries = vec![make_entry(3, 1), make_entry(5, 2)];
        let layout = compute_user_regex_baked_layout(&entries, 0x10_0000, 0x2_0000_0000);
        assert_eq!(layout.entries.len(), 2);
        assert_eq!(layout.entries[0].states_file_offset, 0x10_0000);
        assert_eq!(layout.entries[0].states_vaddr, 0x2_0000_0000);
        assert_eq!(layout.entries[1].states_file_offset, 0x10_0000 + 1060);
        assert_eq!(layout.entries[1].states_vaddr, 0x2_0000_0000 + 1060);
        assert_eq!(layout.entries[0].meta_file_offset, 0x10_0000 + 3184);
        assert_eq!(layout.entries[0].meta_vaddr, 0x2_0000_0000 + 3184);
        assert_eq!(layout.entries[1].meta_file_offset, 0x10_0000 + 3216);
        assert_eq!(layout.entries[1].meta_vaddr, 0x2_0000_0000 + 3216);
        assert_eq!(layout.entries[0].meta_sym, "___torajs_baked_regex_3");
        assert_eq!(layout.entries[1].meta_sym, "___torajs_baked_regex_5");
        assert_eq!(layout.total_size, 3248);
    }

    #[test]
    fn apply_overrides_inserts_outer_sym_only() {
        let entries = vec![make_entry(0, 1), make_entry(1, 1)];
        let layout = compute_user_regex_baked_layout(&entries, 0x4_0000, 0x1_0001_0000);
        let mut sym_table = SymTable::new();
        apply_user_regex_baked_overrides(&layout, &mut sym_table);
        assert_eq!(
            sym_table.get("___torajs_baked_regex_0"),
            Some(&layout.entries[0].meta_vaddr),
        );
        assert_eq!(
            sym_table.get("___torajs_baked_regex_1"),
            Some(&layout.entries[1].meta_vaddr),
        );
        // states_sym must not be in the public table.
        assert!(sym_table.get("___torajs_baked_regex_states_0").is_none());
        assert!(sym_table.get("___torajs_baked_regex_states_1").is_none());
    }

    #[test]
    fn payload_byte_image_single_one_state_entry() {
        // Build entry with a recognisable inner pattern + non-zero
        // start indices to verify outer meta byte layout.
        let mut states_payload = vec![0u8; INNER_DFA_STATE_SIZE as usize];
        states_payload[0] = 0xAB;
        states_payload[1059] = 0xCD;
        let entries = vec![UserBakedRegexEntry {
            index: 0,
            states_payload,
            states_len: 1,
            start: 0x11,
            start_mid: 0x22,
            start_mid_word: 0x33,
            start_mid_nonword: 0x44,
        }];
        let layout = compute_user_regex_baked_layout(&entries, 0x4_0000, 0x1_0001_0000);
        let payload = build_user_regex_baked_payload(&layout, &entries);
        assert_eq!(payload.len() as u32, layout.total_size);
        // Inner table @ 0..1060.
        assert_eq!(payload[0], 0xAB);
        assert_eq!(payload[1059], 0xCD);
        // Pad @ 1060..1064.
        assert_eq!(&payload[1060..1064], &[0u8; 4]);
        // BakedDfaMeta @ 1064..1096.
        // states_ptr u64 @ +0 → 0 (chain-LC rebase target).
        assert_eq!(&payload[1064..1072], &0u64.to_le_bytes());
        // states_len u32 @ +8 → 1.
        assert_eq!(&payload[1072..1076], &1u32.to_le_bytes());
        // start u32 @ +12 → 0x11.
        assert_eq!(&payload[1076..1080], &0x11u32.to_le_bytes());
        // start_mid u32 @ +16 → 0x22.
        assert_eq!(&payload[1080..1084], &0x22u32.to_le_bytes());
        // start_mid_word u32 @ +20 → 0x33.
        assert_eq!(&payload[1084..1088], &0x33u32.to_le_bytes());
        // start_mid_nonword u32 @ +24 → 0x44.
        assert_eq!(&payload[1088..1092], &0x44u32.to_le_bytes());
        // Tail pad @ +28 → 4 zero bytes.
        assert_eq!(&payload[1092..1096], &[0u8; 4]);
    }

    #[test]
    fn payload_empty_for_empty_input() {
        let layout = compute_user_regex_baked_layout(&[], 0x4_0000, 0x1_0001_0000);
        let payload = build_user_regex_baked_payload(&layout, &[]);
        assert!(payload.is_empty());
    }

    #[test]
    fn build_region_empty_returns_zero_sized() {
        let layout = build_user_regex_baked_region(&[], true, 0x4_0000, 0x1_0001_0000).unwrap();
        assert!(layout.entries.is_empty());
        assert_eq!(layout.total_size, 0);
        assert_eq!(layout.file_offset, 0x4_0000);
        assert_eq!(layout.vaddr, 0x1_0001_0000);
    }

    #[test]
    fn build_region_non_empty_requires_dyld() {
        let entries = vec![make_entry(0, 1)];
        let err =
            build_user_regex_baked_region(&entries, false, 0x4_0000, 0x1_0001_0000).unwrap_err();
        assert_eq!(err, 1);
    }

    #[test]
    fn build_region_non_empty_with_dyld_lays_out() {
        let entries = vec![make_entry(0, 1)];
        let layout =
            build_user_regex_baked_region(&entries, true, 0x4_0000, 0x1_0001_0000).unwrap();
        assert_eq!(layout.entries.len(), 1);
        assert_eq!(layout.total_size, 1096);
    }

    #[test]
    fn rebase_targets_empty_when_layout_empty() {
        let layout = compute_user_regex_baked_layout(&[], 0x4_0000, 0x1_0001_0000);
        let targets =
            compute_user_regex_baked_rebase_targets(&layout, 0x1_0000_0000, 0x1_0000_0000);
        assert!(targets.is_empty());
    }

    #[test]
    fn rebase_targets_single_entry() {
        // Layout starts at vaddr 0x1_0001_0000. inner states @ 0x1_0001_0000.
        // meta @ 0x1_0001_0000 + 1064 = 0x1_0001_0428.
        // seg_vmaddr_base = 0x1_0001_0000 (= __DATA_CONST segment base).
        // image_vmaddr_base = 0x1_0000_0000 (= __TEXT segment base).
        let entries = vec![make_entry(0, 1)];
        let layout = compute_user_regex_baked_layout(&entries, 0x4_0000, 0x1_0001_0000);
        let targets =
            compute_user_regex_baked_rebase_targets(&layout, 0x1_0001_0000, 0x1_0000_0000);
        assert_eq!(targets.len(), 1);
        // Slot = meta_vaddr + 0 - seg_vmaddr_base = 0x1_0001_0428 - 0x1_0001_0000 = 0x428.
        assert_eq!(targets[0].0, 0x428);
        // Target = states_vaddr - image_vmaddr_base = 0x1_0001_0000 - 0x1_0000_0000 = 0x1_0000.
        assert_eq!(targets[0].1, 0x1_0000);
    }

    #[test]
    fn rebase_targets_multi_entry_order() {
        // entry0 states_len=1, entry1 states_len=2.
        // inner: entry0 @ 0..1060, entry1 @ 1060..3180
        // outer: align_up(3180, 8) = 3184; meta0 @ 3184, meta1 @ 3216
        let entries = vec![make_entry(0, 1), make_entry(1, 2)];
        let layout = compute_user_regex_baked_layout(&entries, 0x4_0000, 0x1_0001_0000);
        let targets =
            compute_user_regex_baked_rebase_targets(&layout, 0x1_0001_0000, 0x1_0000_0000);
        assert_eq!(targets.len(), 2);
        // entry0 slot @ 3184, target @ 0x1_0000.
        assert_eq!(targets[0].0, 3184);
        assert_eq!(targets[0].1, 0x1_0000);
        // entry1 slot @ 3216, target @ 0x1_0000 + 1060.
        assert_eq!(targets[1].0, 3216);
        assert_eq!(targets[1].1, 0x1_0000 + 1060);
    }
}
