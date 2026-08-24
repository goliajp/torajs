//! S2 dead-strip blade 1 — reachability accounting REPORT. The
//! closure itself lives in [`crate::dead_strip_reach`]; this module
//! only renders its result to stderr. Gated on
//! `TORAJS_LINK_DEADSTRIP_DIAG`; zero effect on emitted bytes.

use std::collections::{BTreeMap, BTreeSet};

use crate::archives_merge::{MergedArchives, RequiredMembers};
use crate::dead_strip_reach::{MemberReach, compute_reachability, trim16};
use crate::exec::LinkConfig;

/// Entry point — called (env-gated) by
/// `compute_archive_layout_with_merged` after the member closure.
/// Diagnostic only: internal failures print one warning line and
/// return, they never fail the link.
pub(crate) fn report(
    cfg: &LinkConfig,
    merged: &MergedArchives<'_>,
    required: &RequiredMembers,
    extra_defined_syms: &BTreeSet<String>,
) {
    match compute_reachability(cfg, merged, required, extra_defined_syms) {
        Ok(r) => eprint!("{}", render_report(&r.members, &r.unresolved)),
        Err(e) => eprintln!("[deadstrip-diag] FAILED: {e}"),
    }
}

/// Format the accounting report.
fn render_report(
    reach: &BTreeMap<(usize, usize), MemberReach<'_>>,
    unresolved: &BTreeSet<String>,
) -> String {
    use std::fmt::Write;
    let (mut t_tot, mut t_live, mut d_tot, mut d_live) = (0u64, 0u64, 0u64, 0u64);
    let mut fallbacks = 0usize;
    let mut rows: Vec<(u64, u64, &str)> = Vec::new();
    let mut live_rows: Vec<(u64, u64, &str)> = Vec::new();
    let mut by_sect: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    for r in reach.values() {
        let (mut mt_tot, mut mt_live) = (0u64, 0u64);
        let mut m_live_all = 0u64;
        for sa in &r.sects {
            let live: u64 = sa
                .atoms
                .iter()
                .zip(&sa.live)
                .filter(|&(_, &l)| l)
                .map(|(&(s, e), _)| e - s)
                .sum();
            fallbacks += usize::from(sa.all_live);
            m_live_all += live;
            let e = by_sect
                .entry(String::from_utf8_lossy(trim16(&sa.sectname)).into_owned())
                .or_insert((0, 0));
            e.0 += live;
            e.1 += sa.size;
            if sa.is_text {
                mt_tot += sa.size;
                mt_live += live;
            } else {
                d_tot += sa.size;
                d_live += live;
            }
        }
        t_tot += mt_tot;
        t_live += mt_live;
        rows.push((mt_tot - mt_live, mt_tot, r.member_name));
        live_rows.push((m_live_all, mt_live, r.member_name));
    }
    rows.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    live_rows.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    let mut out = String::new();
    let pct = |l: u64, t: u64| if t > 0 { l * 100 / t } else { 0 };
    let _ = writeln!(
        out,
        "[deadstrip-diag] members={} text_live={}/{} ({}%) other_live={}/{} ({}%) \
         sect_fallbacks={} unresolved={}",
        reach.len(),
        t_live,
        t_tot,
        pct(t_live, t_tot),
        d_live,
        d_tot,
        pct(d_live, d_tot),
        fallbacks,
        unresolved.len(),
    );
    let _ = writeln!(out, "[deadstrip-diag] top dead members (text dead/total):");
    for (dead, sz, name) in rows.iter().take(20) {
        let _ = writeln!(out, "[deadstrip-diag]   {dead:>9} / {sz:>9}  {name}");
    }
    // S2-5 registration-reach accounting: where the LIVE mass sits.
    let _ = writeln!(
        out,
        "[deadstrip-diag] top live members (all-sect live / text live):"
    );
    for (live, tl, name) in live_rows.iter().take(20) {
        let _ = writeln!(out, "[deadstrip-diag]   {live:>9} / {tl:>9}  {name}");
    }
    let mut sect_rows: Vec<(&String, &(u64, u64))> = by_sect.iter().collect();
    sect_rows.sort_unstable_by(|a, b| b.1.0.cmp(&a.1.0));
    let _ = writeln!(out, "[deadstrip-diag] live by section (live/total):");
    for (name, (live, tot)) in sect_rows {
        let _ = writeln!(out, "[deadstrip-diag]   {live:>9} / {tot:>9}  {name}");
    }
    for u in unresolved.iter().take(10) {
        let _ = writeln!(out, "[deadstrip-diag]   unresolved: {u}");
    }
    out
}
