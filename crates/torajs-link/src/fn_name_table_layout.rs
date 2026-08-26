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
//! - `__torajs_fn_name_table`       — N × 40-byte entries:
//!     ```text
//!       +0..7   : fn_addr   u64 (link-time chain-fixup target)
//!       +8..15  : name_ptr  *const u8 (link-time chain-fixup target
//!                                       to __TEXT,__cstring entry)
//!       +16..19 : name_len  u32
//!       +20..23 : arity     u32 (ES-spec Function.length, chunk 716)
//!       +24..31 : src_ptr   *const u8 (chain-fixup target to the
//!                                       type-erased fn source text,
//!                                       or literal NULL — RFC
//!                                       20260719-fn-tostring-source B3b)
//!       +32..35 : src_len   u32
//!       +36..39 : _pad      u32
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
use crate::user_strings_layout::UserStringsLayout;

/// Per-entry layout — one row of the `__torajs_fn_name_table[]`
/// rodata array.
#[derive(Debug, Clone)]
pub struct FnNameTableEntryLayout {
    /// `__torajs_fn_<fid>` alias sym (resolves to the user fn's
    /// vaddr after `register_fn_addr_syms`).
    pub fn_addr_sym: String,
    /// `__torajs_str_dyn_<sid>` alias sym (RawBytes flavour
    /// registered by `apply_user_string_overrides`). Resolves
    /// to the raw char payload's vaddr — no 16-byte Str header
    /// in front, so the runtime helper can `putc` each byte
    /// directly. The user_strings_layout substrate places the
    /// actual bytes; we just need the sym for the chain-fixup.
    pub name_ptr_sym: String,
    /// Code-unit count per ES `String.length`. Lives in the
    /// `name_len: u32` field (NOT a chain-fixup target).
    pub name_len: u32,
    /// ES-spec `Function.length` (chunk 716) — written into the
    /// entry's former `_pad: u32` slot (literal value, no chain
    /// fixup).
    pub arity: u32,
    /// RFC 20260719-fn-tostring-source B3b — `__torajs_str_dyn_<sid>`
    /// alias for the type-erased source text (RawBytes flavour, same
    /// substrate as `name_ptr_sym`). `None` writes a literal NULL
    /// `src_ptr` slot with NO chain-fixup link value consumed and no
    /// rebase target emitted.
    pub src_ptr_sym: Option<String>,
    /// ES `String.length` of the erased source; 0 when
    /// `src_ptr_sym` is `None`.
    pub src_len: u32,
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
pub const ENTRY_SIZE: u32 = 40;
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
    force_emit_globals: bool,
) -> FnNameTableLayout {
    // entries empty + !force_emit → no bytes / no syms. Self-contained
    // probes (no libtorajs_fnname.a) take this path so the pre-Step-5
    // byte stream survives.
    //
    // entries empty + force_emit → emit only the 8-byte count global
    // (= 0). table_vaddr aliases count_vaddr; the runtime reads count
    // first and skips the empty-array walk so the alias is safe.
    if entries.is_empty() && !force_emit_globals {
        return FnNameTableLayout {
            entries,
            table_vaddr: vaddr_base,
            table_file_offset: cursor,
            count_vaddr: vaddr_base,
            count_file_offset: cursor,
            total_size: 0,
        };
    }
    if entries.is_empty() {
        // 8-align cursor + vaddr so the count global lands on an
        // 8-aligned address (u64 load).
        let align_pad = (8 - (cursor % 8)) % 8;
        let cursor = cursor + align_pad;
        let vaddr_base = vaddr_base + u64::from(align_pad);
        return FnNameTableLayout {
            entries,
            table_vaddr: vaddr_base,
            table_file_offset: cursor,
            count_vaddr: vaddr_base,
            count_file_offset: cursor,
            total_size: align_pad + COUNT_SIZE,
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
            arity: 0,
            src_ptr_sym: None,
            src_len: 0,
        }
    }

    #[test]
    fn empty_input_zero_total() {
        let layout = compute_fn_name_table_layout(Vec::new(), 0x4000, 0x1_0000_4000, false);
        assert!(layout.entries.is_empty());
        assert_eq!(layout.total_size, 0);
    }

    #[test]
    fn single_entry_layout() {
        let layout =
            compute_fn_name_table_layout(vec![entry(0, 0, 3)], 0x4000, 0x1_0000_4000, false);
        assert_eq!(layout.entries.len(), 1);
        assert_eq!(layout.table_vaddr, 0x1_0000_4000);
        assert_eq!(layout.table_file_offset, 0x4000);
        // count is placed right after the single 40-byte entry.
        assert_eq!(layout.count_vaddr, 0x1_0000_4028);
        assert_eq!(layout.count_file_offset, 0x4028);
        // total = 40 entry + 8 count = 48 bytes.
        assert_eq!(layout.total_size, 48);
    }

    #[test]
    fn multi_entry_layout() {
        let entries = vec![entry(0, 0, 3), entry(1, 1, 5), entry(2, 2, 7)];
        let layout = compute_fn_name_table_layout(entries, 0x4000, 0x1_0000_4000, false);
        assert_eq!(layout.entries.len(), 3);
        assert_eq!(layout.table_vaddr, 0x1_0000_4000);
        // count placed right after 3 × 40 = 120 byte entries.
        assert_eq!(layout.count_vaddr, 0x1_0000_4078);
        assert_eq!(layout.count_file_offset, 0x4078);
        // total = 120 entries + 8 count = 128 bytes.
        assert_eq!(layout.total_size, 128);
    }

    #[test]
    fn unaligned_cursor_gets_pad() {
        // Cursor 0x4001 needs +7 align pad → entries start at 0x4008.
        let layout =
            compute_fn_name_table_layout(vec![entry(0, 0, 3)], 0x4001, 0x1_0000_4001, false);
        assert_eq!(layout.table_vaddr, 0x1_0000_4008);
        assert_eq!(layout.table_file_offset, 0x4008);
        assert_eq!(layout.count_vaddr, 0x1_0000_4030);
        // total includes the 7-byte pad + 40 entry + 8 count = 55.
        assert_eq!(layout.total_size, 55);
    }

    #[test]
    fn payload_writes_null_src_slot_without_link_value() {
        // One src-less + one src-carrying entry: the src-less row
        // consumes 2 link values and writes a literal 0 src_ptr; the
        // src row consumes 3.
        let mut with_src = entry(1, 1, 5);
        with_src.src_ptr_sym = Some("__user_string_9".into());
        with_src.src_len = 21;
        let layout = compute_fn_name_table_layout(
            vec![entry(0, 0, 3), with_src],
            0x4000,
            0x1_0000_4000,
            false,
        );
        let payload = build_fn_name_table_payload(&layout, &[0xA1, 0xA2, 0xB1, 0xB2, 0xB3]);
        // Entry 0: fn_addr=A1, name_ptr=A2, src_ptr=NULL, src_len=0.
        assert_eq!(payload[0..8], 0xA1u64.to_le_bytes());
        assert_eq!(payload[8..16], 0xA2u64.to_le_bytes());
        assert_eq!(payload[24..32], 0u64.to_le_bytes());
        assert_eq!(payload[32..36], 0u32.to_le_bytes());
        // Entry 1: fn_addr=B1, name_ptr=B2, src_ptr=B3, src_len=21.
        assert_eq!(payload[40..48], 0xB1u64.to_le_bytes());
        assert_eq!(payload[48..56], 0xB2u64.to_le_bytes());
        assert_eq!(payload[64..72], 0xB3u64.to_le_bytes());
        assert_eq!(payload[72..76], 21u32.to_le_bytes());
        // Trailing count global.
        assert_eq!(payload[80..88], 2u64.to_le_bytes());
    }

    #[test]
    fn rebase_targets_skip_null_src_slot() {
        let mut with_src = entry(1, 1, 5);
        with_src.src_ptr_sym = Some("__user_string_9".into());
        with_src.src_len = 21;
        let layout = compute_fn_name_table_layout(
            vec![entry(0, 0, 3), with_src],
            0x4000,
            0x1_0000_4000,
            false,
        );
        let mut sym_table = SymTable::new();
        sym_table.insert("__torajs_fn_0".into(), 0x1_0000_1000);
        sym_table.insert("__torajs_fn_1".into(), 0x1_0000_1100);
        sym_table.insert("__user_string_0".into(), 0x1_0000_2000);
        sym_table.insert("__user_string_1".into(), 0x1_0000_2100);
        sym_table.insert("__user_string_9".into(), 0x1_0000_2900);
        let targets =
            compute_fn_name_table_rebase_targets(&layout, &sym_table, 0x1_0000_0000, 0x1_0000_0000);
        // 2 slots for the src-less entry + 3 for the src entry.
        assert_eq!(targets.len(), 5);
        // The 5th target is the src slot at entry1 + 24 = 0x4028 + 24.
        assert_eq!(targets[4].0, 0x4040);
        assert_eq!(targets[4].1, 0x2900);
    }
}

/// Build the contiguous bytes for the table + count globals,
/// consuming the chain-fixup link values pre-computed for the
/// fn_addr + name_ptr slots. Layout per `compute_fn_name_table_layout`.
///
/// Each entry contributes two or three chain-fixup link values in
/// flat `entries[*]` walk order — `fn_addr` first, then `name_ptr`,
/// then `src_ptr` IF `src_ptr_sym` is `Some` (a `None` src slot is a
/// literal NULL: no link value consumed — the walk order must stay
/// in lockstep with `compute_fn_name_table_rebase_targets`).
/// `name_len` / `arity` / `src_len` are written directly (literal
/// values, no chain fixup).
///
/// The trailing `__torajs_fn_name_table_count: u64` global gets the
/// raw entry count as a little-endian u64.
pub fn build_fn_name_table_payload(
    layout: &FnNameTableLayout,
    slot_link_values: &[u64],
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(layout.total_size as usize);
    if layout.entries.is_empty() && layout.total_size == 0 {
        // Empty + !force_emit — no bytes.
        return buf;
    }
    if layout.entries.is_empty() {
        // Empty + force_emit — leading 8-align pad then count global
        // (= 0). The table sym aliases the count sym so the empty-
        // array walk in the runtime helper short-circuits via the
        // 0-count read before any slot deref.
        let leading_pad = layout.total_size as usize - COUNT_SIZE as usize;
        buf.resize(leading_pad, 0);
        buf.extend_from_slice(&0u64.to_le_bytes());
        debug_assert_eq!(buf.len() as u32, layout.total_size);
        return buf;
    }
    let entries_total = (layout.entries.len() as u32) * ENTRY_SIZE;
    let leading_pad = layout.total_size as usize - entries_total as usize - COUNT_SIZE as usize;
    buf.resize(leading_pad, 0);
    // Each entry: 2-3 chain-fixup-encoded u64s (fn_addr, name_ptr,
    // src_ptr when present) + literal name_len / arity / src_len /
    // pad. Link-value count varies per entry, so walk with a cursor.
    let expected_link_count: usize = layout
        .entries
        .iter()
        .map(|e| 2 + usize::from(e.src_ptr_sym.is_some()))
        .sum();
    debug_assert_eq!(
        slot_link_values.len(),
        expected_link_count,
        "slot_link_values mismatch — expected {} (2-3 per entry × {} entries), got {}",
        expected_link_count,
        layout.entries.len(),
        slot_link_values.len(),
    );
    let mut lv_cursor = 0usize;
    for entry in layout.entries.iter() {
        let fn_addr_lv = slot_link_values[lv_cursor];
        let name_ptr_lv = slot_link_values[lv_cursor + 1];
        lv_cursor += 2;
        let src_ptr_lv = if entry.src_ptr_sym.is_some() {
            let lv = slot_link_values[lv_cursor];
            lv_cursor += 1;
            lv
        } else {
            0
        };
        buf.extend_from_slice(&fn_addr_lv.to_le_bytes());
        buf.extend_from_slice(&name_ptr_lv.to_le_bytes());
        buf.extend_from_slice(&entry.name_len.to_le_bytes());
        buf.extend_from_slice(&entry.arity.to_le_bytes());
        buf.extend_from_slice(&src_ptr_lv.to_le_bytes());
        buf.extend_from_slice(&entry.src_len.to_le_bytes());
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
    if layout.total_size == 0 {
        // Empty + !force_emit case — nothing to register.
        return;
    }
    // Triple underscore — mirrors the strtab byte sequence Rust emits
    // for `extern "C" static __torajs_fn_name_table` (Mach-O leading-`_`
    // convention). The member reloc encoder consults sym_table with
    // the strtab key directly, so the key here must match too. See
    // `fn_name_table_extra_defined_syms` doc for the worklist symmetry.
    sym_table.insert("___torajs_fn_name_table".into(), layout.table_vaddr);
    sym_table.insert("___torajs_fn_name_table_count".into(), layout.count_vaddr);
}

/// Sym names to flag as link-defined in the worklist closure (so
/// the resolver doesn't classify them as undefined dyld imports).
/// Takes the raw `cfg.fn_name_globals` slice (mirrors the
/// `user_vtables_extra_defined_syms` pattern) so the worklist
/// closure can extend `extra` before the actual layout is computed.
/// Empty when `fn_name_globals` is empty.
pub fn fn_name_table_extra_defined_syms(
    fn_name_globals: &[crate::exec::UserFnNameEntry],
    force_emit_globals: bool,
) -> std::collections::BTreeSet<String> {
    let mut s = std::collections::BTreeSet::new();
    if !fn_name_globals.is_empty() || force_emit_globals {
        // Triple underscore = Mach-O leading-`_` convention applied
        // to Rust's `extern "C" static __torajs_fn_name_table`. The
        // worklist closure compares against the strtab byte sequence
        // verbatim (no prefix-stripping), and Rust adds the leading
        // `_` on Apple platforms — see `rewrite_extern_relocs` in
        // `crates/torajs-cli/src/cmd_build.rs` for the symmetric
        // codegen-side rewrite that fixes up `extern fn` reloc
        // targets so they line up with strtab too.
        s.insert("___torajs_fn_name_table".to_string());
        s.insert("___torajs_fn_name_table_count".to_string());
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
    let mut targets = Vec::with_capacity(layout.entries.len() * 3);
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
        // Slot 2 — src_ptr at entry_vaddr + 24, ONLY when a source
        // string is recorded (B3b). A `None` src slot stays a
        // literal NULL: no rebase target, matching the payload
        // builder's no-link-value contract.
        if let Some(src_sym) = &entry.src_ptr_sym {
            let src_slot_off = ((entry_vaddr + 24) - seg_vmaddr_base) as u32 as u64;
            let src_target_vaddr = *sym_table.get(src_sym).unwrap_or_else(|| {
                panic!(
                    "fn_name_table entry {i:?} src_ptr_sym {src_sym:?} missing from sym table — \
                     apply user_string overrides before computing rebase targets",
                )
            });
            targets.push((src_slot_off, src_target_vaddr - image_vmaddr_base));
        }
    }
    targets
}

/// Pipeline helper — given `text_vmaddr_base` + `file_offset` cursor,
/// derive the vaddr_base and compute the layout in one call.
pub fn build_fn_name_table_region(
    entries: Vec<FnNameTableEntryLayout>,
    text_vmaddr_base: u64,
    file_offset: u32,
    force_emit_globals: bool,
) -> FnNameTableLayout {
    compute_fn_name_table_layout(
        entries,
        file_offset,
        text_vmaddr_base + u64::from(file_offset),
        force_emit_globals,
    )
}

/// archive_link-side helper — mirror of
/// [`crate::user_vtables_layout::vtable_rebase_targets_from_fn_vaddrs`].
/// Builds a mini sym table containing fn_addr aliases
/// (`__torajs_fn_<fid>` → `fn_vaddrs[fid]`) + user-string aliases
/// (`entry.sym` → `entry.vaddr` for every `user_strings_layout` entry)
/// so [`compute_fn_name_table_rebase_targets`] resolves both slot
/// targets without needing the full archive_emit-side
/// `effective_sym_table`. Returns the per-slot
/// `(slot_off_in_seg, target_off_in_image)` pairs the chained-fixup
/// pipeline feeds into `build_chained_fixups` so dyld rebases each
/// `fn_addr` + `name_ptr` slot at image load.
pub fn fn_name_table_rebase_targets_from_layouts(
    layout: &FnNameTableLayout,
    fn_vaddrs: &[u64],
    user_strings_layout: &UserStringsLayout,
    seg_vmaddr_base: u64,
    image_vmaddr_base: u64,
) -> Vec<RebaseTarget> {
    if layout.entries.is_empty() {
        return Vec::new();
    }
    let mut sym_table = SymTable::new();
    for (fid, vaddr) in fn_vaddrs.iter().enumerate() {
        sym_table.insert(format!("__torajs_fn_{fid}"), *vaddr);
    }
    for entry in &user_strings_layout.entries {
        sym_table.insert(entry.sym.clone(), entry.vaddr);
        // A literal's `__torajs_str_dyn_<sid>` reader points at the
        // payload behind its Str header — the same alias
        // `apply_user_string_overrides` registers on the emit side.
        if let Some(alias) = &entry.payload_alias {
            sym_table.insert(
                alias.clone(),
                entry.vaddr + u64::from(crate::user_strings_layout::STR_HEADER_SIZE),
            );
        }
    }
    compute_fn_name_table_rebase_targets(layout, &sym_table, seg_vmaddr_base, image_vmaddr_base)
}
