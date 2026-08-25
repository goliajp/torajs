//! Merge multiple parsed `.a` archives into a single global
//! `name → (archive_idx, member_idx, n_value)` symbol lookup.
//!
//! S7-C1 — first half of wiring `LinkConfig.archives` into the link
//! path so the worklist closure (S7-C2: `compute_required_members`)
//! can resolve `__torajs_*` / `_libc_*` externs from `libtorajs_*.a`
//! instead of relying on a hand-filled `SymTable`.
//!
//! Pipeline: `archives` → per-archive `parse_archive` (S7-A) →
//! per-archive `build_archive_index` (S7-B) → first-archive-wins
//! merge into a unified `name → MergedSymbol` lookup (matches Apple
//! `ld64` search-order semantics).

use std::collections::{BTreeMap, BTreeSet};

use torajs_codegen::CompiledFunction;
use torajs_codegen::reloc::{CallTarget, RelocKind};
use torajs_obj::{LC_SYMTAB, MH_MAGIC_64, N_EXT, N_TYPE, N_UNDF};

use crate::archive::{
    ArMember, ArParseError, ArchiveParseError, MemberSymtabError, build_archive_index,
    parse_archive,
};

/// One entry in a `MergedArchives` global lookup. Tells the link
/// driver *which* archive owns the symbol, *which* `.o` member
/// inside that archive defines it, and the section-relative offset
/// inside that member's `__TEXT,__text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedSymbol {
    /// Index into the `&[Vec<u8>]` passed to `merge_archive_indexes`.
    pub archive_idx: usize,
    /// Index into `per_archive_members[archive_idx]`.
    pub member_idx: usize,
    /// `nlist_64.n_value` for the defining symbol — section-
    /// relative offset inside that member's `__TEXT,__text`.
    pub n_value: u64,
}

/// Result of merging every archive: the per-archive parsed
/// `ArMember` list (so a later resolver can pull member bytes)
/// plus the unified `name → MergedSymbol` lookup.
///
/// Borrows from the `&[Vec<u8>]` that was passed in — no copying
/// of archive payload bytes.
#[derive(Debug)]
pub struct MergedArchives<'a> {
    /// `per_archive_members[i]` = parsed members of `archives[i]`,
    /// in the same order `parse_archive` returned them. The
    /// resolver pairs `(archive_idx, member_idx)` with this Vec
    /// to grab the underlying `.o` bytes.
    pub per_archive_members: Vec<Vec<ArMember<'a>>>,
    /// Global defined-external lookup. First archive defining a
    /// given name wins (Apple `ld64` search-order semantics).
    /// Names borrow the archive bytes — the index build was
    /// ~17ms/case as owned Strings (phase-timing.md merge split).
    pub index: BTreeMap<&'a str, MergedSymbol>,
}

/// Failures `merge_archive_indexes` can report. Distinguishes the
/// outer `ar` framing layer (`parse_archive`) from the per-member
/// Mach-O layer (`build_archive_index`) so the caller can blame a
/// specific staticlib *and* a specific layer.
#[derive(Debug)]
pub enum ArchiveMergeError {
    /// Top-level archive bytes are malformed — bad magic, bad fmag,
    /// truncated member etc.
    Ar {
        archive_idx: usize,
        err: ArParseError,
    },
    /// A Mach-O member inside an archive is malformed.
    Member {
        archive_idx: usize,
        err: ArchiveParseError,
    },
}

/// Parse + symbol-index every archive in `archives`, then merge
/// the per-archive indexes into a single global lookup.
///
/// First-archive-wins on duplicate symbols — `archives[0]` is
/// searched before `archives[1]`. Matches Apple `ld64` static
/// library search order.
pub fn merge_archive_indexes<'a>(
    archives: &'a [std::borrow::Cow<'static, [u8]>],
) -> Result<MergedArchives<'a>, ArchiveMergeError> {
    let mut per_archive_members: Vec<Vec<ArMember<'_>>> = Vec::with_capacity(archives.len());
    let mut index: BTreeMap<&'a str, MergedSymbol> = BTreeMap::new();

    for (archive_idx, bytes) in archives.iter().enumerate() {
        let members = parse_archive(bytes.as_ref())
            .map_err(|err| ArchiveMergeError::Ar { archive_idx, err })?;
        let per_archive_index = build_archive_index(&members)
            .map_err(|err| ArchiveMergeError::Member { archive_idx, err })?;
        for (name, sym) in per_archive_index {
            index.entry(name).or_insert(MergedSymbol {
                archive_idx,
                member_idx: sym.member_idx,
                n_value: sym.n_value,
            });
        }
        per_archive_members.push(members);
    }

    Ok(MergedArchives {
        per_archive_members,
        index,
    })
}

// ---- S7-C2: worklist closure of required archive members ----

/// Set of `(archive_idx, member_idx)` pairs that must be
/// integrated into the final binary so every undefined extern
/// reachable from `user_funcs` resolves to a defined symbol.
///
/// `dyld_imports` records the orthogonal `name → lib_ordinal`
/// map the worklist classified as dyld-resolved (SD-1 libSystem +
/// SD-4b libcurl; see `dyld_syms::dyld_lib_for`). These bind
/// through `LC_LOAD_DYLIB` + a per-symbol `__TEXT,__stubs`
/// trampoline (SD-2) + chained-fixups entries (SD-3) rather than
/// being pulled from any archive — keeping them out of
/// `unresolved` is what unblocks `compute_required_members` for
/// real production `___torajs_*_alloc` reloc graphs. `lib_ordinal`
/// is the `dyld_chained_import.lib_ordinal` value (`1` =
/// libSystem, `2` = libcurl) so downstream encoders can pack the
/// right LC chain offset without re-classifying.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequiredMembers {
    pub members: BTreeSet<(usize, usize)>,
    pub dyld_imports: BTreeMap<String, u8>,
}

/// Failures from the worklist closure pass. Either some extern
/// names never matched a defined symbol anywhere (the user is
/// missing a `.a`), or a member's `.o` symtab was malformed
/// during the transitive walk.
#[derive(Debug)]
pub enum ArchiveLinkError {
    /// One or more externs referenced from `user_funcs` or from
    /// transitively-required members never resolved to any
    /// archive's defined-external set. Sorted + deduped so the
    /// CLI error message is reproducible.
    UnresolvedExterns { names: Vec<String> },
    /// A member's Mach-O symtab failed to parse mid-walk —
    /// blame this specific archive + member.
    Member {
        archive_idx: usize,
        member_idx: usize,
        err: MemberSymtabError,
    },
}

/// Compute the transitive closure of archive members needed to
/// resolve every extern reachable from `user_funcs`.
///
/// Algorithm: start with the set of extern names referenced by
/// `user_funcs`'s relocs (minus names that are defined by another
/// `user_funcs[i]`). For each name, look it up in
/// `merged.index`:
///
///   - hit  → mark that `(archive_idx, member_idx)` as required,
///            then parse the member's `LC_SYMTAB` and enqueue
///            every undefined extern it references in turn.
///   - miss → record as unresolved.
///
/// Visited-names set prevents revisiting symbols; visited-members
/// set prevents re-parsing the same `.o` for its undef list.
///
/// Returns `Err(UnresolvedExterns)` if any externs never landed;
/// `Err(Member)` if a transitively-pulled member's symtab was
/// malformed.
pub fn compute_required_members(
    user_funcs: &[CompiledFunction],
    merged: &MergedArchives<'_>,
    extra_defined_syms: &BTreeSet<String>,
) -> Result<RequiredMembers, ArchiveLinkError> {
    // SD-4c-prereq+e1 — `extra_defined_syms` carries link-defined syms
    // (e.g. `__torajs_str_lit_*` from `LinkConfig.strings`) so the
    // worklist doesn't flag them as `UnresolvedExterns` before emit.
    let defined_in_user: BTreeSet<&str> = user_funcs
        .iter()
        .map(|f| f.name.as_str())
        .chain(extra_defined_syms.iter().map(|s| s.as_str()))
        .collect();

    let mut required: BTreeSet<(usize, usize)> = BTreeSet::new();
    let mut visited_names: BTreeSet<String> = BTreeSet::new();
    let mut worklist: Vec<String> = Vec::new();
    let mut unresolved: BTreeSet<String> = BTreeSet::new();
    let mut dyld_imports: BTreeMap<String, u8> = BTreeMap::new();

    // Seed the worklist with every extern name referenced by a
    // user function reloc that isn't defined by another user
    // function. `CallSite::Func(fid)` doesn't name an extern —
    // it's a SSA-local FuncId resolved by the layout pass — so
    // skip it.
    for f in user_funcs {
        for r in &f.relocs {
            if let Some(name) = reloc_target_name(&r.kind)
                && !defined_in_user.contains(name)
            {
                worklist.push(name.to_string());
            }
        }
    }

    while let Some(name) = worklist.pop() {
        if !visited_names.insert(name.clone()) {
            continue;
        }
        if defined_in_user.contains(name.as_str()) {
            continue;
        }
        let Some(sym) = merged.index.get(name.as_str()) else {
            // SD-1 / SD-4b: dyld-class externs (libSystem or
            // libcurl) bind through `LC_LOAD_DYLIB`, not through
            // any archive's defined-extern set. Classify them out
            // of `unresolved` so the link can proceed; record the
            // resolving dylib's `lib_ordinal` so downstream
            // encoders know which LC_LOAD_DYLIB to bind against.
            if let Some(lib) = crate::dyld_syms::dyld_lib_for(&name) {
                dyld_imports.insert(name, lib.ordinal());
            } else {
                unresolved.insert(name);
            }
            continue;
        };
        let key = (sym.archive_idx, sym.member_idx);
        if !required.insert(key) {
            // Already pulled in — its undef externs were either
            // enqueued earlier (and either visited, queued, or
            // satisfied) or are about to be on this branch.
            continue;
        }
        let member = &merged.per_archive_members[sym.archive_idx][sym.member_idx];
        let undefs =
            parse_member_undef_externs(member).map_err(|err| ArchiveLinkError::Member {
                archive_idx: sym.archive_idx,
                member_idx: sym.member_idx,
                err,
            })?;
        for u in undefs {
            if !visited_names.contains(&u) {
                worklist.push(u);
            }
        }
    }

    if !unresolved.is_empty() {
        return Err(ArchiveLinkError::UnresolvedExterns {
            names: unresolved.into_iter().collect(),
        });
    }

    if std::env::var_os("TORAJS_LINK_CLOSURE_DIAG").is_some() {
        let names: Vec<&str> = required
            .iter()
            .map(|&(a, m)| merged.per_archive_members[a][m].name)
            .collect();
        eprintln!(
            "[closure-diag] members={} dyld={} names={names:?}",
            required.len(),
            dyld_imports.len(),
        );
    }
    Ok(RequiredMembers {
        members: required,
        dyld_imports,
    })
}

/// Name of the external symbol a `RelocKind` references, or
/// `None` for `CallSite::Func` (SSA-local, resolved by layout).
pub(crate) fn reloc_target_name(kind: &RelocKind) -> Option<&str> {
    match kind {
        RelocKind::CallSite {
            target: CallTarget::Func(_),
        } => None,
        RelocKind::CallSite {
            target: CallTarget::Extern(name),
        } => Some(name.as_str()),
        RelocKind::Page21 { target_sym }
        | RelocKind::PageOff12 { target_sym }
        | RelocKind::AbsPtr64 { target_sym } => Some(target_sym.as_str()),
    }
}

/// Walk one `.o` member's `LC_SYMTAB` and return every
/// `(N_UNDF | N_EXT)` symbol name — those are the *imports* the
/// member needs satisfied by something else in the link.
///
/// Mirror of `archive::parse_member_defined_externs` with the
/// filter flipped to `N_UNDF`. Defined externs (the things
/// `build_archive_index` collects) and undefined externs (the
/// things this returns) sit in the same `LC_SYMTAB`,
/// distinguished only by `n_type & N_TYPE`.
pub fn parse_member_undef_externs(member: &ArMember<'_>) -> Result<Vec<String>, MemberSymtabError> {
    let bytes = member.data;
    if bytes.len() < 32 {
        return Err(MemberSymtabError::TruncatedHeader);
    }
    let magic = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    if magic != MH_MAGIC_64 {
        return Ok(Vec::new());
    }
    let ncmds = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;

    let mut cursor = 32usize;
    let mut symtab: Option<(u32, u32, u32, u32)> = None;
    for _ in 0..ncmds {
        if bytes.len() < cursor + 8 {
            return Err(MemberSymtabError::TruncatedLoadCommand { offset: cursor });
        }
        let cmd = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap());
        let cmdsize =
            u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
        if cmdsize < 8 || bytes.len() < cursor + cmdsize {
            return Err(MemberSymtabError::TruncatedLoadCommand { offset: cursor });
        }
        if cmd == LC_SYMTAB {
            if cmdsize < 24 {
                return Err(MemberSymtabError::TruncatedSymtabCmd { offset: cursor });
            }
            let symoff = u32::from_le_bytes(bytes[cursor + 8..cursor + 12].try_into().unwrap());
            let nsyms = u32::from_le_bytes(bytes[cursor + 12..cursor + 16].try_into().unwrap());
            let stroff = u32::from_le_bytes(bytes[cursor + 16..cursor + 20].try_into().unwrap());
            let strsize = u32::from_le_bytes(bytes[cursor + 20..cursor + 24].try_into().unwrap());
            symtab = Some((symoff, nsyms, stroff, strsize));
        }
        cursor += cmdsize;
    }

    let Some((symoff, nsyms, stroff, strsize)) = symtab else {
        return Ok(Vec::new());
    };

    let symoff = symoff as usize;
    let stroff = stroff as usize;
    let strsize = strsize as usize;
    let nsyms = nsyms as usize;
    if bytes.len() < symoff + nsyms * 16 {
        return Err(MemberSymtabError::TruncatedSymtab {
            symoff,
            nsyms,
            file_size: bytes.len(),
        });
    }
    if bytes.len() < stroff + strsize {
        return Err(MemberSymtabError::TruncatedStrtab {
            stroff,
            strsize,
            file_size: bytes.len(),
        });
    }

    let strtab = &bytes[stroff..stroff + strsize];
    let mut out: Vec<String> = Vec::new();
    for i in 0..nsyms {
        let off = symoff + i * 16;
        let n_strx = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()) as usize;
        let n_type = bytes[off + 4];
        if (n_type & N_TYPE) != N_UNDF {
            continue;
        }
        if (n_type & N_EXT) == 0 {
            continue;
        }
        if n_strx >= strtab.len() {
            return Err(MemberSymtabError::BadStrx {
                sym_index: i,
                n_strx,
            });
        }
        let end_off = strtab[n_strx..]
            .iter()
            .position(|&b| b == 0)
            .map(|p| n_strx + p)
            .ok_or(MemberSymtabError::UnterminatedSymName {
                sym_index: i,
                n_strx,
            })?;
        let name_bytes = &strtab[n_strx..end_off];
        let name = std::str::from_utf8(name_bytes)
            .map_err(|_| MemberSymtabError::NameNotUtf8 { sym_index: i })?;
        out.push(name.to_string());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::{AR_HEADER_SIZE, AR_MAGIC};
    use torajs_codegen::compile_function;
    use torajs_core::ssa::{
        BinOp, Block, BlockId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId,
        ValueInfo,
    };
    use torajs_obj::write_object;

    /// Build a minimal valid `.a` containing a single member with
    /// a short name (no BSD long-name encoding). Mirrors the
    /// helper in `archive::tests` — kept private here so test
    /// modules stay self-contained.
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

    /// Build a `Function` named `name` that returns the constant
    /// `value` (i64). Compiles to a deterministic 4-instruction
    /// `mov / add / ret`-equivalent stream; gives the `.o` a
    /// single defined-external entry under `name`.
    fn make_ret_const_fn(name: &str, value: i64) -> Function {
        let v0 = ValueId(0);
        Function {
            name: name.into(),
            params: Vec::new(),
            ret: Type::I64,
            values: vec![ValueInfo {
                ty: Type::I64,
                name: Some("v0".into()),
            }],
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![Inst {
                    result: Some(v0),
                    kind: InstKind::BinOp(
                        BinOp::Add,
                        Operand::ConstI64(value),
                        Operand::ConstI64(0),
                    ),
                    origin: None,
                }],
                term: Terminator::Ret(Some(Operand::Value(v0))),
            }],
            current_origin: None,
        }
    }

    /// Wrap a single `Function` into a one-member `.a` archive by
    /// going through the real `codegen → obj → ar` chain so the
    /// member is a genuine Mach-O the parser must walk in full.
    fn wrap_fn_in_archive(member_name: &str, f: &Function) -> Vec<u8> {
        let cf = compile_function(f);
        let obj_bytes = write_object(std::slice::from_ref(&cf));
        build_short_name_archive(member_name, &obj_bytes)
    }

    /// Two `.o`s packed into one `.a`: useful for verifying
    /// member_idx is wired through correctly when there's more
    /// than one member.
    fn wrap_two_fns_in_archive(member_name: &str, a: &Function, b: &Function) -> Vec<u8> {
        let cfs = [compile_function(a), compile_function(b)];
        let obj_bytes = write_object(&cfs);
        build_short_name_archive(member_name, &obj_bytes)
    }

    #[test]
    fn empty_archives_merge_to_empty_index() {
        let merged = merge_archive_indexes(&[]).expect("empty input must merge cleanly");
        assert!(merged.per_archive_members.is_empty());
        assert!(merged.index.is_empty());
    }

    #[test]
    fn single_archive_round_trips() {
        let foo = make_ret_const_fn("_foo", 7);
        let archives: Vec<std::borrow::Cow<'static, [u8]>> =
            vec![wrap_fn_in_archive("a.o", &foo).into()];

        let merged = merge_archive_indexes(&archives).unwrap();
        assert_eq!(merged.per_archive_members.len(), 1);
        assert_eq!(merged.per_archive_members[0].len(), 1);
        assert_eq!(merged.index.len(), 1);
        let sym = &merged.index["_foo"];
        assert_eq!(sym.archive_idx, 0);
        assert_eq!(sym.member_idx, 0);
        assert_eq!(sym.n_value, 0);
    }

    /// Two archives, each defining one unique symbol plus a
    /// shared one — merged index must list every unique name
    /// once and resolve the duplicate to the *first* archive.
    #[test]
    fn merge_two_archives_first_wins_on_duplicate() {
        // archive_a: [a.o = {_foo, _shared}]
        let foo = make_ret_const_fn("_foo", 1);
        let shared_a = make_ret_const_fn("_shared", 2);
        let archive_a = wrap_two_fns_in_archive("a.o", &foo, &shared_a);

        // archive_b: [b.o = {_bar, _shared}]
        let bar = make_ret_const_fn("_bar", 3);
        let shared_b = make_ret_const_fn("_shared", 4);
        let archive_b = wrap_two_fns_in_archive("b.o", &bar, &shared_b);

        let archives: Vec<std::borrow::Cow<'static, [u8]>> =
            vec![archive_a.into(), archive_b.into()];
        let merged = merge_archive_indexes(&archives).unwrap();

        // Three distinct names — _shared collapses to one entry.
        assert_eq!(merged.index.len(), 3);

        // _foo lives only in archive_a.
        let foo_sym = &merged.index["_foo"];
        assert_eq!(foo_sym.archive_idx, 0);
        assert_eq!(foo_sym.member_idx, 0);

        // _bar lives only in archive_b.
        let bar_sym = &merged.index["_bar"];
        assert_eq!(bar_sym.archive_idx, 1);
        assert_eq!(bar_sym.member_idx, 0);

        // _shared is defined in both — first-wins picks archive_a.
        let shared_sym = &merged.index["_shared"];
        assert_eq!(shared_sym.archive_idx, 0);

        assert_eq!(merged.per_archive_members.len(), 2);
        assert_eq!(merged.per_archive_members[0].len(), 1);
        assert_eq!(merged.per_archive_members[1].len(), 1);
    }

    /// Real-world spot check: merge `libtorajs_syscall.a` +
    /// `libtorajs_str.a` (both shipped by `release-build.sh`) and
    /// confirm the combined index has at least as many entries
    /// as the larger of the two, with overlap deduped. Skips
    /// silently on a fresh checkout where neither file exists.
    #[test]
    fn real_libtorajs_two_archives_merge_clean() {
        let candidates = [
            (
                "target/aarch64-apple-darwin/release/libtorajs_syscall.a",
                "../../target/aarch64-apple-darwin/release/libtorajs_syscall.a",
            ),
            (
                "target/aarch64-apple-darwin/release/libtorajs_str.a",
                "../../target/aarch64-apple-darwin/release/libtorajs_str.a",
            ),
        ];
        let mut archives: Vec<std::borrow::Cow<'static, [u8]>> = Vec::new();
        for (a, b) in &candidates {
            match std::fs::read(a).or_else(|_| std::fs::read(b)) {
                Ok(bytes) => archives.push(bytes.into()),
                Err(_) => {
                    eprintln!("skip: real libtorajs_*.a not built yet");
                    return;
                }
            }
        }

        let merged = merge_archive_indexes(&archives).expect("real archives must merge");
        assert_eq!(merged.per_archive_members.len(), 2);

        // Per-archive sym counts via build_archive_index.
        let idx_a = build_archive_index(&merged.per_archive_members[0]).unwrap();
        let idx_b = build_archive_index(&merged.per_archive_members[1]).unwrap();
        assert!(!idx_a.is_empty(), "syscall archive must define externs");
        assert!(!idx_b.is_empty(), "str archive must define externs");

        let max_individual = std::cmp::max(idx_a.len(), idx_b.len());
        assert!(
            merged.index.len() >= max_individual,
            "merged index ({}) must cover at least the larger archive ({})",
            merged.index.len(),
            max_individual,
        );
        // And cannot exceed the unduped union.
        assert!(merged.index.len() <= idx_a.len() + idx_b.len());

        // Every entry in the merged index must point at a real
        // archive/member pair we recorded.
        for sym in merged.index.values() {
            assert!(sym.archive_idx < merged.per_archive_members.len());
            assert!(
                sym.member_idx < merged.per_archive_members[sym.archive_idx].len(),
                "sym.member_idx out of range",
            );
        }
    }

    /// Bad archive bytes must surface as a typed error with the
    /// offending archive's index — not a panic, not a silently
    /// shorter merged index.
    #[test]
    fn malformed_archive_surfaces_typed_error() {
        let bad: Vec<u8> = b"not an archive".to_vec();
        let err = merge_archive_indexes(&[bad.into()]).unwrap_err();
        match err {
            ArchiveMergeError::Ar { archive_idx, .. } => assert_eq!(archive_idx, 0),
            ArchiveMergeError::Member { .. } => panic!("expected Ar variant"),
        }
    }

    // ---- S7-C2 tests: compute_required_members worklist closure ----

    use torajs_codegen::CompiledFunction;
    use torajs_codegen::frame::FrameLayout;
    use torajs_codegen::reloc::{CallTarget, Reloc, RelocKind};

    /// Aarch64 `BL #0` placeholder — the displacement bytes are
    /// patched later by the resolver; the 0x94 high byte is what
    /// makes it a `BL`.
    const BL_PLACEHOLDER: [u8; 4] = [0x00, 0x00, 0x00, 0x94];
    /// Aarch64 `RET` (LE).
    const RET_BYTES: [u8; 4] = [0xC0, 0x03, 0x5F, 0xD6];

    /// Hand-build a `CompiledFunction` whose body is `bl <ext1>;
    /// bl <ext2>; ...; ret` so each `extern_names` entry produces a
    /// real `CallSite::Extern` reloc the worklist algorithm picks
    /// up. No SSA / register allocator needed.
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

    /// Hand-build a CF that returns a constant and emits no relocs
    /// — leaf function.
    fn fn_leaf(name: &str) -> CompiledFunction {
        CompiledFunction {
            name: name.into(),
            bytes: RET_BYTES.to_vec(),
            relocs: Vec::new(),
            frame: FrameLayout::leaf_no_spill(),
        }
    }

    /// Wrap an arbitrary list of `CompiledFunction`s into a `.o`
    /// then into a single-member `.a`.
    fn wrap_cfs_in_archive(member_name: &str, cfs: &[CompiledFunction]) -> Vec<u8> {
        let obj_bytes = torajs_obj::write_object(cfs);
        build_short_name_archive(member_name, &obj_bytes)
    }

    #[test]
    fn no_externs_returns_empty_required_set() {
        let user = vec![fn_leaf("_main")];
        let merged = merge_archive_indexes(&[]).unwrap();
        let req = compute_required_members(&user, &merged, &BTreeSet::new()).unwrap();
        assert!(req.members.is_empty());
    }

    /// User → archive `_foo` (no further externs from `_foo`) →
    /// closure pulls in exactly one member.
    #[test]
    fn single_hop_pulls_one_member() {
        let foo = fn_leaf("_foo");
        let archive = wrap_cfs_in_archive("a.o", std::slice::from_ref(&foo));
        let archives: Vec<std::borrow::Cow<'static, [u8]>> = vec![archive.into()];
        let merged = merge_archive_indexes(&archives).unwrap();

        let user = vec![fn_with_extern_calls("_main", &["_foo"])];
        let req = compute_required_members(&user, &merged, &BTreeSet::new()).unwrap();

        assert_eq!(req.members.len(), 1);
        assert!(req.members.contains(&(0, 0)));
    }

    /// `_main` → `_foo` → `_bar` → `_baz`, each in its own member.
    /// Closure must transitively pull in all three.
    #[test]
    fn transitive_closure_pulls_chain_of_members() {
        let foo = fn_with_extern_calls("_foo", &["_bar"]);
        let bar = fn_with_extern_calls("_bar", &["_baz"]);
        let baz = fn_leaf("_baz");
        let archive_a = wrap_cfs_in_archive("a.o", std::slice::from_ref(&foo));
        let archive_b = wrap_cfs_in_archive("b.o", std::slice::from_ref(&bar));
        let archive_c = wrap_cfs_in_archive("c.o", std::slice::from_ref(&baz));
        let archives: Vec<std::borrow::Cow<'static, [u8]>> =
            vec![archive_a.into(), archive_b.into(), archive_c.into()];
        let merged = merge_archive_indexes(&archives).unwrap();

        let user = vec![fn_with_extern_calls("_main", &["_foo"])];
        let req = compute_required_members(&user, &merged, &BTreeSet::new()).unwrap();

        assert_eq!(req.members.len(), 3);
        assert!(req.members.contains(&(0, 0)));
        assert!(req.members.contains(&(1, 0)));
        assert!(req.members.contains(&(2, 0)));
    }

    /// Cyclic dependency: `_foo → _bar → _foo`. Closure must
    /// terminate (visited-set short-circuits) and pull both.
    #[test]
    fn cyclic_extern_dep_terminates() {
        let foo = fn_with_extern_calls("_foo", &["_bar"]);
        let bar = fn_with_extern_calls("_bar", &["_foo"]);
        let archive_a = wrap_cfs_in_archive("a.o", std::slice::from_ref(&foo));
        let archive_b = wrap_cfs_in_archive("b.o", std::slice::from_ref(&bar));
        let archives: Vec<std::borrow::Cow<'static, [u8]>> =
            vec![archive_a.into(), archive_b.into()];
        let merged = merge_archive_indexes(&archives).unwrap();

        let user = vec![fn_with_extern_calls("_main", &["_foo"])];
        let req = compute_required_members(&user, &merged, &BTreeSet::new()).unwrap();

        assert_eq!(req.members.len(), 2);
    }

    /// Duplicate references to the same extern from a single user
    /// function — closure dedupes to one member.
    #[test]
    fn duplicate_extern_calls_dedupe_to_one_member() {
        let foo = fn_leaf("_foo");
        let archive = wrap_cfs_in_archive("a.o", std::slice::from_ref(&foo));
        let archives: Vec<std::borrow::Cow<'static, [u8]>> = vec![archive.into()];
        let merged = merge_archive_indexes(&archives).unwrap();

        let user = vec![fn_with_extern_calls("_main", &["_foo", "_foo", "_foo"])];
        let req = compute_required_members(&user, &merged, &BTreeSet::new()).unwrap();
        assert_eq!(req.members.len(), 1);
    }

    /// Extern that no archive defines surfaces as an
    /// `UnresolvedExterns` error listing every missing name in
    /// sorted/deduped order — the CLI message must be reproducible.
    #[test]
    fn unresolved_extern_returns_typed_error() {
        let merged = merge_archive_indexes(&[]).unwrap();
        let user = vec![fn_with_extern_calls("_main", &["_qux", "_zap", "_qux"])];
        let err = compute_required_members(&user, &merged, &BTreeSet::new()).unwrap_err();
        match err {
            ArchiveLinkError::UnresolvedExterns { names } => {
                assert_eq!(names, vec!["_qux".to_string(), "_zap".to_string()]);
            }
            other => panic!("expected UnresolvedExterns, got {other:?}"),
        }
    }

    /// SD-1 — a `_malloc` reference that no archive defines is
    /// classified as libSystem-resolved (binds via dyld) instead
    /// of being flagged as unresolved.
    #[test]
    fn libsystem_externs_route_into_dyld_imports_not_unresolved() {
        let merged = merge_archive_indexes(&[]).unwrap();
        let user = vec![fn_with_extern_calls(
            "_main",
            &["_malloc", "_pthread_create", "_free"],
        )];
        let req = compute_required_members(&user, &merged, &BTreeSet::new()).unwrap();
        assert!(req.members.is_empty(), "no archive member should be pulled");
        assert_eq!(req.dyld_imports.len(), 3);
        assert!(req.dyld_imports.contains_key("_malloc"));
        assert!(req.dyld_imports.contains_key("_pthread_create"));
        assert!(req.dyld_imports.contains_key("_free"));
    }

    /// SD-1 — a mix of `_malloc` (dyld) and `_qux` (truly missing)
    /// must keep the libSystem name out of `unresolved` while still
    /// reporting `_qux` as unresolved.
    #[test]
    fn libsystem_and_truly_missing_externs_split_correctly() {
        let merged = merge_archive_indexes(&[]).unwrap();
        let user = vec![fn_with_extern_calls("_main", &["_malloc", "_qux"])];
        let err = compute_required_members(&user, &merged, &BTreeSet::new()).unwrap_err();
        match err {
            ArchiveLinkError::UnresolvedExterns { names } => {
                assert_eq!(names, vec!["_qux".to_string()]);
            }
            other => panic!("expected UnresolvedExterns (_qux only), got {other:?}"),
        }
    }

    /// SD-4b — `_curl_easy_init` is now classified as libcurl
    /// (ordinal 2), so the worklist routes it into `dyld_imports`
    /// alongside the libSystem (ordinal 1) symbols instead of
    /// surfacing it as unresolved. dyld binds it at load time
    /// through the second `LC_LOAD_DYLIB`.
    #[test]
    fn libcurl_externs_route_into_dyld_imports_with_ordinal_2() {
        let merged = merge_archive_indexes(&[]).unwrap();
        let user = vec![fn_with_extern_calls(
            "_main",
            &["_curl_easy_init", "_malloc"],
        )];
        let req = compute_required_members(&user, &merged, &BTreeSet::new()).unwrap();
        assert_eq!(req.dyld_imports.len(), 2);
        assert_eq!(req.dyld_imports.get("_curl_easy_init").copied(), Some(2));
        assert_eq!(req.dyld_imports.get("_malloc").copied(), Some(1));
    }

    /// An extern reference that resolves to *another* user
    /// function (i.e. SSA-local extern by name) doesn't need an
    /// archive member.
    #[test]
    fn extern_resolved_by_another_user_func_pulls_no_member() {
        let user = vec![
            fn_with_extern_calls("_main", &["_helper"]),
            fn_leaf("_helper"),
        ];
        let merged = merge_archive_indexes(&[]).unwrap();
        let req = compute_required_members(&user, &merged, &BTreeSet::new()).unwrap();
        assert!(req.members.is_empty());
    }

    /// `parse_member_undef_externs` round-trips through `write_object`
    /// — a function with two extern call sites surfaces both names
    /// from the wrapped `.o`'s `LC_SYMTAB`.
    #[test]
    fn parse_member_undef_externs_round_trips() {
        let caller = fn_with_extern_calls("_caller", &["_a", "_b"]);
        let obj = torajs_obj::write_object(std::slice::from_ref(&caller));
        let archive = build_short_name_archive("m.o", &obj);
        let members = parse_archive(&archive).unwrap();
        assert_eq!(members.len(), 1);
        let undefs = parse_member_undef_externs(&members[0]).unwrap();
        assert!(undefs.contains(&"_a".to_string()));
        assert!(undefs.contains(&"_b".to_string()));
        // `_caller` itself is *defined* in the member; must not
        // appear in the undef list.
        assert!(!undefs.contains(&"_caller".to_string()));
    }
}
