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
    MachHeader64, NLIST_64_SIZE, SECTION_64_SIZE, SEGMENT_COMMAND_64_SIZE, SYMTAB_COMMAND_SIZE,
};

use crate::archive::{MemberSymtabError, parse_member_defined_externs};
use crate::archives_merge::{
    ArchiveLinkError, ArchiveMergeError, compute_required_members, merge_archive_indexes,
};
use crate::chained_fixups::build_chained_fixups;
use crate::exec::LinkConfig;
use crate::lc::{
    APPLE_SILICON_PAGE_SIZE, BUILD_VERSION_CMDSIZE, DYSYMTAB_CMDSIZE, LINKEDIT_DATA_CMDSIZE,
    LOAD_DYLIB_LIBCURL_CMDSIZE, LOAD_DYLIB_LIBSYSTEM_CMDSIZE, LOAD_DYLINKER_CMDSIZE, MAIN_CMDSIZE,
    TEXT_VMADDR_BASE,
};
use crate::member_apply::MemberRelocApplyError;
pub use crate::member_text::{MemberTextError, parse_member_text_section};
use crate::sign::adhoc_codesign_blob_size;
use crate::stubs::{LA_PTR_SLOT_SIZE, STUB_SIZE};
use std::collections::BTreeMap;

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
    /// SD-1 / SD-4b — `name → lib_ordinal` map classified by
    /// `dyld_syms::dyld_lib_for` during the worklist closure.
    /// When non-empty, `archive_emit::emit_binary` adds an
    /// `LC_LOAD_DYLIB` per dylib whose ordinals appear (libSystem
    /// = 1, libcurl = 2) plus the SD-2 `__TEXT,__stubs` +
    /// `__DATA,__la_symbol_ptr` sections and SD-3
    /// `LC_DYLD_CHAINED_FIXUPS` blob. SD-2b wires `stub_vaddrs`
    /// into the effective sym table so user-fn / member relocs
    /// resolve against the trampoline addresses listed here.
    pub dyld_imports: BTreeMap<String, u8>,
    /// SD-2a — final image vaddr of the first byte of
    /// `__TEXT,__stubs`. `0` when `dyld_imports` is empty (the
    /// section isn't emitted in that case, so callers can rely on
    /// `dyld_imports.is_empty()` as the gate before reading this).
    pub stubs_section_vaddr: u64,
    /// On-disk / vmsize length of the `__TEXT,__stubs` section.
    /// Always `dyld_imports.len() * STUB_SIZE`; `0` for the
    /// empty case.
    pub stubs_section_size: u64,
    /// File offset of the first byte of `__TEXT,__stubs`.
    pub stubs_file_offset: u32,
    /// Final image vaddr of the first byte of
    /// `__DATA,__la_symbol_ptr`. `0` for the empty case.
    pub la_ptr_section_vaddr: u64,
    /// On-disk / vmsize length of the `__DATA,__la_symbol_ptr`
    /// section. Always `dyld_imports.len() * LA_PTR_SLOT_SIZE`;
    /// `0` for the empty case.
    pub la_ptr_section_size: u64,
    /// File offset of the first byte of `__DATA,__la_symbol_ptr`.
    pub la_ptr_file_offset: u32,
    /// On-disk / vmsize of the `__DATA` segment — page-aligned
    /// length covering `__la_symbol_ptr`. `0` for the empty case
    /// (no `__DATA` segment is emitted at all).
    pub data_vmsize: u64,
    /// Vaddr of the `__DATA` segment (= `la_ptr_section_vaddr`,
    /// since `__la_symbol_ptr` is the segment's only section and
    /// lives at its base). `0` for the empty case.
    pub data_vmaddr: u64,
    /// SD-2a — per-import stub vaddr. Same iteration order as
    /// `dyld_imports` (sorted by name, since both are `BTreeSet` /
    /// `BTreeMap`-derived). SD-2b inserts `(name → stub_vaddr)`
    /// into the effective sym table so user-fn reloc sites BL the
    /// trampoline instead of the raw libSystem symbol.
    pub stub_vaddrs: BTreeMap<String, u64>,
    /// SD-3 — fully assembled `LC_DYLD_CHAINED_FIXUPS` blob to
    /// land at `chained_fixups_dataoff` inside `__LINKEDIT`.
    /// Empty when `dyld_imports` is empty (no LC, no payload).
    pub chained_fixups_blob: Vec<u8>,
    /// File offset of the chained-fixups blob; `0` for the empty
    /// case.
    pub chained_fixups_dataoff: u32,
    /// On-disk size of the chained-fixups blob; `0` for the
    /// empty case.
    pub chained_fixups_datasize: u32,
    /// SD-3 — per-import 8-byte chain-link value to write into
    /// `__DATA,__la_symbol_ptr`. Replaces the SD-2a zero-fill so
    /// dyld can walk the chain at bind time. Same iteration
    /// order as `dyld_imports`.
    pub la_ptr_slot_values: Vec<u64>,
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
    // members, plus SD-2a __TEXT,__stubs + __DATA,__la_symbol_ptr
    // sections when dyld_imports is non-empty).
    let has_dyld = !required.dyld_imports.is_empty();
    // SD-4b — count LC_LOAD_DYLIB entries by dylib ordinal. Any
    // dyld-using binary always emits libSystem at ordinal 1 (so
    // dyld's ordinal numbering matches the chained-fixups encoder),
    // even when only libcurl symbols are referenced.
    let has_libcurl_lc = required.dyld_imports.values().any(|&o| o == 2);
    let load_dylib_size = if has_dyld {
        LOAD_DYLIB_LIBSYSTEM_CMDSIZE
            + if has_libcurl_lc {
                LOAD_DYLIB_LIBCURL_CMDSIZE
            } else {
                0
            }
    } else {
        0
    };
    // Segment count and section count grow when dyld_imports is
    // populated: __TEXT gains a second section (__stubs) and a
    // brand-new __DATA segment carries __la_symbol_ptr.
    //
    //   empty: PAGEZERO + TEXT (1 section) + LINKEDIT
    //          = 3 segments, 1 section
    //   has_dyld: PAGEZERO + TEXT (2 sections) + DATA (1) + LINKEDIT
    //             = 4 segments, 3 sections
    let (segment_count, section_count) = if has_dyld { (4, 3) } else { (3, 1) };
    // SD-3 adds LC_DYLD_CHAINED_FIXUPS when has_dyld — same
    // `linkedit_data_command` shape as LC_CODE_SIGNATURE (16 B).
    let chained_fixups_lc_size = if has_dyld { LINKEDIT_DATA_CMDSIZE } else { 0 };
    let sizeofcmds = (SEGMENT_COMMAND_64_SIZE * segment_count)
        + (SECTION_64_SIZE * section_count)
        + LOAD_DYLINKER_CMDSIZE
        + load_dylib_size
        + BUILD_VERSION_CMDSIZE
        + MAIN_CMDSIZE
        + SYMTAB_COMMAND_SIZE
        + DYSYMTAB_CMDSIZE
        + chained_fixups_lc_size
        + LINKEDIT_DATA_CMDSIZE;
    let header_plus_lc = (MachHeader64::SIZE as u32) + sizeofcmds;
    let text_file_offset = round_up_to(u64::from(header_plus_lc), APPLE_SILICON_PAGE_SIZE) as u32;

    let user_text_size: u32 = cfg.funcs.iter().map(|f| f.bytes.len() as u32).sum();
    let members_text_total: u32 = member_text_sizes.iter().copied().sum();
    let text_size = user_text_size + members_text_total;

    // __TEXT segment file region = __text + __stubs (when has_dyld).
    // Both sections live in the same segment so the segment's
    // vmsize covers everything from byte 0 of the file to the end
    // of __stubs, rounded up to the page boundary.
    let dyld_count = required.dyld_imports.len() as u64;
    let stubs_section_size = if has_dyld { dyld_count * STUB_SIZE } else { 0 };
    let la_ptr_section_size = if has_dyld {
        dyld_count * LA_PTR_SLOT_SIZE
    } else {
        0
    };
    let stubs_file_offset = text_file_offset + text_size;
    let text_segment_file_end = u64::from(stubs_file_offset) + stubs_section_size;
    let text_vmsize = round_up_to(text_segment_file_end, APPLE_SILICON_PAGE_SIZE);

    // __DATA segment lives directly after __TEXT in both vaddr and
    // file order (no other LC_SEGMENT_64s in between). Its vmsize
    // is page-aligned coverage of __la_symbol_ptr.
    let data_vmsize = if has_dyld {
        round_up_to(la_ptr_section_size, APPLE_SILICON_PAGE_SIZE)
    } else {
        0
    };
    let data_file_offset = if has_dyld { text_vmsize as u32 } else { 0 };
    let data_vmaddr = if has_dyld {
        TEXT_VMADDR_BASE + text_vmsize
    } else {
        0
    };

    let linkedit_file_offset = (text_vmsize + data_vmsize) as u32;
    let linkedit_vmaddr = TEXT_VMADDR_BASE + text_vmsize + data_vmsize;

    // Section vaddrs follow directly from segment vmaddrs +
    // section file offsets.
    let stubs_section_vaddr = if has_dyld {
        TEXT_VMADDR_BASE + u64::from(stubs_file_offset)
    } else {
        0
    };
    let la_ptr_section_vaddr = data_vmaddr;

    // Per-import stub vaddr stride is STUB_SIZE. BTreeMap iter is
    // sorted-by-name → stub_vaddrs key order is reproducible.
    let mut stub_vaddrs: BTreeMap<String, u64> = BTreeMap::new();
    if has_dyld {
        for (i, name) in required.dyld_imports.keys().enumerate() {
            stub_vaddrs.insert(name.clone(), stubs_section_vaddr + (i as u64) * STUB_SIZE);
        }
    }

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

    // SD-3 — build the chained-fixups blob now that the layout
    // numbers it depends on (text_vmsize → __DATA vmaddr offset
    // from image base) are settled. The blob lands inside
    // `__LINKEDIT` immediately after the string table and before
    // the codesign blob so the ad-hoc CodeDirectory hash covers
    // it. Empty `dyld_imports` skips the blob entirely.
    let (chained_fixups_blob, la_ptr_slot_values): (Vec<u8>, Vec<u64>) = if has_dyld {
        let built = build_chained_fixups(
            &required.dyld_imports,
            text_vmsize,
            0,
            segment_count,
            // __DATA is index 2 in PAGEZERO + TEXT + DATA + LINKEDIT
            // order.
            2,
        );
        (built.blob, built.slot_values)
    } else {
        (Vec::new(), Vec::new())
    };
    let chained_fixups_dataoff = if has_dyld { stroff + strsize } else { 0 };
    let chained_fixups_datasize = chained_fixups_blob.len() as u32;

    let codesign_dataoff = if has_dyld {
        chained_fixups_dataoff + chained_fixups_datasize
    } else {
        stroff + strsize
    };
    let codesign_datasize = adhoc_codesign_blob_size(codesign_dataoff, &cfg.codesign_ident);
    let linkedit_data_size = nlist_size + strsize + chained_fixups_datasize + codesign_datasize;
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
        dyld_imports: required.dyld_imports,
        stubs_section_vaddr,
        stubs_section_size,
        stubs_file_offset,
        la_ptr_section_vaddr,
        la_ptr_section_size,
        la_ptr_file_offset: data_file_offset,
        data_vmsize,
        data_vmaddr,
        stub_vaddrs,
        chained_fixups_blob,
        chained_fixups_dataoff,
        chained_fixups_datasize,
        la_ptr_slot_values,
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
        // SD-2a — empty dyld_imports leaves the new stubs/la_ptr
        // fields zero/empty so the rest of the emit path stays
        // byte-identical to SD-1.
        assert!(layout.dyld_imports.is_empty());
        assert_eq!(layout.stubs_section_vaddr, 0);
        assert_eq!(layout.stubs_section_size, 0);
        assert_eq!(layout.la_ptr_section_vaddr, 0);
        assert_eq!(layout.la_ptr_section_size, 0);
        assert_eq!(layout.data_vmsize, 0);
        assert_eq!(layout.data_vmaddr, 0);
        assert!(layout.stub_vaddrs.is_empty());
    }

    /// SD-2a — a user fn calling a libSystem-resolved extern
    /// (`_malloc`) must show up in `dyld_imports`, populate the
    /// `__TEXT,__stubs` + `__DATA,__la_symbol_ptr` section fields,
    /// and grow `text_vmsize` / `linkedit_file_offset` to make
    /// room for the new sections + segment.
    #[test]
    fn layout_populates_dyld_stubs_when_libsystem_referenced() {
        let main = fn_with_extern_calls("_main", &["_malloc"]);
        let cfg = LinkConfig {
            funcs: vec![main.clone()],
            entry: "_main".into(),
            sym_table: SymTable::new(),
            codesign_ident: "tora".into(),
            archives: Vec::new(),
        };
        let layout = compute_archive_layout(&cfg).unwrap();
        assert_eq!(layout.dyld_imports.len(), 1);
        assert!(layout.dyld_imports.contains_key("_malloc"));

        // Stubs section + slot section non-zero, sized to the
        // single import.
        assert_eq!(layout.stubs_section_size, super::STUB_SIZE);
        assert_eq!(layout.la_ptr_section_size, super::LA_PTR_SLOT_SIZE);

        // Stubs land right after the user __text.
        assert_eq!(
            u64::from(layout.stubs_file_offset),
            u64::from(layout.text_file_offset) + u64::from(layout.text_size),
        );
        assert_eq!(
            layout.stubs_section_vaddr,
            TEXT_VMADDR_BASE + u64::from(layout.stubs_file_offset),
        );

        // __DATA segment follows the page-aligned end of __TEXT.
        assert_eq!(layout.data_vmaddr, TEXT_VMADDR_BASE + layout.text_vmsize);
        assert_eq!(layout.la_ptr_section_vaddr, layout.data_vmaddr);
        assert!(layout.data_vmsize >= APPLE_SILICON_PAGE_SIZE);
        assert!(layout.data_vmsize % APPLE_SILICON_PAGE_SIZE == 0);

        // __LINKEDIT follows __DATA, not __TEXT.
        assert_eq!(
            u64::from(layout.linkedit_file_offset),
            layout.text_vmsize + layout.data_vmsize,
        );

        // stub_vaddrs map populated for SD-2b's effective sym
        // table extension.
        assert_eq!(layout.stub_vaddrs.len(), 1);
        assert_eq!(
            layout.stub_vaddrs.get("_malloc").copied(),
            Some(layout.stubs_section_vaddr),
        );
    }

    /// SD-2a — multiple imports stride correctly through both the
    /// `__stubs` section and the `__la_symbol_ptr` slot table.
    /// Both sections are emitted in `BTreeSet` (sorted-by-name)
    /// order so byte output is reproducible across runs.
    #[test]
    fn layout_strides_stub_vaddrs_per_import() {
        let main = fn_with_extern_calls("_main", &["_malloc", "_free", "_pthread_create"]);
        let cfg = LinkConfig {
            funcs: vec![main],
            entry: "_main".into(),
            sym_table: SymTable::new(),
            codesign_ident: "tora".into(),
            archives: Vec::new(),
        };
        let layout = compute_archive_layout(&cfg).unwrap();
        assert_eq!(layout.dyld_imports.len(), 3);
        assert_eq!(layout.stubs_section_size, 3 * super::STUB_SIZE);
        assert_eq!(layout.la_ptr_section_size, 3 * super::LA_PTR_SLOT_SIZE);
        // BTreeSet iter is sorted-by-name → _free < _malloc < _pthread_create.
        let names: Vec<&String> = layout.stub_vaddrs.keys().collect();
        assert_eq!(names, vec!["_free", "_malloc", "_pthread_create"]);
        let stride_zero = layout.stub_vaddrs["_free"];
        let stride_one = layout.stub_vaddrs["_malloc"];
        let stride_two = layout.stub_vaddrs["_pthread_create"];
        assert_eq!(stride_zero, layout.stubs_section_vaddr);
        assert_eq!(stride_one, stride_zero + super::STUB_SIZE);
        assert_eq!(stride_two, stride_zero + 2 * super::STUB_SIZE);
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
