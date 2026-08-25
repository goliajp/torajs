//! S2 dead-strip blade 1 — reachability accounting REPORT. The
//! closure itself lives in [`crate::dead_strip_reach`]; this module
//! only renders its result to stderr. Gated on
//! `TORAJS_LINK_DEADSTRIP_DIAG`; zero effect on emitted bytes.

use std::collections::{BTreeMap, BTreeSet};

use crate::archives_merge::{MergedArchives, RequiredMembers};
use crate::dead_strip_reach::{
    MemberReach, Node, Pred, ReachProbes, ReachResult, compute_reachability, trim16,
};
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
    let why = std::env::var("TORAJS_LINK_DEADSTRIP_WHY").ok();
    // S2-5 pricing probes (see dead_strip_reach::ReachProbes): CUT
    // = live-but-stubbed what-if, CUT_IN = definition-absent
    // what-if. The report then shows the priced closure.
    let pats = |v: &str| -> Vec<String> {
        std::env::var(v)
            .ok()
            .map(|s| {
                s.split(',')
                    .filter(|p| !p.is_empty())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default()
    };
    let probes = ReachProbes {
        cuts: pats("TORAJS_LINK_DEADSTRIP_CUT"),
        cut_ins: pats("TORAJS_LINK_DEADSTRIP_CUT_IN"),
    };
    let who = pats("TORAJS_LINK_DEADSTRIP_WHO");
    let active = !probes.cuts.is_empty() || !probes.cut_ins.is_empty();
    match compute_reachability(
        cfg,
        merged,
        required,
        extra_defined_syms,
        why.is_some(),
        active.then_some(&probes),
    ) {
        Ok(r) => {
            if active {
                eprintln!(
                    "[deadstrip-diag] what-if active: cut=[{}] cut_in=[{}]",
                    probes.cuts.join(","),
                    probes.cut_ins.join(","),
                );
            }
            eprint!("{}", render_report(&r.members, &r.unresolved));
            let live_dump = pats("TORAJS_LINK_DEADSTRIP_LIVEDUMP");
            if !live_dump.is_empty() {
                eprint!("{}", render_live_dump(&r, &live_dump));
            }
            if let Some(pats) = why {
                eprint!("{}", render_why(&r, &pats));
            }
            if !who.is_empty() {
                let edges = crate::dead_strip_who::who_census(
                    cfg,
                    merged,
                    required,
                    extra_defined_syms,
                    &r,
                    &who,
                );
                eprintln!("[deadstrip-who] {} live in-edges:", edges.len());
                for (src, tgt) in &edges {
                    eprintln!(
                        "[deadstrip-who]   {}  ->  {}",
                        node_label(&r, *src),
                        node_label(&r, *tgt)
                    );
                }
            }
        }
        Err(e) => eprintln!("[deadstrip-diag] FAILED: {e}"),
    }
}

/// Live-atom dump: for each member whose name contains one of
/// `pats`, print every LIVE atom as `size sect symbol` — the raw
/// input a per-function census groups into families. The report
/// above stops at member granularity; a WHO census needs a target
/// pattern in hand. This is the middle view: what exactly is alive
/// inside one member, symbol by symbol.
fn render_live_dump(r: &ReachResult<'_>, pats: &[String]) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for m in r.members.values() {
        if !pats.iter().any(|p| m.member_name.contains(p.as_str())) {
            continue;
        }
        for (si, sa) in m.sects.iter().enumerate() {
            let sname = String::from_utf8_lossy(trim16(&sa.sectname)).into_owned();
            let snum = si + 1;
            for (&(s, e), &live) in sa.atoms.iter().zip(&sa.live) {
                if !live {
                    continue;
                }
                let sym = m
                    .nlist
                    .iter()
                    .filter(|n| {
                        n.n_type & 0x0e == 0x0e && n.n_sect as usize == snum && n.n_value <= s
                    })
                    .max_by_key(|n| n.n_value)
                    .map(|n| n.name)
                    .unwrap_or("+gap");
                let _ = writeln!(
                    out,
                    "[deadstrip-live] {} {} {} {}",
                    m.member_name,
                    e - s,
                    sname,
                    sym
                );
            }
        }
    }
    out
}

/// Why-live query: for each comma-separated substring in `pats`,
/// find up to 3 live defined symbols whose name contains it and
/// print the recorded liveness chain back to a root.
fn render_why(r: &ReachResult<'_>, pats: &str) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for pat in pats.split(',').filter(|p| !p.is_empty()) {
        let mut shown = 0usize;
        for (&key, m) in &r.members {
            if shown >= 3 {
                break;
            }
            for e in &m.nlist {
                if shown >= 3 {
                    break;
                }
                if e.n_type & 0x0e != 0x0e || !e.name.contains(pat) {
                    continue;
                }
                let Some(sa) = m.sects.get(e.n_sect as usize - 1) else {
                    continue;
                };
                let i = sa.atoms.partition_point(|&(s, _)| s <= e.n_value);
                if i == 0 || !sa.live[i - 1] {
                    continue;
                }
                shown += 1;
                let _ = writeln!(out, "[deadstrip-why] {} ({}):", e.name, m.member_name);
                let mut node = Node::Atom {
                    key,
                    sect: e.n_sect,
                    atom: i - 1,
                };
                for _ in 0..60 {
                    match r.preds.get(&node) {
                        Some(Pred::UserFn(f)) => {
                            let _ = writeln!(out, "[deadstrip-why]   <- user fn reloc: {f}");
                            break;
                        }
                        Some(Pred::FlagRoot) => {
                            let _ = writeln!(out, "[deadstrip-why]   <- section-flag root");
                            break;
                        }
                        Some(Pred::Node(prev)) => {
                            let _ = writeln!(out, "[deadstrip-why]   <- {}", node_label(r, *prev));
                            node = *prev;
                        }
                        None => {
                            let _ = writeln!(out, "[deadstrip-why]   <- (no recorded edge)");
                            break;
                        }
                    }
                }
            }
        }
        if shown == 0 {
            let _ = writeln!(out, "[deadstrip-why] {pat}: no live defined symbol matches");
        }
    }
    out
}

/// Human label for a node: member, section name, and the defined
/// symbol at (or just before) the atom start.
fn node_label(r: &ReachResult<'_>, node: Node) -> String {
    let (key, sect, atom) = match node {
        Node::Atom { key, sect, atom } => (key, sect, Some(atom)),
        Node::AllSect { key, sect } => (key, sect, None),
    };
    let Some(m) = r.members.get(&key) else {
        return "?".into();
    };
    let Some(sa) = m.sects.get(sect as usize - 1) else {
        return format!("{}:sect{}", m.member_name, sect);
    };
    let sname = String::from_utf8_lossy(trim16(&sa.sectname)).into_owned();
    let Some(a) = atom else {
        return format!("{}:{sname}:*", m.member_name);
    };
    let start = sa.atoms[a].0;
    let sym = m
        .nlist
        .iter()
        .filter(|e| e.n_type & 0x0e == 0x0e && e.n_sect == sect && e.n_value <= start)
        .max_by_key(|e| e.n_value)
        .map(|e| e.name)
        .unwrap_or("+gap");
    format!("{}:{sname}:{sym}", m.member_name)
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
