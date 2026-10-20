//! Thread-local variables in the prelinked image. Each TLV descriptor
//! (`thunk`, `key`, `offset`) keeps `thunk` and `key` zero for the
//! loader to fill and gets `offset` = its initial value's address minus
//! the start of the per-thread block, which spans every
//! `__thread_data` / `__thread_bss` byte. The loader allocates that
//! block per thread and copies the `__thread_data` part into it.

use std::collections::BTreeMap;

use torajs_obj::ARM64_RELOC_UNSIGNED;

use crate::archive_emit::member_data_layouts;
use crate::archive_link::ArchiveLayout;
use crate::archives_merge::MergedArchives;
use crate::member_resolve::resolve_local_nsect;
use crate::resolve::SymTable;
use crate::rtlinked_build::RtLinkedBuildError;
use crate::rtlinked_build_encode::ImageParts;
use crate::rtlinked_build_regions::off;
use crate::tlv_descriptor_layout::TlvDescriptorLayout;

fn unsupported(msg: String) -> RtLinkedBuildError {
    RtLinkedBuildError::Unsupported(msg)
}

/// `(start, end)` of the merged sections named `names`.
fn span(layout: &ArchiveLayout, names: &[&[u8]]) -> Option<(u64, u64)> {
    layout
        .data_merged_sections
        .iter()
        .filter(|s| {
            names
                .iter()
                .any(|n| s.sectname.starts_with(n) && s.sectname[n.len()] == 0)
        })
        .map(|s| (s.vaddr, s.vaddr + s.size))
        .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
}

/// Fill the TLV descriptors in `parts.data` (built by
/// [`crate::rtlinked_build_regions::data`]) and record them.
pub(crate) fn tlv(
    layout: &ArchiveLayout,
    merged: &MergedArchives<'_>,
    sym_table: &SymTable,
    origin: u64,
    parts: &mut ImageParts,
) -> Result<(), RtLinkedBuildError> {
    if layout.tlv_descriptors.is_empty() {
        return Ok(());
    }
    let (start, end) = span(layout, &[b"__thread_data", b"__thread_bss"])
        .ok_or_else(|| unsupported("TLV descriptors without thread-local sections".into()))?;
    let init = span(layout, &[b"__thread_data"]).unwrap_or((start, start));
    let member_index: BTreeMap<(usize, usize), usize> = layout
        .member_layouts
        .iter()
        .enumerate()
        .map(|(i, m)| (m.key, i))
        .collect();
    for d in &layout.tlv_descriptors {
        let init_vaddr = descriptor_init(layout, merged, sym_table, &member_index, d)?;
        let at = (d.descriptor_vaddr - layout.data_vmaddr) as usize;
        let slot = &mut parts.data[at..at + 24];
        slot[..16].fill(0);
        slot[16..].copy_from_slice(&(init_vaddr - start).to_le_bytes());
        parts.tlv.push(off(d.descriptor_vaddr, origin)?);
    }
    parts.tlv_span = (off(start, origin)?, (end - start) as u32);
    parts.tlv_init = (off(init.0, origin)?, (init.1 - init.0) as u32);
    Ok(())
}

/// Address of the initial value the descriptor's `offset` reloc names,
/// in-place addend included.
fn descriptor_init(
    layout: &ArchiveLayout,
    merged: &MergedArchives<'_>,
    sym_table: &SymTable,
    member_index: &BTreeMap<(usize, usize), usize>,
    d: &TlvDescriptorLayout,
) -> Result<u64, RtLinkedBuildError> {
    let mi = member_index[&d.member_key];
    let m = &layout.member_layouts[mi];
    let member = &merged.member(m.key.0, m.key.1);
    let data_sections = member_data_layouts(layout, mi);
    let sec = data_sections
        .iter()
        .find(|s| s.section_index == d.section_index)
        .ok_or_else(|| unsupported(format!("TLV section missing in {}", member.name)))?;
    let r_address = (d.offset_slot_vaddr - sec.final_vaddr) as u32;
    let obj = merged
        .obj(m.key.0, m.key.1)
        .map_err(|e| unsupported(format!("{}: {e:?}", member.name)))?;
    let relocs = obj
        .section_relocs(sec.section_index)
        .map_err(|e| unsupported(format!("{} TLV relocs: {e:?}", member.name)))?;
    let r = relocs
        .iter()
        .find(|r| r.r_address == r_address && r.r_type == ARM64_RELOC_UNSIGNED && r.r_extern == 1)
        .ok_or_else(|| unsupported(format!("TLV offset without reloc in {}", member.name)))?;
    let symtab = obj
        .symtab()
        .map_err(|e| unsupported(format!("{} symtab: {e:?}", member.name)))?;
    let name = obj
        .sym_name(&symtab, r.r_symbolnum)
        .map_err(|e| unsupported(format!("{} TLV name: {e:?}", member.name)))?;
    let target = sym_table
        .get(name)
        .copied()
        .or_else(|| {
            resolve_local_nsect(
                obj.sym(&symtab, r.r_symbolnum),
                m.vaddr,
                &m.non_text_sections,
                data_sections,
            )
        })
        .ok_or_else(|| unsupported(format!("TLV initial value {name} unresolved")))?;
    let src = r_address as usize;
    let payload = obj.payload(sec.section_index);
    let addend = u64::from_le_bytes(payload[src..src + 8].try_into().unwrap());
    Ok(target.wrapping_add(addend))
}
