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

/// Build the contiguous user-vtables byte payload. Each slot is
/// resolved through `sym_table` — Some(name) → name's final vaddr LE;
/// None → zero (null ptr). `sym_table` must already include every
/// alias the user-vtables target (typically e3 fn_addr aliases).
pub fn build_user_vtables_payload(layout: &UserVtablesLayout, sym_table: &SymTable) -> Vec<u8> {
    let mut buf = Vec::with_capacity(layout.total_size as usize);
    // total_size includes a leading 8-align pad (see
    // compute_user_vtables_layout); emit pad zeros first so the
    // caller's splice stays contiguous and slot bytes start 8-aligned.
    let slot_bytes_total: usize = layout.entries.iter().map(|e| e.byte_size as usize).sum();
    let leading_pad = layout.total_size as usize - slot_bytes_total;
    buf.resize(leading_pad, 0);
    for entry in &layout.entries {
        for slot in &entry.slot_syms {
            let vaddr: u64 = match slot {
                Some(name) => *sym_table.get(name).unwrap_or_else(|| {
                    panic!("user-vtable slot sym {name:?} not in effective sym table")
                }),
                None => 0,
            };
            buf.extend_from_slice(&vaddr.to_le_bytes());
        }
    }
    debug_assert_eq!(buf.len() as u32, layout.total_size);
    buf
}

/// Register each vtable's `sym → vaddr` into the effective sym table.
pub fn apply_user_vtable_overrides(layout: &UserVtablesLayout, sym_table: &mut SymTable) {
    for entry in &layout.entries {
        sym_table.insert(entry.sym.clone(), entry.vaddr);
    }
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
    fn build_payload_resolves_slot_syms() {
        let vtables = [entry("__vtable_A", vec![Some("fn_a"), None, Some("fn_b")])];
        let layout = compute_user_vtables_layout(&vtables, 0x4000, 0x1_0000_4000);
        let mut sym_table = SymTable::new();
        sym_table.insert("fn_a".into(), 0x1_0000_4100);
        sym_table.insert("fn_b".into(), 0x1_0000_4200);
        let payload = build_user_vtables_payload(&layout, &sym_table);
        // 3 slots × 8 = 24 bytes, already 4-aligned.
        assert_eq!(payload.len(), 24);
        let slot0 = u64::from_le_bytes(payload[0..8].try_into().unwrap());
        let slot1 = u64::from_le_bytes(payload[8..16].try_into().unwrap());
        let slot2 = u64::from_le_bytes(payload[16..24].try_into().unwrap());
        assert_eq!(slot0, 0x1_0000_4100);
        assert_eq!(slot1, 0);
        assert_eq!(slot2, 0x1_0000_4200);
    }

    #[test]
    fn apply_overrides_inserts_vtable_sym() {
        let vtables = [entry("__vtable_A", vec![Some("fn_a")])];
        let layout = compute_user_vtables_layout(&vtables, 0x4000, 0x1_0000_4000);
        let mut sym_table = SymTable::new();
        apply_user_vtable_overrides(&layout, &mut sym_table);
        assert_eq!(sym_table.get("__vtable_A"), Some(&0x1_0000_4000));
    }
}
