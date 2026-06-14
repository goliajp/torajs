//! Fn-name registry Phase 2 Step 3b — layout + emit for the single
//! `__torajs_fn_name_table[]` rodata global, the runtime
//! binary-search target consumed by `__torajs_fn_print_inline` to
//! turn a fn_addr into the bun `[Function: name]` form.
//!
//! Mirrors `user_vtables_layout` (chain-fixup-aware) but the
//! shape is a single contiguous array of fixed-size entries (no
//! per-symbol grouping), keyed on fn_addr ascending so the runtime
//! can binary-search. Two adjacent globals:
//!
//! - `__torajs_fn_name_table`       — N × 24-byte entries:
//!     ```text
//!       +0..7   : fn_addr   u64 (link-time chain-fixup target)
//!       +8..15  : name_ptr  *const u8 (link-time chain-fixup target
//!                                       to __TEXT,__cstring entry)
//!       +16..19 : name_len  u32
//!       +20..23 : _pad      u32
//!     ```
//! - `__torajs_fn_name_table_count` — u64 entry count.
//!
//! Both globals get sym-table overrides so the runtime helper's
//! externs resolve. The table is sorted by `fn_addr` ascending at
//! emit time so the runtime can binary-search.
//!
//! Placement: `__DATA_CONST` segment, after `user_vtables_layout`
//! + `user_class_layouts_layout`. The two chain-fixup-target slots
//! (`fn_addr`, `name_ptr`) require dyld rebase at load time, which
//! invalidates the codesigned `__TEXT` page hash if placed in
//! `__TEXT,__cstring` — the same reason `user_vtables_layout`
//! migrated to `__DATA_CONST`. `SG_READ_ONLY` keeps the runtime
//! view of the table immutable.

use crate::chained_fixups_starts::RebaseTarget;
use crate::resolve::SymTable;

/// Per-entry layout — one row of the `__torajs_fn_name_table[]`
/// rodata array.
#[derive(Debug, Clone)]
pub struct FnNameTableEntryLayout {
    /// `__torajs_fn_<fid>` alias sym (resolves to the user fn's
    /// vaddr after `register_fn_addr_syms`).
    pub fn_addr_sym: String,
    /// `__user_string_<sid>` alias sym (resolves to the
    /// `__TEXT,__cstring` byte payload's vaddr after
    /// `apply_user_string_overrides`). The user_strings_layout
    /// substrate already places the actual bytes; we just need
    /// the sym for the chain-fixup.
    pub name_ptr_sym: String,
    /// Code-unit count per ES `String.length`. Lives in the
    /// `name_len: u32` field (NOT a chain-fixup target).
    pub name_len: u32,
}

/// Aggregated layout for the single `__torajs_fn_name_table[]` +
/// `__torajs_fn_name_table_count` globals.
#[derive(Debug, Clone, Default)]
pub struct FnNameTableLayout {
    /// Per-row entries. Sorted by `fn_addr_sym` ASCII for stable
    /// emit order (final vaddr-sort happens at chain-fixup time
    /// when sym_table is populated). Empty when no fn declarations
    /// land an entry (the common no-fn-decl program case).
    pub entries: Vec<FnNameTableEntryLayout>,
    /// Sym for the table itself — `__torajs_fn_name_table`. The
    /// table starts here.
    pub table_vaddr: u64,
    pub table_file_offset: u32,
    /// Sym for the count global — `__torajs_fn_name_table_count`.
    /// Placed 8-aligned right after the entries.
    pub count_vaddr: u64,
    pub count_file_offset: u32,
    /// Section-relative cumulative byte size = N×24 + 8 (count
    /// global) + leading alignment pad. Rounded up to 8-byte
    /// alignment so the next __TEXT,__cstring section lands aligned.
    pub total_size: u32,
}

/// Per-entry byte size.
pub const ENTRY_SIZE: u32 = 24;
/// `__torajs_fn_name_table_count: u64` size.
pub const COUNT_SIZE: u32 = 8;

/// Compute file-offset + vaddr placements for the table + count
/// globals starting at `cursor` / `vaddr_base`. The `entries`
/// must already be sorted by `fn_addr_sym` (or any deterministic
/// stable key) for reproducible emit.
pub fn compute_fn_name_table_layout(
    entries: Vec<FnNameTableEntryLayout>,
    cursor: u32,
    vaddr_base: u64,
) -> FnNameTableLayout {
    if entries.is_empty() {
        return FnNameTableLayout {
            entries,
            table_vaddr: vaddr_base,
            table_file_offset: cursor,
            count_vaddr: vaddr_base,
            count_file_offset: cursor,
            total_size: 0,
        };
    }
    // 8-byte align cursor + vaddr so the first entry (and every
    // subsequent entry, since ENTRY_SIZE = 24 is 8-aligned) lands
    // on an 8-aligned address. ARM64 LDR Xn, [Xm] requires 8-aligned,
    // otherwise the kernel signals SIGBUS / SIGSEGV.
    let align_pad = (8 - (cursor % 8)) % 8;
    let cursor = cursor + align_pad;
    let vaddr_base = vaddr_base + u64::from(align_pad);

    let n = entries.len() as u32;
    let entries_total = n * ENTRY_SIZE;
    let count_file_offset = cursor + entries_total;
    let count_vaddr = vaddr_base + u64::from(entries_total);

    FnNameTableLayout {
        entries,
        table_vaddr: vaddr_base,
        table_file_offset: cursor,
        count_vaddr,
        count_file_offset,
        // total_size includes the leading 8-align pad + entries +
        // count global. Already 8-aligned (24 × N + 8 stays 8-aligned).
        total_size: align_pad + entries_total + COUNT_SIZE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(fid: u32, sid: u32, name_len: u32) -> FnNameTableEntryLayout {
        FnNameTableEntryLayout {
            fn_addr_sym: format!("__torajs_fn_{fid}"),
            name_ptr_sym: format!("__user_string_{sid}"),
            name_len,
        }
    }

    #[test]
    fn empty_input_zero_total() {
        let layout = compute_fn_name_table_layout(Vec::new(), 0x4000, 0x1_0000_4000);
        assert!(layout.entries.is_empty());
        assert_eq!(layout.total_size, 0);
    }

    #[test]
    fn single_entry_layout() {
        let layout = compute_fn_name_table_layout(vec![entry(0, 0, 3)], 0x4000, 0x1_0000_4000);
        assert_eq!(layout.entries.len(), 1);
        assert_eq!(layout.table_vaddr, 0x1_0000_4000);
        assert_eq!(layout.table_file_offset, 0x4000);
        // count is placed right after the single 24-byte entry.
        assert_eq!(layout.count_vaddr, 0x1_0000_4018);
        assert_eq!(layout.count_file_offset, 0x4018);
        // total = 24 entry + 8 count = 32 bytes.
        assert_eq!(layout.total_size, 32);
    }

    #[test]
    fn multi_entry_layout() {
        let entries = vec![entry(0, 0, 3), entry(1, 1, 5), entry(2, 2, 7)];
        let layout = compute_fn_name_table_layout(entries, 0x4000, 0x1_0000_4000);
        assert_eq!(layout.entries.len(), 3);
        assert_eq!(layout.table_vaddr, 0x1_0000_4000);
        // count placed right after 3 × 24 = 72 byte entries.
        assert_eq!(layout.count_vaddr, 0x1_0000_4048);
        assert_eq!(layout.count_file_offset, 0x4048);
        // total = 72 entries + 8 count = 80 bytes.
        assert_eq!(layout.total_size, 80);
    }

    #[test]
    fn unaligned_cursor_gets_pad() {
        // Cursor 0x4001 needs +7 align pad → entries start at 0x4008.
        let layout = compute_fn_name_table_layout(vec![entry(0, 0, 3)], 0x4001, 0x1_0000_4001);
        assert_eq!(layout.table_vaddr, 0x1_0000_4008);
        assert_eq!(layout.table_file_offset, 0x4008);
        assert_eq!(layout.count_vaddr, 0x1_0000_4020);
        // total includes the 7-byte pad + 24 entry + 8 count = 39.
        assert_eq!(layout.total_size, 39);
    }
}

/// Build the contiguous bytes for the table + count globals,
/// consuming the chain-fixup link values pre-computed for the
/// fn_addr + name_ptr slots. Layout per `compute_fn_name_table_layout`.
///
/// Each entry contributes two chain-fixup link values in flat
/// `entries[*]` walk order — `fn_addr` first, then `name_ptr`.
/// `name_len: u32` is written directly (no chain fixup needed since
/// it's a literal value, not a pointer). `_pad: u32` is always 0.
///
/// The trailing `__torajs_fn_name_table_count: u64` global gets the
/// raw entry count as a little-endian u64.
pub fn build_fn_name_table_payload(
    layout: &FnNameTableLayout,
    slot_link_values: &[u64],
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(layout.total_size as usize);
    if layout.entries.is_empty() {
        return buf;
    }
    let entries_total = (layout.entries.len() as u32) * ENTRY_SIZE;
    let leading_pad = layout.total_size as usize - entries_total as usize - COUNT_SIZE as usize;
    buf.resize(leading_pad, 0);
    // Each entry: 2 chain-fixup-encoded u64s (fn_addr, name_ptr) +
    // 1 u32 name_len + 1 u32 pad.
    let expected_link_count = layout.entries.len() * 2;
    debug_assert_eq!(
        slot_link_values.len(),
        expected_link_count,
        "slot_link_values mismatch — expected {} (2 per entry × {} entries), got {}",
        expected_link_count,
        layout.entries.len(),
        slot_link_values.len(),
    );
    for (i, entry) in layout.entries.iter().enumerate() {
        let fn_addr_lv = slot_link_values[i * 2];
        let name_ptr_lv = slot_link_values[i * 2 + 1];
        buf.extend_from_slice(&fn_addr_lv.to_le_bytes());
        buf.extend_from_slice(&name_ptr_lv.to_le_bytes());
        buf.extend_from_slice(&entry.name_len.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
    }
    // Trailing count global — raw u64 entry count, no chain fixup
    // (the count is a plain immediate, not a pointer).
    let count = layout.entries.len() as u64;
    buf.extend_from_slice(&count.to_le_bytes());
    debug_assert_eq!(buf.len() as u32, layout.total_size);
    buf
}

/// Register the table + count sym names with their final vaddrs in
/// the effective sym table so the runtime helper's externs resolve.
pub fn apply_fn_name_table_overrides(layout: &FnNameTableLayout, sym_table: &mut SymTable) {
    if layout.entries.is_empty() {
        return;
    }
    sym_table.insert("__torajs_fn_name_table".into(), layout.table_vaddr);
    sym_table.insert("__torajs_fn_name_table_count".into(), layout.count_vaddr);
}

/// Sym names to flag as link-defined in the worklist closure (so
/// the resolver doesn't classify them as undefined dyld imports).
/// Takes the raw `cfg.fn_name_globals` slice (mirrors the
/// `user_vtables_extra_defined_syms` pattern) so the worklist
/// closure can extend `extra` before the actual layout is computed.
/// Empty when `fn_name_globals` is empty.
pub fn fn_name_table_extra_defined_syms(
    fn_name_globals: &[crate::exec::UserFnNameEntry],
) -> std::collections::BTreeSet<String> {
    let mut s = std::collections::BTreeSet::new();
    if !fn_name_globals.is_empty() {
        s.insert("__torajs_fn_name_table".to_string());
        s.insert("__torajs_fn_name_table_count".to_string());
    }
    s
}

/// Chain-fixup rebase target list — one `(slot_off_in_seg, target_off_in_image)`
/// pair per chain-fixup-encoded link value. Each entry contributes
/// two targets: fn_addr slot at `entry_base + 0`, name_ptr slot at
/// `entry_base + 8`. Walk order matches `build_fn_name_table_payload`'s
/// `slot_link_values` consumption order.
///
/// `sym_table` must already resolve both `fn_addr_sym` (via
/// `register_fn_addr_syms` aliases) and `name_ptr_sym` (via
/// `apply_user_string_overrides`) to their final vaddrs before
/// this is called.
///
/// `seg_vmaddr_base` is the segment containing the table slots
/// (currently `__TEXT,__cstring` → __TEXT vmaddr base); offsets
/// are computed relative to it for dyld's per-page chain walker.
/// `image_vmaddr_base` is the image base (`TEXT_VMADDR_BASE`) so
/// the returned target field is the offset format-6 expects.
pub fn compute_fn_name_table_rebase_targets(
    layout: &FnNameTableLayout,
    sym_table: &SymTable,
    seg_vmaddr_base: u64,
    image_vmaddr_base: u64,
) -> Vec<RebaseTarget> {
    let mut targets = Vec::with_capacity(layout.entries.len() * 2);
    if layout.entries.is_empty() {
        return targets;
    }
    for (i, entry) in layout.entries.iter().enumerate() {
        let entry_vaddr = layout.table_vaddr + (i as u64) * (ENTRY_SIZE as u64);
        // Slot 0 — fn_addr at entry_vaddr + 0.
        let fn_slot_off = (entry_vaddr - seg_vmaddr_base) as u32 as u64;
        let fn_target_vaddr = *sym_table.get(&entry.fn_addr_sym).unwrap_or_else(|| {
            panic!(
                "fn_name_table entry {:?} fn_addr_sym {:?} missing from sym table — \
                 register fn_addr aliases before computing rebase targets",
                i, entry.fn_addr_sym,
            )
        });
        targets.push((fn_slot_off, fn_target_vaddr - image_vmaddr_base));
        // Slot 1 — name_ptr at entry_vaddr + 8.
        let name_slot_off = ((entry_vaddr + 8) - seg_vmaddr_base) as u32 as u64;
        let name_target_vaddr = *sym_table.get(&entry.name_ptr_sym).unwrap_or_else(|| {
            panic!(
                "fn_name_table entry {:?} name_ptr_sym {:?} missing from sym table — \
                 apply user_string overrides before computing rebase targets",
                i, entry.name_ptr_sym,
            )
        });
        targets.push((name_slot_off, name_target_vaddr - image_vmaddr_base));
    }
    targets
}

/// Pipeline helper — given `text_vmaddr_base` + `file_offset` cursor,
/// derive the vaddr_base and compute the layout in one call.
pub fn build_fn_name_table_region(
    entries: Vec<FnNameTableEntryLayout>,
    text_vmaddr_base: u64,
    file_offset: u32,
) -> FnNameTableLayout {
    compute_fn_name_table_layout(
        entries,
        file_offset,
        text_vmaddr_base + u64::from(file_offset),
    )
}
