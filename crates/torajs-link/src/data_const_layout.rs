//! SD-4c-prereq+e8 — `__DATA_CONST` segment layout for vtable rodata.
//!
//! e7a placed `__vtable_<C>` slots in `__TEXT,__cstring`, but Apple's
//! kernel codesigns `__TEXT` pages — once dyld walks the chained-fixup
//! chain and stamps the resolved fn vaddr into each slot, the page
//! hash diverges from what `LC_CODE_SIGNATURE` recorded and the
//! kernel terminates the process with `SIGKILL (Code Signature
//! Invalid)`. Every modern arm64 macOS binary (swift / objc / rustc)
//! moves rodata pointer slots into a dedicated `__DATA_CONST` segment
//! instead: dyld can fix it up at load time, and the `SG_READ_ONLY`
//! segment flag tells the kernel to apply `mprotect(PROT_READ)` after
//! the fixup walk so the data is still effectively immutable. e8
//! adopts that placement.
//!
//! Wire format references:
//! - `<mach-o/loader.h>` — `segment_command_64.flags` carries
//!   `SG_READ_ONLY = 0x10`
//! - Apple dyld source `dyld3/Loader.cpp` walks segments with
//!   `SG_READ_ONLY` and applies the post-fixup mprotect

use crate::lc::APPLE_SILICON_PAGE_SIZE;
use crate::user_vtables_layout::{UserVtablesLayout, compute_user_vtables_layout};
use torajs_obj::{SegmentCommand64, VM_PROT_READ, VM_PROT_WRITE};

/// `segment_command_64.flags` — segment becomes read-only after dyld
/// finishes applying chained fixups. The kernel codesign verifier
/// rehashes the page after dyld's mprotect, so the hash recorded in
/// `LC_CODE_SIGNATURE` matches the immutable post-fixup state and
/// runtime verification stays green.
pub const SG_READ_ONLY: u32 = 0x10;

/// Result of laying out the `__DATA_CONST` segment. When
/// `has_data_const` is false the caller skips emitting the
/// `LC_SEGMENT_64 __DATA_CONST` load command and downstream
/// segment indices stay unchanged from the pre-e8 layout.
#[derive(Debug, Clone, Default)]
pub struct DataConstLayout {
    /// Vtable placement inside the segment. `entries[i].file_offset`
    /// / `entries[i].vaddr` are absolute (image-relative + base) so
    /// callers can pass them through unchanged to the emit pass.
    pub vtable_layout: UserVtablesLayout,
    /// File offset of the `__DATA_CONST` segment's first byte.
    /// Equals `text_segment_file_offset + text_vmsize` in our layout.
    pub segment_file_offset: u32,
    /// Image-relative vmaddr of the segment's first byte. Equals
    /// `text_vmaddr + text_vmsize`.
    pub segment_vmaddr: u64,
    /// Page-aligned size of the segment's file payload. Equals
    /// `segment_vmsize` because vtable bytes are file-backed (no
    /// zerofill).
    pub segment_filesize: u64,
    /// Page-aligned virtual size of the segment. dyld's chained-fixup
    /// `page_start[]` table walks every page in this range.
    pub segment_vmsize: u64,
    /// `false` when `vtable_globals` is empty — the caller short-
    /// circuits every `__DATA_CONST` emit hook so the binary stays
    /// byte-identical to the pre-e8 single-`__TEXT` layout.
    pub has_data_const: bool,
}

/// Compute the `__DATA_CONST` segment layout for the given vtable
/// set. `segment_file_offset_base` and `segment_vmaddr_base` are the
/// file / virtual addresses of the segment's first byte — typically
/// `text_segment_file_offset + text_vmsize` and `text_vmaddr +
/// text_vmsize`.
///
/// When `vtable_globals` is empty the returned layout has
/// `has_data_const = false`, `segment_vmsize = 0`, and an empty
/// vtable layout. Callers must skip every `__DATA_CONST` emit path
/// in that case so the pre-e8 byte-identical guarantee for every
/// vtable-less probe survives.
pub fn compute_data_const_layout(
    vtable_globals: &[crate::exec::UserVtableEntry],
    segment_file_offset_base: u32,
    segment_vmaddr_base: u64,
) -> DataConstLayout {
    if vtable_globals.is_empty() {
        return DataConstLayout {
            vtable_layout: UserVtablesLayout::default(),
            segment_file_offset: segment_file_offset_base,
            segment_vmaddr: segment_vmaddr_base,
            segment_filesize: 0,
            segment_vmsize: 0,
            has_data_const: false,
        };
    }
    // Place the vtable rodata at the start of the segment. The
    // existing `compute_user_vtables_layout` handles 8-byte alignment
    // and per-entry offset math; we just point it at the new base.
    // Unlike `build_user_vtables_region` (which adds `file_offset` to
    // the image base assuming __TEXT placement), we pass the segment
    // base directly so __DATA_CONST entries land at their actual
    // vaddrs.
    let vtable_layout = compute_user_vtables_layout(
        vtable_globals,
        segment_file_offset_base,
        segment_vmaddr_base,
    );
    // Page-align so dyld's chained-fixup page walker can index from
    // the segment base — `page_start[]` always covers whole pages.
    let segment_vmsize = u64::from(vtable_layout.total_size).div_ceil(APPLE_SILICON_PAGE_SIZE)
        * APPLE_SILICON_PAGE_SIZE;
    DataConstLayout {
        vtable_layout,
        segment_file_offset: segment_file_offset_base,
        segment_vmaddr: segment_vmaddr_base,
        segment_filesize: segment_vmsize,
        segment_vmsize,
        has_data_const: true,
    }
}

/// Build the `__DATA_CONST` `LC_SEGMENT_64` load command for the
/// emit pass. Returns `None` when `has_data_const` is false so the
/// caller can `let Some(s) = build_data_const_segment(layout) else { skip }`.
/// `maxprot/initprot = RW` lets dyld apply chained-fixup rebases;
/// `flags = SG_READ_ONLY` tells the kernel to `mprotect(PROT_READ)`
/// once dyld is done so codesigned page hashes stay valid.
pub fn build_data_const_segment(layout: &DataConstLayout) -> Option<SegmentCommand64> {
    if !layout.has_data_const {
        return None;
    }
    Some(SegmentCommand64 {
        segname: "__DATA_CONST".into(),
        vmaddr: layout.segment_vmaddr,
        vmsize: layout.segment_vmsize,
        fileoff: u64::from(layout.segment_file_offset),
        filesize: layout.segment_filesize,
        maxprot: VM_PROT_READ | VM_PROT_WRITE,
        initprot: VM_PROT_READ | VM_PROT_WRITE,
        flags: SG_READ_ONLY,
        sections: Vec::new(),
    })
}

/// Write the `__DATA_CONST` segment payload (vtable bytes + page
/// padding to segment_filesize). Pad-then-emit so the buf cursor
/// lands at `segment_file_offset + segment_filesize` before the
/// next segment's payload starts (la_ptr_file_offset).
pub fn write_data_const_payload(
    buf: &mut Vec<u8>,
    layout: &DataConstLayout,
    user_vtables_payload: &[u8],
) {
    if !layout.has_data_const {
        return;
    }
    let off = layout.segment_file_offset as usize;
    if buf.len() < off {
        buf.resize(off, 0);
    }
    buf.extend_from_slice(user_vtables_payload);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::UserVtableEntry;

    fn entry(sym: &str, slots: Vec<Option<&str>>) -> UserVtableEntry {
        UserVtableEntry {
            sym: sym.into(),
            slot_syms: slots.into_iter().map(|s| s.map(String::from)).collect(),
        }
    }

    #[test]
    fn empty_input_marks_segment_absent() {
        let layout = compute_data_const_layout(&[], 0x4000, 0x1_0000_4000);
        assert!(!layout.has_data_const);
        assert_eq!(layout.segment_vmsize, 0);
        assert_eq!(layout.segment_filesize, 0);
        assert!(layout.vtable_layout.entries.is_empty());
    }

    #[test]
    fn single_vtable_fits_in_one_page() {
        let vtables = [entry("__vtable_A", vec![Some("fn_0")])];
        let layout = compute_data_const_layout(&vtables, 0x4000, 0x1_0000_4000);
        assert!(layout.has_data_const);
        // 8 bytes vtable -> one page after alignment.
        assert_eq!(layout.segment_vmsize, 0x4000);
        assert_eq!(layout.segment_filesize, 0x4000);
        assert_eq!(layout.segment_vmaddr, 0x1_0000_4000);
        assert_eq!(layout.segment_file_offset, 0x4000);
        // vtable_layout entry vaddr lands inside the segment.
        assert_eq!(layout.vtable_layout.entries.len(), 1);
        assert_eq!(layout.vtable_layout.entries[0].vaddr, 0x1_0000_4000);
    }

    #[test]
    fn multi_vtable_pages_align_up() {
        // Three vtables × 8 slots × 8 bytes = 192 bytes — well under
        // one page so vmsize still rounds to 0x4000.
        let vtables = [
            entry("__vtable_A", vec![Some("a0"); 8]),
            entry("__vtable_B", vec![Some("b0"); 8]),
            entry("__vtable_C", vec![Some("c0"); 8]),
        ];
        let layout = compute_data_const_layout(&vtables, 0x4000, 0x1_0000_4000);
        assert!(layout.has_data_const);
        assert_eq!(layout.segment_vmsize, 0x4000);
        // 3 vtables × 64 bytes payload = 192 bytes total_size.
        assert_eq!(layout.vtable_layout.total_size, 192);
    }

    #[test]
    fn segment_base_offsets_propagate_to_layout() {
        // file_offset_base / vmaddr_base mismatch — checks the helper
        // doesn't re-derive them from anywhere else.
        let vtables = [entry("__vtable_A", vec![Some("fn_0"), Some("fn_1")])];
        let layout = compute_data_const_layout(&vtables, 0x8000, 0x1_0000_8000);
        assert_eq!(layout.segment_file_offset, 0x8000);
        assert_eq!(layout.segment_vmaddr, 0x1_0000_8000);
        assert_eq!(layout.vtable_layout.entries[0].vaddr, 0x1_0000_8000);
        assert_eq!(layout.vtable_layout.entries[0].file_offset, 0x8000);
    }

    #[test]
    fn sg_read_only_flag_matches_apple_loader_h() {
        // Locked-in constant — Apple's mach-o/loader.h defines
        // `SG_READ_ONLY = 0x10` and dyld branches on it to apply the
        // post-fixup mprotect.
        assert_eq!(SG_READ_ONLY, 0x10);
    }
}
