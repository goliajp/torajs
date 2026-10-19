//! S2 dead-strip blade 2b — archive repack driver (RFC
//! 20260824-s2-dead-strip, plan C).
//!
//! Runs the full pre-pass over the caller's merged archives: member
//! closure → atom reachability → per-member `.o` rewrite. The caller
//! (`link_to_exec_with_archives`, gated on `TORAJS_LINK_DEADSTRIP`)
//! swaps the rewritten members into that merged index
//! ([`swap_in_stripped`]) and runs the unchanged pipeline against
//! them; no archive is re-serialized or re-parsed.

use std::collections::BTreeMap;

use torajs_obj::{
    N_EXT, N_TYPE, N_UNDF, NLIST_64_SIZE, RELOCATION_INFO_SIZE, S_4BYTE_LITERALS, S_8BYTE_LITERALS,
    S_16BYTE_LITERALS, S_CSTRING_LITERALS, S_REGULAR, SECTION_TYPE,
};

use torajs_codegen::CompiledFunction;

use crate::archive_link_extra_syms::collect_extra_defined_syms;
use crate::archives_merge::{MergedArchives, compute_required_members_cached};
use crate::dead_strip_reach::{MemberReach, SectAtoms, compute_reachability};
use crate::dead_strip_reach_cache::ReachCache;
use crate::dead_strip_rewrite::{DeadRowPlacement, StrippedMember, scan_load_commands};
use crate::dead_strip_rewrite_sections::{SectionCut, strip_sections};
use crate::exec::{LinkConfig, UserClassLayoutEntry, UserFnNameEntry};
use crate::phase_timer::PhaseTimer;

/// nlist rows (ascending indices) whose atom the pre-pass dropped,
/// keyed by `(archive_idx, member_idx)`. The rows stay defined
/// (clamped to the section base) because resolution still leans on
/// them, so `LC_SYMTAB` needs this list to leave them out.
pub type DeadRows = BTreeMap<(usize, usize), Vec<u32>>;

/// Rewritten member bodies by `(archive_idx, member_idx)`.
pub(crate) type StrippedMembers = BTreeMap<(usize, usize), Vec<u8>>;

/// What the pre-pass rewrote: each side is `None` (or empty) when
/// that input is unchanged (the caller then links the original).
pub(crate) struct StripOutput {
    pub(crate) members: StrippedMembers,
    /// r499 — user fns with elided drain sites patched
    /// (`dead_strip_elide`); the members above were stripped
    /// against these.
    pub(crate) funcs: Option<Vec<CompiledFunction>>,
    /// r502 — the class-layout table with the twin column dropped
    /// (`dead_strip_elide`); the fns above were re-rooted against it.
    pub(crate) class_layouts: Option<Vec<UserClassLayoutEntry>>,
    /// r502 — the fn-name registry minus the rows of fns that
    /// re-rooting stripped.
    pub(crate) fn_name_globals: Option<Vec<UserFnNameEntry>>,
    /// Filled only when member symbols are listed
    /// (`!cfg.strip_member_symbols`); the stripped table never needs it.
    pub(crate) dead_rows: DeadRows,
}

/// Run the dead-strip pre-pass over `merged` (`cfg.archives`): judge
/// the conditional call sites first, then strip against the patched
/// fns.
pub(crate) fn strip_archives<'a>(
    cfg: &LinkConfig,
    merged: &MergedArchives<'a>,
    cache: &mut ReachCache<'a>,
) -> Result<StripOutput, String> {
    let mut t = PhaseTimer::start(2);
    let extra = collect_extra_defined_syms(cfg);
    let judged = crate::dead_strip_elide::judge_and_patch(cfg, merged, &extra, cache)?;
    t.tick("elide_judge_and_patch");
    let (assumed, probed) = judged.map_or((None, None), |(a, c)| (Some(a), c));
    // the closure reads only the user fns: the patched ones when
    // the judgment changed any
    let funcs = assumed.as_ref().map_or(&cfg.funcs, |a| &a.funcs);
    let reach = match probed {
        Some(reach) => reach,
        None => {
            let required = compute_required_members_cached(funcs, merged, &extra, cache)
                .map_err(|e| format!("member closure: {e:?}"))?;
            // probes=None always: the CUT/CUT_IN what-ifs are diag-only
            // pricing probes and must never shape emitted bytes.
            t.tick("required_members");
            compute_reachability(funcs, merged, &required, &extra, false, None, cache)?
        }
    };
    t.tick("reachability");

    let mut members = StrippedMembers::new();
    let mut dead_rows = DeadRows::new();
    for (&key, r) in &reach.members {
        let m = &merged.member(key.0, key.1);
        if let Some(StrippedMember {
            bytes,
            dead_rows: mut dead,
        }) = strip_member(m.data, r, DeadRowPlacement::SectionBase)?
        {
            if !cfg.strip_member_symbols && !dead.is_empty() {
                dead.sort_unstable();
                dead_rows.insert(key, dead);
            }
            members.insert(key, bytes);
        }
    }
    t.tick("strip_members");
    let (funcs, class_layouts, fn_name_globals) = match assumed {
        Some(a) => (Some(a.funcs), a.class_layouts, a.fn_name_globals),
        None => (None, None, None),
    };
    Ok(StripOutput {
        members,
        funcs,
        class_layouts,
        fn_name_globals,
        dead_rows,
    })
}

/// Point `merged` at the rewritten member bodies. This is what
/// re-merging archives repacked with them would give: the rewrite
/// keeps every nlist row and its type (only `n_value` moves, and only
/// undef rows lose `N_EXT`), so every defined extern keeps its owner.
pub(crate) fn swap_in_stripped<'a>(merged: &mut MergedArchives<'a>, stripped: &'a StrippedMembers) {
    for (&key, bytes) in stripped {
        merged.swap_member_data(key, bytes);
    }
}

/// Which sections the rewriter may subset (S2 blade 3): regular
/// file-storage payloads plus the cstring / literal pools. Zerofill
/// has no bytes to drop; the TLV family flows through dedicated
/// descriptor / thunk layout passes (and is rooted by the closure
/// anyway); flag-rooted sections come back `all_live`.
fn strippable(sa: &SectAtoms) -> bool {
    if sa.no_file_storage || sa.all_live || sa.size == 0 {
        return false;
    }
    matches!(
        sa.flags & SECTION_TYPE,
        S_REGULAR | S_CSTRING_LITERALS | S_4BYTE_LITERALS | S_8BYTE_LITERALS | S_16BYTE_LITERALS
    ) && sa.live.iter().any(|&l| !l)
}

/// Rewrite one member against every eligible section's live atoms,
/// handed over section-relative (the rewrite shifts sections; offsets
/// within a section survive, absolute reach addresses do not).
pub(crate) fn strip_member(
    bytes: &[u8],
    reach: &MemberReach<'_>,
    dead_at: DeadRowPlacement,
) -> Result<Option<StrippedMember>, String> {
    let eligible: Vec<(u8, Vec<(u64, u64)>, &[bool])> = reach
        .sects
        .iter()
        .enumerate()
        .filter(|(_, sa)| strippable(sa))
        .map(|(i, sa)| {
            let atoms_rel = sa.atoms.iter().map(|&(s, e)| (s - sa.addr, e - sa.addr));
            ((i + 1) as u8, atoms_rel.collect(), &sa.live[..])
        })
        .collect();
    let cuts: Vec<SectionCut<'_>> = eligible
        .iter()
        .map(|(ord, atoms_rel, live)| SectionCut {
            ord: *ord,
            atoms_rel,
            live,
        })
        .collect();
    let cut = strip_sections(bytes, &cuts, dead_at)?;
    // S2 blade 3 dead-undef cleanup: with the dead reloc sites gone,
    // clear N_EXT on undef nlist rows nothing references any more.
    let stripped = cut.is_some();
    let (mut out, dead_rows) = match cut {
        Some(m) => (m.bytes, m.dead_rows),
        None => (bytes.to_vec(), Vec::new()),
    };
    let neutered = neuter_dead_undefs(&mut out)?;
    if !stripped && neutered == 0 {
        return Ok(None);
    }
    Ok(Some(StrippedMember {
        bytes: out,
        dead_rows,
    }))
}

/// Clear `N_EXT` on undefined nlist rows that no surviving reloc
/// references. `parse_member_undef_externs` then stops reporting
/// them, so the downstream member closure over the stripped
/// archives pulls fewer members and derives fewer dyld binds.
/// Physical row layout is untouched — extern reloc indices stay
/// valid. Returns the number of rows neutered.
fn neuter_dead_undefs(bytes: &mut [u8]) -> Result<u32, String> {
    let (_, symtab_pos, _, sects) = scan_load_commands(bytes)?;
    let Some(sp) = symtab_pos else {
        return Ok(0);
    };
    let symoff = u32::from_le_bytes(bytes[sp + 8..sp + 12].try_into().unwrap()) as usize;
    let nsyms = u32::from_le_bytes(bytes[sp + 12..sp + 16].try_into().unwrap()) as usize;
    let mut referenced = vec![false; nsyms];
    let esz = RELOCATION_INFO_SIZE as usize;
    for s in &sects {
        if s.reloff == 0 {
            continue;
        }
        for i in 0..s.nreloc as usize {
            let e = s.reloff as usize + i * esz;
            let info = u32::from_le_bytes(bytes[e + 4..e + 8].try_into().unwrap());
            if (info >> 27) & 1 == 1 {
                let idx = (info & 0x00FF_FFFF) as usize;
                if idx < nsyms {
                    referenced[idx] = true;
                }
            }
        }
    }
    let mut n = 0;
    for (i, r) in referenced.iter().enumerate() {
        let p = symoff + i * NLIST_64_SIZE as usize;
        let t = bytes[p + 4];
        if t & N_TYPE == N_UNDF && t & N_EXT != 0 && !r {
            bytes[p + 4] = t & !N_EXT;
            n += 1;
        }
    }
    Ok(n)
}
