//! SD-4c-prereq+e8 — layout + emit for the cycle collector's
//! per-class `child_offsets` tables.
//!
//! Byte ABI from the LLVM-era `ssa_inkwell::class_globals::
//! emit_class_layouts` (canonical here since the swap). The
//! cycle collector indexes by `class_tag - 1`; for each entry it
//! reads `n_children` then walks `offsets[]` to enumerate the
//! refcounted-pointer fields. Three rodata blocks per `class_layouts`
//! input, all sharing `__DATA_CONST` with the vtables:
//!
//! 1. `N` inner `.__class_offsets_<i>` arrays — `[K x i32]` (private
//!    naming; never enters the public sym table). Empty entry → no
//!    inner global allocated, the outer slot packs `(0, NULL)`.
//! 2. `__torajs_class_layouts` outer table — `[N x { u32 n; ptr;
//!    ptr field_metadata }]` laid out as 24-byte entries (`n` +
//!    4-byte pad + 8-byte child_offsets ptr + 8-byte field_metadata
//!    ptr). W-J Phase A2: the field_metadata slot is reserved + NULL;
//!    A3 will fill it with a pointer to per-class field-name/offset/
//!    type-tag metadata so the reflection consumers (Phase B `gOPD`
//!    struct arm / Phase C `Object.keys`/`values`/`entries` / Phase D
//!    `inspect.rs` Tag::Obj walker) can do struct cell reflection.
//! 3. `__torajs_n_class_layouts` count — `u32 = N`.
//!
//! Each non-empty entry's outer ptr slot needs an `LC_DYLD_CHAINED_FIXUPS`
//! rebase entry so dyld stamps the inner global's runtime vaddr after
//! the ASLR slide (same e7b path the vtable slots ride on).

use crate::chained_fixups_starts::RebaseTarget;
use crate::exec::UserClassLayoutEntry;
use crate::resolve::SymTable;

/// Public sym names the link layer registers into the effective sym
/// table after this layout's vaddr placement is known. Apple `_`-prefix
/// form (the actual Mach-O on-disk name) so staticlib members that
/// reference these via `extern "C"` resolve at the link layer; codegen
/// callers (probe + tr build) emit `Page21`/`PageOff12` relocs against
/// the same name.
pub const CLASS_LAYOUTS_SYM: &str = "___torajs_class_layouts";
pub const N_CLASS_LAYOUTS_SYM: &str = "___torajs_n_class_layouts";

/// On-disk size of one outer-table entry. W-J Phase A2 extended
/// 16 → 24 to carry a reserved `field_metadata` ptr slot (NULL until
/// Phase A3 fills it). Must stay in lockstep with
/// `torajs_cycle::layout::ClassLayout`'s `size_of` (24B, asserted in
/// that crate's `class_layout_struct` test).
///   `u32 n_children` (4) + 4-byte pad + `ptr child_offsets` (8) +
///   `ptr field_metadata` (8) = 24
const OUTER_ENTRY_SIZE: u32 = 24;
/// Outer-table entry alignment — derived from the `ptr` field's
/// 8-byte alignment requirement. Matches LLVM's struct layout for
/// `{ i32, ptr }` packed=false.
const OUTER_ENTRY_ALIGN: u32 = 8;
/// Inner-array element size — `i32`.
const INNER_ELEM_SIZE: u32 = 4;
/// Inner-array element alignment — `i32`.
const INNER_ELEM_ALIGN: u32 = 4;
/// Count global on-disk size — `u32`.
const COUNT_SIZE: u32 = 4;
/// Count global alignment — `u32`.
const COUNT_ALIGN: u32 = 4;
/// Outer-entry byte offset where the inner-ptr field lives
/// (4 bytes `n_children` + 4 bytes padding).
const OUTER_PTR_OFFSET_IN_ENTRY: u32 = 8;

/// Per-entry placement — both the inner `.__class_offsets_<i>` global
/// (if `child_offsets` is non-empty) and the outer-table slot.
#[derive(Debug, Clone)]
pub struct UserClassLayoutEntryLayout {
    /// Number of refcounted-pointer fields the entry advertises.
    /// Mirrors `child_offsets.len()`; cached for the build pass.
    pub n_children: u32,
    /// Cloned offsets for the build pass — written little-endian
    /// into the inner global.
    pub child_offsets: Vec<u32>,
    /// `None` when `child_offsets` is empty: no inner global is
    /// allocated and the outer slot packs `(0, NULL)`. `Some(vaddr)`
    /// is the absolute vaddr of `.__class_offsets_<i>`.
    pub inner_vaddr: Option<u64>,
    /// File offset of the inner global (or `0` when no inner is
    /// emitted — caller never indexes it in that case).
    pub inner_file_offset: u32,
    /// Outer-table entry vaddr (= `outer_vaddr + i * 16`). Inner ptr
    /// slot lives at `entry_vaddr + OUTER_PTR_OFFSET_IN_ENTRY`.
    pub entry_vaddr: u64,
    /// Outer-table entry file offset.
    pub entry_file_offset: u32,
}

/// Aggregated layout for one full class-layouts region.
#[derive(Debug, Clone, Default)]
pub struct UserClassLayoutsLayout {
    pub entries: Vec<UserClassLayoutEntryLayout>,
    /// Region's first byte file offset (= leading-pad start, before
    /// any inner globals).
    pub file_offset: u32,
    /// Region's first byte vaddr (matches `file_offset` after the
    /// caller-supplied `vaddr_base` translation).
    pub vaddr: u64,
    /// `__torajs_class_layouts` outer-table vaddr (= sym vaddr for
    /// `apply_user_class_layouts_overrides`).
    pub outer_vaddr: u64,
    /// `__torajs_class_layouts` outer-table file offset.
    pub outer_file_offset: u32,
    /// `__torajs_n_class_layouts` count global vaddr.
    pub count_vaddr: u64,
    /// `__torajs_n_class_layouts` count global file offset.
    pub count_file_offset: u32,
    /// Total cumulative byte size = leading pad + inner globals +
    /// outer/count alignment pads + outer table + count u32. Caller
    /// adds this to the segment cursor.
    pub total_size: u32,
}

/// Compute placements for the inner globals, outer table, and count
/// global. Returns an empty layout (no entries, `total_size = 0`) when
/// `class_layouts` is empty so callers can short-circuit before
/// touching any segment bytes (mirrors `compute_user_vtables_layout`).
pub fn compute_user_class_layouts_layout(
    class_layouts: &[UserClassLayoutEntry],
    cursor: u32,
    vaddr_base: u64,
    force_emit_globals: bool,
) -> UserClassLayoutsLayout {
    // class_layouts empty + force_emit_globals=false → no bytes, no
    // sym registration. Self-contained probes (no `libtorajs_cycle.a`
    // dep) take this path and stay byte-identical to the pre-e8
    // baseline.
    //
    // class_layouts empty + force_emit_globals=true → emit a 4-byte
    // count global (= 0) and register both syms aliased to it; the
    // empty outer table has zero bytes so `outer_vaddr` sharing the
    // count slot is safe (runtime reads count first, only walks the
    // outer table when N > 0). `tr build` takes this path because
    // every staticlib bundle pulls `libtorajs_cycle.a`, which
    // unconditionally references these syms.
    if class_layouts.is_empty() && !force_emit_globals {
        return UserClassLayoutsLayout {
            entries: Vec::new(),
            file_offset: cursor,
            vaddr: vaddr_base,
            outer_vaddr: vaddr_base,
            outer_file_offset: cursor,
            count_vaddr: vaddr_base,
            count_file_offset: cursor,
            total_size: 0,
        };
    }
    // Inner globals come first — they need 4-byte alignment, which
    // matches the caller's cursor when the prior region ended on a
    // 4-aligned boundary (vtables end on an 8-aligned boundary, which
    // is also 4-aligned).
    let align_pad_to_inner = pad_to(cursor, INNER_ELEM_ALIGN);
    let region_file_offset = cursor + align_pad_to_inner;
    let region_vaddr = vaddr_base + u64::from(align_pad_to_inner);

    let mut entries: Vec<UserClassLayoutEntryLayout> = Vec::with_capacity(class_layouts.len());
    let mut cumulative: u32 = 0;
    for entry in class_layouts {
        let n_children = entry.child_offsets.len() as u32;
        let (inner_vaddr, inner_file_offset, inner_size) = if entry.child_offsets.is_empty() {
            (None, 0u32, 0u32)
        } else {
            let inner_file_offset = region_file_offset + cumulative;
            let inner_vaddr = region_vaddr + u64::from(cumulative);
            (
                Some(inner_vaddr),
                inner_file_offset,
                n_children * INNER_ELEM_SIZE,
            )
        };
        cumulative += inner_size;
        entries.push(UserClassLayoutEntryLayout {
            n_children,
            child_offsets: entry.child_offsets.clone(),
            inner_vaddr,
            inner_file_offset,
            entry_vaddr: 0, // filled in below once outer placement is known
            entry_file_offset: 0,
        });
    }

    // Outer table — 8-aligned past the inner globals.
    let inner_end_file_offset = region_file_offset + cumulative;
    let inner_end_vaddr = region_vaddr + u64::from(cumulative);
    let outer_align_pad = pad_to(inner_end_file_offset, OUTER_ENTRY_ALIGN);
    let outer_file_offset = inner_end_file_offset + outer_align_pad;
    let outer_vaddr = inner_end_vaddr + u64::from(outer_align_pad);
    for (i, slot) in entries.iter_mut().enumerate() {
        slot.entry_file_offset = outer_file_offset + (i as u32) * OUTER_ENTRY_SIZE;
        slot.entry_vaddr = outer_vaddr + (i as u64) * u64::from(OUTER_ENTRY_SIZE);
    }
    let outer_size = entries.len() as u32 * OUTER_ENTRY_SIZE;

    // Count global — 4-aligned past the outer table.
    let count_align_pad = pad_to(outer_file_offset + outer_size, COUNT_ALIGN);
    let count_file_offset = outer_file_offset + outer_size + count_align_pad;
    let count_vaddr = outer_vaddr + u64::from(outer_size + count_align_pad);
    let total_size = (count_file_offset + COUNT_SIZE) - cursor;

    UserClassLayoutsLayout {
        entries,
        file_offset: region_file_offset,
        vaddr: region_vaddr,
        outer_vaddr,
        outer_file_offset,
        count_vaddr,
        count_file_offset,
        total_size,
    }
}

fn pad_to(cursor: u32, align: u32) -> u32 {
    debug_assert!(
        align.is_power_of_two(),
        "align {align} must be a power of two"
    );
    (align - (cursor % align)) % align
}

/// Build the contiguous byte payload: inner globals, leading pad,
/// outer-table entries (each filled from `outer_inner_link_values`
/// for entries with a `Some(inner_vaddr)`; `None` entries pack
/// `(0, NULL)`), trailing pad, then the count u32. `outer_inner_link_values`
/// carries one entry per non-empty `child_offsets` — the chained-fixup
/// link value the e7b path produces for each inner-ptr rebase target.
///
/// Returns a buffer of exactly `layout.total_size` bytes. The caller
/// splices it into `__DATA_CONST` at `layout.file_offset`.
pub fn build_user_class_layouts_payload(
    layout: &UserClassLayoutsLayout,
    outer_inner_link_values: &[u64],
) -> Vec<u8> {
    let mut buf: Vec<u8> = Vec::with_capacity(layout.total_size as usize);
    if layout.total_size == 0 {
        debug_assert!(outer_inner_link_values.is_empty());
        return buf;
    }
    // Inner globals — concatenated `[K x i32]` little-endian.
    for entry in &layout.entries {
        if entry.inner_vaddr.is_none() {
            continue;
        }
        for &off in &entry.child_offsets {
            buf.extend_from_slice(&off.to_le_bytes());
        }
    }
    // Pad to outer-table alignment.
    while buf.len() < (layout.outer_file_offset - layout.file_offset) as usize {
        buf.push(0);
    }
    // Outer table — 24 bytes per entry: u32 n + 4 pad + u64
    // inner_link_value + u64 field_metadata_link_value. W-J Phase A2:
    // field_metadata is a NULL stub (Phase A3 will populate it);
    // since it's NULL there's no chained-fixup rebase target for it
    // (the rebase target list still has one entry per non-empty
    // child_offsets — compute_class_layouts_rebase_targets unchanged).
    let mut link_idx = 0usize;
    for entry in &layout.entries {
        buf.extend_from_slice(&entry.n_children.to_le_bytes());
        buf.extend_from_slice(&[0u8; 4]);
        let link_value: u64 = if entry.inner_vaddr.is_some() {
            let v = *outer_inner_link_values.get(link_idx).unwrap_or_else(|| {
                panic!(
                    "outer_inner_link_values too short — expected at least {} entries, got {}",
                    link_idx + 1,
                    outer_inner_link_values.len(),
                )
            });
            link_idx += 1;
            v
        } else {
            0
        };
        buf.extend_from_slice(&link_value.to_le_bytes());
        // W-J Phase A2: field_metadata_ptr stub (NULL).
        buf.extend_from_slice(&0u64.to_le_bytes());
    }
    debug_assert_eq!(
        link_idx,
        outer_inner_link_values.len(),
        "outer_inner_link_values carries extras the layout did not consume — caller misalignment",
    );
    // Pad to count alignment.
    while buf.len() < (layout.count_file_offset - layout.file_offset) as usize {
        buf.push(0);
    }
    // Count u32.
    buf.extend_from_slice(&(layout.entries.len() as u32).to_le_bytes());
    debug_assert_eq!(buf.len() as u32, layout.total_size);
    buf
}

/// Register `__torajs_class_layouts` + `__torajs_n_class_layouts` →
/// vaddr into the effective sym table.
pub fn apply_user_class_layouts_overrides(
    layout: &UserClassLayoutsLayout,
    sym_table: &mut SymTable,
) {
    if layout.total_size == 0 {
        return;
    }
    sym_table.insert(CLASS_LAYOUTS_SYM.into(), layout.outer_vaddr);
    sym_table.insert(N_CLASS_LAYOUTS_SYM.into(), layout.count_vaddr);
}

/// Sym names to flag as link-defined so the worklist closure doesn't
/// report them as unresolved externs.
pub fn user_class_layouts_extra_defined_syms(
    class_layouts: &[UserClassLayoutEntry],
    force_emit_globals: bool,
) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    if !class_layouts.is_empty() || force_emit_globals {
        out.insert(CLASS_LAYOUTS_SYM.into());
        out.insert(N_CLASS_LAYOUTS_SYM.into());
    }
    out
}

/// Derive the rebase target list for the outer-table inner-ptr slots
/// — one `RebaseTarget` per entry whose `inner_vaddr` is `Some(_)`.
/// `seg_vmaddr_base` = `__DATA_CONST` segment base; `image_vmaddr_base`
/// = `TEXT_VMADDR_BASE`. Same conventions as `compute_vtable_rebase_targets`.
pub fn compute_class_layouts_rebase_targets(
    layout: &UserClassLayoutsLayout,
    seg_vmaddr_base: u64,
    image_vmaddr_base: u64,
) -> Vec<RebaseTarget> {
    let mut targets = Vec::new();
    for entry in &layout.entries {
        let Some(inner_vaddr) = entry.inner_vaddr else {
            continue;
        };
        let slot_vaddr = entry.entry_vaddr + u64::from(OUTER_PTR_OFFSET_IN_ENTRY);
        debug_assert!(
            slot_vaddr >= seg_vmaddr_base,
            "class_layouts slot vaddr {slot_vaddr:#x} cannot precede segment base {seg_vmaddr_base:#x}",
        );
        debug_assert!(
            inner_vaddr >= image_vmaddr_base,
            "class_layouts inner_vaddr {inner_vaddr:#x} cannot precede image base {image_vmaddr_base:#x}",
        );
        let slot_off_in_seg = slot_vaddr - seg_vmaddr_base;
        let target_off_in_image = inner_vaddr - image_vmaddr_base;
        targets.push((slot_off_in_seg, target_off_in_image));
    }
    targets
}

/// Pipeline helper — derive vaddr_base from segment base + cursor and
/// compute the layout in one call. Mirrors `build_user_vtables_region`.
pub fn build_user_class_layouts_region(
    class_layouts: &[UserClassLayoutEntry],
    seg_vmaddr_base: u64,
    file_offset: u32,
    seg_file_offset_base: u32,
    force_emit_globals: bool,
) -> UserClassLayoutsLayout {
    let vaddr_base = seg_vmaddr_base + u64::from(file_offset - seg_file_offset_base);
    compute_user_class_layouts_layout(class_layouts, file_offset, vaddr_base, force_emit_globals)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(offsets: &[u32]) -> UserClassLayoutEntry {
        UserClassLayoutEntry {
            child_offsets: offsets.to_vec(),
        }
    }

    #[test]
    fn empty_input_zero_total() {
        let layout = compute_user_class_layouts_layout(&[], 0x4000, 0x1_0000_4000, false);
        assert!(layout.entries.is_empty());
        assert_eq!(layout.total_size, 0);
    }

    #[test]
    fn empty_input_force_emit_yields_4byte_count_global() {
        // tr build path — staticlib references the syms even when the
        // program has no classes; force_emit_globals lands a 4-byte u32
        // count = 0 + aliases both sym vaddrs to it.
        let layout = compute_user_class_layouts_layout(&[], 0x4000, 0x1_0000_4000, true);
        assert!(layout.entries.is_empty());
        assert_eq!(layout.total_size, 4);
        assert_eq!(layout.outer_vaddr, 0x1_0000_4000);
        assert_eq!(layout.count_vaddr, 0x1_0000_4000);

        let payload = build_user_class_layouts_payload(&layout, &[]);
        assert_eq!(payload, vec![0u8, 0, 0, 0]);

        let mut sym_table = SymTable::new();
        apply_user_class_layouts_overrides(&layout, &mut sym_table);
        assert_eq!(sym_table.get(CLASS_LAYOUTS_SYM), Some(&0x1_0000_4000));
        assert_eq!(sym_table.get(N_CLASS_LAYOUTS_SYM), Some(&0x1_0000_4000));
    }

    #[test]
    fn single_entry_two_offsets() {
        // 1 entry, 2 child offsets → inner 8 bytes + outer 24 bytes +
        // count 4 bytes = 36 bytes (W-J Phase A2: was 28 pre-A2 when
        // OUTER_ENTRY_SIZE was 16). cursor 0x4000 is 8-aligned so no
        // leading pads needed.
        let class_layouts = [entry(&[24, 32])];
        let layout =
            compute_user_class_layouts_layout(&class_layouts, 0x4000, 0x1_0000_4000, false);
        assert_eq!(layout.entries.len(), 1);
        // Inner starts at base.
        assert_eq!(layout.entries[0].inner_file_offset, 0x4000);
        assert_eq!(layout.entries[0].inner_vaddr, Some(0x1_0000_4000));
        // Outer past 8-byte inner.
        assert_eq!(layout.outer_file_offset, 0x4008);
        assert_eq!(layout.outer_vaddr, 0x1_0000_4008);
        // Count past 24-byte outer.
        assert_eq!(layout.count_file_offset, 0x4020);
        assert_eq!(layout.count_vaddr, 0x1_0000_4020);
        // Total = 8 + 24 + 4 = 36.
        assert_eq!(layout.total_size, 36);
    }

    #[test]
    fn empty_child_offsets_skips_inner_global() {
        // 1 entry, 0 child offsets → no inner; outer 24 bytes (n=0,
        // ptr=0, field_meta=0) + count 4 bytes = 28 bytes (W-J Phase A2:
        // was 20 pre-A2 with 16B entry).
        let class_layouts = [entry(&[])];
        let layout =
            compute_user_class_layouts_layout(&class_layouts, 0x4000, 0x1_0000_4000, false);
        assert!(layout.entries[0].inner_vaddr.is_none());
        assert_eq!(layout.outer_file_offset, 0x4000);
        assert_eq!(layout.total_size, 28);
    }

    #[test]
    fn mixed_empty_and_nonempty_pack_back_to_back() {
        // Entry 0: 1 offset (inner 4 bytes + pad 4 to outer 8-align)
        // Entry 1: empty (no inner)
        // Entry 2: 2 offsets (inner 8 bytes back-to-back after entry 0's 4)
        let class_layouts = [entry(&[16]), entry(&[]), entry(&[32, 40])];
        let layout =
            compute_user_class_layouts_layout(&class_layouts, 0x4000, 0x1_0000_4000, false);
        assert_eq!(layout.entries[0].inner_file_offset, 0x4000);
        assert_eq!(layout.entries[0].inner_vaddr, Some(0x1_0000_4000));
        assert!(layout.entries[1].inner_vaddr.is_none());
        // Entry 2 inner sits at 0x4004 (right after entry 0's 4 bytes).
        assert_eq!(layout.entries[2].inner_file_offset, 0x4004);
        // Total inner = 4 + 8 = 12 bytes; outer 8-align pad = 4 bytes.
        // Outer starts at 0x4010.
        assert_eq!(layout.outer_file_offset, 0x4010);
        // W-J Phase A2: 3 outer entries × 24 = 72 bytes; count at 0x4058.
        assert_eq!(layout.count_file_offset, 0x4058);
        assert_eq!(layout.total_size, 4 + 8 + 4 + 72 + 4); // 92
    }

    #[test]
    fn build_payload_emits_inner_outer_count_in_order() {
        let class_layouts = [entry(&[16, 24])];
        let layout =
            compute_user_class_layouts_layout(&class_layouts, 0x4000, 0x1_0000_4000, false);
        let link_values = [0xDEAD_BEEFu64];
        let payload = build_user_class_layouts_payload(&layout, &link_values);
        // W-J Phase A2: 8 inner + 24 outer + 4 count = 36 (was 28).
        assert_eq!(payload.len(), 36);
        // [0..8]   inner [16, 24] LE
        assert_eq!(u32::from_le_bytes(payload[0..4].try_into().unwrap()), 16);
        assert_eq!(u32::from_le_bytes(payload[4..8].try_into().unwrap()), 24);
        // [8..12]  outer entry n_children = 2
        assert_eq!(u32::from_le_bytes(payload[8..12].try_into().unwrap()), 2);
        // [12..16] outer entry pad = 0
        assert_eq!(u32::from_le_bytes(payload[12..16].try_into().unwrap()), 0);
        // [16..24] outer entry inner-ptr = link value
        assert_eq!(
            u64::from_le_bytes(payload[16..24].try_into().unwrap()),
            0xDEAD_BEEF
        );
        // W-J Phase A2: [24..32] outer entry field_metadata_ptr = NULL stub
        assert_eq!(u64::from_le_bytes(payload[24..32].try_into().unwrap()), 0);
        // [32..36] count = 1
        assert_eq!(u32::from_le_bytes(payload[32..36].try_into().unwrap()), 1);
    }

    #[test]
    fn build_payload_zeroes_empty_entry_outer_slot() {
        let class_layouts = [entry(&[])];
        let layout =
            compute_user_class_layouts_layout(&class_layouts, 0x4000, 0x1_0000_4000, false);
        let payload = build_user_class_layouts_payload(&layout, &[]);
        // W-J Phase A2: 24 outer + 4 count = 28 (was 20).
        assert_eq!(payload.len(), 28);
        // outer entry n_children = 0
        assert_eq!(u32::from_le_bytes(payload[0..4].try_into().unwrap()), 0);
        // outer entry inner-ptr = NULL
        assert_eq!(u64::from_le_bytes(payload[8..16].try_into().unwrap()), 0);
        // W-J Phase A2: field_metadata_ptr NULL stub
        assert_eq!(u64::from_le_bytes(payload[16..24].try_into().unwrap()), 0);
        // count = 1
        assert_eq!(u32::from_le_bytes(payload[24..28].try_into().unwrap()), 1);
    }

    #[test]
    fn apply_overrides_inserts_both_syms() {
        let class_layouts = [entry(&[16])];
        let layout =
            compute_user_class_layouts_layout(&class_layouts, 0x4000, 0x1_0000_4000, false);
        let mut sym_table = SymTable::new();
        apply_user_class_layouts_overrides(&layout, &mut sym_table);
        assert_eq!(sym_table.get(CLASS_LAYOUTS_SYM), Some(&layout.outer_vaddr));
        assert_eq!(
            sym_table.get(N_CLASS_LAYOUTS_SYM),
            Some(&layout.count_vaddr)
        );
    }

    #[test]
    fn apply_overrides_skips_empty_input() {
        let layout = compute_user_class_layouts_layout(&[], 0x4000, 0x1_0000_4000, false);
        let mut sym_table = SymTable::new();
        apply_user_class_layouts_overrides(&layout, &mut sym_table);
        assert!(sym_table.get(CLASS_LAYOUTS_SYM).is_none());
        assert!(sym_table.get(N_CLASS_LAYOUTS_SYM).is_none());
    }

    #[test]
    fn extra_defined_syms_two_when_nonempty_or_force_emit() {
        let empty = user_class_layouts_extra_defined_syms(&[], false);
        assert!(empty.is_empty());
        let forced = user_class_layouts_extra_defined_syms(&[], true);
        assert_eq!(forced.len(), 2);
        assert!(forced.contains(CLASS_LAYOUTS_SYM));
        assert!(forced.contains(N_CLASS_LAYOUTS_SYM));
        let class_layouts = [entry(&[16])];
        let some = user_class_layouts_extra_defined_syms(&class_layouts, false);
        assert_eq!(some.len(), 2);
        assert!(some.contains(CLASS_LAYOUTS_SYM));
        assert!(some.contains(N_CLASS_LAYOUTS_SYM));
    }

    #[test]
    fn rebase_targets_emit_one_per_nonempty_entry() {
        let class_layouts = [entry(&[16]), entry(&[]), entry(&[24, 32])];
        let layout =
            compute_user_class_layouts_layout(&class_layouts, 0x4000, 0x1_0000_4000, false);
        let targets = compute_class_layouts_rebase_targets(&layout, 0x1_0000_0000, 0x1_0000_0000);
        // 2 non-empty entries → 2 rebase targets.
        assert_eq!(targets.len(), 2);
        // Entry 0 slot vaddr = entry_vaddr + 8 (ptr offset), target =
        // entry 0's inner_vaddr.
        let entry0_slot_off = (layout.entries[0].entry_vaddr - 0x1_0000_0000) + 8;
        let entry0_target_off = layout.entries[0].inner_vaddr.unwrap() - 0x1_0000_0000;
        assert_eq!(targets[0], (entry0_slot_off, entry0_target_off));
        // Entry 2 (skipping empty entry 1).
        let entry2_slot_off = (layout.entries[2].entry_vaddr - 0x1_0000_0000) + 8;
        let entry2_target_off = layout.entries[2].inner_vaddr.unwrap() - 0x1_0000_0000;
        assert_eq!(targets[1], (entry2_slot_off, entry2_target_off));
    }
}
