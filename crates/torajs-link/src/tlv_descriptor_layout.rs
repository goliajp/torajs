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

use crate::data_section_layout::DataSectionLayout;
use crate::member_sections::is_tlv_descriptor;
// Chunk 474 — the TLV sym-override walk (`compute_tlv_sym_overrides`
// / `apply_tlv_overrides` / `TlvSymOverrideError`) moved to the
// sibling `tlv_sym_overrides.rs`; re-exported here so existing
// `crate::tlv_descriptor_layout::*` caller paths stay valid.
pub use crate::tlv_sym_overrides::{
    TlvSymOverrideError, apply_tlv_overrides, compute_tlv_sym_overrides,
};

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
