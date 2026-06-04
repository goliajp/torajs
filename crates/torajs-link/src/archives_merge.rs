//! Merge multiple parsed `.a` archives into a single global
//! `name → (archive_idx, member_idx, n_value)` symbol lookup.
//!
//! S7-C1 — first half of wiring `LinkConfig.archives` into the
//! link path. `link_to_exec` previously only knew about user-
//! supplied `CompiledFunction`s plus a hand-filled `SymTable` of
//! external vaddrs; production swap (#9) needs to resolve
//! `__torajs_*` / `_libc_*` externs from `libtorajs_*.a` instead.
//!
//! Conceptual model:
//!
//! ```text
//!   archives: [libtorajs_syscall.a, libtorajs_str.a, ...]
//!     │
//!     │  parse_archive  (S7-A)
//!     ▼
//!   per_archive_members: Vec<Vec<ArMember>>
//!     │
//!     │  build_archive_index  (S7-B) on each archive
//!     ▼
//!   per_archive_index: BTreeMap<String, ArchiveSymbol>
//!     │
//!     │  merge with first-archive-wins on duplicates
//!     ▼
//!   index: BTreeMap<String, MergedSymbol>
//! ```
//!
//! First-archive-wins matches Apple `ld64`'s search-order
//! convention: when the user lists `libfoo.a libbar.a` and both
//! define `_baz`, `libfoo.a`'s copy is the one linked in. Same
//! convention here so downstream behaviour matches the C
//! toolchain we're replacing.

use std::collections::BTreeMap;

use crate::archive::{
    ArMember, ArParseError, ArchiveParseError, build_archive_index, parse_archive,
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
    pub index: BTreeMap<String, MergedSymbol>,
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
pub fn merge_archive_indexes(
    archives: &[Vec<u8>],
) -> Result<MergedArchives<'_>, ArchiveMergeError> {
    let mut per_archive_members: Vec<Vec<ArMember<'_>>> = Vec::with_capacity(archives.len());
    let mut index: BTreeMap<String, MergedSymbol> = BTreeMap::new();

    for (archive_idx, bytes) in archives.iter().enumerate() {
        let members = parse_archive(bytes.as_slice())
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
        let archives = vec![wrap_fn_in_archive("a.o", &foo)];

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

        let archives = vec![archive_a, archive_b];
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
        let mut archives: Vec<Vec<u8>> = Vec::new();
        for (a, b) in &candidates {
            match std::fs::read(a).or_else(|_| std::fs::read(b)) {
                Ok(bytes) => archives.push(bytes),
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
        let err = merge_archive_indexes(&[bad]).unwrap_err();
        match err {
            ArchiveMergeError::Ar { archive_idx, .. } => assert_eq!(archive_idx, 0),
            ArchiveMergeError::Member { .. } => panic!("expected Ar variant"),
        }
    }
}
