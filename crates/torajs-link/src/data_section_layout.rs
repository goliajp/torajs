//! `__DATA` section layout pass for archive members —
//! SD-4c-prereq-c-fix-c2.
//!
//! Pairs with [`crate::non_text_layout`] (which handles `__TEXT,*`
//! non-text sections). This module routes `__DATA,*` sections —
//! both regular file-storage (`__data`, `__const_data`, etc.) and
//! zerofill-family (`__common`, `__bss`, `__thread_bss`) — through
//! a dedicated layout pass that fix-c3 will wire into
//! [`crate::archive_link::ArchiveLayout`] and fix-c4 will hand to
//! the emit pass.
//!
//! **Layout assumptions** (kept stable across fix-c2/3/4):
//!
//! - `__DATA` segment starts at `data_vmaddr` (set by SD-2's
//!   `compute_archive_layout`, directly after `__TEXT`).
//! - `__la_symbol_ptr` (SD-2) stays at `data_vmaddr + 0` — fix-c3
//!   inserts member `__DATA,*` sections **after** it so SD-3's
//!   `chained_fixups` encoder's `segment_offset=0` assumption holds.
//! - Member file-storage sections occupy `__DATA` file bytes
//!   contiguously, in `member_keys` order, after `__la_symbol_ptr`.
//! - Zerofill sections occupy `__DATA` vmsize **after** the file
//!   region — no file payload, loader supplies zeros at startup.
//!
//! Layout output stays shadow-only in fix-c2 — fix-c3 wires it into
//! `ArchiveLayout` (re-shaping `data_vmsize` etc.) and fix-c4
//! emits the section table entries + file payload.

use crate::archives_merge::MergedArchives;
use crate::lc::TEXT_VMADDR_BASE;
use crate::member_sections::{MemberSectionInfo, collect_member_sections, is_no_file_storage};
use crate::non_text_layout::NonTextLayoutError;

const DATA_SEGNAME: &[u8] = b"__DATA";

/// Final-binary placement for a single `__DATA,*` section pulled
/// from an archive member. `has_file_storage` discriminates the two
/// emit paths fix-c3/c4 will route through:
///
/// - `true` → fix-c4 slices `member.data[member_internal_offset..
///   member_internal_offset + size]` and writes the bytes at
///   `final_file_offset` in the binary's `__DATA` region.
/// - `false` → loader zero-inits `size` bytes at `final_vaddr` on
///   startup; `member_internal_offset` is conventionally 0 and
///   `final_file_offset` is invalid (set to 0 sentinel — never use
///   without checking `has_file_storage` first).
///
/// `flags` preserves the raw `section_64.flags` so fix-c4 can copy
/// it verbatim into the emitted `section_64` entry (preserving
/// alignment hints and attribute bits the loader may inspect).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataSectionLayout {
    /// 1-based section ordinal inside the source member.
    pub section_index: u8,
    /// Raw 16-byte `sectname` slot.
    pub sectname: [u8; 16],
    /// Raw 16-byte `segname` slot (always `__DATA` for this pass).
    pub segname: [u8; 16],
    /// Byte offset inside the member where the file-storage payload
    /// starts. Sentinel `0` for zerofill sections (use
    /// `has_file_storage` to discriminate).
    pub member_internal_offset: u32,
    /// vm size of the section. Equals the file-storage size for
    /// regular sections; for zerofill sections, the loader allocates
    /// + zero-inits this many bytes per the section's `final_vaddr`.
    pub size: u32,
    /// Raw `section_64.flags`. Preserves both the type (low 8 bits)
    /// and attribute flags (high 24 bits) so the emit pass can write
    /// them back verbatim.
    pub flags: u32,
    /// `true` when the section's bytes live in the binary's file
    /// image (`S_REGULAR`, `S_CSTRING_LITERALS`, etc.). `false` for
    /// the zerofill family.
    pub has_file_storage: bool,
    /// File offset inside the final binary where fix-c4 lands the
    /// payload. Valid only when `has_file_storage`; sentinel `0`
    /// otherwise.
    pub final_file_offset: u32,
    /// Virtual address inside the final binary where the section's
    /// first byte lives — always valid for both file-storage and
    /// zerofill sections.
    pub final_vaddr: u64,
    /// `section_64.addr` from inside the source `.o` member. Needed
    /// by SD-4c-prereq+b2 to recover descriptor-relative offsets:
    /// rustc emits non-zero `member_addr` for `__DATA,*` sections in
    /// `.rcgu.o` members (cumulative section addresses), and
    /// `nlist.n_value` for a thread-local sym equals `member_addr +
    /// offset_in_section`. Without this, the layout pass can't tell
    /// which descriptor a given sym refers to.
    pub member_addr: u64,
    /// Required alignment as a power-of-two log2 exponent, copied from
    /// the source `section_64.align`. `final_vaddr` is rounded up to
    /// `1 << align`; the emit pass writes it into the output
    /// `section_64.align_log2` and pads the file payload so the
    /// on-disk offset matches (SD-4c swap-2i).
    pub align: u8,
}

/// Output of [`compute_data_section_layouts`]. `per_member[i]` lines
/// up 1:1 with the input `member_keys[i]`; each inner vec preserves
/// the order sections appeared in the member's `LC_SEGMENT_64` walk
/// (file-storage sections first, zerofill sections last — keeps the
/// emit pass's two-phase structure straightforward).
///
/// `file_region_size` is the total bytes the file-storage half of
/// the `__DATA` non-text region will occupy. `zerofill_vmsize` is
/// the additional vmsize the zerofill half needs (no file storage).
/// Total `__DATA` vmsize fix-c3 needs to budget =
/// `pre_existing_data_size + file_region_size + zerofill_vmsize`.
/// One emitted `section_64` of the `__DATA` segment (r503): every
/// member's `(segname, sectname)`-named section laid out back to
/// back, the way ld merges same-named input sections. The per-member
/// [`DataSectionLayout`] entries keep their own placements inside
/// the span; this is only what the load command says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedDataSection {
    pub sectname: [u8; 16],
    pub segname: [u8; 16],
    /// The OR of the members' flags — one section type by
    /// construction (same name, same kind), attribute bits pooled.
    pub flags: u32,
    /// The largest member alignment in the group.
    pub align: u8,
    pub vaddr: u64,
    /// Span from the first member's start to the last member's end,
    /// intra-group alignment padding included.
    pub size: u64,
    /// File offset of the first member's bytes; 0 for zerofill.
    pub file_offset: u32,
    pub has_file_storage: bool,
}

#[derive(Debug, Clone)]
pub struct DataSectionLayoutResult {
    pub per_member: Vec<Vec<DataSectionLayout>>,
    /// The `section_64` entries, file-storage groups first, then
    /// zerofill — one per distinct name in first-seen order.
    pub merged: Vec<MergedDataSection>,
    pub file_region_size: u32,
    pub zerofill_vmsize: u32,
}

/// The distinct `(segname, sectname)` names among `raw`'s `__DATA`
/// sections of one storage kind, in first-seen order.
fn section_name_groups(
    per_member_raw: &[Vec<MemberSectionInfo>],
    file_storage: bool,
) -> Vec<([u8; 16], [u8; 16])> {
    let mut groups: Vec<([u8; 16], [u8; 16])> = Vec::new();
    for raw in per_member_raw {
        for sec in raw {
            if !is_data_segment(sec) || is_no_file_storage(sec.flags) == file_storage {
                continue;
            }
            let key = (sec.segname, sec.sectname);
            if !groups.contains(&key) {
                groups.push(key);
            }
        }
    }
    groups
}

/// Padding bytes needed to round `addr` up to the next `1 << align`
/// boundary (`align` is a log2 exponent ≤ 31, so `1u64 << align`
/// never overflows). Returns 0 when already aligned or `align == 0`.
fn round_up_pad(addr: u64, align: u8) -> u64 {
    let a = 1u64 << align;
    (a - (addr & (a - 1))) & (a - 1)
}

/// `segname == "__DATA"` (trimmed of trailing NUL / space padding).
fn is_data_segment(s: &MemberSectionInfo) -> bool {
    let mut end = s.segname.len();
    while end > 0 && (s.segname[end - 1] == 0 || s.segname[end - 1] == b' ') {
        end -= 1;
    }
    &s.segname[..end] == DATA_SEGNAME
}

/// Walk every required member's `__DATA,*` sections and assign each
/// one a final-binary placement. Two passes:
///
/// 1. File-storage sections — accumulate `final_file_offset` and
///    `final_vaddr` from the supplied `(file_region_start_offset,
///    vaddr_start)` pair, rounding each section up to its
///    `section_64.align` boundary (same pad applied to both cursors so
///    they stay page-congruent for mmap).
/// 2. Zerofill sections — accumulate `final_vaddr` only (vmsize
///    follows the file region in `__DATA` per Mach-O convention),
///    likewise alignment-rounded; `final_file_offset` stays at 0
///    sentinel. Returned region sizes include the alignment padding.
///
/// TLV descriptor sections (`S_THREAD_LOCAL_VARIABLES`) flow through
/// the file-storage path — the descriptors themselves have file
/// payload; SD-4c-prereq+c will dyld-bind their thunk slots
/// separately via the chained-fixups encoder.
///
/// `vaddr_start` is the virtual address of the first __DATA non-text
/// byte — typically `data_vmaddr + la_ptr_section_size` (= directly
/// after `__la_symbol_ptr`, which SD-2 reserves at the `__DATA`
/// segment's vmaddr start). `file_region_start_offset` mirrors that
/// in file-offset space.
/// Count of `section_64` headers the `__DATA,*` walk will emit —
/// `compute_data_section_layouts` run for its section list alone (the
/// cursor arguments only shape offsets, never membership, so zeros are
/// fine). The header/LC sizing in `compute_text_region_plan` needs
/// this BEFORE the data phase runs: `sizeofcmds` used to omit these
/// section headers entirely and lean on the page round-up's slack to
/// absorb them. When new data content pushed the real load-command
/// region past the page boundary, every byte of `__text` shifted while
/// every address stayed put — the entrypoint landed on the tail of the
/// preceding function's epilogue, popped argc off the start-up stack
/// into (fp, lr), and ret'd to PC=1. Reusing the layout walk (rather
/// than duplicating its membership predicate) keeps the count and the
/// emit permanently in lockstep.
pub fn count_data_section_64s(
    merged: &MergedArchives,
    member_keys: &[(usize, usize)],
) -> Result<u32, NonTextLayoutError> {
    let res = compute_data_section_layouts(merged, member_keys, 0, 0)?;
    Ok(res.merged.len() as u32)
}

pub fn compute_data_section_layouts(
    merged: &MergedArchives,
    member_keys: &[(usize, usize)],
    file_region_start_offset: u32,
    vaddr_start: u64,
) -> Result<DataSectionLayoutResult, NonTextLayoutError> {
    // Collect once so the two passes share an iterator-free view.
    let mut per_member_raw: Vec<Vec<MemberSectionInfo>> = Vec::with_capacity(member_keys.len());
    for &(a_idx, m_idx) in member_keys {
        let member = &merged.per_archive_members[a_idx][m_idx];
        let all = collect_member_sections(member).map_err(|err| NonTextLayoutError {
            archive_idx: a_idx,
            member_idx: m_idx,
            err,
        })?;
        per_member_raw.push(all);
    }

    // r503 — same-named sections across members lay out back to
    // back (the way ld merges input sections), so one `section_64`
    // covers each name instead of one per member: 23 headers on a
    // class program became 5. The per-member entries keep their
    // own placements; only the walk order changed, and every
    // consumer walks these tables in the same order. TLV descriptor
    // tables are regular file storage here; prereq+c hooks the
    // per-entry thunk dyld-bind separately.
    let mut per_member: Vec<Vec<DataSectionLayout>> = vec![Vec::new(); member_keys.len()];
    let mut merged: Vec<MergedDataSection> = Vec::new();
    let mut vaddr_cursor: u64 = vaddr_start;
    let mut file_cursor: u64 = u64::from(file_region_start_offset);
    for (segname, sectname) in section_name_groups(&per_member_raw, true) {
        let mut group: Option<MergedDataSection> = None;
        for (i, raw) in per_member_raw.iter().enumerate() {
            for sec in raw {
                if !is_data_segment(sec)
                    || is_no_file_storage(sec.flags)
                    || (sec.segname, sec.sectname) != (segname, sectname)
                {
                    continue;
                }
                let pad = round_up_pad(vaddr_cursor, sec.align);
                vaddr_cursor += pad;
                file_cursor += pad;
                per_member[i].push(DataSectionLayout {
                    section_index: sec.section_index,
                    sectname: sec.sectname,
                    segname: sec.segname,
                    member_internal_offset: sec.member_internal_offset,
                    size: sec.size,
                    flags: sec.flags,
                    has_file_storage: true,
                    final_file_offset: file_cursor as u32,
                    final_vaddr: vaddr_cursor,
                    member_addr: sec.member_addr,
                    align: sec.align,
                });
                let g = group.get_or_insert(MergedDataSection {
                    sectname,
                    segname,
                    flags: 0,
                    align: 0,
                    vaddr: vaddr_cursor,
                    size: 0,
                    file_offset: file_cursor as u32,
                    has_file_storage: true,
                });
                g.flags |= sec.flags;
                g.align = g.align.max(sec.align);
                vaddr_cursor += u64::from(sec.size);
                file_cursor += u64::from(sec.size);
                g.size = vaddr_cursor - g.vaddr;
            }
        }
        merged.extend(group);
    }
    let file_region_size = (file_cursor - u64::from(file_region_start_offset)) as u32;

    let zerofill_base = vaddr_start + u64::from(file_region_size);
    let mut zf_vaddr: u64 = zerofill_base;
    for (segname, sectname) in section_name_groups(&per_member_raw, false) {
        let mut group: Option<MergedDataSection> = None;
        for (i, raw) in per_member_raw.iter().enumerate() {
            for sec in raw {
                if !is_data_segment(sec)
                    || !is_no_file_storage(sec.flags)
                    || (sec.segname, sec.sectname) != (segname, sectname)
                {
                    continue;
                }
                zf_vaddr += round_up_pad(zf_vaddr, sec.align);
                per_member[i].push(DataSectionLayout {
                    section_index: sec.section_index,
                    sectname: sec.sectname,
                    segname: sec.segname,
                    member_internal_offset: 0, // S_ZEROFILL sets offset=0
                    size: sec.size,
                    flags: sec.flags,
                    has_file_storage: false,
                    final_file_offset: 0,
                    final_vaddr: zf_vaddr,
                    member_addr: sec.member_addr,
                    align: sec.align,
                });
                let g = group.get_or_insert(MergedDataSection {
                    sectname,
                    segname,
                    flags: 0,
                    align: 0,
                    vaddr: zf_vaddr,
                    size: 0,
                    file_offset: 0,
                    has_file_storage: false,
                });
                g.flags |= sec.flags;
                g.align = g.align.max(sec.align);
                zf_vaddr += u64::from(sec.size);
                g.size = zf_vaddr - g.vaddr;
            }
        }
        merged.extend(group);
    }
    let zerofill_vmsize = (zf_vaddr - zerofill_base) as u32;

    let _ = TEXT_VMADDR_BASE;

    Ok(DataSectionLayoutResult {
        per_member,
        merged,
        file_region_size,
        zerofill_vmsize,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::{AR_HEADER_SIZE, AR_MAGIC};
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

    /// Members with no `__DATA,*` sections produce empty per-member
    /// vecs and zero region sizes — drop-in safe when fix-c3 wires
    /// this into the existing layout (syscall-only path stays
    /// byte-identical).
    #[test]
    fn leaf_member_produces_empty_layout() {
        let foo = fn_leaf("_foo");
        let obj = torajs_obj::write_object(std::slice::from_ref(&foo));
        let archive = build_short_name_archive("a.o", &obj);
        let archives: Vec<std::borrow::Cow<'static, [u8]>> = vec![archive.into()];
        let merged = merge_archive_indexes(&archives).unwrap();
        let res = compute_data_section_layouts(&merged, &[(0usize, 0usize)], 0x2000, 0xDA00_0000)
            .unwrap();
        assert_eq!(res.per_member.len(), 1);
        assert!(res.per_member[0].is_empty());
        assert_eq!(res.file_region_size, 0);
        assert_eq!(res.zerofill_vmsize, 0);
    }

    /// Empty `member_keys` short-circuits — no member walks, both
    /// region sizes 0, `per_member` empty.
    #[test]
    fn empty_member_keys_short_circuits() {
        let merged = merge_archive_indexes(&[]).unwrap();
        let res = compute_data_section_layouts(&merged, &[], 0x2000, 0xDA00_0000).unwrap();
        assert!(res.per_member.is_empty());
        assert_eq!(res.file_region_size, 0);
        assert_eq!(res.zerofill_vmsize, 0);
    }

    /// Hand-crafted member with mixed-segment sections — verifies
    /// the segname `__DATA` admission rule, the file-storage vs
    /// zerofill split, and the cumulative file_offset + vaddr
    /// stride.
    #[test]
    fn data_segment_splits_file_storage_from_zerofill() {
        let obj = build_data_mix_member();
        let archive = build_short_name_archive("dm.o", &obj);
        let archives: Vec<std::borrow::Cow<'static, [u8]>> = vec![archive.into()];
        let merged = merge_archive_indexes(&archives).unwrap();
        let file_start = 0x4000u32;
        let vaddr_start = 0x0000_0001_0000_8000u64;
        let res =
            compute_data_section_layouts(&merged, &[(0usize, 0usize)], file_start, vaddr_start)
                .unwrap();
        assert_eq!(res.per_member.len(), 1);
        let layouts = &res.per_member[0];
        // Fixture: __text(T), __cstring(T), __data(D file), __common(D zf 2KB),
        // __bss(D zf 4KB).
        // pass 1 = [__data 16B]; pass 2 = [__common 2KB, __bss 4KB].
        assert_eq!(layouts.len(), 3, "1 file + 2 zerofill in __DATA");
        // pass 1 entry — __data.
        assert!(layouts[0].has_file_storage);
        assert_eq!(layouts[0].size, 16);
        assert_eq!(layouts[0].final_file_offset, file_start);
        assert_eq!(layouts[0].final_vaddr, vaddr_start);
        // pass 2 entries — __common then __bss, both zerofill.
        assert!(!layouts[1].has_file_storage);
        assert_eq!(layouts[1].size, 2048);
        assert_eq!(
            layouts[1].final_file_offset, 0,
            "zerofill file_offset sentinel"
        );
        assert_eq!(layouts[1].final_vaddr, vaddr_start + 16);
        assert!(!layouts[2].has_file_storage);
        assert_eq!(layouts[2].size, 4096);
        assert_eq!(layouts[2].final_vaddr, vaddr_start + 16 + 2048);
        // Region totals.
        assert_eq!(res.file_region_size, 16);
        assert_eq!(res.zerofill_vmsize, 2048 + 4096);
    }

    /// Two members each contributing __DATA,* sections — verifies
    /// the file region accumulates across member boundaries (file
    /// region of member 1 picks up where member 0 left off) and the
    /// zerofill region lives entirely after the file region in
    /// vmaddr space.
    #[test]
    fn two_members_cumulate_across_member_boundary() {
        let obj_a = build_data_only_member(b"_dA", 32, b"__data");
        let obj_b = build_data_only_member(b"_dB", 64, b"__data");
        let archive =
            build_short_name_archive("ab.o", &concat_members_into_archive_data(&[&obj_a, &obj_b]));
        // Use the helper-built archive directly with parse_archive
        // — `concat_members_into_archive_data` already includes ar
        // headers, so we wrap once.
        let archive_bytes = build_concat_archive(&[(b"a.o", &obj_a), (b"b.o", &obj_b)]);
        let archives: Vec<std::borrow::Cow<'static, [u8]>> = vec![archive_bytes.into()];
        let merged = merge_archive_indexes(&archives).unwrap();
        let file_start = 0x6000u32;
        let vaddr_start = 0x0000_0001_0001_0000u64;
        let res = compute_data_section_layouts(
            &merged,
            &[(0usize, 0usize), (0usize, 1usize)],
            file_start,
            vaddr_start,
        )
        .unwrap();
        assert_eq!(res.per_member.len(), 2);
        assert_eq!(res.per_member[0].len(), 1);
        assert_eq!(res.per_member[1].len(), 1);
        // member 0 __data: file_offset = file_start + 0, size 32.
        assert_eq!(res.per_member[0][0].final_file_offset, file_start);
        assert_eq!(res.per_member[0][0].size, 32);
        // member 1 __data: file_offset = file_start + 32.
        assert_eq!(res.per_member[1][0].final_file_offset, file_start + 32);
        assert_eq!(res.per_member[1][0].size, 64);
        assert_eq!(res.file_region_size, 32 + 64);
        assert_eq!(res.zerofill_vmsize, 0);
        // Suppress unused warning from the manually-built archive
        // helper we don't use in the assertions below.
        let _ = archive;
    }

    /// Sections that declare a `section_64.align` get their
    /// `final_vaddr` rounded up to `1 << align` — the swap-2i fix. The
    /// fixture packs __data(align 0, 8B) then __const(align 4 ⇒ 16B)
    /// then __common(align 3 ⇒ 8B zerofill); the linker must insert an
    /// 8-byte pad before __const and a 4-byte pad before __common, and
    /// fold both pads into the region sizes.
    #[test]
    fn sections_round_up_to_declared_alignment() {
        let obj = build_data_aligned_member();
        let archive = build_short_name_archive("al.o", &obj);
        let archives: Vec<std::borrow::Cow<'static, [u8]>> = vec![archive.into()];
        let merged = merge_archive_indexes(&archives).unwrap();
        // 16-aligned, file/vaddr congruent mod page.
        let file_start = 0x8000u32;
        let vaddr_start = 0x0000_0001_0000_8000u64;
        let res =
            compute_data_section_layouts(&merged, &[(0usize, 0usize)], file_start, vaddr_start)
                .unwrap();
        let layouts = &res.per_member[0];
        assert_eq!(layouts.len(), 3);
        // __data — align 0, lands at the base.
        assert_eq!(layouts[0].align, 0);
        assert_eq!(layouts[0].final_vaddr, vaddr_start);
        assert_eq!(layouts[0].final_file_offset, file_start);
        // __const — align 4 (16B): base+8 rounds up to base+16, with
        // file offset rounded by the same pad.
        assert_eq!(layouts[1].align, 4);
        assert_eq!(layouts[1].final_vaddr, vaddr_start + 16);
        assert_eq!(layouts[1].final_file_offset, file_start + 16);
        // file region spans base..base+16+4 = 20 bytes (incl. the pad).
        assert_eq!(res.file_region_size, 20);
        // __common — align 3 (8B) zerofill: zerofill base = base+20
        // (%8 == 4) rounds up to base+24.
        assert!(!layouts[2].has_file_storage);
        assert_eq!(layouts[2].align, 3);
        assert_eq!(layouts[2].final_vaddr, vaddr_start + 24);
        // zerofill vmsize spans base+20..base+24+16 = 20 bytes.
        assert_eq!(res.zerofill_vmsize, 20);
    }

    // ── fixture builders ────────────────────────────────────────────

    /// 5-section member: `__text` + `__cstring` (TEXT) + `__data`
    /// (DATA regular 16B) + `__common` (DATA zerofill 2KB) +
    /// `__bss` (DATA zerofill 4KB). Stresses the three filter
    /// dimensions fix-c2 cares about (segname, zerofill).
    fn build_data_mix_member() -> Vec<u8> {
        use torajs_obj::{
            LC_SEGMENT_64, MH_MAGIC_64, S_ATTR_PURE_INSTRUCTIONS, S_ATTR_SOME_INSTRUCTIONS,
            S_CSTRING_LITERALS, S_REGULAR, S_ZEROFILL, SECTION_64_SIZE, SEGMENT_COMMAND_64_SIZE,
        };
        let nsects: u32 = 5;
        let header_size: u32 = 32;
        let seg_cmd_size: u32 = SEGMENT_COMMAND_64_SIZE + nsects * SECTION_64_SIZE;
        let text: [u8; 4] = RET_BYTES;
        let cstr: [u8; 8] = *b"hi\0\0\0\0\0\0";
        let data_bytes: [u8; 16] = [0xAB; 16];
        let text_off = header_size + seg_cmd_size;
        let cstr_off = text_off + text.len() as u32;
        let data_off = cstr_off + cstr.len() as u32;
        let total = data_off + data_bytes.len() as u32;

        let mut out: Vec<u8> = Vec::new();
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

        let text_flags = S_REGULAR | S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS;
        write_section_64(
            &mut out, b"__text", b"__TEXT", 0, 4, text_off, text_flags, 0,
        );
        write_section_64(
            &mut out,
            b"__cstring",
            b"__TEXT",
            4,
            8,
            cstr_off,
            S_CSTRING_LITERALS,
            0,
        );
        write_section_64(
            &mut out, b"__data", b"__DATA", 12, 16, data_off, S_REGULAR, 0,
        );
        // Zerofill sections: offset = 0 per Mach-O spec.
        write_section_64(&mut out, b"__common", b"__DATA", 28, 2048, 0, S_ZEROFILL, 0);
        write_section_64(&mut out, b"__bss", b"__DATA", 2076, 4096, 0, S_ZEROFILL, 0);

        out.extend_from_slice(&text);
        out.extend_from_slice(&cstr);
        out.extend_from_slice(&data_bytes);
        out
    }

    /// 4-section member exercising `section_64.align`: `__text` (TEXT)
    /// + `__data` (DATA file 8B, align 0) + `__const` (DATA file 4B,
    /// align 4) + `__common` (DATA zerofill 16B, align 3). Drives the
    /// `sections_round_up_to_declared_alignment` test.
    fn build_data_aligned_member() -> Vec<u8> {
        use torajs_obj::{
            LC_SEGMENT_64, MH_MAGIC_64, S_ATTR_PURE_INSTRUCTIONS, S_ATTR_SOME_INSTRUCTIONS,
            S_REGULAR, S_ZEROFILL, SECTION_64_SIZE, SEGMENT_COMMAND_64_SIZE,
        };
        let nsects: u32 = 4;
        let header_size: u32 = 32;
        let seg_cmd_size: u32 = SEGMENT_COMMAND_64_SIZE + nsects * SECTION_64_SIZE;
        let text: [u8; 4] = RET_BYTES;
        let data_bytes: [u8; 8] = [0xAB; 8];
        let const_bytes: [u8; 4] = [0xCD; 4];
        let text_off = header_size + seg_cmd_size;
        let data_off = text_off + text.len() as u32;
        let const_off = data_off + data_bytes.len() as u32;
        let total = const_off + const_bytes.len() as u32;

        let mut out: Vec<u8> = Vec::new();
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

        let text_flags = S_REGULAR | S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS;
        write_section_64(
            &mut out, b"__text", b"__TEXT", 0, 4, text_off, text_flags, 0,
        );
        write_section_64(&mut out, b"__data", b"__DATA", 0, 8, data_off, S_REGULAR, 0);
        write_section_64(
            &mut out, b"__const", b"__DATA", 8, 4, const_off, S_REGULAR, 4,
        );
        // zerofill: offset = 0 per Mach-O spec, align 3 (8B).
        write_section_64(&mut out, b"__common", b"__DATA", 12, 16, 0, S_ZEROFILL, 3);

        out.extend_from_slice(&text);
        out.extend_from_slice(&data_bytes);
        out.extend_from_slice(&const_bytes);
        out
    }

    /// 2-section member: `__text` + a single `__DATA` regular
    /// section with the supplied size. Used for the two-member
    /// cumulative test.
    fn build_data_only_member(_name: &[u8], data_size: u32, sectname: &[u8]) -> Vec<u8> {
        use torajs_obj::{
            LC_SEGMENT_64, MH_MAGIC_64, S_ATTR_PURE_INSTRUCTIONS, S_ATTR_SOME_INSTRUCTIONS,
            S_REGULAR, SECTION_64_SIZE, SEGMENT_COMMAND_64_SIZE,
        };
        let nsects: u32 = 2;
        let header_size: u32 = 32;
        let seg_cmd_size: u32 = SEGMENT_COMMAND_64_SIZE + nsects * SECTION_64_SIZE;
        let text_off = header_size + seg_cmd_size;
        let data_off = text_off + 4;
        let total = data_off + data_size;

        let mut out: Vec<u8> = Vec::new();
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

        let text_flags = S_REGULAR | S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS;
        write_section_64(
            &mut out, b"__text", b"__TEXT", 0, 4, text_off, text_flags, 0,
        );
        write_section_64(
            &mut out,
            sectname,
            b"__DATA",
            4,
            u64::from(data_size),
            data_off,
            S_REGULAR,
            0,
        );

        out.extend_from_slice(&RET_BYTES);
        out.extend(std::iter::repeat(0xCD).take(data_size as usize));
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn write_section_64(
        out: &mut Vec<u8>,
        sectname: &[u8],
        segname: &[u8],
        addr: u64,
        size: u64,
        offset: u32,
        flags: u32,
        align: u32,
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
        out.extend_from_slice(&align.to_le_bytes()); // align
        out.extend_from_slice(&0u32.to_le_bytes()); // reloff
        out.extend_from_slice(&0u32.to_le_bytes()); // nreloc
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // reserved1
        out.extend_from_slice(&0u32.to_le_bytes()); // reserved2
        out.extend_from_slice(&0u32.to_le_bytes()); // reserved3
    }

    /// Build an ar archive containing N members, each with the
    /// supplied 16-char-max name. Used by `two_members_cumulate_*`.
    fn build_concat_archive(entries: &[(&[u8], &Vec<u8>)]) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(AR_MAGIC);
        for (name, data) in entries {
            let mut header = [b' '; AR_HEADER_SIZE];
            let name_str = std::str::from_utf8(name).unwrap();
            assert!(name_str.len() <= 16);
            header[..name_str.len()].copy_from_slice(name);
            header[16] = b'0';
            let size_str = format!("{}", data.len());
            header[48..48 + size_str.len()].copy_from_slice(size_str.as_bytes());
            header[58] = b'`';
            header[59] = b'\n';
            out.extend_from_slice(&header);
            out.extend_from_slice(data);
            if data.len() % 2 != 0 {
                out.push(0);
            }
        }
        out
    }

    /// Convenience wrapper kept around in case future tests want a
    /// flat byte concat without ar headers — currently unused, but
    /// keeps the test-suite shape symmetric with `non_text_layout`.
    fn concat_members_into_archive_data(members: &[&Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        for m in members {
            out.extend_from_slice(m);
        }
        out
    }
}
