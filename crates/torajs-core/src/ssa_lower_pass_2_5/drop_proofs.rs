//! Which per-cell tests of the `__drop_<C>` release fns the finished
//! module settles (`class_drop_fill::DropHooks`).
//!
//! The props test releases an expando bag, the WeakRef test tells the
//! registry a target died. Only the runtime gives a cell a bag or a
//! WeakRef, and only a cell that reached it as a value ([`exposed`]).
//! A class sid is *sealed* when no slot of it is open to a cell of
//! another layout (`cell_slot_flow`) and no layout its cells can be
//! read as (along the prefix paths `cell_slot_flow` records), nor one
//! whose cells can reach its slots, is exposed.
//!
//! All of it is read over the functions that run (`asleep`): a class's
//! method rows are entered only once an instance or the class itself
//! became a value, which an exposed class instance, or a name that
//! reaches a class or a scope by a road no census follows, would make
//! possible; then nothing is settled. `TORAJS_DROP_PROOF_STATS=1` prints
//! the verdict per sid.

mod asleep;
mod exposed;
#[cfg(test)]
mod tests;

use std::collections::{BTreeSet, HashMap};

use crate::ast::{Ast, Expr};
use crate::ssa::{Module, StructId};
use crate::ssa_lower_emit_drop_value::class_drop_fill::DropHooks;

use super::any_holds::dead_adapters;
use super::cell_slot_flow::{EnvLayouts, open_sids_with};

/// Names that reach a class, a prototype or a scope by a road the
/// SSA does not show.
const ROADS: &[&str] = &[
    "eval",
    "Function",
    "Reflect",
    "Proxy",
    "globalThis",
    "arguments",
];

fn stats() -> bool {
    std::env::var_os("TORAJS_DROP_PROOF_STATS").is_some()
}

/// Settle every test of `hooks` the module proves.
pub(crate) fn settle(module: &mut Module, ast: &Ast, envs: &EnvLayouts, hooks: &DropHooks) {
    if hooks.hooks.is_empty() {
        return;
    }
    let verdicts = match verdicts(module, ast, envs, hooks) {
        Ok(v) => v,
        Err(why) => {
            if stats() {
                eprintln!("[drop_proof] none: {why}");
            }
            return;
        }
    };
    let n = hooks.settle(module, |sid, _| verdicts[&sid]);
    if stats() {
        let mut v: Vec<_> = verdicts.into_iter().collect();
        v.sort_unstable();
        for (sid, sealed) in v {
            eprintln!("[drop_proof] sid {sid}: sealed {sealed}");
        }
        eprintln!("[drop_proof] {n} of {} tests settled", hooks.hooks.len());
    }
}

/// Per hooked sid, whether it is sealed.
fn verdicts(
    module: &Module,
    ast: &Ast,
    envs: &EnvLayouts,
    hooks: &DropHooks,
) -> Result<HashMap<u32, bool>, String> {
    if let Some(why) = wakes_rows(ast) {
        return Err(why);
    }
    let asleep = asleep::row_only(module);
    let dead = dead_adapters(module);
    let skip: Vec<bool> = asleep.iter().zip(&dead).map(|(a, d)| *a || *d).collect();
    let exposed = exposed::exposed_sids(module, &skip);
    let mut classes: Vec<u32> = hooks.class_sids.iter().copied().collect();
    classes.sort_unstable();
    if let Some(s) = classes.iter().find(|s| exposed[**s as usize]) {
        return Err(format!("an instance of class sid {s} reaches the runtime"));
    }
    let (open, widens) = open_sids_with(module, envs, &skip);
    let sids: BTreeSet<u32> = hooks.hooks.iter().map(|h| h.sid).collect();
    let mut out = HashMap::new();
    for sid in sids {
        let base = hooks.class_sids.contains(&sid) && !open[sid as usize];
        let sealed = base
            && reach(&widens, sid, false).iter().all(|w| {
                reach(&widens, *w, true)
                    .iter()
                    .all(|n| !exposed[*n as usize])
            });
        out.insert(sid, sealed);
    }
    Ok(out)
}

/// A name in the program that reaches a class or a scope as a value:
/// a road, a class named as a value, a `new` of a computed callee, or
/// `new.target`.
fn wakes_rows(ast: &Ast) -> Option<String> {
    if !ast.new_target_fns.is_empty() {
        return Some("`new.target`".into());
    }
    ast.exprs.iter().find_map(|e| match e {
        Expr::Ident(n) if ROADS.contains(&n.as_str()) => Some(format!("the road `{n}`")),
        Expr::Ident(n) if ast.class_parents.contains_key(n) => {
            Some(format!("the class `{n}` as a value"))
        }
        Expr::NewDynamic { .. } => Some("a `new` of a computed callee".into()),
        _ => None,
    })
}

/// `sid` and every sid a prefix path leads to from it (`down`: the
/// layouts its cells are read as) or from which one leads to it.
fn reach(widens: &[(StructId, StructId)], sid: u32, down: bool) -> Vec<u32> {
    let mut seen = vec![sid];
    let mut i = 0;
    while i < seen.len() {
        let at = seen[i];
        for (wide, narrow) in widens {
            let (from, to) = if down { (wide, narrow) } else { (narrow, wide) };
            if from.0 == at && !seen.contains(&to.0) {
                seen.push(to.0);
            }
        }
        i += 1;
    }
    seen
}
