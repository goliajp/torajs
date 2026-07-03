//! TLV symbol vaddr overrides — split out of
//! [`crate::tlv_descriptor_layout`] (chunk 474; that file was at the
//! ≤ 500 LOC line and `compute_tlv_sym_overrides` at 219 over the
//! ≤ 200 fn hard limit). The sibling keeps the descriptor layout
//! enumeration; this file owns the `LC_SYMTAB` walk that maps each
//! thread-local defined extern to its final descriptor vaddr.
//!
//! **Why this exists** — `parse_member_defined_externs` returns
//! `(name, n_value)` per defined extern, and `archive_emit` plugs it
//! into the sym table as `m.vaddr + n_value`. That math is right for
//! `__text` symbols (`m.vaddr` is the member's `__text` start), but
//! **wrong for thread-local symbols** because their `n_value` is a
//! section-relative offset inside `__DATA,__thread_vars`, not `__text`.
//! Reading the resulting "vaddr" as a pointer at runtime would land
//! somewhere inside the binary's `__TEXT` region — almost certainly
//! not a valid TLV descriptor — and dyld's `_tlv_bootstrap` thunk
//! would never get reached.

use std::collections::{BTreeMap, BTreeSet};

use torajs_obj::{LC_SYMTAB, MH_MAGIC_64, N_EXT, N_SECT, N_TYPE};

use crate::archive::MemberSymtabError;
use crate::archives_merge::MergedArchives;
use crate::data_section_layout::DataSectionLayout;
use crate::layout_types::ArchiveLayout;
use crate::member_sections::is_tlv_descriptor;
use crate::resolve::SymTable;
use crate::tlv_descriptor_layout::{TLV_DESCRIPTOR_SIZE, TlvDescriptorLayout};

/// Parse one member's Mach-O header + load commands and return its
/// `LC_SYMTAB` as `(symoff, nsyms, strtab)` with bounds already
/// validated. `Ok(None)` = member isn't a 64-bit Mach-O or carries
/// no `LC_SYMTAB` — caller skips it (same as the pre-split
/// `continue`s).
fn parse_member_symtab<'a>(
    bytes: &'a [u8],
    key: &(usize, usize),
) -> Result<Option<(usize, usize, &'a [u8])>, TlvSymOverrideError> {
    if bytes.len() < 32 {
        return Err(TlvSymOverrideError::Symtab {
            archive_idx: key.0,
            member_idx: key.1,
            err: MemberSymtabError::TruncatedHeader,
        });
    }
    let magic = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    if magic != MH_MAGIC_64 {
        return Ok(None);
    }
    let ncmds = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
    let mut cursor = 32usize;
    let mut symtab: Option<(u32, u32, u32, u32)> = None;
    for _ in 0..ncmds {
        if bytes.len() < cursor + 8 {
            return Err(TlvSymOverrideError::Symtab {
                archive_idx: key.0,
                member_idx: key.1,
                err: MemberSymtabError::TruncatedLoadCommand { offset: cursor },
            });
        }
        let cmd = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap());
        let cmdsize =
            u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
        if cmdsize < 8 || bytes.len() < cursor + cmdsize {
            return Err(TlvSymOverrideError::Symtab {
                archive_idx: key.0,
                member_idx: key.1,
                err: MemberSymtabError::TruncatedLoadCommand { offset: cursor },
            });
        }
        if cmd == LC_SYMTAB {
            if cmdsize < 24 {
                return Err(TlvSymOverrideError::Symtab {
                    archive_idx: key.0,
                    member_idx: key.1,
                    err: MemberSymtabError::TruncatedSymtabCmd { offset: cursor },
                });
            }
            let symoff = u32::from_le_bytes(bytes[cursor + 8..cursor + 12].try_into().unwrap());
            let nsyms = u32::from_le_bytes(bytes[cursor + 12..cursor + 16].try_into().unwrap());
            let stroff = u32::from_le_bytes(bytes[cursor + 16..cursor + 20].try_into().unwrap());
            let strsize = u32::from_le_bytes(bytes[cursor + 20..cursor + 24].try_into().unwrap());
            symtab = Some((symoff, nsyms, stroff, strsize));
        }
        cursor += cmdsize;
    }

    let (symoff, nsyms, stroff, strsize) = match symtab {
        Some(v) => v,
        None => return Ok(None),
    };
    let symoff = symoff as usize;
    let stroff = stroff as usize;
    let strsize = strsize as usize;
    let nsyms = nsyms as usize;
    if bytes.len() < symoff + nsyms * 16 || bytes.len() < stroff + strsize {
        return Err(TlvSymOverrideError::Symtab {
            archive_idx: key.0,
            member_idx: key.1,
            err: MemberSymtabError::TruncatedSymtab {
                symoff,
                nsyms,
                file_size: bytes.len(),
            },
        });
    }
    Ok(Some((symoff, nsyms, &bytes[stroff..stroff + strsize])))
}

/// Map one thread-local sym's `(n_sect, n_value)` to its final-binary
/// descriptor vaddr: subtract the section's source `member_addr`
/// (rustc emits cumulative non-zero `section.addr` for `__DATA,*`
/// sections in `.rcgu.o` members), require 24-byte alignment, and
/// index the `(member_key, section_index)` descriptor bucket.
fn descriptor_vaddr_for_sym(
    key: &(usize, usize),
    sym_index: usize,
    n_sect: u8,
    n_value: u64,
    section_member_addrs: &BTreeMap<((usize, usize), u8), u64>,
    by_member_section: &BTreeMap<((usize, usize), u8), Vec<&TlvDescriptorLayout>>,
) -> Result<u64, TlvSymOverrideError> {
    let section_member_addr = section_member_addrs.get(&(*key, n_sect)).copied().ok_or(
        TlvSymOverrideError::MissingDescriptorBucket {
            archive_idx: key.0,
            member_idx: key.1,
            section_index: n_sect,
        },
    )?;
    if n_value < section_member_addr {
        return Err(TlvSymOverrideError::MisalignedTlvSymValue {
            archive_idx: key.0,
            member_idx: key.1,
            sym_index,
            n_value,
        });
    }
    let offset_in_section = n_value - section_member_addr;
    if offset_in_section % u64::from(TLV_DESCRIPTOR_SIZE) != 0 {
        return Err(TlvSymOverrideError::MisalignedTlvSymValue {
            archive_idx: key.0,
            member_idx: key.1,
            sym_index,
            n_value,
        });
    }
    let descriptor_index = (offset_in_section / u64::from(TLV_DESCRIPTOR_SIZE)) as usize;
    let bucket = by_member_section.get(&(*key, n_sect)).ok_or(
        TlvSymOverrideError::MissingDescriptorBucket {
            archive_idx: key.0,
            member_idx: key.1,
            section_index: n_sect,
        },
    )?;
    let descriptor =
        bucket
            .get(descriptor_index)
            .ok_or(TlvSymOverrideError::DescriptorIndexOutOfRange {
                archive_idx: key.0,
                member_idx: key.1,
                section_index: n_sect,
                descriptor_index,
                descriptor_count: bucket.len(),
            })?;
    Ok(descriptor.descriptor_vaddr)
}

/// Read a NUL-terminated UTF-8 sym name out of the member's string
/// table at `n_strx`.
fn read_sym_name<'a>(
    strtab: &'a [u8],
    n_strx: usize,
    key: &(usize, usize),
    sym_index: usize,
) -> Result<&'a str, TlvSymOverrideError> {
    if n_strx >= strtab.len() {
        return Err(TlvSymOverrideError::Symtab {
            archive_idx: key.0,
            member_idx: key.1,
            err: MemberSymtabError::BadStrx { sym_index, n_strx },
        });
    }
    let end_off = strtab[n_strx..]
        .iter()
        .position(|&b| b == 0)
        .map(|p| n_strx + p)
        .ok_or(TlvSymOverrideError::Symtab {
            archive_idx: key.0,
            member_idx: key.1,
            err: MemberSymtabError::UnterminatedSymName { sym_index, n_strx },
        })?;
    let name_bytes = &strtab[n_strx..end_off];
    std::str::from_utf8(name_bytes).map_err(|_| TlvSymOverrideError::Symtab {
        archive_idx: key.0,
        member_idx: key.1,
        err: MemberSymtabError::NameNotUtf8 { sym_index },
    })
}

/// Walk every integrated member's `LC_SYMTAB` for thread-local
/// defined externals (`N_SECT|N_EXT` whose `n_sect` indexes a TLV
/// descriptor section), and produce `sym_name → descriptor_vaddr`
/// overrides for the link driver's `effective_sym_table`.
///
/// This helper recomputes the correct vaddr — the final binary's
/// TLV descriptor address — by matching each thread-local sym's
/// `(n_sect, n_value)` against the [`TlvDescriptorLayout`] vec
/// produced by
/// [`crate::tlv_descriptor_layout::compute_tlv_descriptor_layouts`].
/// The link driver then overlays the returned map onto
/// `effective_sym_table` after it inserts the raw `m.vaddr + n_value`
/// entries, replacing each TLV sym's wrong vaddr with
/// `descriptor_vaddr`.
///
/// **Assumption — `.o` section addr == 0** — for relocatable Mach-O
/// `.o` files (which is what archives bundle), `section_64.addr` is
/// conventionally `0` across the board (rustc + clang both emit `0`).
/// `nlist.n_value` for a `N_SECT` sym is then exactly its offset
/// inside the section, and `n_value / TLV_DESCRIPTOR_SIZE` gives
/// the descriptor index. The helper validates this via the
/// `MisalignedTlvSymValue` error when the value isn't a multiple
/// of 24.
pub fn compute_tlv_sym_overrides(
    merged: &MergedArchives,
    member_keys: &[(usize, usize)],
    data_section_layouts: &[Vec<DataSectionLayout>],
    tlv_descriptors: &[TlvDescriptorLayout],
) -> Result<BTreeMap<String, u64>, TlvSymOverrideError> {
    if tlv_descriptors.is_empty() {
        return Ok(BTreeMap::new());
    }

    // Bucket descriptors by (member_key, section_index) so the
    // walk below can look them up in O(log) without rescanning the
    // flat vec per sym.
    let mut by_member_section: BTreeMap<((usize, usize), u8), Vec<&TlvDescriptorLayout>> =
        BTreeMap::new();
    for d in tlv_descriptors {
        by_member_section
            .entry((d.member_key, d.section_index))
            .or_default()
            .push(d);
    }

    // Index `member_addr` by (member_key, section_index). rustc emits
    // non-zero section.addr for `__DATA,*` sections in `.rcgu.o`
    // members (cumulative section addresses inside one anonymous
    // `LC_SEGMENT_64`), so `nlist.n_value` for a thread-local sym
    // equals `member_addr + offset_in_section`. We can't recover
    // member_addr from `TlvDescriptorLayout` alone — it carries
    // final-binary vaddrs, not source `.o` vmaddrs — so the caller
    // passes the original `data_section_layouts` and we read
    // `member_addr` straight from there.
    let mut section_member_addrs: BTreeMap<((usize, usize), u8), u64> = BTreeMap::new();
    for (mi, key) in member_keys.iter().enumerate() {
        if mi >= data_section_layouts.len() {
            break;
        }
        for sec in &data_section_layouts[mi] {
            if is_tlv_descriptor(sec.flags) {
                section_member_addrs.insert((*key, sec.section_index), sec.member_addr);
            }
        }
    }

    let mut overrides: BTreeMap<String, u64> = BTreeMap::new();
    for key in member_keys {
        // TLV section ordinals present in this member, if any.
        let mut tlv_sections: BTreeSet<u8> = BTreeSet::new();
        for d in tlv_descriptors {
            if d.member_key == *key {
                tlv_sections.insert(d.section_index);
            }
        }
        if tlv_sections.is_empty() {
            continue;
        }

        let member = &merged.per_archive_members[key.0][key.1];
        let bytes = member.data;
        let (symoff, nsyms, strtab) = match parse_member_symtab(bytes, key)? {
            Some(v) => v,
            None => continue,
        };

        for i in 0..nsyms {
            let off = symoff + i * 16;
            let n_strx = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()) as usize;
            let n_type = bytes[off + 4];
            let n_sect = bytes[off + 5];
            let n_value = u64::from_le_bytes(bytes[off + 8..off + 16].try_into().unwrap());

            if (n_type & N_TYPE) != N_SECT || (n_type & N_EXT) == 0 {
                continue;
            }
            if !tlv_sections.contains(&n_sect) {
                continue;
            }
            let vaddr = descriptor_vaddr_for_sym(
                key,
                i,
                n_sect,
                n_value,
                &section_member_addrs,
                &by_member_section,
            )?;
            let name = read_sym_name(strtab, n_strx, key, i)?;
            overrides.insert(name.to_string(), vaddr);
        }
    }

    Ok(overrides)
}

/// Convenience wrapper used by [`crate::archive_emit`] — gathers
/// `member_keys` from `layout`, calls [`compute_tlv_sym_overrides`],
/// and inserts each `(name, descriptor_vaddr)` into the supplied
/// `sym_table` (overriding any earlier `m.vaddr + n_value` entry).
pub fn apply_tlv_overrides(
    layout: &ArchiveLayout,
    merged: &MergedArchives,
    sym_table: &mut SymTable,
) -> Result<(), TlvSymOverrideError> {
    let member_keys: Vec<(usize, usize)> = layout.member_layouts.iter().map(|m| m.key).collect();
    let overrides = compute_tlv_sym_overrides(
        merged,
        &member_keys,
        &layout.data_non_text_layouts,
        &layout.tlv_descriptors,
    )?;
    for (name, vaddr) in overrides {
        sym_table.insert(name, vaddr);
    }
    Ok(())
}

/// Errors [`compute_tlv_sym_overrides`] can report.
#[derive(Debug)]
pub enum TlvSymOverrideError {
    /// Member's `LC_SYMTAB` walk failed — bubbles up the underlying
    /// [`MemberSymtabError`] tagged with the offending member.
    Symtab {
        archive_idx: usize,
        member_idx: usize,
        err: MemberSymtabError,
    },
    /// A TLV sym's `n_value` was not a multiple of 24 — would mean
    /// the sym points into the middle of a descriptor, which is a
    /// malformed member.
    MisalignedTlvSymValue {
        archive_idx: usize,
        member_idx: usize,
        sym_index: usize,
        n_value: u64,
    },
    /// Internal invariant — the descriptor bucket lookup failed.
    /// Shouldn't fire because the bucket was built from the same
    /// `tlv_descriptors` vec the sym walk iterates over.
    MissingDescriptorBucket {
        archive_idx: usize,
        member_idx: usize,
        section_index: u8,
    },
    /// A TLV sym's `n_value / 24` indexed past the descriptor count
    /// in that section — implies the sym lies beyond the section's
    /// declared `size`, malformed member.
    DescriptorIndexOutOfRange {
        archive_idx: usize,
        member_idx: usize,
        section_index: u8,
        descriptor_index: usize,
        descriptor_count: usize,
    },
}
