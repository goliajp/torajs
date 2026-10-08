//! Calls to a cloned function from outside its family: one whose
//! narrowed arguments are f64 values is converted once — past the
//! test, the integers it produced are the clone's arguments; failing
//! it, the call is the one it was. An integer constant inside the
//! window needs no check. Outside a loop the test is the
//! `Round::Exact` chain at the call; inside one it is hoisted
//! ([`super::hoist`]).

use crate::id_hash::HashMap;

use torajs_core::ssa::{
    BlockId, FuncId, Function, InstKind, Module, Operand, Terminator, Type, ValueId,
};

use super::dispatch::{calls_to, fast_call, split_call};
use super::hoist::{Loops, Placement, emit_test};
use super::{Window, int_const};
use crate::cfg_edit::new_block;
use crate::int_entry::guard::{Need, Round, build_chain};

/// The f64 sources to check and the clone's arguments, for a call from
/// outside the family; none when an argument cannot go to the clone.
fn external_route(
    f: &Function,
    args: &[Operand],
    windows: &[Option<Window>],
) -> Option<(Vec<Need>, Vec<Operand>)> {
    if args.len() != windows.len() {
        return None;
    }
    let mut fast = args.to_vec();
    let mut needs: HashMap<ValueId, Window> = HashMap::default();
    for (i, w) in windows.iter().enumerate() {
        let Some(w) = w else { continue };
        match &args[i] {
            Operand::Value(v) if f.values[v.0 as usize].ty == Type::F64 => {
                let n = needs.entry(*v).or_insert(*w);
                *n = Window {
                    lo: n.lo.max(w.lo),
                    hi: n.hi.min(w.hi),
                };
            }
            c => match int_const(c) {
                Some(k) if (w.lo..=w.hi).contains(&k) => fast[i] = Operand::ConstI64(k),
                _ => return None,
            },
        }
    }
    let mut needs: Vec<Need> = needs
        .into_iter()
        .map(|(source, w)| Need {
            source,
            round: Round::Exact,
            lo: w.lo,
            hi: w.hi,
        })
        .collect();
    needs.sort_unstable_by_key(|n| n.source.0);
    Some((needs, fast))
}

/// Where each call to `target` in `g` that can go to the clone has its
/// test, with the test's needs and the clone's arguments.
type Routed = ((usize, usize), Placement, Vec<Need>, Vec<Operand>);

/// A hoisted test: the bool that holds when it passes, and each
/// source's integer.
type Hoisted = (Operand, HashMap<ValueId, ValueId>);

fn routed_sites(
    g: &Function,
    target: FuncId,
    windows: &[Option<Window>],
    loops: &mut Option<Loops>,
) -> Vec<Routed> {
    let mut out = Vec::new();
    let dense = g
        .blocks
        .iter()
        .enumerate()
        .all(|(i, b)| b.id.0 as usize == i);
    if !dense {
        return out;
    }
    for (b, i) in calls_to(g, target) {
        let InstKind::Call(_, args) = &g.blocks[b].insts[i].kind else {
            unreachable!("a collected call site");
        };
        let Some((needs, fast)) = external_route(g, args, windows) else {
            continue;
        };
        let place = if needs.is_empty() {
            Placement::AtCall
        } else {
            let sources: Vec<ValueId> = needs.iter().map(|n| n.source).collect();
            loops
                .get_or_insert_with(|| Loops::compute(g))
                .placement(g, BlockId(b as u32), &sources)
        };
        if place != Placement::Varies {
            out.push(((b, i), place, needs, fast));
        }
    }
    out
}

/// Some call from outside the family can be routed to the clone.
pub(super) fn has_external_site(
    module: &Module,
    callee: usize,
    windows: &[Option<Window>],
    loops: &mut [Option<Loops>],
) -> bool {
    let target = FuncId(callee as u32);
    module
        .funcs
        .iter()
        .zip(loops)
        .enumerate()
        .any(|(gi, (g, l))| gi != callee && !routed_sites(g, target, windows, l).is_empty())
}

/// Route every call to `orig` from outside the family; answers how
/// many were routed.
pub(super) fn redirect_external(
    module: &mut Module,
    orig: FuncId,
    clone_id: FuncId,
    windows: &[Option<Window>],
    loops: &mut [Option<Loops>],
) -> u32 {
    let mut routed = 0;
    for (gi, g) in module.funcs.iter_mut().enumerate() {
        if gi == orig.0 as usize || gi == clone_id.0 as usize {
            continue;
        }
        // a clone has no slot: its loops are worked out afresh
        let mut fresh = None;
        let slot = loops.get_mut(gi).unwrap_or(&mut fresh);
        let mut sites = routed_sites(g, orig, windows, slot);
        if !sites.is_empty() {
            // the calls below change the blocks
            *slot = None;
        }
        // every hoisted test goes in before any call splits a block: a
        // call routed at its own site may sit in a test's block, ahead
        // of the end the test is appended to, and its split carries
        // that end (with the sources the test reads) to a new block.
        // Appended, a test moves no call collected above; one hoisted
        // for several calls is emitted once.
        let mut tests: Tests = HashMap::default();
        for (_, place, needs, _) in &sites {
            if let Placement::Before(pre) = *place {
                tests
                    .entry((pre, test_key(needs)))
                    .or_insert_with_key(|(pre, w)| emit_test(g, *pre, w));
            }
        }
        sites.sort_unstable_by_key(|s| std::cmp::Reverse(s.0));
        for site in sites {
            routed += 1;
            route(g, site, clone_id, windows, &tests);
        }
    }
    routed
}

type Tests = HashMap<(BlockId, Vec<(ValueId, Window)>), Hoisted>;

fn test_key(needs: &[Need]) -> Vec<(ValueId, Window)> {
    needs
        .iter()
        .map(|n| (n.source, Window { lo: n.lo, hi: n.hi }))
        .collect()
}

/// Send one call to the clone behind its test.
fn route(
    g: &mut Function,
    ((b, i), place, needs, mut fast): Routed,
    clone_id: FuncId,
    windows: &[Option<Window>],
    tests: &Tests,
) {
    if needs.is_empty() {
        g.blocks[b].insts[i].kind = InstKind::Call(clone_id, fast);
        return;
    }
    let (slow, open) = split_call(g, b, i, &[]);
    let at = new_block(g);
    let ints: HashMap<ValueId, ValueId> = match place {
        Placement::Before(pre) => {
            let (passes, ints) = tests[&(pre, test_key(&needs))].clone();
            // the clone goes in the block laid out right after the
            // call's, so the loop falls through into it
            g.blocks[b].term = Terminator::CondBr {
                cond: passes,
                then_blk: slow,
                else_blk: at,
            };
            ints
        }
        _ => {
            let built = build_chain(g, &needs, slow, at);
            g.blocks[b].term = Terminator::Br(built.head);
            needs
                .iter()
                .map(|n| (n.source, built.rounded[&(n.source, Round::Exact)]))
                .collect()
        }
    };
    for (a, w) in fast.iter_mut().zip(windows) {
        if let (Operand::Value(v), Some(_)) = (*a, w) {
            *a = Operand::Value(ints[&v]);
        }
    }
    fast_call(g, at, open, clone_id, fast, slow);
    if let Placement::Before(_) = place {
        let (s, a) = (slow.0 as usize, at.0 as usize);
        let moved = std::mem::take(&mut g.blocks[a].insts);
        g.blocks[a].insts = std::mem::replace(&mut g.blocks[s].insts, moved);
    }
}
