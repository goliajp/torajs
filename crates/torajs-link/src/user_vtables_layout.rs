//! SD-4c-prereq+e7 — layout + emit for user-binary virtual method
//! tables (`__vtable_<C>` `[N x ptr]` rodata blocks).
//!
//! Placement: directly after `user_strings_layout` inside
//! `__TEXT,__cstring` (same segment / same section_64 entry — the
//! kernel maps the segment RX, the section is treated as RO data by
//! the C runtime). One 8-byte slot per `UserVtableEntry.slot_syms[i]`.
//!
//! Emit is two-phase:
//!
//! 1. Layout (`compute_user_vtables_layout`) — assigns each entry's
//!    final file offset / vaddr; doesn't touch byte content yet (sym
//!    resolution is deferred until the effective sym table is built).
//! 2. Payload (`build_user_vtables_payload`) — runs from
//!    `archive_emit::link_to_exec_with_archives` once the effective
//!    sym table has absorbed every override, so per-slot syms can be
//!    looked up to their final vaddrs in a single LE-u64 write.

use crate::chained_fixups_starts::RebaseTarget;
use crate::exec::UserVtableEntry;
use crate::resolve::SymTable;

/// Per-vtable placement.
#[derive(Debug, Clone)]
pub struct UserVtableEntryLayout {
    pub sym: String,
    pub vaddr: u64,
    pub file_offset: u32,
    /// Total bytes the entry occupies = 8 * slot_syms.len().
    pub byte_size: u32,
    pub slot_syms: Vec<Option<String>>,
}

/// Aggregated layout for one user-vtable set.
#[derive(Debug, Clone, Default)]
pub struct UserVtablesLayout {
    pub entries: Vec<UserVtableEntryLayout>,
    pub file_offset: u32,
    pub vaddr: u64,
    /// Section-relative cumulative byte size; rounded up to 4-byte
    /// alignment so the next __TEXT section lands aligned (mirrors the
    /// e1 user_strings_layout fix for __stubs misalign SIGILL).
    pub total_size: u32,
}

const SLOT_SIZE: u32 = 8;

/// Compute file-offset + vaddr placements for the given vtable set,
/// starting at `cursor` / `vaddr_base`.
pub fn compute_user_vtables_layout(
    vtables: &[UserVtableEntry],
    cursor: u32,
    vaddr_base: u64,
) -> UserVtablesLayout {
    if vtables.is_empty() {
        return UserVtablesLayout {
            entries: Vec::new(),
            file_offset: cursor,
            vaddr: vaddr_base,
            total_size: 0,
        };
    }
    // 8-byte align both cursor + vaddr so the first slot (and every
    // subsequent slot, since SLOT_SIZE = 8) lands on an 8-aligned
    // address. ARM64 LDR Xn, [Xm] requires 8-aligned, otherwise the
    // kernel signals SIGBUS / SIGSEGV.
    let align_pad = (8 - (cursor % 8)) % 8;
    let cursor = cursor + align_pad;
    let vaddr_base = vaddr_base + u64::from(align_pad);

    let mut entries = Vec::with_capacity(vtables.len());
    let mut running: u32 = 0;
    for v in vtables {
        let byte_size = (v.slot_syms.len() as u32) * SLOT_SIZE;
        entries.push(UserVtableEntryLayout {
            sym: v.sym.clone(),
            vaddr: vaddr_base + u64::from(running),
            file_offset: cursor + running,
            byte_size,
            slot_syms: v.slot_syms.clone(),
        });
        running += byte_size;
    }
    UserVtablesLayout {
        entries,
        file_offset: cursor,
        vaddr: vaddr_base,
        // total_size includes the leading 8-align pad so caller's
        // text_size += total_size accounts for both the pad and the
        // slot bytes. SLOT_SIZE = 8 keeps trailing alignment too.
        total_size: align_pad + running,
    }
}

/// SD-4c-prereq+e7b-4 — build the contiguous user-vtables byte
/// payload from chain-link u64 values, not absolute vaddrs.
///
/// `slot_link_values` is the same vec `ChainedFixupsBlob.
/// text_rebase_link_values` returns: one entry per `Some` slot, in
/// flat `entries[*].slot_syms` walk order. `None` slots emit a zero
/// 8-byte word (slot pointer stays null, dyld leaves it alone — same
/// semantics SD-3 uses for weak la-ptr imports).
///
/// The vaddr-based path (pre-e7b-4) was load-time wrong: PIE
/// binaries must let dyld add the ASLR slide via the chain, so
/// shipping bare vaddrs aliased every vtable slot to a fixed
/// virtual address that the kernel was free to relocate.
pub fn build_user_vtables_payload(layout: &UserVtablesLayout, slot_link_values: &[u64]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(layout.total_size as usize);
    // total_size includes a leading 8-align pad (see
    // compute_user_vtables_layout); emit pad zeros first so the
    // caller's splice stays contiguous and slot bytes start 8-aligned.
    let slot_bytes_total: usize = layout.entries.iter().map(|e| e.byte_size as usize).sum();
    let leading_pad = layout.total_size as usize - slot_bytes_total;
    buf.resize(leading_pad, 0);
    let mut link_idx = 0;
    for entry in &layout.entries {
        for slot in &entry.slot_syms {
            let link_value: u64 = match slot {
                Some(_) => {
                    let v = *slot_link_values.get(link_idx).unwrap_or_else(|| {
                        panic!(
                            "slot_link_values too short — expected at least {} entries, got {}",
                            link_idx + 1,
                            slot_link_values.len(),
                        )
                    });
                    link_idx += 1;
                    v
                }
                None => 0,
            };
            buf.extend_from_slice(&link_value.to_le_bytes());
        }
    }
    debug_assert_eq!(
        link_idx,
        slot_link_values.len(),
        "slot_link_values carries extras the layout did not consume — caller misalignment",
    );
    debug_assert_eq!(buf.len() as u32, layout.total_size);
    buf
}

/// Register each vtable's `sym → vaddr` into the effective sym table.
pub fn apply_user_vtable_overrides(layout: &UserVtablesLayout, sym_table: &mut SymTable) {
    for entry in &layout.entries {
        sym_table.insert(entry.sym.clone(), entry.vaddr);
    }
}

/// SD-4c-prereq+e7b-4 — derive the `(slot_off_in_text_segment,
/// target_offset_in_image)` rebase target list the chained-fixups
/// `__TEXT` chain will encode. Each `Some(name)` slot maps to one
/// `RebaseTarget`; `None` slots skip the chain entirely (they will
/// stay zero in the emit pass, mirroring the SD-3 la-ptr handling
/// of weak imports).
///
/// `sym_table` must already resolve every `slot_syms[i]` to its
/// final vaddr — typically populated upstream from `fn_vaddrs`
/// via `register_fn_addr_syms` so `__torajs_fn_<fid>` aliases
/// resolve to the correct `__TEXT` body offset.
///
/// `text_vmaddr_base` is the image base (`TEXT_VMADDR_BASE`), so the
/// returned target field is the offset format-6 wants (offset from
/// image base / `text_vmaddr`).
pub fn compute_vtable_rebase_targets(
    layout: &UserVtablesLayout,
    sym_table: &SymTable,
    text_vmaddr_base: u64,
) -> Vec<RebaseTarget> {
    let mut targets = Vec::new();
    for entry in &layout.entries {
        for (slot_idx, slot) in entry.slot_syms.iter().enumerate() {
            let Some(name) = slot else { continue };
            let slot_vaddr = entry.vaddr + (slot_idx as u64) * 8;
            debug_assert!(
                slot_vaddr >= text_vmaddr_base,
                "vtable slot vaddr {slot_vaddr:#x} cannot precede image base {text_vmaddr_base:#x}",
            );
            let slot_off_in_text_seg = slot_vaddr - text_vmaddr_base;
            let target_vaddr = *sym_table.get(name).unwrap_or_else(|| {
                panic!("vtable slot sym {name:?} missing from sym table — register e3 fn_addr aliases before computing rebase targets")
            });
            debug_assert!(
                target_vaddr >= text_vmaddr_base,
                "vtable slot {name:?} target vaddr {target_vaddr:#x} cannot precede image base {text_vmaddr_base:#x}",
            );
            let target_off_in_image = target_vaddr - text_vmaddr_base;
            targets.push((slot_off_in_text_seg, target_off_in_image));
        }
    }
    targets
}

/// SD-4c-prereq+e7b-4 — thin wrapper: derive the vtable rebase
/// target list from `fn_vaddrs` directly (registering each
/// `__torajs_fn_<fid>` alias under the hood). Keeps the
/// archive_link.rs call site at one line so the layout pass stays
/// readable. Returns an empty vec when the layout has no entries —
/// short-circuit so callers can use `is_empty()` to decide whether
/// to pass `Some(TextRebaseScope)` to `build_chained_fixups`.
pub fn vtable_rebase_targets_from_fn_vaddrs(
    layout: &UserVtablesLayout,
    fn_vaddrs: &[u64],
    text_vmaddr_base: u64,
) -> Vec<RebaseTarget> {
    if layout.entries.is_empty() {
        return Vec::new();
    }
    let mut sym_table = SymTable::new();
    for (fid, vaddr) in fn_vaddrs.iter().enumerate() {
        sym_table.insert(format!("__torajs_fn_{fid}"), *vaddr);
    }
    compute_vtable_rebase_targets(layout, &sym_table, text_vmaddr_base)
}

/// Sym names to flag as link-defined in the worklist closure.
pub fn user_vtables_extra_defined_syms(
    vtables: &[UserVtableEntry],
) -> std::collections::BTreeSet<String> {
    vtables.iter().map(|v| v.sym.clone()).collect()
}

/// Pipeline helper — given `text_vmaddr_base` + `file_offset` cursor,
/// derive the vaddr_base and compute the layout in one call. Keeps
/// `archive_link.rs`'s call-site at one line per the ≥soft floor.
pub fn build_user_vtables_region(
    vtables: &[UserVtableEntry],
    text_vmaddr_base: u64,
    file_offset: u32,
) -> UserVtablesLayout {
    compute_user_vtables_layout(
        vtables,
        file_offset,
        text_vmaddr_base + u64::from(file_offset),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(sym: &str, slots: Vec<Option<&str>>) -> UserVtableEntry {
        UserVtableEntry {
            sym: sym.into(),
            slot_syms: slots.into_iter().map(|s| s.map(String::from)).collect(),
        }
    }

    #[test]
    fn empty_input_zero_total() {
        let layout = compute_user_vtables_layout(&[], 0x4000, 0x1_0000_4000);
        assert!(layout.entries.is_empty());
        assert_eq!(layout.total_size, 0);
    }

    #[test]
    fn single_vtable_two_slots() {
        let vtables = [entry("__vtable_A", vec![Some("fn_0"), None])];
        let layout = compute_user_vtables_layout(&vtables, 0x4000, 0x1_0000_4000);
        assert_eq!(layout.entries.len(), 1);
        assert_eq!(layout.entries[0].byte_size, 16);
        assert_eq!(layout.entries[0].vaddr, 0x1_0000_4000);
        assert_eq!(layout.total_size, 16);
    }

    #[test]
    fn multi_vtable_cumulative_offsets() {
        let vtables = [
            entry("__vtable_A", vec![Some("a0"), Some("a1")]),
            entry("__vtable_B", vec![Some("b0")]),
        ];
        let layout = compute_user_vtables_layout(&vtables, 0x4000, 0x1_0000_4000);
        assert_eq!(layout.entries[0].vaddr, 0x1_0000_4000);
        assert_eq!(layout.entries[1].vaddr, 0x1_0000_4010);
        assert_eq!(layout.total_size, 24);
    }

    #[test]
    fn build_payload_splices_link_values_and_zeroes_none_slots() {
        // 3 slots: Some / None / Some. e7b-4 payload comes from
        // `slot_link_values` (one entry per `Some`), with `None`
        // slots emitting an 8-byte zero word.
        let vtables = [entry("__vtable_A", vec![Some("fn_a"), None, Some("fn_b")])];
        let layout = compute_user_vtables_layout(&vtables, 0x4000, 0x1_0000_4000);
        let slot_link_values = [0xDEAD_BEEFu64, 0xCAFE_BABEu64];

        let payload = build_user_vtables_payload(&layout, &slot_link_values);

        // 3 slots × 8 = 24 bytes, already 4-aligned.
        assert_eq!(payload.len(), 24);
        let slot0 = u64::from_le_bytes(payload[0..8].try_into().unwrap());
        let slot1 = u64::from_le_bytes(payload[8..16].try_into().unwrap());
        let slot2 = u64::from_le_bytes(payload[16..24].try_into().unwrap());
        assert_eq!(slot0, 0xDEAD_BEEF, "Some slot 0 picks slot_link_values[0]");
        assert_eq!(slot1, 0, "None slot stays zero — dyld leaves it alone");
        assert_eq!(
            slot2, 0xCAFE_BABE,
            "Some slot 2 picks slot_link_values[1] (None slot skipped)"
        );
    }

    #[test]
    fn apply_overrides_inserts_vtable_sym() {
        let vtables = [entry("__vtable_A", vec![Some("fn_a")])];
        let layout = compute_user_vtables_layout(&vtables, 0x4000, 0x1_0000_4000);
        let mut sym_table = SymTable::new();
        apply_user_vtable_overrides(&layout, &mut sym_table);
        assert_eq!(sym_table.get("__vtable_A"), Some(&0x1_0000_4000));
    }

    #[test]
    fn rebase_targets_skip_none_slots_and_resolve_some_slots() {
        // Vtable __vtable_A at vaddr 0x1_0000_4000 with 3 slots:
        // slot 0 → fn_a (resolves to 0x1_0000_4100)
        // slot 1 → None (must be skipped, not emit a target)
        // slot 2 → fn_b (resolves to 0x1_0000_4200)
        // text_vmaddr_base = 0x1_0000_0000 (TEXT_VMADDR_BASE).
        let vtables = [entry("__vtable_A", vec![Some("fn_a"), None, Some("fn_b")])];
        let layout = compute_user_vtables_layout(&vtables, 0x4000, 0x1_0000_4000);
        let mut sym_table = SymTable::new();
        sym_table.insert("fn_a".into(), 0x1_0000_4100);
        sym_table.insert("fn_b".into(), 0x1_0000_4200);

        let targets = compute_vtable_rebase_targets(&layout, &sym_table, 0x1_0000_0000);

        assert_eq!(targets.len(), 2, "None slots must not emit rebase targets");
        // Slot 0: offset_in_text_seg = 0x4000, target = fn_a - base = 0x4100.
        assert_eq!(targets[0], (0x4000, 0x4100));
        // Slot 2 (skipping slot 1): offset_in_text_seg = 0x4010,
        // target = fn_b - base = 0x4200.
        assert_eq!(targets[1], (0x4010, 0x4200));
    }

    #[test]
    fn rebase_targets_handle_multi_vtable_cumulative_offsets() {
        let vtables = [
            entry("__vtable_A", vec![Some("a0"), Some("a1")]),
            entry("__vtable_B", vec![Some("b0")]),
        ];
        let layout = compute_user_vtables_layout(&vtables, 0x4000, 0x1_0000_4000);
        let mut sym_table = SymTable::new();
        sym_table.insert("a0".into(), 0x1_0000_4100);
        sym_table.insert("a1".into(), 0x1_0000_4108);
        sym_table.insert("b0".into(), 0x1_0000_4200);

        let targets = compute_vtable_rebase_targets(&layout, &sym_table, 0x1_0000_0000);
        assert_eq!(targets.len(), 3);
        // a-vtable slots at 0x4000, 0x4008; b-vtable at 0x4010.
        assert_eq!(targets[0], (0x4000, 0x4100));
        assert_eq!(targets[1], (0x4008, 0x4108));
        assert_eq!(targets[2], (0x4010, 0x4200));
    }

    #[test]
    fn rebase_targets_empty_when_layout_empty() {
        let layout = compute_user_vtables_layout(&[], 0x4000, 0x1_0000_4000);
        let sym_table = SymTable::new();
        let targets = compute_vtable_rebase_targets(&layout, &sym_table, 0x1_0000_0000);
        assert!(targets.is_empty());
    }
}
