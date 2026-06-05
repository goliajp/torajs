//! Archive-aware layout pass — S7-C3.
//!
//! Extends the base `exec.rs::compute_layout` with the ability to
//! integrate `.o` members pulled from `LinkConfig.archives`. Each
//! required member's `__TEXT,__text` payload concatenates after
//! the user functions in the final image, and every defined-extern
//! symbol the member exports flattens into the binary's
//! `LC_SYMTAB` alongside the user function names.
//!
//! S7-C3 covers layout only — the actual emit pass + cross-member
//! reloc patch lands in S7-C4. This file's `ArchiveLayout` is the
//! data structure the S7-C4 emit pass will consume.
//!
//! Map from `compute_archive_layout`'s phases to S7-C2 pieces:
//!
//! ```text
//!   cfg.archives ───► merge_archive_indexes  (S7-C1)
//!                              │
//!                              ▼
//!                       MergedArchives
//!                              │
//!   cfg.funcs ───────► compute_required_members  (S7-C2)
//!                              │
//!                              ▼
//!                      RequiredMembers
//!                              │
//!                  (this file: parse member text size + defs)
//!                              │
//!                              ▼
//!                      ArchiveLayout
//! ```
//!
//! `ArchiveLayout` extends `ExecLayout` with three Vecs indexed
//! by `member_keys` enumeration order — `member_text_sizes`,
//! `member_vaddrs`, and `member_defined_syms`. Indices line up
//! 1:1 across the three so the S7-C4 emit pass walks them as
//! parallel arrays.

use torajs_obj::{
    LC_SEGMENT_64, MH_MAGIC_64, MachHeader64, NLIST_64_SIZE, SECTION_64_SIZE,
    SEGMENT_COMMAND_64_SIZE, SYMTAB_COMMAND_SIZE,
};

use crate::archive::{ArMember, MemberSymtabError, parse_member_defined_externs};
use crate::archives_merge::{
    ArchiveLinkError, ArchiveMergeError, compute_required_members, merge_archive_indexes,
};
use crate::exec::LinkConfig;
use crate::lc::{
    APPLE_SILICON_PAGE_SIZE, BUILD_VERSION_CMDSIZE, DYSYMTAB_CMDSIZE, LINKEDIT_DATA_CMDSIZE,
    LOAD_DYLINKER_CMDSIZE, MAIN_CMDSIZE, TEXT_VMADDR_BASE,
};
use crate::member_apply::MemberRelocApplyError;
use crate::sign::adhoc_codesign_blob_size;

/// Mach-O segment names + section names referenced by the parser.
const TEXT_SEGNAME: &[u8] = b"__TEXT";
const TEXT_SECTNAME: &[u8] = b"__text";

/// Per-archive-member computed data the S7-C4 emit pass needs.
/// `defined_syms` carries `(name, n_value)` exactly as the member's
/// `LC_SYMTAB` records them — `n_value` is section-relative inside
/// the member's `__text`.
#[derive(Debug, Clone)]
pub struct MemberLayout {
    /// `(archive_idx, member_idx)` into `cfg.archives` /
    /// `MergedArchives::per_archive_members`.
    pub key: (usize, usize),
    /// Byte length of the member's `__TEXT,__text` payload.
    pub text_size: u32,
    /// Final virtual address where the member's `__text` lands.
    pub vaddr: u64,
    /// File offset within `__TEXT,__text` of the member's payload
    /// (i.e. text_file_offset + member's offset inside the
    /// concatenated payload).
    pub file_offset: u32,
    /// File offset of the member's `__text` *inside the `.o`* —
    /// where to slice the member bytes from. From the member's
    /// `LC_SEGMENT_64.__TEXT.section[0].offset` field.
    pub member_text_offset_in_member: u32,
    /// Defined-external symbols the member exports. The S7-C4
    /// emit pass writes each as `Nlist64::defined_extern(strx,
    /// 1, vaddr + n_value)`.
    pub defined_syms: Vec<(String, u64)>,
}

/// Layout decisions for an archive-aware link — superset of
/// `ExecLayout` with per-member integration data.
#[derive(Debug, Clone)]
pub struct ArchiveLayout {
    /// File offset where the `__text` payload begins.
    pub text_file_offset: u32,
    /// Total `__text` size = sum(user_funcs.bytes.len()) +
    /// sum(member_layouts.text_size).
    pub text_size: u32,
    pub text_vmaddr: u64,
    pub text_vmsize: u64,
    pub linkedit_file_offset: u32,
    pub linkedit_vmaddr: u64,
    pub linkedit_vmsize: u64,
    pub symoff: u32,
    pub stroff: u32,
    pub total_size: u32,
    /// `cfg.funcs.len() + sum(member.defined_syms.len())`.
    pub nsyms: u32,
    pub strsize: u32,
    /// Final vaddr of the entry symbol — `LC_MAIN.entry_off` will
    /// store `entry_file_offset` for kernel-side image base
    /// resolution.
    pub entry_file_offset: u32,
    /// Final vaddr per user function, same indexing as `cfg.funcs`.
    pub fn_vaddrs: Vec<u64>,
    /// Per integrated member, in sorted `(archive_idx, member_idx)`
    /// order. Parallel to `member_defined_syms` etc.
    pub member_layouts: Vec<MemberLayout>,
    pub codesign_dataoff: u32,
    pub codesign_datasize: u32,
}

/// Failures `compute_archive_layout` can report.
#[derive(Debug)]
pub enum ArchiveLayoutError {
    /// Top-level archive bytes were malformed.
    Merge(ArchiveMergeError),
    /// User code references externs that no archive defines.
    Link(ArchiveLinkError),
    /// A pulled member's Mach-O `__TEXT,__text` section couldn't
    /// be parsed.
    MemberText {
        archive_idx: usize,
        member_idx: usize,
        err: MemberTextError,
    },
    /// A pulled member's symtab walk failed during defined-extern
    /// enumeration (different code path than the worklist's undef
    /// enumeration in S7-C2).
    MemberSymtab {
        archive_idx: usize,
        member_idx: usize,
        err: MemberSymtabError,
    },
    /// `cfg.entry` doesn't match any user function name. (The
    /// entry symbol must be user-defined; integrated members are
    /// callable but can't be the entry.)
    EntryNotInUserFuncs { entry: String },
    /// Member-internal reloc decode/patch failed during the S7-C5
    /// pass — wire format broken or sym table doesn't cover an
    /// extern the member references.
    MemberReloc {
        archive_idx: usize,
        member_idx: usize,
        err: MemberRelocApplyError,
    },
}

/// Failures `parse_member_text_section` can report.
#[derive(Debug, PartialEq, Eq)]
pub enum MemberTextError {
    /// Member is too small to be a Mach-O 64.
    TruncatedHeader,
    /// A load command's cmdsize would extend past the buffer or
    /// is smaller than the minimum 8-byte header.
    TruncatedLoadCommand { offset: usize },
    /// `LC_SEGMENT_64` advertised more sections than fit.
    TruncatedSegmentSections { offset: usize },
    /// No `__TEXT,__text` section was found. A `.o` codegen-emit
    /// won't ship without one; if it happens the member isn't a
    /// torajs-obj output.
    NoTextSection,
}

/// Parse a `.o` member's `__TEXT,__text` section. Returns
/// `(text_offset_in_member, text_size)` where `text_offset_in_member`
/// is the byte offset inside the member where the payload starts
/// (= the `section_64.offset` field) and `text_size` is the
/// payload length (= the `section_64.size` field).
///
/// The caller pairs `text_offset_in_member` with
/// `ArMember.data[text_offset_in_member..+ text_size]` to slice
/// out the member's `__text` payload bytes.
pub fn parse_member_text_section(member: &ArMember<'_>) -> Result<(u32, u32), MemberTextError> {
    let bytes = member.data;
    if bytes.len() < 32 {
        return Err(MemberTextError::TruncatedHeader);
    }
    let magic = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    if magic != MH_MAGIC_64 {
        return Err(MemberTextError::TruncatedHeader);
    }
    let ncmds = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;

    let mut cursor = 32usize;
    for _ in 0..ncmds {
        if bytes.len() < cursor + 8 {
            return Err(MemberTextError::TruncatedLoadCommand { offset: cursor });
        }
        let cmd = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap());
        let cmdsize =
            u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
        if cmdsize < 8 || bytes.len() < cursor + cmdsize {
            return Err(MemberTextError::TruncatedLoadCommand { offset: cursor });
        }
        if cmd == LC_SEGMENT_64 {
            // segname @ cursor+8 .. cursor+24 (16 byte)
            // nsects @ cursor+64 .. cursor+68 (u32)
            if cmdsize < SEGMENT_COMMAND_64_SIZE as usize {
                return Err(MemberTextError::TruncatedLoadCommand { offset: cursor });
            }
            let segname = name16_eq(&bytes[cursor + 8..cursor + 24], TEXT_SEGNAME);
            if !segname {
                cursor += cmdsize;
                continue;
            }
            let nsects =
                u32::from_le_bytes(bytes[cursor + 64..cursor + 68].try_into().unwrap()) as usize;
            let sections_start = cursor + SEGMENT_COMMAND_64_SIZE as usize;
            let need = sections_start + nsects * SECTION_64_SIZE as usize;
            if bytes.len() < need || cmdsize < need - cursor {
                return Err(MemberTextError::TruncatedSegmentSections { offset: cursor });
            }
            for i in 0..nsects {
                let sec = sections_start + i * SECTION_64_SIZE as usize;
                // sectname @ sec+0..16, segname @ sec+16..32
                if !name16_eq(&bytes[sec..sec + 16], TEXT_SECTNAME) {
                    continue;
                }
                let size = u64::from_le_bytes(bytes[sec + 40..sec + 48].try_into().unwrap());
                let offset = u32::from_le_bytes(bytes[sec + 48..sec + 52].try_into().unwrap());
                return Ok((offset, size as u32));
            }
        }
        cursor += cmdsize;
    }
    Err(MemberTextError::NoTextSection)
}

/// 16-byte name-field comparison: returns true iff `slot`'s
/// leading bytes (up to NUL / space / end-of-slot) match `name`.
fn name16_eq(slot: &[u8], name: &[u8]) -> bool {
    if slot.len() != 16 {
        return false;
    }
    let mut end = slot.len();
    while end > 0 && (slot[end - 1] == 0 || slot[end - 1] == b' ') {
        end -= 1;
    }
    &slot[..end] == name
}

fn round_up_to(value: u64, align: u64) -> u64 {
    debug_assert!(align.is_power_of_two());
    (value + align - 1) & !(align - 1)
}

/// Compute the file + virtual layout for a `LinkConfig` whose
/// `archives` field is populated. The fast path when
/// `cfg.archives.is_empty()` behaves like `exec.rs::compute_layout`
/// — returns the same numbers wrapped in `ArchiveLayout` with an
/// empty `member_layouts`.
pub fn compute_archive_layout(cfg: &LinkConfig) -> Result<ArchiveLayout, ArchiveLayoutError> {
    let merged = merge_archive_indexes(&cfg.archives).map_err(ArchiveLayoutError::Merge)?;
    let required =
        compute_required_members(&cfg.funcs, &merged).map_err(ArchiveLayoutError::Link)?;

    // Sort member keys for reproducible layout.
    let mut member_keys: Vec<(usize, usize)> = required.members.iter().copied().collect();
    member_keys.sort();

    // Phase 2: per-member __text size + defined symbols.
    let mut member_text_sizes: Vec<u32> = Vec::with_capacity(member_keys.len());
    let mut member_text_offsets: Vec<u32> = Vec::with_capacity(member_keys.len());
    let mut member_defined_syms: Vec<Vec<(String, u64)>> = Vec::with_capacity(member_keys.len());
    for &(a_idx, m_idx) in &member_keys {
        let member = &merged.per_archive_members[a_idx][m_idx];
        let (text_off, text_size) =
            parse_member_text_section(member).map_err(|err| ArchiveLayoutError::MemberText {
                archive_idx: a_idx,
                member_idx: m_idx,
                err,
            })?;
        member_text_offsets.push(text_off);
        member_text_sizes.push(text_size);
        let defs = parse_member_defined_externs(member).map_err(|err| {
            ArchiveLayoutError::MemberSymtab {
                archive_idx: a_idx,
                member_idx: m_idx,
                err,
            }
        })?;
        member_defined_syms.push(defs);
    }

    // Entry must be a user function — integrated members can be
    // called but aren't the entry point.
    let entry_idx = cfg
        .funcs
        .iter()
        .position(|f| f.name == cfg.entry)
        .ok_or_else(|| ArchiveLayoutError::EntryNotInUserFuncs {
            entry: cfg.entry.clone(),
        })?;

    // Phase 3: layout (mirrors exec.rs::compute_layout with
    // text_size + nsyms + strsize extended by the integrated
    // members).
    let sizeofcmds = (SEGMENT_COMMAND_64_SIZE * 3)
        + SECTION_64_SIZE
        + LOAD_DYLINKER_CMDSIZE
        + BUILD_VERSION_CMDSIZE
        + MAIN_CMDSIZE
        + SYMTAB_COMMAND_SIZE
        + DYSYMTAB_CMDSIZE
        + LINKEDIT_DATA_CMDSIZE;
    let header_plus_lc = (MachHeader64::SIZE as u32) + sizeofcmds;
    let text_file_offset = round_up_to(u64::from(header_plus_lc), APPLE_SILICON_PAGE_SIZE) as u32;

    let user_text_size: u32 = cfg.funcs.iter().map(|f| f.bytes.len() as u32).sum();
    let members_text_total: u32 = member_text_sizes.iter().copied().sum();
    let text_size = user_text_size + members_text_total;
    let text_vmsize = round_up_to(
        u64::from(text_file_offset) + u64::from(text_size),
        APPLE_SILICON_PAGE_SIZE,
    );
    let linkedit_file_offset = text_vmsize as u32;
    let linkedit_vmaddr = TEXT_VMADDR_BASE + text_vmsize;

    // Per-user-func vaddrs.
    let mut fn_vaddrs: Vec<u64> = Vec::with_capacity(cfg.funcs.len());
    let mut running: u32 = 0;
    for f in &cfg.funcs {
        debug_assert_eq!(f.bytes.len() % 4, 0, "user fn bytes must be 4-aligned");
        fn_vaddrs.push(TEXT_VMADDR_BASE + u64::from(text_file_offset) + u64::from(running));
        running += f.bytes.len() as u32;
    }
    let entry_file_offset = text_file_offset
        + (fn_vaddrs[entry_idx] - TEXT_VMADDR_BASE - u64::from(text_file_offset)) as u32;

    // Per-member layout pinning — each member's __text follows the
    // previous member's, cumulative-summed past the user funcs.
    let mut member_layouts: Vec<MemberLayout> = Vec::with_capacity(member_keys.len());
    let mut cumulative: u32 = running;
    for (i, &key) in member_keys.iter().enumerate() {
        let text_size_i = member_text_sizes[i];
        member_layouts.push(MemberLayout {
            key,
            text_size: text_size_i,
            file_offset: text_file_offset + cumulative,
            vaddr: TEXT_VMADDR_BASE + u64::from(text_file_offset) + u64::from(cumulative),
            member_text_offset_in_member: member_text_offsets[i],
            defined_syms: std::mem::take(&mut member_defined_syms[i]),
        });
        cumulative += text_size_i;
    }

    // nsyms = user fn count + sum(member defined syms).
    let user_nsyms = cfg.funcs.len() as u32;
    let member_nsyms: u32 = member_layouts
        .iter()
        .map(|m| m.defined_syms.len() as u32)
        .sum();
    let nsyms = user_nsyms + member_nsyms;

    let nlist_size = nsyms * NLIST_64_SIZE;
    let symoff = linkedit_file_offset;
    let stroff = symoff + nlist_size;

    // strtab: "\0" sentinel + user fn names + member defined-sym names.
    let mut strsize: u32 = 1;
    for f in &cfg.funcs {
        strsize += f.name.len() as u32 + 1;
    }
    for m in &member_layouts {
        for (name, _) in &m.defined_syms {
            strsize += name.len() as u32 + 1;
        }
    }

    let codesign_dataoff = stroff + strsize;
    let codesign_datasize = adhoc_codesign_blob_size(codesign_dataoff, &cfg.codesign_ident);
    let linkedit_data_size = nlist_size + strsize + codesign_datasize;
    let linkedit_vmsize = round_up_to(u64::from(linkedit_data_size), APPLE_SILICON_PAGE_SIZE);
    let total_size = linkedit_file_offset + linkedit_data_size;

    Ok(ArchiveLayout {
        text_file_offset,
        text_size,
        text_vmaddr: TEXT_VMADDR_BASE,
        text_vmsize,
        linkedit_file_offset,
        linkedit_vmaddr,
        linkedit_vmsize,
        symoff,
        stroff,
        total_size,
        nsyms,
        strsize,
        entry_file_offset,
        fn_vaddrs,
        member_layouts,
        codesign_dataoff,
        codesign_datasize,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::{AR_HEADER_SIZE, AR_MAGIC};
    use crate::resolve::SymTable;
    use torajs_codegen::CompiledFunction;
    use torajs_codegen::frame::FrameLayout;
    use torajs_codegen::reloc::{CallTarget, Reloc, RelocKind};

    const BL_PLACEHOLDER: [u8; 4] = [0x00, 0x00, 0x00, 0x94];
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

    fn fn_with_extern_calls(name: &str, extern_names: &[&str]) -> CompiledFunction {
        let mut bytes: Vec<u8> = Vec::new();
        let mut relocs: Vec<Reloc> = Vec::new();
        for ext in extern_names {
            let byte_offset = bytes.len() as u32;
            bytes.extend_from_slice(&BL_PLACEHOLDER);
            relocs.push(Reloc {
                byte_offset,
                kind: RelocKind::CallSite {
                    target: CallTarget::Extern((*ext).into()),
                },
            });
        }
        bytes.extend_from_slice(&RET_BYTES);
        CompiledFunction {
            name: name.into(),
            bytes,
            relocs,
            frame: FrameLayout::leaf_no_spill(),
        }
    }

    fn fn_leaf(name: &str) -> CompiledFunction {
        CompiledFunction {
            name: name.into(),
            bytes: RET_BYTES.to_vec(),
            relocs: Vec::new(),
            frame: FrameLayout::leaf_no_spill(),
        }
    }

    /// Parsing a synthetic `.o` should surface the `__TEXT,__text`
    /// size as the sum of the wrapped fn's byte length.
    #[test]
    fn parse_member_text_section_round_trips() {
        let foo = fn_leaf("_foo");
        let obj = torajs_obj::write_object(std::slice::from_ref(&foo));
        let archive = build_short_name_archive("a.o", &obj);
        let members = crate::archive::parse_archive(&archive).unwrap();
        let (offset, size) = parse_member_text_section(&members[0]).unwrap();
        assert!(offset >= 32, "text offset must be past Mach-O header");
        assert_eq!(size as usize, foo.bytes.len());
    }

    /// Pure-user-funcs layout (empty `cfg.archives`) must produce
    /// numbers byte-identical to `exec.rs::compute_layout` for the
    /// shared fields. Validates the wrapper doesn't perturb the
    /// baseline.
    #[test]
    fn empty_archives_layout_matches_exec_baseline() {
        let main = fn_leaf("_main");
        let cfg = LinkConfig {
            funcs: vec![main],
            entry: "_main".into(),
            sym_table: SymTable::new(),
            codesign_ident: "tora".into(),
            archives: Vec::new(),
        };
        let layout = compute_archive_layout(&cfg).unwrap();
        let base = crate::exec::compute_layout(&cfg);
        assert_eq!(layout.text_file_offset, base.text_file_offset);
        assert_eq!(layout.text_size, base.text_size);
        assert_eq!(layout.text_vmsize, base.text_vmsize);
        assert_eq!(layout.linkedit_file_offset, base.linkedit_file_offset);
        assert_eq!(layout.nsyms, base.nsyms);
        assert_eq!(layout.strsize, base.strsize);
        assert_eq!(layout.total_size, base.total_size);
        assert_eq!(layout.fn_vaddrs, base.fn_vaddrs);
        assert!(layout.member_layouts.is_empty());
    }

    /// `_main` calls `_foo` extern; archive contains `_foo`.
    /// Layout must:
    ///   - place `_main` at TEXT_VMADDR_BASE + text_file_offset
    ///   - place the integrated member directly after `_main`
    ///   - report a `text_size` covering both user fn + member
    ///   - count the member's defined-extern in `nsyms`
    ///   - include `_foo` in the strtab size budget
    #[test]
    fn layout_includes_integrated_member() {
        let foo = fn_leaf("_foo");
        let archive =
            build_short_name_archive("a.o", &torajs_obj::write_object(std::slice::from_ref(&foo)));
        let main = fn_with_extern_calls("_main", &["_foo"]);
        let cfg = LinkConfig {
            funcs: vec![main.clone()],
            entry: "_main".into(),
            sym_table: SymTable::new(),
            codesign_ident: "tora".into(),
            archives: vec![archive],
        };
        let layout = compute_archive_layout(&cfg).unwrap();

        assert_eq!(layout.member_layouts.len(), 1);
        let m = &layout.member_layouts[0];
        assert_eq!(m.key, (0, 0));
        assert_eq!(m.text_size as usize, foo.bytes.len());

        // _main lands at the page boundary; the member's __text
        // lands right after _main's bytes.
        assert_eq!(
            layout.fn_vaddrs[0],
            TEXT_VMADDR_BASE + u64::from(layout.text_file_offset),
        );
        assert_eq!(m.vaddr, layout.fn_vaddrs[0] + main.bytes.len() as u64);
        assert_eq!(
            m.file_offset,
            layout.text_file_offset + main.bytes.len() as u32,
        );

        // text_size = user fn + member fn.
        assert_eq!(
            layout.text_size as usize,
            main.bytes.len() + foo.bytes.len(),
        );

        // nsyms = 1 (_main) + 1 (_foo) = 2.
        assert_eq!(layout.nsyms, 2);
        // strsize covers "\0" + "_main\0" + "_foo\0" = 1 + 6 + 5 = 12.
        assert_eq!(layout.strsize, 1 + (main.name.len() as u32 + 1) + 5);

        // member_defined_syms contains _foo at n_value 0 (foo is
        // the only fn in its .o so it lands at section-offset 0).
        assert_eq!(m.defined_syms.len(), 1);
        assert_eq!(m.defined_syms[0].0, "_foo");
        assert_eq!(m.defined_syms[0].1, 0);
    }

    /// Transitive integration: `_main → _foo → _bar`, each in
    /// its own archive member. Layout must list both members in
    /// `member_layouts`, in sorted (archive_idx, member_idx)
    /// order, with cumulative vaddrs.
    #[test]
    fn layout_integrates_transitive_chain() {
        let foo = fn_with_extern_calls("_foo", &["_bar"]);
        let bar = fn_leaf("_bar");
        let archive_a =
            build_short_name_archive("a.o", &torajs_obj::write_object(std::slice::from_ref(&foo)));
        let archive_b =
            build_short_name_archive("b.o", &torajs_obj::write_object(std::slice::from_ref(&bar)));
        let main = fn_with_extern_calls("_main", &["_foo"]);
        let cfg = LinkConfig {
            funcs: vec![main.clone()],
            entry: "_main".into(),
            sym_table: SymTable::new(),
            codesign_ident: "tora".into(),
            archives: vec![archive_a, archive_b],
        };
        let layout = compute_archive_layout(&cfg).unwrap();

        assert_eq!(layout.member_layouts.len(), 2);
        // Members sorted by (archive_idx, member_idx).
        assert_eq!(layout.member_layouts[0].key, (0, 0));
        assert_eq!(layout.member_layouts[1].key, (1, 0));

        // Cumulative vaddrs: main → foo → bar.
        let main_start = layout.fn_vaddrs[0];
        assert_eq!(
            layout.member_layouts[0].vaddr,
            main_start + main.bytes.len() as u64,
        );
        assert_eq!(
            layout.member_layouts[1].vaddr,
            layout.member_layouts[0].vaddr + layout.member_layouts[0].text_size as u64,
        );
    }

    /// Unresolved extern bubbles up as `ArchiveLayoutError::Link`
    /// (typed) rather than panicking — production callers can
    /// surface the missing-library message.
    #[test]
    fn unresolved_extern_returns_typed_error() {
        let main = fn_with_extern_calls("_main", &["_missing"]);
        let cfg = LinkConfig {
            funcs: vec![main],
            entry: "_main".into(),
            sym_table: SymTable::new(),
            codesign_ident: "tora".into(),
            archives: Vec::new(),
        };
        let err = compute_archive_layout(&cfg).unwrap_err();
        match err {
            ArchiveLayoutError::Link(ArchiveLinkError::UnresolvedExterns { names }) => {
                assert_eq!(names, vec!["_missing".to_string()]);
            }
            other => panic!("expected Link::UnresolvedExterns, got {other:?}"),
        }
    }

    /// Entry symbol must be a user function. Pointing it at a
    /// symbol that only exists in an archive surfaces a typed
    /// error.
    #[test]
    fn entry_must_be_user_func() {
        let foo = fn_leaf("_foo");
        let archive =
            build_short_name_archive("a.o", &torajs_obj::write_object(std::slice::from_ref(&foo)));
        let main = fn_with_extern_calls("_main", &["_foo"]);
        let cfg = LinkConfig {
            funcs: vec![main],
            entry: "_foo".into(), // wrong — _foo isn't a user fn
            sym_table: SymTable::new(),
            codesign_ident: "tora".into(),
            archives: vec![archive],
        };
        let err = compute_archive_layout(&cfg).unwrap_err();
        assert!(matches!(
            err,
            ArchiveLayoutError::EntryNotInUserFuncs { entry } if entry == "_foo"
        ));
    }
}
