//! The runtime image `tr build` and `tr run` link against, with every
//! atom no program can reach already removed. Built once, when `tr`
//! is compiled, so a link never reads, walks or strips those atoms.
//!
//! The roots are the names a program can reference
//! ([`torajs_rtimage::prune::nameable_by_program`]): every global a
//! torajs runtime member defines and every unmangled global. Their
//! atom-level closure is the reach a dead-stripped link computes, with
//! no call site judged dead, so it holds every atom any link keeps.
//! Each member that splits at its symbols (`MH_SUBSECTIONS_VIA_SYMBOLS`;
//! any other is kept whole) is rewritten to those atoms:
//!   - the rows of dropped atoms become anonymous locals on the gap
//!     they left: still an atom boundary, so a link's own strip of
//!     the member places its runs as it would on the original, but no
//!     name the index or a symbol listing carries;
//!   - an import only dropped atoms used stops being an import, so the
//!     image builder then leaves out the members nothing imports any
//!     more (std's backtrace symbolizer: gimli, object, addr2line,
//!     rustc_demangle).

use std::collections::BTreeMap;

use torajs_obj::{N_EXT, N_SECT, N_TYPE, NLIST_64_SIZE, RELOCATION_INFO_SIZE};
use torajs_rtimage::image::{BuildError, build_reachable, build_reachable_from_members};
use torajs_rtimage::prune::nameable_by_program;

use crate::archive_link_extra_syms::collect_extra_defined_syms;
use crate::archives_merge::{
    ArchiveLinkError, compute_required_members_cached, merge_archive_indexes,
};
use crate::dead_strip_reach::compute_reachability;
use crate::dead_strip_reach_build::splits_at_symbols;
use crate::dead_strip_reach_cache::ReachCache;
use crate::dead_strip_repack::strip_member;
use crate::dead_strip_rewrite::{DeadRowPlacement, SectHdr, scan_load_commands};
use crate::exec_user_entries::UserDataGlobalEntry;
use crate::rtlinked_build::{link_cfg, root_fn};

/// Why the stripped image could not be built.
#[derive(Debug)]
pub enum PrestripError {
    Image(BuildError),
    /// Merge, closure, reach or member rewrite failure.
    Link(String),
}

/// Build the runtime image from `archives` (link search order) with
/// every atom no program can reach removed.
pub fn prestripped_runtime_image(archives: &[&[u8]]) -> Result<Vec<u8>, PrestripError> {
    let image = build_reachable(archives).map_err(PrestripError::Image)?;
    let merged =
        merge_archive_indexes(&image).map_err(|e| PrestripError::Link(format!("{e:?}")))?;
    let roots: Vec<&str> = merged
        .index
        .iter()
        .filter(|(n, s)| {
            nameable_by_program(
                n,
                merged.per_archive_members[s.archive_idx][s.member_idx].name,
            )
        })
        .map(|(n, _)| *n)
        .collect();
    let mut cfg = link_cfg(&[], root_fn(&roots));
    let mut cache = ReachCache::default();
    // A name nothing defines (no archive member, no dylib the linker
    // binds) can only be the import of a member no program that links
    // pulls; the walk treats it as defined by the program and goes on.
    let (required, extra) = loop {
        let extra = collect_extra_defined_syms(&cfg);
        match compute_required_members_cached(&cfg.funcs, &merged, &extra, &mut cache) {
            Ok(required) => break (required, extra),
            Err(ArchiveLinkError::UnresolvedExterns { names }) if !names.is_empty() => {
                cfg.data_globals
                    .extend(names.into_iter().map(|sym| UserDataGlobalEntry {
                        sym,
                        size: 8,
                        align_log2: 3,
                    }));
            }
            Err(e) => return Err(PrestripError::Link(format!("member closure: {e:?}"))),
        }
    };
    let reach = compute_reachability(
        &cfg.funcs, &merged, &required, &extra, false, None, &mut cache,
    )
    .map_err(PrestripError::Link)?;

    let mut rewritten: BTreeMap<(usize, usize), Vec<u8>> = BTreeMap::new();
    for (&(a, m), r) in &reach.members {
        let member = &merged.per_archive_members[a][m];
        // an object that does not split at its symbols has nothing to
        // cut short of a whole section: it stays as it is
        if !splits_at_symbols(member.data) {
            continue;
        }
        // nameless section rows are how a link tells a pre-stripped
        // section apart (`dead_content_in_prestripped`)
        let (_, symtab_pos, _, _) = scan_load_commands(member.data).map_err(PrestripError::Link)?;
        if let Some(sp) = symtab_pos
            && nameless_row_sections(member.data, sp).next().is_some()
        {
            return Err(PrestripError::Link(format!(
                "{}: a nameless section row in the input",
                member.name
            )));
        }
        let Some(stripped) = strip_member(member.data, r, DeadRowPlacement::GapStart)
            .map_err(PrestripError::Link)?
        else {
            continue;
        };
        let mut bytes = stripped.bytes;
        anonymize_rows(&mut bytes, &stripped.dead_rows)
            .map_err(|e| PrestripError::Link(format!("{}: {e}", member.name)))?;
        rewritten.insert((a, m), bytes);
    }
    let mut members = merged.per_archive_members.clone();
    for (&(a, m), bytes) in &rewritten {
        members[a][m].data = bytes;
    }
    build_reachable_from_members(&members).map_err(PrestripError::Image)
}

/// Turn nlist `rows` into nameless locals: out of the merged index and
/// of every symbol listing, still an atom start where they sit.
pub(crate) fn anonymize_rows(bytes: &mut [u8], rows: &[u32]) -> Result<(), String> {
    if rows.is_empty() {
        return Ok(());
    }
    let (_, symtab_pos, _, _) = scan_load_commands(bytes)?;
    let sp = symtab_pos.ok_or("dead rows without LC_SYMTAB")?;
    let rd = |b: &[u8], o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as usize;
    let (symoff, stroff, strsize) = (rd(bytes, sp + 8), rd(bytes, sp + 16), rd(bytes, sp + 20));
    // any NUL in the string table starts an empty name
    let empty = bytes[stroff..stroff + strsize]
        .iter()
        .position(|&b| b == 0)
        .ok_or("string table without a NUL")? as u32;
    for &row in rows {
        let p = symoff + row as usize * NLIST_64_SIZE as usize;
        bytes[p..p + 4].copy_from_slice(&empty.to_le_bytes());
        bytes[p + 4] &= !N_EXT;
    }
    Ok(())
}

/// Whether a link must rewrite section `hdr` of member `bytes` even
/// though dropping its dead atoms buys no bytes. The pre-strip stands
/// in for the original member: where it rewrote a section (and left
/// nameless rows in it), stripping the original against the same
/// program would have bought bytes and left zeros where every dead
/// atom was. So a dead atom holding a non-zero byte or a reloc site
/// must not reach the binary as it is.
pub(crate) fn dead_content_in_prestripped(
    bytes: &[u8],
    symtab_pos: Option<usize>,
    hdr: &SectHdr,
    atoms_rel: &[(u64, u64)],
    live: &[bool],
) -> bool {
    let payload = &bytes[hdr.offset as usize..][..hdr.size as usize];
    let relocs =
        &bytes[hdr.reloff as usize..][..hdr.nreloc as usize * RELOCATION_INFO_SIZE as usize];
    let reloc_in = |s: u64, e: u64| {
        relocs
            .as_chunks::<{ RELOCATION_INFO_SIZE as usize }>()
            .0
            .iter()
            .any(|r| (s..e).contains(&u64::from(u32::from_le_bytes(r[0..4].try_into().unwrap()))))
    };
    let dead_content = atoms_rel.iter().zip(live).any(|(&(s, e), &l)| {
        !l && (payload[s as usize..e as usize].iter().any(|&b| b != 0) || reloc_in(s, e))
    });
    dead_content
        && symtab_pos.is_some_and(|sp| nameless_row_sections(bytes, sp).any(|n| n == hdr.ord))
}

/// The section ordinal of every nameless local row (what
/// [`anonymize_rows`] leaves) in the member whose `LC_SYMTAB` command
/// sits at `symtab_pos`.
fn nameless_row_sections(bytes: &[u8], symtab_pos: usize) -> impl Iterator<Item = u8> + '_ {
    let rd = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap()) as usize;
    let (symoff, nsyms, stroff) = (rd(symtab_pos + 8), rd(symtab_pos + 12), rd(symtab_pos + 16));
    (0..nsyms).filter_map(move |i| {
        let p = symoff + i * NLIST_64_SIZE as usize;
        let n_type = bytes[p + 4];
        (n_type & N_TYPE == N_SECT && n_type & N_EXT == 0 && bytes[stroff + rd(p)] == 0)
            .then_some(bytes[p + 5])
    })
}
