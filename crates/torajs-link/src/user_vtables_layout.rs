//! Layout + emit for user-binary virtual method tables
//! (`__vtable_<C>`, one 8-byte slot per method index).
//!
//! **Slots are relative, not absolute** (r506). Each `Some` slot
//! holds `target_fn_vaddr - vtable_base_vaddr` as a little-endian
//! i64; the dispatch side (`ssa_lower_call_vtable_dispatch`) does
//! `fn = vt + *(i64*)(vt + idx*8)` — the LLVM / Swift
//! relative-vtables shape. Both operands are link-time constants
//! inside one image, so the slide cancels: the table needs no dyld
//! fixup, which is what lets it live in `__TEXT` rodata again
//! (e7a's placement — directly after `user_strings_layout` inside
//! `__TEXT,__cstring`). Absolute slots had to move to a
//! `__DATA_CONST` segment so dyld could stamp the slide without
//! invalidating the codesigned `__TEXT` pages, and that segment
//! cost every class-override program one 16 KB page
//! (`__DATA_CONST` filesize is page-rounded). A `None` slot emits
//! 0 — the class has no impl at that method index and no dispatch
//! ever loads it.
//!
//! Emit is two-phase:
//!
//! 1. Layout (`compute_user_vtables_layout`) — assigns each entry's
//!    final file offset / vaddr; slot bytes are not touched yet.
//! 2. Payload (`build_user_vtables_payload`) — once `fn_vaddrs` is
//!    pinned, each `__torajs_fn_<fid>` slot resolves to
//!    `fn_vaddrs[fid]` and the difference to the table base is
//!    written in a single LE-i64 store.

use crate::exec::UserVtableEntry;
use crate::fn_addr_syms::parse_fn_addr_alias;
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
    /// Section-relative cumulative byte size, INCLUDING the leading
    /// 8-align pad; the caller folds it into `text_size` so the next
    /// `__TEXT` region lands right after the last slot.
    pub total_size: u32,
}

const SLOT_SIZE: u32 = 8;

impl UserVtablesLayout {
    /// File offset where the region's bytes begin — the leading
    /// 8-align pad precedes `file_offset` (the first slot).
    pub fn region_file_offset(&self) -> u32 {
        let slot_bytes: u32 = self.entries.iter().map(|e| e.byte_size).sum();
        self.file_offset - (self.total_size - slot_bytes)
    }
}

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
        total_size: align_pad + running,
    }
}

/// Build the contiguous user-vtables byte payload: leading align pad
/// (zeros), then per slot the LE-i64 `fn_vaddrs[fid] - entry.vaddr`
/// for `Some("__torajs_fn_<fid>")`, 0 for `None`. `fn_vaddrs` is the
/// layout's per-user-fn vaddr table (`FuncId` order).
pub fn build_user_vtables_payload(layout: &UserVtablesLayout, fn_vaddrs: &[u64]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(layout.total_size as usize);
    let slot_bytes_total: usize = layout.entries.iter().map(|e| e.byte_size as usize).sum();
    let leading_pad = layout.total_size as usize - slot_bytes_total;
    buf.resize(leading_pad, 0);
    for entry in &layout.entries {
        for slot in &entry.slot_syms {
            let rel: i64 = match slot {
                Some(name) => {
                    let fid = parse_fn_addr_alias(name).unwrap_or_else(|| {
                        panic!("vtable slot sym {name:?} is not a `__torajs_fn_<fid>` alias")
                    });
                    let target = *fn_vaddrs.get(fid).unwrap_or_else(|| {
                        panic!(
                            "vtable slot {name:?} names fn {fid} but fn_vaddrs has {} entries",
                            fn_vaddrs.len()
                        )
                    });
                    target as i64 - entry.vaddr as i64
                }
                None => 0,
            };
            buf.extend_from_slice(&rel.to_le_bytes());
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
/// derive the vaddr_base and compute the layout in one call.
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
        let vtables = [entry("__vtable_A", vec![Some("__torajs_fn_0"), None])];
        let layout = compute_user_vtables_layout(&vtables, 0x4000, 0x1_0000_4000);
        assert_eq!(layout.entries.len(), 1);
        assert_eq!(layout.entries[0].byte_size, 16);
        assert_eq!(layout.entries[0].vaddr, 0x1_0000_4000);
        assert_eq!(layout.total_size, 16);
    }

    #[test]
    fn multi_vtable_cumulative_offsets() {
        let vtables = [
            entry(
                "__vtable_A",
                vec![Some("__torajs_fn_0"), Some("__torajs_fn_1")],
            ),
            entry("__vtable_B", vec![Some("__torajs_fn_2")]),
        ];
        let layout = compute_user_vtables_layout(&vtables, 0x4000, 0x1_0000_4000);
        assert_eq!(layout.entries[0].vaddr, 0x1_0000_4000);
        assert_eq!(layout.entries[1].vaddr, 0x1_0000_4010);
        assert_eq!(layout.total_size, 24);
    }

    #[test]
    fn unaligned_cursor_pads_and_counts_the_pad() {
        let vtables = [entry("__vtable_A", vec![Some("__torajs_fn_0")])];
        let layout = compute_user_vtables_layout(&vtables, 0x4003, 0x1_0000_4003);
        assert_eq!(layout.entries[0].vaddr, 0x1_0000_4008);
        assert_eq!(layout.entries[0].file_offset, 0x4008);
        assert_eq!(layout.total_size, 5 + 8);
        assert_eq!(layout.region_file_offset(), 0x4003);
        let payload = build_user_vtables_payload(&layout, &[0x1_0000_4100]);
        assert_eq!(payload.len(), 13);
        assert!(payload[..5].iter().all(|b| *b == 0));
    }

    #[test]
    fn payload_slots_are_relative_to_the_table_base() {
        // 3 slots: fn_0 / None / fn_1. Table at 0x1_0000_4000; fn_0
        // sits ABOVE it, fn_1 BELOW it (user text precedes the
        // rodata region in practice — negative offsets are the
        // common case).
        let vtables = [entry(
            "__vtable_A",
            vec![Some("__torajs_fn_0"), None, Some("__torajs_fn_1")],
        )];
        let layout = compute_user_vtables_layout(&vtables, 0x4000, 0x1_0000_4000);
        let fn_vaddrs = [0x1_0000_4100u64, 0x1_0000_3F80u64];

        let payload = build_user_vtables_payload(&layout, &fn_vaddrs);

        assert_eq!(payload.len(), 24);
        let slot = |i: usize| i64::from_le_bytes(payload[i * 8..i * 8 + 8].try_into().unwrap());
        assert_eq!(slot(0), 0x100, "fn_0 - base");
        assert_eq!(slot(1), 0, "None slot stays zero");
        assert_eq!(slot(2), -0x80, "fn_1 - base is negative");
    }

    #[test]
    fn second_table_measures_from_its_own_base() {
        let vtables = [
            entry("__vtable_A", vec![Some("__torajs_fn_0")]),
            entry("__vtable_B", vec![Some("__torajs_fn_0")]),
        ];
        let layout = compute_user_vtables_layout(&vtables, 0x4000, 0x1_0000_4000);
        let payload = build_user_vtables_payload(&layout, &[0x1_0000_4200]);
        let slot = |i: usize| i64::from_le_bytes(payload[i * 8..i * 8 + 8].try_into().unwrap());
        assert_eq!(slot(0), 0x200);
        assert_eq!(slot(1), 0x1F8, "same target, base 8 bytes further along");
    }

    #[test]
    fn apply_overrides_inserts_vtable_sym() {
        let vtables = [entry("__vtable_A", vec![Some("__torajs_fn_0")])];
        let layout = compute_user_vtables_layout(&vtables, 0x4000, 0x1_0000_4000);
        let mut sym_table = SymTable::new();
        apply_user_vtable_overrides(&layout, &mut sym_table);
        assert_eq!(sym_table.get("__vtable_A"), Some(&0x1_0000_4000));
    }
}
