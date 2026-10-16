//! Shrink the candidate set to the values every def-site, use-site and
//! `-0` condition admits, before any region is planned.
//!
//! A value stays when (fit) each of its defs rewrites exactly or takes
//! a guard over in-set operands, (useful) some use reads it natively:
//! the def of an in-set value, or an fcmp that converts to icmp, and
//! (escape) every other use, a `ret` included, reads a value proven
//! `-0`-free, where the `-0`-free values are the greatest subset of the
//! set whose defs cannot mint `-0` from `-0`-free operands.
//!
//! Each condition only gets easier as the set grows, so the greatest
//! set meeting all three is unique. Evicting one value can only break
//! the conditions of the values next to it in the def-use graph, so a
//! worklist that re-checks those reaches that set in time linear in
//! the graph, where re-checking every value after each eviction wave
//! took one wave per link of a use chain (a 4500-statement main took
//! 4500 waves).

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{Function, InstKind, Operand, Terminator, ValueId};

use super::fit::{self, Fit};
use crate::interval::transfer::NumFact;
use crate::rc_peephole::visit_value_operands;

/// One place a value is read.
enum Site {
    /// An instruction: its result, and its operands when it is an
    /// fcmp (the one non-def use that can read a value natively).
    Inst(Option<ValueId>, Option<(Operand, Operand)>),
    Ret,
}

/// The def-use graph of one function, as the conditions read it, over
/// the values of the starting candidate set.
pub(super) struct Graph {
    sites: HashMap<ValueId, Vec<Site>>,
    /// Per value, the values whose defs read it.
    def_users: HashMap<ValueId, Vec<ValueId>>,
}

impl Graph {
    pub(super) fn of(
        func: &Function,
        defs: &HashMap<ValueId, Vec<InstKind>>,
        set: &HashSet<ValueId>,
    ) -> Graph {
        let mut sites: HashMap<ValueId, Vec<Site>> = HashMap::default();
        for block in &func.blocks {
            for inst in &block.insts {
                let fcmp = match &inst.kind {
                    InstKind::FCmp(_, a, b) => Some((*a, *b)),
                    _ => None,
                };
                visit_value_operands(&inst.kind, |v| {
                    if set.contains(&v) {
                        sites
                            .entry(v)
                            .or_default()
                            .push(Site::Inst(inst.result, fcmp));
                    }
                });
            }
            if let Terminator::Ret(Some(Operand::Value(v))) = &block.term
                && set.contains(v)
            {
                sites.entry(*v).or_default().push(Site::Ret);
            }
        }
        let mut def_users: HashMap<ValueId, Vec<ValueId>> = HashMap::default();
        for (v, dl) in defs {
            for d in dl {
                visit_value_operands(d, |o| def_users.entry(o).or_default().push(*v));
            }
        }
        Graph { sites, def_users }
    }

    fn def_users(&self, v: ValueId) -> &[ValueId] {
        self.def_users.get(&v).map_or(&[], |u| u.as_slice())
    }
}

/// Shrink `set` to the greatest subset meeting the three conditions.
pub(super) fn settle(
    set: &mut HashSet<ValueId>,
    defs: &HashMap<ValueId, Vec<InstKind>>,
    facts: &HashMap<ValueId, NumFact>,
    graph: &Graph,
) {
    let mut nz: HashSet<ValueId> = set.clone();
    let mut nz_work: Vec<ValueId> = nz.iter().copied().collect();
    let mut work: Vec<ValueId> = set.iter().copied().collect();
    loop {
        // settle the -0-free subset first: a value checks its escapes
        // against it
        while let Some(v) = nz_work.pop() {
            let clean = defs
                .get(&v)
                .is_some_and(|dl| dl.iter().all(|d| fit::def_no_neg_zero(d, facts, &nz)));
            if nz.contains(&v) && !clean {
                nz.remove(&v);
                work.push(v);
                nz_work.extend(graph.def_users(v).iter().filter(|u| nz.contains(u)));
            }
        }
        let Some(v) = work.pop() else {
            return;
        };
        if !set.contains(&v) || stays(v, set, &nz, defs, facts, graph) {
            continue;
        }
        set.remove(&v);
        if nz.remove(&v) {
            nz_work.extend(graph.def_users(v).iter().filter(|u| nz.contains(u)));
        }
        // users' defs lose an in-set operand
        work.extend(graph.def_users(v).iter().filter(|u| set.contains(u)));
        // the operands of v's defs lose a native use
        for d in defs.get(&v).into_iter().flatten() {
            visit_value_operands(d, |o| work.push(o));
        }
        // an fcmp reading v may stop converting
        for site in graph.sites.get(&v).into_iter().flatten() {
            if let Site::Inst(_, Some((a, b))) = site {
                for op in [a, b] {
                    if let Operand::Value(o) = op {
                        work.push(*o);
                    }
                }
            }
        }
    }
}

fn stays(
    v: ValueId,
    set: &HashSet<ValueId>,
    nz: &HashSet<ValueId>,
    defs: &HashMap<ValueId, Vec<InstKind>>,
    facts: &HashMap<ValueId, NumFact>,
    graph: &Graph,
) -> bool {
    let fits = defs.get(&v).is_some_and(|dl| {
        !dl.is_empty()
            && dl
                .iter()
                .all(|d| fit::def_fit(d, &v, facts, set) != Fit::No)
    });
    if !fits {
        return false;
    }
    let mut useful = false;
    for site in graph.sites.get(&v).into_iter().flatten() {
        match site {
            Site::Inst(Some(r), _) if set.contains(r) => useful = true,
            Site::Inst(_, Some((a, b))) if fit::fcmp_convertible(a, b, set) => useful = true,
            _ if !nz.contains(&v) => return false,
            _ => {}
        }
    }
    useful
}
