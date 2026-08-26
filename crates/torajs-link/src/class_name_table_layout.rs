//! W-J Phase A3c — layout + emit for the single
//! `__torajs_class_name_table[]` rodata global, the runtime
//! binary-search target consumed by
//! `__torajs_struct_class_name(class_tag)` to turn a class_tag
//! into the source-text class name (Point / User / ...).
//!
//! Mirrors `fn_name_table_layout` but each row carries a u32
//! `class_tag` immediate (not a chain-fixup target) plus a single
//! `name_ptr` chain-fixup slot:
//!
//! - `__torajs_class_name_table`       — N × 24-byte entries:
//!     ```text
//!       +0..3   : class_tag u32 (immediate; SSA-assigned tag)
//!       +4..7   : _pad      u32 (0)
//!       +8..15  : name_ptr  *const u8 (link-time chain-fixup target
//!                                       to a `__user_string_<sid>`
//!                                       RawBytes payload)
//!       +16..19 : name_len  u32
//!       +20..23 : _pad2     u32 (0)
//!     ```
//! - `__torajs_n_class_names` — u64 entry count.
//!
//! Both globals get sym-table overrides so the runtime helper's
//! externs resolve. Entries are sorted by `class_tag` ascending at
//! emit time so the runtime can binary-search.
//!
//! Placement: `__DATA_CONST` segment, after `fn_name_table`. The
//! single chain-fixup-target slot (`name_ptr`) requires dyld rebase
//! at load time — same rationale as `fn_name_table_layout`'s
//! migration to `__DATA_CONST`.

use crate::chained_fixups_starts::RebaseTarget;
use crate::resolve::SymTable;
use crate::user_strings_layout::UserStringsLayout;

/// Per-entry layout — one row of the `__torajs_class_name_table[]`
/// rodata array.
#[derive(Debug, Clone)]
pub struct ClassNameTableEntryLayout {
    /// SSA-assigned class_tag (1-based, 0 reserved for anonymous
    /// non-stamped). Written as a literal `u32` at entry offset 0.
    pub class_tag: u32,
    /// `__torajs_str_dyn_<sid>` alias sym (RawBytes flavour
    /// registered by `apply_user_string_overrides`). Resolves to
    /// the raw char payload's vaddr at link time.
    pub name_ptr_sym: String,
    /// Code-unit count per ES `String.length`. Lives in the
    /// `name_len: u32` field (NOT a chain-fixup target).
    pub name_len: u32,
}

/// Aggregated layout for the single `__torajs_class_name_table[]` +
/// `__torajs_n_class_names` globals.
#[derive(Debug, Clone, Default)]
pub struct ClassNameTableLayout {
    /// Per-row entries. Sorted by `class_tag` asc for stable emit
    /// order and runtime binary-search.
    pub entries: Vec<ClassNameTableEntryLayout>,
    /// Sym `__torajs_class_name_table` placement.
    pub table_vaddr: u64,
    pub table_file_offset: u32,
    /// Sym `__torajs_n_class_names` placement (8-aligned after entries).
    pub count_vaddr: u64,
    pub count_file_offset: u32,
    /// Cumulative bytes = leading 8-align pad + N × 24 entries + 8 count.
    pub total_size: u32,
}

/// Per-entry byte size.
pub const ENTRY_SIZE: u32 = 24;
/// `__torajs_n_class_names: u64` size.
pub const COUNT_SIZE: u32 = 8;

/// Compute file-offset + vaddr placements for the table + count
/// globals starting at `cursor` / `vaddr_base`. The `entries`
/// must already be sorted by `class_tag` ascending for reproducible
/// emit + runtime binary-search.
pub fn compute_class_name_table_layout(
    entries: Vec<ClassNameTableEntryLayout>,
    cursor: u32,
    vaddr_base: u64,
    force_emit_globals: bool,
) -> ClassNameTableLayout {
    if entries.is_empty() && !force_emit_globals {
        return ClassNameTableLayout {
            entries,
            table_vaddr: vaddr_base,
            table_file_offset: cursor,
            count_vaddr: vaddr_base,
            count_file_offset: cursor,
            total_size: 0,
        };
    }
    if entries.is_empty() {
        let align_pad = (8 - (cursor % 8)) % 8;
        let cursor = cursor + align_pad;
        let vaddr_base = vaddr_base + u64::from(align_pad);
        return ClassNameTableLayout {
            entries,
            table_vaddr: vaddr_base,
            table_file_offset: cursor,
            count_vaddr: vaddr_base,
            count_file_offset: cursor,
            total_size: align_pad + COUNT_SIZE,
        };
    }
    let align_pad = (8 - (cursor % 8)) % 8;
    let cursor = cursor + align_pad;
    let vaddr_base = vaddr_base + u64::from(align_pad);

    let n = entries.len() as u32;
    let entries_total = n * ENTRY_SIZE;
    let count_file_offset = cursor + entries_total;
    let count_vaddr = vaddr_base + u64::from(entries_total);

    ClassNameTableLayout {
        entries,
        table_vaddr: vaddr_base,
        table_file_offset: cursor,
        count_vaddr,
        count_file_offset,
        total_size: align_pad + entries_total + COUNT_SIZE,
    }
}

/// Build the contiguous bytes for the table + count globals,
/// consuming chain-fixup link values pre-computed for the
/// `name_ptr` slot of each entry (one slot per entry, vs
/// fn_name_table's two).
///
/// Walk order matches `compute_class_name_table_rebase_targets`'s
/// emit order — flat `entries[*]` ascending.
pub fn build_class_name_table_payload(
    layout: &ClassNameTableLayout,
    slot_link_values: &[u64],
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(layout.total_size as usize);
    if layout.entries.is_empty() && layout.total_size == 0 {
        return buf;
    }
    if layout.entries.is_empty() {
        let leading_pad = layout.total_size as usize - COUNT_SIZE as usize;
        buf.resize(leading_pad, 0);
        buf.extend_from_slice(&0u64.to_le_bytes());
        debug_assert_eq!(buf.len() as u32, layout.total_size);
        return buf;
    }
    let entries_total = (layout.entries.len() as u32) * ENTRY_SIZE;
    let leading_pad = layout.total_size as usize - entries_total as usize - COUNT_SIZE as usize;
    buf.resize(leading_pad, 0);
    let expected_link_count = layout.entries.len();
    debug_assert_eq!(
        slot_link_values.len(),
        expected_link_count,
        "slot_link_values mismatch — expected {} (1 per entry × {} entries), got {}",
        expected_link_count,
        layout.entries.len(),
        slot_link_values.len(),
    );
    for (i, entry) in layout.entries.iter().enumerate() {
        let name_ptr_lv = slot_link_values[i];
        // +0..3 class_tag u32 immediate, +4..7 _pad u32.
        buf.extend_from_slice(&entry.class_tag.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        // +8..15 name_ptr u64 chain-fixup target.
        buf.extend_from_slice(&name_ptr_lv.to_le_bytes());
        // +16..19 name_len u32, +20..23 _pad2 u32.
        buf.extend_from_slice(&entry.name_len.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
    }
    let count = layout.entries.len() as u64;
    buf.extend_from_slice(&count.to_le_bytes());
    debug_assert_eq!(buf.len() as u32, layout.total_size);
    buf
}

/// Register the table + count sym names with their final vaddrs in
/// the effective sym table so the runtime helper's externs resolve.
pub fn apply_class_name_table_overrides(layout: &ClassNameTableLayout, sym_table: &mut SymTable) {
    if layout.total_size == 0 {
        return;
    }
    // Triple underscore = Mach-O leading-`_` convention applied to
    // Rust's `extern "C" static __torajs_class_name_table`.
    sym_table.insert("___torajs_class_name_table".into(), layout.table_vaddr);
    sym_table.insert("___torajs_n_class_names".into(), layout.count_vaddr);
}

/// Sym names to flag as link-defined in the worklist closure.
/// Mirror of `fn_name_table_extra_defined_syms`.
pub fn class_name_table_extra_defined_syms(
    class_names: &[crate::exec::UserClassNameEntry],
    force_emit_globals: bool,
) -> std::collections::BTreeSet<String> {
    let mut s = std::collections::BTreeSet::new();
    if !class_names.is_empty() || force_emit_globals {
        s.insert("___torajs_class_name_table".to_string());
        s.insert("___torajs_n_class_names".to_string());
    }
    s
}

/// Chain-fixup rebase target list — one
/// `(slot_off_in_seg, target_off_in_image)` pair per entry. Each
/// entry contributes a single target: `name_ptr` slot at
/// `entry_base + 8`. Walk order matches
/// `build_class_name_table_payload`'s `slot_link_values` order.
///
/// `sym_table` must already resolve `name_ptr_sym` (via
/// `apply_user_string_overrides`) before this is called.
pub fn compute_class_name_table_rebase_targets(
    layout: &ClassNameTableLayout,
    sym_table: &SymTable,
    seg_vmaddr_base: u64,
    image_vmaddr_base: u64,
) -> Vec<RebaseTarget> {
    let mut targets = Vec::with_capacity(layout.entries.len());
    if layout.entries.is_empty() {
        return targets;
    }
    for (i, entry) in layout.entries.iter().enumerate() {
        let entry_vaddr = layout.table_vaddr + (i as u64) * (ENTRY_SIZE as u64);
        // name_ptr slot at +8.
        let name_slot_off = ((entry_vaddr + 8) - seg_vmaddr_base) as u32 as u64;
        let name_target_vaddr = *sym_table.get(&entry.name_ptr_sym).unwrap_or_else(|| {
            panic!(
                "class_name_table entry {:?} name_ptr_sym {:?} missing from sym table — \
                 apply user_string overrides before computing rebase targets",
                i, entry.name_ptr_sym,
            )
        });
        targets.push((name_slot_off, name_target_vaddr - image_vmaddr_base));
    }
    targets
}

/// Pipeline helper — derive vaddr_base from text_vmaddr_base +
/// file_offset and compute the layout in one call.
pub fn build_class_name_table_region(
    entries: Vec<ClassNameTableEntryLayout>,
    text_vmaddr_base: u64,
    file_offset: u32,
    force_emit_globals: bool,
) -> ClassNameTableLayout {
    compute_class_name_table_layout(
        entries,
        file_offset,
        text_vmaddr_base + u64::from(file_offset),
        force_emit_globals,
    )
}

/// archive_link-side helper — mirror of
/// `fn_name_table_rebase_targets_from_layouts`. Builds a mini sym
/// table containing user-string aliases so
/// `compute_class_name_table_rebase_targets` resolves without
/// needing the full archive_emit-side `effective_sym_table`.
pub fn class_name_table_rebase_targets_from_layouts(
    layout: &ClassNameTableLayout,
    user_strings_layout: &UserStringsLayout,
    seg_vmaddr_base: u64,
    image_vmaddr_base: u64,
) -> Vec<RebaseTarget> {
    if layout.entries.is_empty() {
        return Vec::new();
    }
    let mut sym_table = SymTable::new();
    for entry in &user_strings_layout.entries {
        sym_table.insert(entry.sym.clone(), entry.vaddr);
        if let Some(alias) = &entry.payload_alias {
            sym_table.insert(
                alias.clone(),
                entry.vaddr + u64::from(crate::user_strings_layout::STR_HEADER_SIZE),
            );
        }
    }
    compute_class_name_table_rebase_targets(layout, &sym_table, seg_vmaddr_base, image_vmaddr_base)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(tag: u32, sid: u32, name_len: u32) -> ClassNameTableEntryLayout {
        ClassNameTableEntryLayout {
            class_tag: tag,
            name_ptr_sym: format!("__user_string_{sid}"),
            name_len,
        }
    }

    #[test]
    fn empty_input_zero_total() {
        let layout = compute_class_name_table_layout(Vec::new(), 0x4000, 0x1_0000_4000, false);
        assert!(layout.entries.is_empty());
        assert_eq!(layout.total_size, 0);
    }

    #[test]
    fn empty_force_emit_8_byte_count() {
        let layout = compute_class_name_table_layout(Vec::new(), 0x4000, 0x1_0000_4000, true);
        assert!(layout.entries.is_empty());
        assert_eq!(layout.total_size, COUNT_SIZE);
        assert_eq!(layout.count_vaddr, layout.table_vaddr);
    }

    #[test]
    fn single_entry_layout() {
        let layout =
            compute_class_name_table_layout(vec![entry(1, 0, 5)], 0x4000, 0x1_0000_4000, false);
        assert_eq!(layout.entries.len(), 1);
        assert_eq!(layout.table_vaddr, 0x1_0000_4000);
        assert_eq!(layout.table_file_offset, 0x4000);
        assert_eq!(layout.count_vaddr, 0x1_0000_4018);
        assert_eq!(layout.count_file_offset, 0x4018);
        assert_eq!(layout.total_size, 32);
    }

    #[test]
    fn multi_entry_layout_sorted_by_tag() {
        let entries = vec![entry(1, 0, 5), entry(2, 1, 4), entry(3, 2, 6)];
        let layout = compute_class_name_table_layout(entries, 0x4000, 0x1_0000_4000, false);
        assert_eq!(layout.entries.len(), 3);
        assert_eq!(layout.count_vaddr, 0x1_0000_4048);
        assert_eq!(layout.total_size, 80);
    }

    #[test]
    fn unaligned_cursor_gets_pad() {
        let layout =
            compute_class_name_table_layout(vec![entry(1, 0, 5)], 0x4001, 0x1_0000_4001, false);
        assert_eq!(layout.table_vaddr, 0x1_0000_4008);
        assert_eq!(layout.table_file_offset, 0x4008);
        assert_eq!(layout.total_size, 39);
    }

    #[test]
    fn payload_single_entry_layout_bytes() {
        let layout =
            compute_class_name_table_layout(vec![entry(7, 0, 5)], 0x4000, 0x1_0000_4000, false);
        let payload = build_class_name_table_payload(&layout, &[0xdead_beef_0000_1234u64]);
        assert_eq!(payload.len(), 32);
        // +0..3 class_tag = 7 LE
        assert_eq!(&payload[0..4], &7u32.to_le_bytes());
        // +4..7 _pad = 0
        assert_eq!(&payload[4..8], &0u32.to_le_bytes());
        // +8..15 name_ptr = supplied link value LE
        assert_eq!(&payload[8..16], &0xdead_beef_0000_1234u64.to_le_bytes());
        // +16..19 name_len = 5 LE
        assert_eq!(&payload[16..20], &5u32.to_le_bytes());
        // +20..23 _pad2 = 0
        assert_eq!(&payload[20..24], &0u32.to_le_bytes());
        // +24..31 count = 1 LE
        assert_eq!(&payload[24..32], &1u64.to_le_bytes());
    }

    #[test]
    fn extra_defined_syms_present_when_entries() {
        let entries = vec![crate::exec::UserClassNameEntry {
            class_tag: 1,
            name_ptr_sym: "__torajs_str_dyn_0".into(),
            name_len: 3,
        }];
        let syms = class_name_table_extra_defined_syms(&entries, false);
        assert!(syms.contains("___torajs_class_name_table"));
        assert!(syms.contains("___torajs_n_class_names"));
    }

    #[test]
    fn extra_defined_syms_force_emit_empty() {
        let entries: Vec<crate::exec::UserClassNameEntry> = Vec::new();
        let syms = class_name_table_extra_defined_syms(&entries, true);
        assert_eq!(syms.len(), 2);
    }

    #[test]
    fn extra_defined_syms_empty_no_force() {
        let entries: Vec<crate::exec::UserClassNameEntry> = Vec::new();
        let syms = class_name_table_extra_defined_syms(&entries, false);
        assert!(syms.is_empty());
    }
}
