//! Non-`__text` section layout pass for archive members —
//! SD-4c-prereq-b2.
//!
//! [`crate::member_sections::collect_member_sections`] yields every
//! section a member exposes. SD-4c-prereq-b2 takes that list,
//! filters out the primary `__TEXT,__text` slice (already laid out
//! in [`crate::archive_link::MemberLayout`]), and assigns each
//! remaining section a final-binary `(file_offset, vaddr)` pair so
//! the SectionRef arm of [`crate::member_apply::apply_member_relocs`]
//! (wired in SD-4c-prereq-b3) can issue correct patch targets.
//!
//! **Layout numbers stay shadow-only in prereq-b2** — the helper
//! computes file_offsets *assuming* a hypothetical non-text region
//! immediately following member `__text` payloads and preceding
//! `__TEXT,__stubs`, but it does **not** shift `stubs_file_offset`
//! or grow `text_vmsize`. SD-4c-prereq-c performs the real layout
//! shift + bytes emit; the shadow numbers are correct only after
//! prereq-c lands. Keeping prereq-b2 layout-neutral preserves
//! byte-identical output for probes that don't touch r_extern=0
//! relocs (e.g. `probe_real_link ___torajs_syscall_abort`).
//!
//! Module extraction (vs adding to `archive_link.rs`) is driven by
//! the 500-prod-LOC hard limit — `archive_link.rs` is at 486 prod
//! after SD-2/3/4, adding the non-text computation inline would
//! push it past 500.

use crate::archives_merge::MergedArchives;
use crate::lc::TEXT_VMADDR_BASE;
use crate::member_reloc::MemberRelocError;
use crate::member_sections::{MemberSectionInfo, collect_member_sections, is_no_file_storage};

const TEXT_SECTNAME: &[u8] = b"__text";
const TEXT_SEGNAME: &[u8] = b"__TEXT";

/// Final-binary placement for a single non-`__text` section pulled
/// from an archive member. SD-4c-prereq-b3 dispatches an
/// `r_extern = 0` reloc by looking the entry up by `section_index`
/// and computing `target_vaddr = final_vaddr + addend`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NonTextSectionLayout {
    /// 1-based section ordinal *inside the member* — same value the
    /// reloc encoder writes into `r_symbolnum` when `r_extern = 0`.
    pub section_index: u8,
    /// Raw 16-byte `sectname` slot from the member's `section_64`.
    pub sectname: [u8; 16],
    /// Raw 16-byte `segname` slot from the member's `section_64`.
    pub segname: [u8; 16],
    /// Byte offset *inside the member* where the payload starts —
    /// pair with `size` to slice the member's bytes.
    pub member_internal_offset: u32,
    /// On-disk size of the section payload.
    pub size: u32,
    /// File offset inside the final binary where SD-4c-prereq-c
    /// will land the payload bytes.
    pub final_file_offset: u32,
    /// Virtual address inside the final binary where the payload
    /// will live. Always `TEXT_VMADDR_BASE + final_file_offset`
    /// (the non-text region sits inside `__TEXT`).
    pub final_vaddr: u64,
    /// `section_64.addr` — the section's vmaddr **as declared inside
    /// the `.o`**. Conventionally 0 for single-section members but
    /// **non-zero for multi-section staticlib members** (e.g.
    /// `libtorajs_fmt.a`'s `__TEXT,__cstring` lands at object-file
    /// addr 0x3814 after `__text` + `__const`). swap-2k:
    /// `nlist.n_value` for a `l_anon.*` local sym equals
    /// `member_addr + intra_section_offset`, so resolvers must
    /// subtract `member_addr` before adding `final_vaddr` —
    /// mirrors [`crate::data_section_layout::DataSectionLayout::member_addr`].
    pub member_addr: u64,
}

/// Output of [`compute_non_text_layouts`]. `per_member[i]` lines up
/// 1:1 with the input `member_keys[i]`. `region_size` is the total
/// number of bytes the non-text region will occupy in the final
/// binary; SD-4c-prereq-c shifts `stubs_file_offset` by this much.
#[derive(Debug, Clone)]
pub struct NonTextLayoutResult {
    pub per_member: Vec<Vec<NonTextSectionLayout>>,
    pub region_size: u32,
}

/// Per-member error tuple emitted by [`compute_non_text_layouts`].
/// Carries the `(archive_idx, member_idx)` so the link driver can
/// blame a specific member when the walk fails.
#[derive(Debug, PartialEq, Eq)]
pub struct NonTextLayoutError {
    pub archive_idx: usize,
    pub member_idx: usize,
    pub err: MemberRelocError,
}

/// Trim trailing NULs / spaces from a 16-byte name slot.
fn name_eq(slot: &[u8; 16], expected: &[u8]) -> bool {
    let mut end = slot.len();
    while end > 0 && (slot[end - 1] == 0 || slot[end - 1] == b' ') {
        end -= 1;
    }
    &slot[..end] == expected
}

fn is_member_text(s: &MemberSectionInfo) -> bool {
    name_eq(&s.sectname, TEXT_SECTNAME) && name_eq(&s.segname, TEXT_SEGNAME)
}

/// Returns `true` for sections SD-4c-prereq-c-fix-c1 keeps in the
/// `__TEXT` non-text region — segname `__TEXT` with regular file
/// storage. `__DATA,*` and zerofill-family sections are filtered
/// out at this gate; fix-c2/3/4 will reintroduce them through a
/// dedicated `__DATA` segment + section layout pass.
///
/// Pulling the predicate out of the per-section loop keeps the
/// rationale grep-friendly and ensures the test suite can assert on
/// the exact admission rule.
fn keeps_in_text_non_text_region(s: &MemberSectionInfo) -> bool {
    if is_no_file_storage(s.flags) {
        return false;
    }
    name_eq(&s.segname, TEXT_SEGNAME)
}

/// Walk every required member, filter the primary `__TEXT,__text`
/// section, and assign each remaining section a final-binary
/// `(file_offset, vaddr)` pair. `region_file_offset` is the
/// hypothetical first byte of the non-text region — SD-4c-prereq-c
/// will make that hypothesis real by shifting `stubs_file_offset`.
///
/// Members appear in the input `member_keys` order (typically the
/// sorted `(archive_idx, member_idx)` tuple compute_archive_layout
/// produces). Within a member, sections retain their
/// `LC_SEGMENT_64` walk order — Mach-O's contiguous section ordinal
/// matches that order, so the SectionRef arm can use array indexing
/// on the returned `Vec<NonTextSectionLayout>` against
/// `section_index` once it filters out the __text slot.
pub fn compute_non_text_layouts(
    merged: &MergedArchives,
    member_keys: &[(usize, usize)],
    region_file_offset: u32,
) -> Result<NonTextLayoutResult, NonTextLayoutError> {
    let mut per_member: Vec<Vec<NonTextSectionLayout>> = Vec::with_capacity(member_keys.len());
    let mut cumulative: u32 = 0;
    for &(a_idx, m_idx) in member_keys {
        let member = &merged.per_archive_members[a_idx][m_idx];
        let all = collect_member_sections(member).map_err(|err| NonTextLayoutError {
            archive_idx: a_idx,
            member_idx: m_idx,
            err,
        })?;
        let mut layouts: Vec<NonTextSectionLayout> = Vec::new();
        for sec in all {
            if is_member_text(&sec) {
                continue;
            }
            // SD-4c-prereq-c-fix-c1 — only `__TEXT,*` regular
            // file-storage sections enter the __TEXT non-text region.
            // `__DATA,*` and `*ZEROFILL` family sections (which the
            // current __TEXT-only emit can't host without permission
            // mismatches or vmsize/filesize confusion) are deferred
            // to fix-c2/3/4's dedicated __DATA layout pass.
            if !keeps_in_text_non_text_region(&sec) {
                continue;
            }
            let final_file_offset = region_file_offset + cumulative;
            let final_vaddr = TEXT_VMADDR_BASE + u64::from(final_file_offset);
            layouts.push(NonTextSectionLayout {
                section_index: sec.section_index,
                sectname: sec.sectname,
                segname: sec.segname,
                member_internal_offset: sec.member_internal_offset,
                size: sec.size,
                final_file_offset,
                final_vaddr,
                member_addr: sec.member_addr,
            });
            cumulative += sec.size;
        }
        per_member.push(layouts);
    }
    Ok(NonTextLayoutResult {
        per_member,
        region_size: cumulative,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::{AR_HEADER_SIZE, AR_MAGIC, parse_archive};
    use crate::archives_merge::merge_archive_indexes;
    use torajs_codegen::CompiledFunction;
    use torajs_codegen::frame::FrameLayout;

    const RET_BYTES: [u8; 4] = [0xC0, 0x03, 0x5F, 0xD6];

    fn build_short_name_archive(name: &str, data: &[u8]) -> Vec<u8> {
        assert!(name.len() <= 16);
        let mut archive: Vec<u8> = Vec::new();
        archive.extend_from_slice(AR_MAGIC);
        let mut header = [b' '; AR_HEADER_SIZE];
        header[..name.len()].copy_from_slice(name.as_bytes());
        header[16] = b'0';
        let size_str = format!("{}", data.len());
        header[48..48 + size_str.len()].copy_from_slice(size_str.as_bytes());
        header[58] = b'`';
        header[59] = b'\n';
        archive.extend_from_slice(&header);
        archive.extend_from_slice(data);
        if data.len() % 2 != 0 {
            archive.push(0);
        }
        archive
    }

    fn fn_leaf(name: &str) -> CompiledFunction {
        CompiledFunction {
            name: name.into(),
            bytes: RET_BYTES.to_vec(),
            relocs: Vec::new(),
            frame: FrameLayout::leaf_no_spill(),
        }
    }

    /// No `member_keys` (empty archives) → empty result, 0 region
    /// size. Compute helper must not allocate anything other than
    /// the empty wrapper vec.
    #[test]
    fn empty_member_keys_produces_empty_result() {
        let merged = merge_archive_indexes(&[]).unwrap();
        let res = compute_non_text_layouts(&merged, &[], 0x0001_0000).unwrap();
        assert!(res.per_member.is_empty());
        assert_eq!(res.region_size, 0);
    }

    /// torajs-obj emits a single `__TEXT,__text` section per `.o`,
    /// so a leaf member contributes no non-text layouts. region_size
    /// stays 0; per_member entry is an empty Vec.
    #[test]
    fn torajs_obj_leaf_member_has_no_non_text_sections() {
        let foo = fn_leaf("_foo");
        let archive = build_short_name_archive("a.o", &torajs_obj::write_object(&[foo]));
        let archives = vec![archive];
        let merged = merge_archive_indexes(&archives).unwrap();
        let member_keys = vec![(0usize, 0usize)];
        let res = compute_non_text_layouts(&merged, &member_keys, 0x0001_2000).unwrap();
        assert_eq!(res.per_member.len(), 1);
        assert!(
            res.per_member[0].is_empty(),
            "torajs-obj members ship only __TEXT,__text",
        );
        assert_eq!(res.region_size, 0);
    }

    /// Synthesize a member with two non-text sections directly, then
    /// hand it through compute_non_text_layouts and verify the
    /// section_index / vaddr / file_offset stride.
    #[test]
    fn cumulative_file_offsets_and_vaddrs_stride_by_size() {
        // Build a hand-crafted Mach-O .o with three sections under
        // one anonymous LC_SEGMENT_64:
        //   1. __TEXT,__text       (4 bytes RET)
        //   2. __TEXT,__cstring    (8 bytes payload)
        //   3. __TEXT,__const      (16 bytes payload)
        // No symtab, no relocs — collect_member_sections only needs
        // the segment + section table.
        let obj = build_three_section_member();
        let archive = build_short_name_archive("multi.o", &obj);
        let archives = vec![archive];
        let merged = merge_archive_indexes(&archives).unwrap();
        let member_keys = vec![(0usize, 0usize)];
        let region_off = 0x0001_4000u32;
        let res = compute_non_text_layouts(&merged, &member_keys, region_off).unwrap();

        assert_eq!(res.per_member.len(), 1);
        let layouts = &res.per_member[0];
        // __text is filtered → 2 layouts.
        assert_eq!(layouts.len(), 2);
        assert_eq!(layouts[0].section_index, 2, "__cstring at ordinal 2");
        assert_eq!(layouts[0].size, 8);
        assert_eq!(layouts[0].final_file_offset, region_off);
        assert_eq!(
            layouts[0].final_vaddr,
            TEXT_VMADDR_BASE + u64::from(region_off),
        );

        assert_eq!(layouts[1].section_index, 3, "__const at ordinal 3");
        assert_eq!(layouts[1].size, 16);
        assert_eq!(layouts[1].final_file_offset, region_off + 8);
        assert_eq!(
            layouts[1].final_vaddr,
            TEXT_VMADDR_BASE + u64::from(region_off + 8),
        );

        // region_size = 8 (cstring) + 16 (const) = 24.
        assert_eq!(res.region_size, 24);
    }

    /// Hand-pack a Mach-O .o with one anonymous LC_SEGMENT_64
    /// carrying __TEXT,__text + __TEXT,__cstring + __TEXT,__const.
    /// Keeps payload bytes minimal — only the section table matters
    /// for compute_non_text_layouts.
    fn build_three_section_member() -> Vec<u8> {
        use torajs_obj::{LC_SEGMENT_64, MH_MAGIC_64, SECTION_64_SIZE, SEGMENT_COMMAND_64_SIZE};
        let nsects: u32 = 3;
        let header_size: u32 = 32;
        let seg_cmd_size: u32 = SEGMENT_COMMAND_64_SIZE + nsects * SECTION_64_SIZE;
        // Payload bytes for each section.
        let text_payload: [u8; 4] = RET_BYTES;
        let cstring_payload: [u8; 8] = *b"abcdefgh";
        let const_payload: [u8; 16] = *b"0123456789abcdef";
        // File layout: header → LC_SEGMENT_64 → text → cstring → const.
        let text_off = header_size + seg_cmd_size;
        let cstring_off = text_off + text_payload.len() as u32;
        let const_off = cstring_off + cstring_payload.len() as u32;
        let total = const_off + const_payload.len() as u32;

        let mut out: Vec<u8> = Vec::with_capacity(total as usize);
        // mach_header_64
        out.extend_from_slice(&MH_MAGIC_64.to_le_bytes());
        out.extend_from_slice(&0x0100_000Cu32.to_le_bytes()); // cputype = ARM64
        out.extend_from_slice(&0u32.to_le_bytes()); // cpusubtype
        out.extend_from_slice(&1u32.to_le_bytes()); // filetype = MH_OBJECT
        out.extend_from_slice(&1u32.to_le_bytes()); // ncmds
        out.extend_from_slice(&seg_cmd_size.to_le_bytes()); // sizeofcmds
        out.extend_from_slice(&0u32.to_le_bytes()); // flags
        out.extend_from_slice(&0u32.to_le_bytes()); // reserved
        // LC_SEGMENT_64 header
        out.extend_from_slice(&LC_SEGMENT_64.to_le_bytes());
        out.extend_from_slice(&seg_cmd_size.to_le_bytes());
        let seg_name = [0u8; 16];
        out.extend_from_slice(&seg_name); // segname (empty)
        out.extend_from_slice(&0u64.to_le_bytes()); // vmaddr
        out.extend_from_slice(&(total as u64 - text_off as u64).to_le_bytes()); // vmsize
        out.extend_from_slice(&(text_off as u64).to_le_bytes()); // fileoff
        out.extend_from_slice(&(total as u64 - text_off as u64).to_le_bytes()); // filesize
        out.extend_from_slice(&7u32.to_le_bytes()); // maxprot RWX
        out.extend_from_slice(&7u32.to_le_bytes()); // initprot RWX
        out.extend_from_slice(&nsects.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // flags

        fn write_section_64(
            out: &mut Vec<u8>,
            sectname: &[u8],
            segname: &[u8],
            addr: u64,
            size: u64,
            offset: u32,
        ) {
            let mut sn = [0u8; 16];
            sn[..sectname.len()].copy_from_slice(sectname);
            out.extend_from_slice(&sn);
            let mut sg = [0u8; 16];
            sg[..segname.len()].copy_from_slice(segname);
            out.extend_from_slice(&sg);
            out.extend_from_slice(&addr.to_le_bytes());
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&offset.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes()); // align
            out.extend_from_slice(&0u32.to_le_bytes()); // reloff
            out.extend_from_slice(&0u32.to_le_bytes()); // nreloc
            out.extend_from_slice(&0u32.to_le_bytes()); // flags
            out.extend_from_slice(&0u32.to_le_bytes()); // reserved1
            out.extend_from_slice(&0u32.to_le_bytes()); // reserved2
            out.extend_from_slice(&0u32.to_le_bytes()); // reserved3
        }

        write_section_64(&mut out, b"__text", b"__TEXT", 0, 4, text_off);
        write_section_64(&mut out, b"__cstring", b"__TEXT", 4, 8, cstring_off);
        write_section_64(&mut out, b"__const", b"__TEXT", 12, 16, const_off);

        // payloads
        out.extend_from_slice(&text_payload);
        out.extend_from_slice(&cstring_payload);
        out.extend_from_slice(&const_payload);
        out
    }

    /// `parse_archive` looks for `parse_archive` on the result of
    /// `build_three_section_member` — sanity-check the fixture
    /// before the cumulative-offset test runs against it.
    #[test]
    fn three_section_fixture_parses_as_three_sections() {
        let archive = build_short_name_archive("multi.o", &build_three_section_member());
        let members = parse_archive(&archive).unwrap();
        let sections = collect_member_sections(&members[0]).unwrap();
        assert_eq!(sections.len(), 3);
        assert_eq!(sections[0].section_index, 1);
        assert_eq!(sections[1].section_index, 2);
        assert_eq!(sections[2].section_index, 3);
    }

    /// SD-4c-prereq-c-fix-c1 — `__DATA,*` sections are dropped from
    /// the __TEXT non-text layout pass. fix-c2/3/4 reintroduces them
    /// through a dedicated __DATA segment layout. Without this
    /// filter, torajs_mmalloc-cgu0's `__DATA,__common` (S_ZEROFILL,
    /// offset=0 size=2MB) overruns archive_emit's slicer at runtime.
    #[test]
    fn fix_c1_drops_data_segment_sections() {
        let obj = build_mixed_seg_member();
        let archive = build_short_name_archive("mix.o", &obj);
        let archives = vec![archive];
        let merged = merge_archive_indexes(&archives).unwrap();
        let res = compute_non_text_layouts(&merged, &[(0usize, 0usize)], 0x0001_6000).unwrap();
        let layouts = &res.per_member[0];
        // Fixture sections: __text, __cstring (TEXT keep), __data
        // (DATA drop), __common ZEROFILL (DATA + zerofill drop).
        // After filter: only __cstring remains.
        assert_eq!(layouts.len(), 1, "only __TEXT,__cstring survives");
        let s = &layouts[0];
        assert_eq!(s.section_index, 2, "__cstring keeps ordinal 2");
        assert_eq!(trim_name(&s.sectname), b"__cstring");
        assert_eq!(trim_name(&s.segname), b"__TEXT");
        // region_size advances only by surviving section's size.
        assert_eq!(res.region_size, s.size);
    }

    /// Zerofill section in `__TEXT` (rare but legal per Mach-O spec)
    /// still drops out — `is_no_file_storage` overrules the segname
    /// `__TEXT` admission. Otherwise emit would slice past the
    /// member end.
    #[test]
    fn fix_c1_drops_zerofill_even_inside_text_segment() {
        let obj = build_text_with_zerofill_member();
        let archive = build_short_name_archive("zft.o", &obj);
        let archives = vec![archive];
        let merged = merge_archive_indexes(&archives).unwrap();
        let res = compute_non_text_layouts(&merged, &[(0usize, 0usize)], 0x0001_6000).unwrap();
        let layouts = &res.per_member[0];
        // __text + __cstring keep, __TEXT,__zfsect (S_ZEROFILL)
        // drops. So only __cstring survives.
        assert_eq!(layouts.len(), 1);
        assert_eq!(trim_name(&layouts[0].sectname), b"__cstring");
    }

    fn trim_name(slot: &[u8; 16]) -> &[u8] {
        let mut end = slot.len();
        while end > 0 && (slot[end - 1] == 0 || slot[end - 1] == b' ') {
            end -= 1;
        }
        &slot[..end]
    }

    /// Hand-pack a Mach-O .o with four sections under one anonymous
    /// `LC_SEGMENT_64`:
    ///   1. __TEXT,__text       (4 B RET, S_REGULAR|PURE|SOME)
    ///   2. __TEXT,__cstring    (8 B,    S_CSTRING_LITERALS)
    ///   3. __DATA,__data       (16 B,   S_REGULAR)
    ///   4. __DATA,__common     (2 KB virtual, S_ZEROFILL — offset 0)
    /// Verifies fix-c1's segname + flags-based admission rule.
    fn build_mixed_seg_member() -> Vec<u8> {
        use torajs_obj::{
            LC_SEGMENT_64, MH_MAGIC_64, S_ATTR_PURE_INSTRUCTIONS, S_ATTR_SOME_INSTRUCTIONS,
            S_CSTRING_LITERALS, S_REGULAR, S_ZEROFILL, SECTION_64_SIZE, SEGMENT_COMMAND_64_SIZE,
        };
        let nsects: u32 = 4;
        let header_size: u32 = 32;
        let seg_cmd_size: u32 = SEGMENT_COMMAND_64_SIZE + nsects * SECTION_64_SIZE;
        let text: [u8; 4] = RET_BYTES;
        let cstr: [u8; 8] = *b"hello\0\0\0";
        let data: [u8; 16] = *b"DDDDDDDDDDDDDDDD";
        let text_off = header_size + seg_cmd_size;
        let cstr_off = text_off + text.len() as u32;
        let data_off = cstr_off + cstr.len() as u32;
        // __common is zerofill — no file payload past data_off.
        let total = data_off + data.len() as u32;

        let text_flags = S_REGULAR | S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS;
        let cstr_flags = S_CSTRING_LITERALS;
        let data_flags = S_REGULAR;
        let common_flags = S_ZEROFILL;

        let mut out: Vec<u8> = Vec::with_capacity(total as usize);
        out.extend_from_slice(&MH_MAGIC_64.to_le_bytes());
        out.extend_from_slice(&0x0100_000Cu32.to_le_bytes()); // cputype ARM64
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes()); // MH_OBJECT
        out.extend_from_slice(&1u32.to_le_bytes()); // ncmds
        out.extend_from_slice(&seg_cmd_size.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // flags
        out.extend_from_slice(&0u32.to_le_bytes()); // reserved

        out.extend_from_slice(&LC_SEGMENT_64.to_le_bytes());
        out.extend_from_slice(&seg_cmd_size.to_le_bytes());
        out.extend_from_slice(&[0u8; 16]); // segname (anonymous)
        out.extend_from_slice(&0u64.to_le_bytes()); // vmaddr
        out.extend_from_slice(&(total as u64 - text_off as u64).to_le_bytes()); // vmsize
        out.extend_from_slice(&(text_off as u64).to_le_bytes()); // fileoff
        out.extend_from_slice(&(total as u64 - text_off as u64).to_le_bytes()); // filesize
        out.extend_from_slice(&7u32.to_le_bytes()); // maxprot RWX
        out.extend_from_slice(&7u32.to_le_bytes()); // initprot RWX
        out.extend_from_slice(&nsects.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // flags

        write_section(&mut out, b"__text", b"__TEXT", 0, 4, text_off, text_flags);
        write_section(
            &mut out,
            b"__cstring",
            b"__TEXT",
            4,
            8,
            cstr_off,
            cstr_flags,
        );
        write_section(&mut out, b"__data", b"__DATA", 12, 16, data_off, data_flags);
        // __common is zerofill — section_64.offset is 0 per Mach-O
        // spec (no file storage), but vmsize is non-zero.
        write_section(&mut out, b"__common", b"__DATA", 28, 2048, 0, common_flags);

        out.extend_from_slice(&text);
        out.extend_from_slice(&cstr);
        out.extend_from_slice(&data);
        // No payload bytes for __common — that's the whole point of
        // S_ZEROFILL: the loader supplies zero-init memory at startup.
        out
    }

    /// Hand-pack a Mach-O .o with three sections all under `__TEXT`,
    /// but one of them is `S_ZEROFILL`. The segname check would
    /// otherwise admit it; verifies `is_no_file_storage` short-
    /// circuits regardless of segment.
    fn build_text_with_zerofill_member() -> Vec<u8> {
        use torajs_obj::{
            LC_SEGMENT_64, MH_MAGIC_64, S_ATTR_PURE_INSTRUCTIONS, S_ATTR_SOME_INSTRUCTIONS,
            S_CSTRING_LITERALS, S_REGULAR, S_ZEROFILL, SECTION_64_SIZE, SEGMENT_COMMAND_64_SIZE,
        };
        let nsects: u32 = 3;
        let header_size: u32 = 32;
        let seg_cmd_size: u32 = SEGMENT_COMMAND_64_SIZE + nsects * SECTION_64_SIZE;
        let text: [u8; 4] = RET_BYTES;
        let cstr: [u8; 8] = *b"world!\0\0";
        let text_off = header_size + seg_cmd_size;
        let cstr_off = text_off + text.len() as u32;
        let total = cstr_off + cstr.len() as u32;

        let text_flags = S_REGULAR | S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS;
        let cstr_flags = S_CSTRING_LITERALS;
        let zf_flags = S_ZEROFILL;

        let mut out: Vec<u8> = Vec::with_capacity(total as usize);
        out.extend_from_slice(&MH_MAGIC_64.to_le_bytes());
        out.extend_from_slice(&0x0100_000Cu32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&seg_cmd_size.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());

        out.extend_from_slice(&LC_SEGMENT_64.to_le_bytes());
        out.extend_from_slice(&seg_cmd_size.to_le_bytes());
        out.extend_from_slice(&[0u8; 16]);
        out.extend_from_slice(&0u64.to_le_bytes());
        out.extend_from_slice(&(total as u64 - text_off as u64).to_le_bytes());
        out.extend_from_slice(&(text_off as u64).to_le_bytes());
        out.extend_from_slice(&(total as u64 - text_off as u64).to_le_bytes());
        out.extend_from_slice(&7u32.to_le_bytes());
        out.extend_from_slice(&7u32.to_le_bytes());
        out.extend_from_slice(&nsects.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());

        write_section(&mut out, b"__text", b"__TEXT", 0, 4, text_off, text_flags);
        write_section(
            &mut out,
            b"__cstring",
            b"__TEXT",
            4,
            8,
            cstr_off,
            cstr_flags,
        );
        write_section(&mut out, b"__zfsect", b"__TEXT", 12, 4096, 0, zf_flags);

        out.extend_from_slice(&text);
        out.extend_from_slice(&cstr);
        out
    }

    fn write_section(
        out: &mut Vec<u8>,
        sectname: &[u8],
        segname: &[u8],
        addr: u64,
        size: u64,
        offset: u32,
        flags: u32,
    ) {
        let mut sn = [0u8; 16];
        sn[..sectname.len()].copy_from_slice(sectname);
        out.extend_from_slice(&sn);
        let mut sg = [0u8; 16];
        sg[..segname.len()].copy_from_slice(segname);
        out.extend_from_slice(&sg);
        out.extend_from_slice(&addr.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // align
        out.extend_from_slice(&0u32.to_le_bytes()); // reloff
        out.extend_from_slice(&0u32.to_le_bytes()); // nreloc
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // reserved1
        out.extend_from_slice(&0u32.to_le_bytes()); // reserved2
        out.extend_from_slice(&0u32.to_le_bytes()); // reserved3
    }
}
