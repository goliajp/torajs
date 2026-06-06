//! TLV descriptor enumeration over already-laid-out `__DATA,*`
//! sections — SD-4c-prereq+b1.
//!
//! Pairs with [`crate::data_section_layout`] (which lays out every
//! `__DATA,*` section including `__thread_vars`) and is consumed by
//! SD-4c-prereq+c's chained-fixups encoder: each TLV descriptor's
//! 8-byte `thunk` slot needs a dyld-bind fixup pointing at
//! `__tlv_bootstrap` in libSystem so the loader can patch it at
//! startup. The encoder only needs the slot vaddrs — the descriptor
//! payload bytes themselves were already emitted by fix-c4's
//! [`crate::data_section_emit::write_data_non_text_file_payloads`]
//! (TLV descriptor sections carry file storage and flow through the
//! file-storage path same as `__data`).
//!
//! **Layout numbers stay metadata-only in prereq+b1** — the helper
//! does not shift any file offset, does not change the binary's
//! emit pass, and does not patch the sym_table. Every probe stays
//! byte-identical with prereq+a's HEAD (`0c65b29`). SD-4c-prereq+c
//! is what wires the per-thunk-slot bind into the chained-fixups
//! blob; SD-4c-prereq+d is the exec-not-SIGSEGV acceptance gate.
//!
//! Layout model — every Mach-O TLV descriptor is a 24-byte triple
//! per `<mach-o/loader.h>` (`struct tlv_descriptor`):
//!
//! ```text
//!   offset 0 : thunk  — function pointer dyld binds to _tlv_bootstrap
//!   offset 8 : key    — pthread TLS key (filled by _tlv_bootstrap)
//!   offset 16: offset — variable's offset inside the TLS block
//! ```
//!
//! The descriptor table in `__DATA,__thread_vars` packs N descriptors
//! contiguously, so the i-th descriptor lives at
//! `section.final_vaddr + i*24`.

use std::collections::{BTreeMap, BTreeSet};

use torajs_obj::{LC_SYMTAB, MH_MAGIC_64, N_EXT, N_SECT, N_TYPE};

use crate::archive::MemberSymtabError;
use crate::archives_merge::MergedArchives;
use crate::data_section_layout::DataSectionLayout;
use crate::layout_types::ArchiveLayout;
use crate::member_sections::is_tlv_descriptor;
use crate::resolve::SymTable;

/// Byte size of a single Mach-O TLV descriptor — three pointer-
/// sized slots: thunk function, pthread key, payload offset.
pub const TLV_DESCRIPTOR_SIZE: u32 = 24;

/// Final-binary placement for one TLV descriptor. Carries the
/// vaddrs of all three 8-byte slots so SD-4c-prereq+c can wire the
/// thunk slot into the chained-fixups bind chain without re-deriving
/// the math.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlvDescriptorLayout {
    /// `(archive_idx, member_idx)` of the source member. Same
    /// indexing convention as `member_keys` everywhere else in
    /// `archive_link`.
    pub member_key: (usize, usize),
    /// 1-based section ordinal inside the source member — same
    /// value the source member's `LC_SYMTAB` records in `nlist.n_sect`
    /// for the thread_local sym that owns this descriptor.
    pub section_index: u8,
    /// Vaddr of the descriptor's first byte (= `section.final_vaddr
    /// + i*24` for the i-th descriptor in the section). Doubles as
    /// the `thunk` slot vaddr since `thunk` lives at offset 0.
    pub descriptor_vaddr: u64,
    /// Vaddr of the 8-byte `thunk` slot. Equal to `descriptor_vaddr`;
    /// kept as a separate field so callers don't need to remember
    /// the layout convention. `__tlv_bootstrap` binds here at load.
    pub thunk_slot_vaddr: u64,
    /// Vaddr of the 8-byte `key` slot (`descriptor_vaddr + 8`).
    /// Loader writes the pthread key value here when it invokes
    /// `_tlv_bootstrap` at startup.
    pub key_slot_vaddr: u64,
    /// Vaddr of the 8-byte `offset` slot (`descriptor_vaddr + 16`).
    /// Carries the variable's offset inside the per-thread TLS block;
    /// the compiler-emitted descriptor sets this at link time.
    pub offset_slot_vaddr: u64,
}

/// Output of [`compute_tlv_descriptor_layouts`]. `descriptors` is a
/// flat vec across every member, in `(member_key, section_index,
/// descriptor_index_within_section)` order. The total count equals
/// the binary's TLV descriptor count and is exactly the number of
/// bind entries SD-4c-prereq+c needs to add to the chained-fixups
/// blob.
#[derive(Debug, Clone)]
pub struct TlvLayoutResult {
    pub descriptors: Vec<TlvDescriptorLayout>,
}

impl TlvLayoutResult {
    /// Convenience for callers that only need to know "are there any
    /// TLV descriptors at all?" — non-TLV-using binaries (e.g. the
    /// syscall_abort probe) take this fast path and skip the entire
    /// bind-chain branch in SD-4c-prereq+c.
    pub fn is_empty(&self) -> bool {
        self.descriptors.is_empty()
    }
}

/// Enumerate every TLV descriptor in every member's already-laid-out
/// `__DATA,__thread_vars` sections. `data_section_layouts[i]` must
/// be the layout for `member_keys[i]` — same outer-vec convention
/// [`crate::data_section_layout::compute_data_section_layouts`]
/// returns through `per_member`.
///
/// `data_section_layouts` is borrowed; the caller (`compute_archive
/// _layout` in `archive_link.rs`) already owns the layout vec and
/// passes a slice. The helper is pure — no `merged` walk, no
/// `LC_SYMTAB` parse, no I/O — so it stays small and free of error
/// surface.
///
/// Empty `member_keys` (no archive members pulled in) returns an
/// empty result. Members with no `S_THREAD_LOCAL_VARIABLES` section
/// (e.g. syscall-abort.o, _malloc-only worklist) likewise contribute
/// zero descriptors. Mismatch between `member_keys.len()` and
/// `data_section_layouts.len()` is rejected up-front with
/// `LayoutLengthMismatch` — caller bug, not data corruption.
pub fn compute_tlv_descriptor_layouts(
    member_keys: &[(usize, usize)],
    data_section_layouts: &[Vec<DataSectionLayout>],
) -> Result<TlvLayoutResult, TlvLayoutError> {
    if member_keys.len() != data_section_layouts.len() {
        return Err(TlvLayoutError::LayoutLengthMismatch {
            member_keys_len: member_keys.len(),
            data_section_layouts_len: data_section_layouts.len(),
        });
    }
    let mut descriptors: Vec<TlvDescriptorLayout> = Vec::new();
    for (mi, key) in member_keys.iter().enumerate() {
        for sec in &data_section_layouts[mi] {
            if !is_tlv_descriptor(sec.flags) {
                continue;
            }
            if sec.size % TLV_DESCRIPTOR_SIZE != 0 {
                return Err(TlvLayoutError::DescriptorSectionMisaligned {
                    archive_idx: key.0,
                    member_idx: key.1,
                    section_index: sec.section_index,
                    size: sec.size,
                });
            }
            let count = sec.size / TLV_DESCRIPTOR_SIZE;
            for i in 0..count {
                let descriptor_vaddr =
                    sec.final_vaddr + u64::from(i) * u64::from(TLV_DESCRIPTOR_SIZE);
                descriptors.push(TlvDescriptorLayout {
                    member_key: *key,
                    section_index: sec.section_index,
                    descriptor_vaddr,
                    thunk_slot_vaddr: descriptor_vaddr,
                    key_slot_vaddr: descriptor_vaddr + 8,
                    offset_slot_vaddr: descriptor_vaddr + 16,
                });
            }
        }
    }
    Ok(TlvLayoutResult { descriptors })
}

/// Walk every integrated member's `LC_SYMTAB` for thread-local
/// defined externals (`N_SECT|N_EXT` whose `n_sect` indexes a TLV
/// descriptor section), and produce `sym_name → descriptor_vaddr`
/// overrides for the link driver's `effective_sym_table`.
///
/// **Why this exists** — `parse_member_defined_externs` returns
/// `(name, n_value)` per defined extern, and `archive_emit` plugs it
/// into the sym table as `m.vaddr + n_value`. That math is right for
/// `__text` symbols (`m.vaddr` is the member's `__text` start), but
/// **wrong for thread-local symbols** because their `n_value` is a
/// section-relative offset inside `__DATA,__thread_vars`, not `__text`.
/// Reading the resulting "vaddr" as a pointer at runtime would land
/// somewhere inside the binary's `__TEXT` region — almost certainly
/// not a valid TLV descriptor — and dyld's `_tlv_bootstrap` thunk
/// would never get reached.
///
/// This helper recomputes the correct vaddr — the final binary's
/// TLV descriptor address — by matching each thread-local sym's
/// `(n_sect, n_value)` against the [`TlvDescriptorLayout`] vec
/// produced by [`compute_tlv_descriptor_layouts`]. The link driver
/// then overlays the returned map onto `effective_sym_table` after
/// it inserts the raw `m.vaddr + n_value` entries, replacing each
/// TLV sym's wrong vaddr with `descriptor_vaddr`.
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
        if bytes.len() < 32 {
            return Err(TlvSymOverrideError::Symtab {
                archive_idx: key.0,
                member_idx: key.1,
                err: MemberSymtabError::TruncatedHeader,
            });
        }
        let magic = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
        if magic != MH_MAGIC_64 {
            continue;
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
                let stroff =
                    u32::from_le_bytes(bytes[cursor + 16..cursor + 20].try_into().unwrap());
                let strsize =
                    u32::from_le_bytes(bytes[cursor + 20..cursor + 24].try_into().unwrap());
                symtab = Some((symoff, nsyms, stroff, strsize));
            }
            cursor += cmdsize;
        }

        let (symoff, nsyms, stroff, strsize) = match symtab {
            Some(v) => v,
            None => continue,
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
        let strtab = &bytes[stroff..stroff + strsize];

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
                    sym_index: i,
                    n_value,
                });
            }
            let offset_in_section = n_value - section_member_addr;
            if offset_in_section % u64::from(TLV_DESCRIPTOR_SIZE) != 0 {
                return Err(TlvSymOverrideError::MisalignedTlvSymValue {
                    archive_idx: key.0,
                    member_idx: key.1,
                    sym_index: i,
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
            let descriptor = bucket.get(descriptor_index).ok_or(
                TlvSymOverrideError::DescriptorIndexOutOfRange {
                    archive_idx: key.0,
                    member_idx: key.1,
                    section_index: n_sect,
                    descriptor_index,
                    descriptor_count: bucket.len(),
                },
            )?;

            if n_strx >= strtab.len() {
                return Err(TlvSymOverrideError::Symtab {
                    archive_idx: key.0,
                    member_idx: key.1,
                    err: MemberSymtabError::BadStrx {
                        sym_index: i,
                        n_strx,
                    },
                });
            }
            let end_off = strtab[n_strx..]
                .iter()
                .position(|&b| b == 0)
                .map(|p| n_strx + p)
                .ok_or(TlvSymOverrideError::Symtab {
                    archive_idx: key.0,
                    member_idx: key.1,
                    err: MemberSymtabError::UnterminatedSymName {
                        sym_index: i,
                        n_strx,
                    },
                })?;
            let name_bytes = &strtab[n_strx..end_off];
            let name =
                std::str::from_utf8(name_bytes).map_err(|_| TlvSymOverrideError::Symtab {
                    archive_idx: key.0,
                    member_idx: key.1,
                    err: MemberSymtabError::NameNotUtf8 { sym_index: i },
                })?;
            overrides.insert(name.to_string(), descriptor.descriptor_vaddr);
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

/// Errors [`compute_tlv_descriptor_layouts`] can report. Both are
/// caller / data invariants — neither is recoverable inside the
/// link pipeline.
#[derive(Debug, PartialEq, Eq)]
pub enum TlvLayoutError {
    /// Caller passed `data_section_layouts.len() != member_keys.len()`
    /// — must agree because the inner vecs index by member position.
    LayoutLengthMismatch {
        member_keys_len: usize,
        data_section_layouts_len: usize,
    },
    /// A TLV descriptor section's `size` is not a multiple of
    /// `TLV_DESCRIPTOR_SIZE` (24). Either the source member is
    /// malformed or [`crate::data_section_layout`] misclassified a
    /// non-TLV section as TLV.
    DescriptorSectionMisaligned {
        archive_idx: usize,
        member_idx: usize,
        section_index: u8,
        size: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_section_layout::DataSectionLayout;
    use torajs_obj::{S_REGULAR, S_THREAD_LOCAL_VARIABLES};

    fn padded(s: &[u8]) -> [u8; 16] {
        let mut out = [0u8; 16];
        out[..s.len()].copy_from_slice(s);
        out
    }

    fn tlv_section(
        section_index: u8,
        final_vaddr: u64,
        size: u32,
        final_file_offset: u32,
    ) -> DataSectionLayout {
        DataSectionLayout {
            section_index,
            sectname: padded(b"__thread_vars"),
            segname: padded(b"__DATA"),
            member_internal_offset: 0,
            size,
            flags: S_THREAD_LOCAL_VARIABLES,
            has_file_storage: true,
            final_file_offset,
            final_vaddr,
            member_addr: 0,
            align: 0,
        }
    }

    fn data_section(
        section_index: u8,
        final_vaddr: u64,
        size: u32,
        final_file_offset: u32,
    ) -> DataSectionLayout {
        DataSectionLayout {
            section_index,
            sectname: padded(b"__data"),
            segname: padded(b"__DATA"),
            member_internal_offset: 0,
            size,
            flags: S_REGULAR,
            has_file_storage: true,
            final_file_offset,
            final_vaddr,
            member_addr: 0,
            align: 0,
        }
    }

    /// Empty inputs → empty result. is_empty() reports true.
    #[test]
    fn empty_inputs_produce_empty_result() {
        let res = compute_tlv_descriptor_layouts(&[], &[]).unwrap();
        assert!(res.is_empty());
        assert_eq!(res.descriptors.len(), 0);
    }

    /// Single member with no TLV section → empty descriptors. Drop-
    /// in safe when wired into syscall_abort / _malloc probes
    /// (no archive members pull TLV).
    #[test]
    fn member_without_tlv_section_contributes_zero() {
        let layouts = vec![vec![data_section(3, 0x1_0000_8000, 16, 0x4000)]];
        let res = compute_tlv_descriptor_layouts(&[(0, 0)], &layouts).unwrap();
        assert!(res.is_empty());
    }

    /// One TLV section with 3 descriptors → 3 layout entries with
    /// 24-byte stride. Thunk / key / offset slot vaddrs derive from
    /// the descriptor base in the canonical (+0, +8, +16) layout.
    #[test]
    fn single_section_yields_one_layout_per_24_bytes() {
        let layouts = vec![vec![tlv_section(4, 0xDA00_1000, 72, 0x7000)]];
        let res = compute_tlv_descriptor_layouts(&[(0, 0)], &layouts).unwrap();
        assert_eq!(res.descriptors.len(), 3, "72 / 24 = 3 descriptors");
        for (i, d) in res.descriptors.iter().enumerate() {
            let expected_base = 0xDA00_1000 + (i as u64) * 24;
            assert_eq!(d.member_key, (0, 0));
            assert_eq!(d.section_index, 4);
            assert_eq!(d.descriptor_vaddr, expected_base);
            assert_eq!(d.thunk_slot_vaddr, expected_base);
            assert_eq!(d.key_slot_vaddr, expected_base + 8);
            assert_eq!(d.offset_slot_vaddr, expected_base + 16);
        }
    }

    /// Mixed member: one `__data` + one `__thread_vars` (48 bytes /
    /// 2 descriptors) → only the TLV section contributes. Filter
    /// preserves correctness when members carry both regular data
    /// and TLV descriptors.
    #[test]
    fn mixed_data_and_tlv_filters_to_tlv_only() {
        let layouts = vec![vec![
            data_section(3, 0xDA00_0000, 16, 0x4000),
            tlv_section(5, 0xDA00_0010, 48, 0x4010),
        ]];
        let res = compute_tlv_descriptor_layouts(&[(0, 0)], &layouts).unwrap();
        assert_eq!(res.descriptors.len(), 2);
        assert_eq!(res.descriptors[0].descriptor_vaddr, 0xDA00_0010);
        assert_eq!(res.descriptors[1].descriptor_vaddr, 0xDA00_0010 + 24);
    }

    /// Two members each contributing a TLV section → descriptors
    /// flat-vec preserves member order then section order then
    /// in-section index.
    #[test]
    fn two_members_descriptors_flatten_in_member_order() {
        let layouts = vec![
            vec![tlv_section(2, 0xDA00_0000, 24, 0x4000)],
            vec![tlv_section(3, 0xDA00_1000, 48, 0x5000)],
        ];
        let keys = vec![(0usize, 0usize), (0usize, 1usize)];
        let res = compute_tlv_descriptor_layouts(&keys, &layouts).unwrap();
        assert_eq!(res.descriptors.len(), 3);
        assert_eq!(res.descriptors[0].member_key, (0, 0));
        assert_eq!(res.descriptors[0].descriptor_vaddr, 0xDA00_0000);
        assert_eq!(res.descriptors[1].member_key, (0, 1));
        assert_eq!(res.descriptors[1].descriptor_vaddr, 0xDA00_1000);
        assert_eq!(res.descriptors[2].member_key, (0, 1));
        assert_eq!(res.descriptors[2].descriptor_vaddr, 0xDA00_1000 + 24);
    }

    /// Length-mismatched inputs → typed error, no partial result.
    /// Guards against caller bugs (wrong outer-vec slice).
    #[test]
    fn mismatched_lengths_returns_typed_error() {
        let keys = vec![(0, 0), (0, 1)];
        let layouts = vec![vec![tlv_section(2, 0xDA00_0000, 24, 0x4000)]];
        let err = compute_tlv_descriptor_layouts(&keys, &layouts).unwrap_err();
        assert_eq!(
            err,
            TlvLayoutError::LayoutLengthMismatch {
                member_keys_len: 2,
                data_section_layouts_len: 1,
            }
        );
    }

    /// TLV section whose size isn't a multiple of 24 → typed error.
    /// Catches malformed members up-front so prereq+c doesn't bind
    /// a partial descriptor.
    #[test]
    fn misaligned_section_size_returns_typed_error() {
        let layouts = vec![vec![tlv_section(4, 0xDA00_0000, 25, 0x4000)]];
        let err = compute_tlv_descriptor_layouts(&[(7, 9)], &layouts).unwrap_err();
        assert_eq!(
            err,
            TlvLayoutError::DescriptorSectionMisaligned {
                archive_idx: 7,
                member_idx: 9,
                section_index: 4,
                size: 25,
            }
        );
    }
}
