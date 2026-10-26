//! S2 dead-strip blade 2b — archive repack driver (RFC
//! 20260824-s2-dead-strip, plan C).
//!
//! Runs the full pre-pass over the caller's merged archives: member
//! closure → atom reachability → per-member cut. The caller
//! (`link_to_exec_with_archives`, gated on `TORAJS_LINK_DEADSTRIP`)
//! installs the cuts in that merged index ([`install_cuts`]), and the
//! rest of the link reads each cut member through its tables; no
//! member is written out or parsed again.

use std::collections::BTreeMap;

use torajs_obj::{
    S_4BYTE_LITERALS, S_8BYTE_LITERALS, S_16BYTE_LITERALS, S_CSTRING_LITERALS, S_REGULAR,
    SECTION_TYPE,
};

use torajs_codegen::CompiledFunction;

use crate::archive_link_extra_syms::collect_extra_defined_syms;
use crate::archives_merge::{MergedArchives, compute_required_members_cached};
use crate::dead_strip_cut::Cut;
use crate::dead_strip_reach::{MemberReach, SectAtoms, compute_reachability};
use crate::dead_strip_reach_cache::ReachCache;
use crate::dead_strip_rewrite::{DeadRowPlacement, StrippedMember};
use crate::dead_strip_rewrite_sections::SectionCut;
use crate::exec::{LinkConfig, UserClassLayoutEntry, UserFnNameEntry};
use crate::layout_types::ArchiveLayoutError;
use crate::phase_timer::PhaseTimer;

/// nlist rows (ascending indices) whose atom the pre-pass dropped,
/// keyed by `(archive_idx, member_idx)`. The rows stay defined
/// (clamped to the section base) because resolution still leans on
/// them, so `LC_SYMTAB` needs this list to leave them out.
pub type DeadRows = BTreeMap<(usize, usize), Vec<u32>>;

/// Cut members by `(archive_idx, member_idx)`.
pub(crate) type StrippedMembers = BTreeMap<(usize, usize), Cut>;

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

    let (members, dead_rows) = cut_members(merged, &reach.members, !cfg.strip_member_symbols)?;
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

/// Cut every member of `reach` against its live atoms, with the rows
/// each cut drops when `list_dead`.
fn cut_members(
    merged: &MergedArchives<'_>,
    reach: &BTreeMap<(usize, usize), MemberReach<'_>>,
    list_dead: bool,
) -> Result<(StrippedMembers, DeadRows), String> {
    let mut members = StrippedMembers::new();
    let mut dead_rows = DeadRows::new();
    for (&key, r) in reach {
        let data = merged.member(key.0, key.1).data;
        let cut = with_cuts(r, |cuts| {
            Cut::plan(data, cuts, DeadRowPlacement::SectionBase, true)
        })?;
        if let Some(cut) = cut {
            if list_dead {
                let dead = cut.dead_rows(data)?;
                if !dead.is_empty() {
                    dead_rows.insert(key, dead);
                }
            }
            members.insert(key, cut);
        }
    }
    Ok((members, dead_rows))
}

/// `cfg` with the user fns and tables the pre-pass rewrote (`None`:
/// unchanged). The other fields are cloned one by one: cloning the
/// whole config would copy the fns only to drop them.
pub(crate) fn rewritten_config(
    cfg: &LinkConfig,
    funcs: Option<Vec<CompiledFunction>>,
    class_layouts: Option<Vec<UserClassLayoutEntry>>,
    fn_name_globals: Option<Vec<UserFnNameEntry>>,
) -> LinkConfig {
    let LinkConfig {
        funcs: cfg_funcs,
        user_loops,
        entry,
        sym_table,
        codesign_ident,
        dead_strip,
        strip_member_symbols,
        elidable_sites,
        guarded_stubs,
        image,
        strings,
        data_globals,
        vtable_globals,
        class_layouts: cfg_class_layouts,
        force_emit_class_layouts_globals,
        force_emit_fn_name_globals,
        fn_name_globals: cfg_fn_name_globals,
        class_names,
        force_emit_class_names_globals,
        baked_regex_entries,
    } = cfg;
    LinkConfig {
        funcs: funcs.unwrap_or_else(|| cfg_funcs.clone()),
        user_loops: user_loops.clone(),
        entry: entry.clone(),
        sym_table: sym_table.clone(),
        codesign_ident: codesign_ident.clone(),
        dead_strip: *dead_strip,
        strip_member_symbols: *strip_member_symbols,
        elidable_sites: elidable_sites.clone(),
        guarded_stubs: guarded_stubs.clone(),
        image: image.clone(),
        strings: strings.clone(),
        data_globals: data_globals.clone(),
        vtable_globals: vtable_globals.clone(),
        class_layouts: class_layouts.unwrap_or_else(|| cfg_class_layouts.clone()),
        force_emit_class_layouts_globals: *force_emit_class_layouts_globals,
        force_emit_fn_name_globals: *force_emit_fn_name_globals,
        fn_name_globals: fn_name_globals.unwrap_or_else(|| cfg_fn_name_globals.clone()),
        class_names: class_names.clone(),
        force_emit_class_names_globals: *force_emit_class_names_globals,
        baked_regex_entries: baked_regex_entries.clone(),
    }
}

/// Have `merged` read each cut member through its cut. The merged
/// index stays as it is: a cut keeps every nlist row and its type
/// (only `n_value` moves, and only undef rows lose `N_EXT`), so every
/// defined extern keeps its owner.
pub(crate) fn install_cuts(
    merged: &MergedArchives<'_>,
    stripped: StrippedMembers,
) -> Result<(), ArchiveLayoutError> {
    for (key, cut) in stripped {
        merged
            .install_cut(key, cut)
            .map_err(|e| ArchiveLayoutError::MemberReloc {
                archive_idx: key.0,
                member_idx: key.1,
                err: e.into(),
            })?;
    }
    Ok(())
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
    with_cuts(reach, |cuts| Cut::plan(bytes, cuts, dead_at, true))?
        .map(|c| c.write(bytes))
        .transpose()
}

/// `f` of the cuts of every eligible section of `reach`.
fn with_cuts<R>(reach: &MemberReach<'_>, f: impl FnOnce(&[SectionCut<'_>]) -> R) -> R {
    let eligible = eligible_sections(reach);
    let cuts: Vec<SectionCut<'_>> = eligible
        .iter()
        .map(|(ord, atoms_rel, live)| SectionCut {
            ord: *ord,
            atoms_rel,
            live,
        })
        .collect();
    f(&cuts)
}

/// A section the rewriter may cut: `(ordinal, section-relative atoms,
/// liveness)`.
pub(crate) type Eligible<'r> = (u8, Vec<(u64, u64)>, &'r [bool]);

/// Every section of `reach` the rewriter may cut, in section order.
pub(crate) fn eligible_sections<'r>(reach: &'r MemberReach<'_>) -> Vec<Eligible<'r>> {
    reach
        .sects
        .iter()
        .enumerate()
        .filter(|(_, sa)| strippable(sa))
        .map(|(i, sa)| {
            let atoms_rel = sa.atoms.iter().map(|&(s, e)| (s - sa.addr, e - sa.addr));
            ((i + 1) as u8, atoms_rel.collect(), &sa.live[..])
        })
        .collect()
}
