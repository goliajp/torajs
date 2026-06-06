//! SD-2a dyld-substrate emit helpers — section builders + payload
//! writers extracted out of `archive_emit::emit_binary` so that
//! file stays under the 500-prod-LOC hard limit (and so the dyld-
//! specific Mach-O encoding lives next to `stubs.rs`).
//!
//! The two new on-disk artifacts introduced by SD-2a are:
//!
//! - `__TEXT,__stubs` — per-import 12-byte ARM64 trampoline.
//!   Section type `S_SYMBOL_STUBS`, `reserved2 = STUB_SIZE` per
//!   the ld64 / dyld contract.
//! - `__DATA,__la_symbol_ptr` — per-import 8-byte lazy pointer
//!   slot. Section type `S_LAZY_SYMBOL_POINTERS`. Zero-init at
//!   link time; dyld writes the resolved fn address at bind time
//!   (SD-3 wires the `LC_DYLD_CHAINED_FIXUPS` encoding that drives
//!   that bind).
//!
//! All four helpers below are `pub(crate)`. They expect a layout
//! whose `dyld_imports` is non-empty; callers must short-circuit
//! the empty case upstream so the SD-1-equivalent byte stream
//! stays byte-identical for the syscall path.

use torajs_obj::{
    S_ATTR_SOME_INSTRUCTIONS, S_LAZY_SYMBOL_POINTERS, S_SYMBOL_STUBS, Section64, SegmentCommand64,
    VM_PROT_READ, VM_PROT_WRITE,
};

use crate::archive_link::ArchiveLayout;
use crate::stubs::{STUB_SIZE, build_stubs};

/// Build the `__TEXT,__stubs` `Section64` entry. Pairs with the
/// existing `__text` section in the same segment (`__TEXT`) and
/// uses the per-stub byte size as `reserved2` so `otool -hv` and
/// dyld can walk the section.
pub(crate) fn build_stubs_section(layout: &ArchiveLayout) -> Section64 {
    Section64 {
        sectname: "__stubs".into(),
        segname: "__TEXT".into(),
        addr: layout.stubs_section_vaddr,
        size: layout.stubs_section_size,
        offset: layout.stubs_file_offset,
        // 4-byte alignment — instructions are 4 bytes wide and
        // the stub trampoline is just three of them.
        align_log2: 2,
        reloff: 0,
        nreloc: 0,
        flags: S_ATTR_SOME_INSTRUCTIONS | S_SYMBOL_STUBS,
        reserved1: 0,
        // Per-stub byte size — both dyld and otool read this to
        // walk the section.
        reserved2: STUB_SIZE as u32,
        reserved3: 0,
    }
}

/// Build the `__DATA` segment with `__la_symbol_ptr` (SD-2) plus
/// any caller-supplied non-text sections (SD-4c-prereq-c-fix-c4 —
/// member `__DATA,*` regulars + zerofill). `extra_sections` lands
/// **after** `__la_symbol_ptr` so SD-3's chained-fixups encoder's
/// `segment_offset = 0` (= `__la_symbol_ptr`) assumption holds.
///
/// `vmsize` covers file storage + zerofill; `filesize` covers only
/// the page-aligned file region the writer emits — the loader
/// supplies zero bytes for the trailing vmsize past filesize.
pub(crate) fn build_data_segment(
    layout: &ArchiveLayout,
    extra_sections: Vec<Section64>,
) -> SegmentCommand64 {
    let la_ptr_section = Section64 {
        sectname: "__la_symbol_ptr".into(),
        segname: "__DATA".into(),
        addr: layout.la_ptr_section_vaddr,
        size: layout.la_ptr_section_size,
        offset: layout.la_ptr_file_offset,
        // 8-byte alignment for 64-bit pointer slots.
        align_log2: 3,
        reloff: 0,
        nreloc: 0,
        flags: S_LAZY_SYMBOL_POINTERS,
        // First indirect-symbol index — `0` for now; SD-3 wires
        // it once LC_DYSYMTAB carries the indirect symtab.
        reserved1: 0,
        reserved2: 0,
        reserved3: 0,
    };
    let mut sections = Vec::with_capacity(1 + extra_sections.len());
    sections.push(la_ptr_section);
    sections.extend(extra_sections);
    SegmentCommand64 {
        segname: "__DATA".into(),
        vmaddr: layout.data_vmaddr,
        vmsize: layout.data_vmsize,
        fileoff: u64::from(layout.la_ptr_file_offset),
        filesize: layout.data_filesize,
        maxprot: VM_PROT_READ | VM_PROT_WRITE,
        initprot: VM_PROT_READ | VM_PROT_WRITE,
        flags: 0,
        sections,
    }
}

/// Write the `__stubs` payload right after the user `__text`
/// (i.e. at `layout.stubs_file_offset`) followed by a page-aligned
/// jump to `la_ptr_file_offset` and one chain-link 8-byte word
/// per import for the slot table. Callers invoke this only when
/// `dyld_imports` is non-empty; the empty case keeps the payload
/// byte-stream byte-identical to SD-1.
///
/// SD-3 — `__la_symbol_ptr` no longer holds zero bytes. Each slot
/// now carries the `dyld_chained_ptr_64_bind` encoding (ordinal
/// + next + bind bit) from `layout.la_ptr_slot_values`, so dyld
/// can walk the chain at load time and overwrite the encoded
/// link with the real libSystem fn address.
pub(crate) fn write_stubs_and_la_ptr(buf: &mut Vec<u8>, layout: &ArchiveLayout) {
    debug_assert_eq!(
        buf.len() as u32,
        layout.stubs_file_offset,
        "stubs payload must land at layout.stubs_file_offset",
    );
    let stubs = build_stubs(
        &layout.dyld_imports,
        layout.stubs_section_vaddr,
        layout.la_ptr_section_vaddr,
    );
    debug_assert_eq!(
        stubs.len() as u64 * STUB_SIZE,
        layout.stubs_section_size,
        "stubs payload size must match layout.stubs_section_size",
    );
    for stub in &stubs {
        buf.extend_from_slice(&stub.bytes);
    }

    let target = layout.la_ptr_file_offset as usize;
    if buf.len() < target {
        buf.resize(target, 0);
    }
    debug_assert_eq!(buf.len(), target, "la_ptr payload must land at offset");
    debug_assert_eq!(
        layout.la_ptr_slot_values.len() * 8,
        layout.la_ptr_section_size as usize,
        "la_ptr_slot_values must cover the whole section",
    );
    for slot in &layout.la_ptr_slot_values {
        buf.extend_from_slice(&slot.to_le_bytes());
    }
}
