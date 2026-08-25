//! SD-4c-prereq+e8 — thin wrapper around `build_chained_fixups` so
//! `compute_archive_layout` can call it without exceeding the 500-
//! prod-LOC file-size hard limit. Carries the data_const seg /
//! rebase-target plumbing the e8 vtable migration introduced.

use std::collections::BTreeMap;

use crate::chained_fixups::{TextRebaseScope, build_chained_fixups};
use crate::chained_fixups_starts::RebaseTarget;
use crate::class_name_table_layout::class_name_table_rebase_targets_from_layouts;
use crate::data_const_layout::DataConstLayout;
use crate::fn_name_table_layout::fn_name_table_rebase_targets_from_layouts;
use crate::layout_types::ArchiveLayout;
use crate::lc::TEXT_VMADDR_BASE;
use crate::user_class_layouts_layout::compute_class_layouts_rebase_targets;
use crate::user_regex_baked_layout::compute_user_regex_baked_rebase_targets;
use crate::user_vtables_layout::vtable_rebase_targets_from_fn_vaddrs;

/// Args bundle for [`compute_chained_fixups_outputs`] — keeps the
/// archive_link call site to a handful of lines while still
/// passing every layout-derived input through explicitly.
pub struct ChainedFixupsInputs<'a> {
    pub dyld_imports: &'a BTreeMap<String, u8>,
    pub data_seg_vmaddr_offset: u64,
    /// `__DATA` FILESIZE (file-backed only — chunk 633: page_count
    /// must not cover zerofill pages; see chained_fixups_starts.rs).
    pub data_seg_filesize: u64,
    pub tlv_thunk_offsets: &'a [u64],
    pub segment_count: u32,
    pub data_seg_idx: u32,
    pub data_const_layout: &'a DataConstLayout,
    pub vtable_rebase_targets: &'a [RebaseTarget],
    /// SD-4c swap-2k chunk 2b — per staticlib-member `__DATA,*`
    /// rebase target list (offset_in_data_segment, target_off_in_image).
    /// Empty slice (default) keeps the binary byte-identical to the
    /// pre-2b path; 2b-4 wires the actual targets from
    /// [`crate::member_data_rebase_layout::compute_member_data_rebase_targets`]
    /// so dyld rebases each member-data vtable slot at image load.
    pub data_seg_rebase_targets: &'a [RebaseTarget],
}

/// Returns `(blob, la_ptr_slot_values, tlv_thunk_link_values,
/// text_rebase_link_values, data_rebase_link_values)`. Short-circuits
/// to five empty vecs when no la-ptr / TLV / vtable rebase / data
/// rebase participants exist.
pub fn compute_chained_fixups_outputs(
    input: ChainedFixupsInputs<'_>,
) -> (Vec<u8>, Vec<u64>, Vec<u64>, Vec<u64>, Vec<u64>) {
    let has_dyld = !input.dyld_imports.is_empty();
    let has_vtable_rebase = !input.vtable_rebase_targets.is_empty();
    let has_data_rebase = !input.data_seg_rebase_targets.is_empty();
    if !has_dyld && !has_vtable_rebase && !has_data_rebase {
        return (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    }
    let text_rebase = if has_vtable_rebase {
        Some(TextRebaseScope {
            // Field names are vestigial — e8+ uses __DATA_CONST.
            text_segment_vmaddr_offset: input.data_const_layout.segment_vmaddr - TEXT_VMADDR_BASE,
            text_segment_vmsize: input.data_const_layout.segment_vmsize,
            text_seg_idx: 2, // __DATA_CONST idx in PAGEZERO+TEXT+DATA_CONST+...
            rebase_targets: input.vtable_rebase_targets,
        })
    } else {
        None
    };
    let built = build_chained_fixups(
        input.dyld_imports,
        input.data_seg_vmaddr_offset,
        0,
        input.data_seg_filesize,
        input.tlv_thunk_offsets,
        input.segment_count,
        input.data_seg_idx,
        text_rebase.as_ref(),
        input.data_seg_rebase_targets,
    );
    (
        built.blob,
        built.la_ptr_slot_values,
        built.tlv_thunk_link_values,
        built.text_rebase_link_values,
        built.data_rebase_link_values,
    )
}

/// swap-2k chunk 2b-4 — re-run [`compute_chained_fixups_outputs`]
/// with the collected staticlib member `__DATA,*` rebase targets,
/// overwrite the chain-related fields on `layout` (blob + per-slot
/// link-value vecs), and return the per-data-slot link values the
/// emit pass writes onto each member `__DATA,*` payload via
/// [`crate::member_data_apply::apply_member_data_const_relocs`].
/// The blob size is invariant in `data_rebase_targets.len()` (page
/// count / import table / sym table all unchanged), so caller-side
/// `chained_fixups_dataoff` / `codesign_dataoff` stay valid.
pub fn recompute_chained_fixups_with_data_rebase(
    layout: &mut ArchiveLayout,
    data_rebase_targets: &[RebaseTarget],
    codesign_ident: &str,
) -> Vec<u64> {
    if data_rebase_targets.is_empty() {
        return Vec::new();
    }
    let has_data_const = layout.data_const_layout.has_data_const;
    let has_dyld = layout.has_dyld;
    // Mirrors `archive_link.rs` segment_count: PAGEZERO + __TEXT +
    // (__DATA_CONST?) + (__DATA?) + __LINKEDIT.
    let segment_count = 3 + u32::from(has_dyld) + u32::from(has_data_const);
    let data_seg_idx: u32 = if has_data_const { 3 } else { 2 };
    let vtable_rebase_targets = vtable_rebase_targets_from_fn_vaddrs(
        &layout.data_const_layout.vtable_layout,
        &layout.fn_vaddrs,
        layout.data_const_layout.segment_vmaddr,
        TEXT_VMADDR_BASE,
    );
    let class_layouts_rebase_targets = compute_class_layouts_rebase_targets(
        &layout.data_const_layout.class_layouts_layout,
        &layout.fn_vaddrs,
        layout.data_const_layout.segment_vmaddr,
        TEXT_VMADDR_BASE,
    );
    // Step 3b.4 + W-J A3c — mirror archive_link.rs walk order:
    // vtable + class_layouts + fn_name_table + class_name_table.
    // The recompute path must produce the same combined list so per-
    // slot link values stay indexed-aligned with text_rebase_link_values
    // after the data-rebase round-trip.
    let fn_name_table_rebase_targets = fn_name_table_rebase_targets_from_layouts(
        &layout.data_const_layout.fn_name_table_layout,
        &layout.fn_vaddrs,
        &layout.user_strings_layout,
        layout.data_const_layout.segment_vmaddr,
        TEXT_VMADDR_BASE,
    );
    let class_name_table_rebase_targets = class_name_table_rebase_targets_from_layouts(
        &layout.data_const_layout.class_name_table_layout,
        &layout.user_strings_layout,
        layout.data_const_layout.segment_vmaddr,
        TEXT_VMADDR_BASE,
    );
    let baked_regex_rebase_targets = compute_user_regex_baked_rebase_targets(
        &layout.data_const_layout.baked_regex_layout,
        layout.data_const_layout.segment_vmaddr,
        TEXT_VMADDR_BASE,
    );
    let mut combined_text_rebase = vtable_rebase_targets;
    combined_text_rebase.extend(class_layouts_rebase_targets);
    combined_text_rebase.extend(fn_name_table_rebase_targets);
    combined_text_rebase.extend(class_name_table_rebase_targets);
    combined_text_rebase.extend(baked_regex_rebase_targets);
    let tlv_thunk_offsets: Vec<u64> = layout
        .tlv_descriptors
        .iter()
        .map(|d| d.thunk_slot_vaddr - layout.data_vmaddr)
        .collect();
    let (blob, la_ptr, tlv_thunk, text_rebase, data_rebase) =
        compute_chained_fixups_outputs(ChainedFixupsInputs {
            dyld_imports: &layout.dyld_imports,
            data_seg_vmaddr_offset: layout.data_vmaddr.saturating_sub(TEXT_VMADDR_BASE),
            data_seg_filesize: layout.data_filesize,
            tlv_thunk_offsets: &tlv_thunk_offsets,
            segment_count,
            data_seg_idx,
            data_const_layout: &layout.data_const_layout,
            vtable_rebase_targets: &combined_text_rebase,
            data_seg_rebase_targets: data_rebase_targets,
        });
    // The recompute can GROW the blob: an import-free closure whose
    // only fixups are member data rebases (the injection-reachability
    // empty shape, RFC 20260825) reaches here with a ZERO-byte first-
    // pass blob — the plan encoded nothing, this pass encodes the
    // header + page buckets. Every downstream size the blob length
    // feeds must follow, or the emitted header disagrees with the
    // payload: the codesign blob lands 80 bytes past the LC's
    // dataoff and the kernel SIGKILLs the unsigned-looking binary.
    // The LC writer runs after this call and reads these fields, so
    // re-deriving here keeps header and payload consistent.
    if blob.len() != layout.chained_fixups_blob.len() {
        layout.chained_fixups_datasize = blob.len() as u32;
        let codesign_dataoff = crate::archive_link::round_up_to(
            u64::from(layout.chained_fixups_dataoff + layout.chained_fixups_datasize),
            8,
        ) as u32;
        layout.codesign_dataoff = codesign_dataoff;
        layout.codesign_datasize =
            crate::sign::adhoc_codesign_blob_size(codesign_dataoff, codesign_ident);
        let linkedit_data_size = (codesign_dataoff + layout.codesign_datasize)
            .saturating_sub(layout.linkedit_file_offset);
        layout.linkedit_vmsize = crate::archive_link::round_up_to(
            u64::from(linkedit_data_size),
            crate::lc::APPLE_SILICON_PAGE_SIZE,
        );
        layout.total_size = layout.linkedit_file_offset + linkedit_data_size;
    }
    layout.chained_fixups_blob = blob;
    layout.la_ptr_slot_values = la_ptr;
    layout.tlv_thunk_link_values = tlv_thunk;
    layout.text_rebase_link_values = text_rebase;
    data_rebase
}
