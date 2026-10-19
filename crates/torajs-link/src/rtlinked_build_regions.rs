//! Region assembly for [`crate::rtlinked_build`]: relocated text, the
//! data template with its rebase list, TLV descriptors and the import
//! slots, all in offsets from the image origin.

use torajs_obj::ARM64_RELOC_UNSIGNED;

use crate::archive_emit::member_data_layouts;
use crate::archive_link::{ArchiveLayout, ArchiveLayoutError};
use crate::archives_merge::MergedArchives;
use crate::dyld_syms::DyldLib;
use crate::lc::TEXT_VMADDR_BASE;
use crate::member_apply::{MemberRelocApplyError, apply_member_relocs_with_got};
use crate::member_data_apply::{collect_member_data_payloads, parse_member_section_relocs};
use crate::member_data_rebase_layout::compute_member_data_rebase_targets;
use crate::member_obj::MemberObj;
use crate::resolve::SymTable;
use crate::rtlinked_build::RtLinkedBuildError;
use crate::rtlinked_build_encode::ImageParts;
use crate::rtlinked_format::{ImportKind, SEGMENT_ALIGN};
use crate::stubs::{LA_PTR_SLOT_SIZE, build_stubs};

/// The image origin: the page holding the first runtime member's text.
/// What the stock layout put below it (Mach-O header, the root fn) is
/// not part of the image.
pub(crate) fn origin(layout: &ArchiveLayout) -> u64 {
    let first = layout
        .member_layouts
        .first()
        .map_or(layout.text_vmaddr, |m| m.vaddr);
    first & !u64::from(SEGMENT_ALIGN - 1)
}

/// `vaddr` as an origin offset.
pub(crate) fn off(vaddr: u64, origin: u64) -> Result<u32, RtLinkedBuildError> {
    vaddr
        .checked_sub(origin)
        .and_then(|d| u32::try_from(d).ok())
        .ok_or_else(|| {
            RtLinkedBuildError::Unsupported(format!(
                "vaddr {vaddr:#x} outside the image at {origin:#x}"
            ))
        })
}

fn reloc_err(key: (usize, usize), err: MemberRelocApplyError) -> RtLinkedBuildError {
    RtLinkedBuildError::Layout(ArchiveLayoutError::MemberReloc {
        archive_idx: key.0,
        member_idx: key.1,
        err,
    })
}

/// A member's code must hold no absolute pointer: the image is mapped
/// anywhere and nothing rebases it.
fn no_text_pointers(obj: &MemberObj<'_>, key: (usize, usize)) -> Result<(), RtLinkedBuildError> {
    let relocs = obj.text_relocs().map_err(|e| reloc_err(key, e.into()))?;
    match relocs.iter().find(|r| r.r_type == ARM64_RELOC_UNSIGNED) {
        Some(r) => Err(RtLinkedBuildError::Unsupported(format!(
            "absolute pointer in {} text at {:#x}",
            obj.member.name, r.r_address
        ))),
        None => Ok(()),
    }
}

fn put(buf: &mut [u8], at: u32, bytes: &[u8]) {
    buf[at as usize..at as usize + bytes.len()].copy_from_slice(bytes);
}

/// Member code relocated in place (user tables through their GOT
/// slots), `__TEXT` constant sections, then the import stubs.
pub(crate) fn text(
    layout: &ArchiveLayout,
    merged: &MergedArchives<'_>,
    sym_table: &SymTable,
    got_slots: &SymTable,
    origin: u64,
    parts: &mut ImageParts,
) -> Result<(), RtLinkedBuildError> {
    let stubs_end = layout.stubs_section_vaddr + layout.stubs_section_size;
    let mut buf = vec![0u8; off(stubs_end, origin)? as usize];
    let (mut const_lo, mut const_hi) = (u64::MAX, 0u64);
    for (mi, m) in layout.member_layouts.iter().enumerate() {
        let obj = merged.obj(m.key.0, m.key.1);
        let obj = obj.map_err(|e| reloc_err(m.key, e.into()))?;
        let member = &obj.member;
        no_text_pointers(&obj, m.key)?;
        let start = m.member_text_offset_in_member as usize;
        let mut bytes = member.data[start..start + m.text_size as usize].to_vec();
        apply_member_relocs_with_got(
            &mut bytes,
            &obj,
            m.vaddr,
            sym_table,
            got_slots,
            &m.non_text_sections,
            member_data_layouts(layout, mi),
        )
        .map_err(|e| reloc_err(m.key, e))?;
        put(&mut buf, off(m.vaddr, origin)?, &bytes);
        for s in &m.non_text_sections {
            let rs = parse_member_section_relocs(member, s.section_index)
                .map_err(|e| reloc_err(m.key, e.into()))?;
            // `__eh_frame` is carried unrelocated, as the stock link does:
            // nothing registers unwind tables (panic=abort substrate), so
            // no reader follows its pointers
            if !rs.is_empty() && !s.sectname.starts_with(b"__eh_frame\0") {
                let types: Vec<u8> = rs.iter().map(|r| r.r_type).collect();
                return Err(RtLinkedBuildError::Unsupported(format!(
                    "relocs {types:?} in {} section {} ({})",
                    member.name,
                    s.section_index,
                    String::from_utf8_lossy(&s.sectname).trim_end_matches('\0'),
                )));
            }
            let src = s.member_internal_offset as usize;
            put(
                &mut buf,
                off(s.final_vaddr, origin)?,
                &member.data[src..][..s.size as usize],
            );
            const_lo = const_lo.min(s.final_vaddr);
            const_hi = const_hi.max(s.final_vaddr + u64::from(s.size));
        }
    }
    let stubs = build_stubs(
        &layout.dyld_imports,
        layout.stubs_section_vaddr,
        layout.la_ptr_section_vaddr,
    );
    let stub_bytes: Vec<u8> = stubs.iter().flat_map(|s| s.bytes).collect();
    put(
        &mut buf,
        off(layout.stubs_section_vaddr, origin)?,
        &stub_bytes,
    );
    if const_lo < const_hi {
        parts.text_const = (off(const_lo, origin)?, (const_hi - const_lo) as u32);
    }
    parts.text = buf;
    parts.text_vm_off = 0;
    Ok(())
}

/// The `__DATA` template with every member pointer written as an
/// absolute address at `preferred_base`, and the list of those slots.
pub(crate) fn data(
    layout: &ArchiveLayout,
    merged: &MergedArchives<'_>,
    sym_table: &SymTable,
    origin: u64,
    parts: &mut ImageParts,
) -> Result<(), RtLinkedBuildError> {
    let targets = compute_member_data_rebase_targets(layout, merged, sym_table, layout.data_vmaddr)
        .map_err(|e| reloc_err((0, 0), e))?;
    let base = parts.preferred_base;
    let values: Vec<u64> = targets
        .iter()
        .map(|&(_, t)| Ok(base + u64::from(off(TEXT_VMADDR_BASE + t, origin)?)))
        .collect::<Result<_, RtLinkedBuildError>>()?;
    let payloads = collect_member_data_payloads(layout, merged, &values)?;

    let sections = layout
        .data_non_text_layouts
        .iter()
        .flatten()
        .filter(|s| s.has_file_storage);
    let la_end = layout.la_ptr_section_vaddr + layout.la_ptr_section_size;
    let file_end = sections
        .clone()
        .map(|s| s.final_vaddr + u64::from(s.size))
        .fold(la_end, u64::max);
    let mut buf = vec![0u8; (file_end - layout.data_vmaddr) as usize];
    for (s, bytes) in sections.zip(&payloads) {
        put(&mut buf, (s.final_vaddr - layout.data_vmaddr) as u32, bytes);
    }
    let data_off = off(layout.data_vmaddr, origin)?;
    let mut rebases: Vec<u32> = targets
        .iter()
        .map(|&(slot, _)| data_off + slot as u32)
        .collect();
    rebases.sort_unstable();
    parts.rebases = rebases;
    parts.data = buf;
    parts.data_vm_off = data_off;
    parts.data_vm_size = layout.data_vmsize as u32;
    Ok(())
}

/// One import slot per dyld import (its `__la_symbol_ptr` slot, which
/// the stubs load) and per user table (its GOT slot).
pub(crate) fn imports(
    layout: &ArchiveLayout,
    got_slots: &SymTable,
    origin: u64,
    parts: &mut ImageParts,
) -> Result<(), RtLinkedBuildError> {
    for (i, (name, ordinal)) in layout.dyld_imports.iter().enumerate() {
        let kind = match *ordinal {
            o if o == DyldLib::LibSystem.ordinal() => ImportKind::LibSystem,
            o if o == DyldLib::LibCurl.ordinal() => ImportKind::LazyDylib,
            o => {
                return Err(RtLinkedBuildError::Unsupported(format!(
                    "import {name} has dylib ordinal {o}"
                )));
            }
        };
        let slot = layout.la_ptr_section_vaddr + i as u64 * LA_PTR_SLOT_SIZE;
        parts.imports.push((kind, name.clone(), off(slot, origin)?));
    }
    for (name, &slot) in got_slots {
        parts
            .imports
            .push((ImportKind::UserTable, name.clone(), off(slot, origin)?));
    }
    Ok(())
}
