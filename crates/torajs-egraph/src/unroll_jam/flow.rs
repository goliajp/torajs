//! Value flow of a planned loop: what lanes run early may read, and
//! which loop values the code after the loop reads.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{BlockId, Function, InstKind, Terminator, ValueId, visit_value_operands};

use super::Plan;
use super::plan::{Ctx, blk, successors, term_value};

fn is_cell(func: &Function, ctx: &Ctx, v: ValueId) -> bool {
    ctx.defs.get(&v).is_some_and(|ds| {
        ds.iter()
            .all(|(b, i)| matches!(blk(func, *b).insts[*i].kind, InstKind::Copy(..)))
    })
}

fn in_loop(ctx: &Ctx, p: &Plan, v: ValueId) -> bool {
    ctx.defs
        .get(&v)
        .is_some_and(|ds| ds.iter().any(|(b, _)| p.body.contains(b)))
}

/// What lanes run early reads only the induction variable, loop
/// invariants and its own iteration's values — never a cell carried
/// from the previous iteration: walking `pre`, the inner header and the
/// inner body in order, every cell read was written earlier in it.
/// A cell the inner loop sets without `pre` setting it first is a
/// per-step temporary (a copy of the counter). A lane that finished in
/// the fused loop never writes it back, so only code an inner-body
/// guard exit leads to may read it after the inner loop (that exit is
/// taken from a lane finishing on its own, under the temporary's own
/// id) — not `post`, not the loop's normal exit, not a `pre` guard's
/// exit — and a body block may set it only ahead of the first guard.
/// Returns the cells `pre` defines.
pub(super) fn lane_cells(func: &Function, ctx: &Ctx, p: &Plan) -> Option<Vec<ValueId>> {
    let pre: Vec<BlockId> = p.pre_order();
    let inner: Vec<BlockId> = p.inner_order();
    let def_block = |v: ValueId| ctx.defs.get(&v).map(|ds| ds[0].0);
    let first_guard = p
        .chain
        .iter()
        .position(|b| matches!(blk(func, *b).term, Terminator::CondBr { .. }))
        .unwrap_or(p.chain.len());
    let mut cells: HashSet<ValueId> = HashSet::new();
    let mut pre_cells: Vec<ValueId> = Vec::new();
    let mut temps: HashSet<ValueId> = HashSet::new();
    let mut ok = true;
    for (pos, &b) in pre.iter().chain(&inner).enumerate() {
        if pos == pre.len() {
            pre_cells = cells.iter().copied().collect();
        }
        let chain_pos = (pos > pre.len()).then(|| pos - pre.len() - 1);
        for inst in &blk(func, b).insts {
            visit_value_operands(&inst.kind, |v| {
                ok &= v == p.iv
                    || !in_loop(ctx, p, v)
                    || if is_cell(func, ctx, v) {
                        cells.contains(&v)
                    } else {
                        def_block(v).is_some_and(|d| pre.contains(&d) || inner.contains(&d))
                    };
            });
            if let Some(r) = inst.result
                && is_cell(func, ctx, r)
                && cells.insert(r)
                && pos >= pre.len()
            {
                ok &= chain_pos.is_none_or(|c| c <= first_guard);
                temps.insert(r);
            }
        }
    }
    let sources: Vec<BlockId> = pre.iter().chain(&p.post).copied().collect();
    let after_loop = reached_from(func, p, &sources);
    let post_reads_temp = p.post.iter().chain(&after_loop).any(|b| {
        let block = blk(func, *b);
        let mut hit = term_value(&block.term).is_some_and(|v| temps.contains(&v));
        for inst in &block.insts {
            visit_value_operands(&inst.kind, |v| hit |= temps.contains(&v));
        }
        hit
    });
    pre_cells.sort_by_key(|v| v.0);
    (ok && !post_reads_temp && !cells.contains(&p.iv)).then_some(pre_cells)
}

/// Loop values read after the loop. A cell is fine (lanes copy back
/// into it); so is a value `post` or the inner loop computes, which the
/// lane clones keep under its id. One `pre` computes is not: lanes have
/// their own copies of it. A loop value with several defs must be a
/// cell.
pub(super) fn escaping(func: &Function, ctx: &Ctx, p: &Plan) -> Option<HashSet<ValueId>> {
    let pre = p.pre_order();
    let loop_noncell = |v: ValueId| in_loop(ctx, p, v) && !is_cell(func, ctx, v);
    for (v, ds) in &ctx.defs {
        if ds.len() > 1 && loop_noncell(*v) {
            return None;
        }
    }
    let sources: Vec<BlockId> = p.post.iter().chain(&p.chain).copied().collect();
    let after_post = reached_from(func, p, &sources);
    let mut out = HashSet::new();
    for block in func.blocks.iter().filter(|b| !p.body.contains(&b.id)) {
        let mut reads = Vec::new();
        for inst in &block.insts {
            visit_value_operands(&inst.kind, |v| reads.push(v));
        }
        reads.extend(term_value(&block.term));
        for v in reads.into_iter().filter(|v| loop_noncell(*v)) {
            if !pre.contains(&ctx.defs[&v][0].0) {
                out.insert(v);
            } else if after_post.contains(&block.id) {
                // lanes leave through post holding their own copy of v
                return None;
            }
        }
    }
    Some(out)
}

/// Blocks outside the loop reachable from `sources`' exits before the
/// loop is entered again.
fn reached_from(func: &Function, p: &Plan, sources: &[BlockId]) -> Vec<BlockId> {
    let mut seen: HashSet<BlockId> = HashSet::new();
    let mut work: Vec<BlockId> = sources
        .iter()
        .flat_map(|b| successors(&blk(func, *b).term))
        .filter(|s| !p.body.contains(s))
        .collect();
    while let Some(b) = work.pop() {
        if p.body.contains(&b) || !seen.insert(b) {
            continue;
        }
        work.extend(successors(&blk(func, b).term));
    }
    seen.into_iter().collect()
}

/// Values every lane computes alike: neither the induction variable
/// nor anything `pre` or the inner loop computes from it goes into
/// them (a trial divisor counting up from 1, its square). In the fused
/// part a lane past the first reads lane 0's instead of computing its
/// own: all lanes enter the fused loop together and step it together,
/// so such a value is the same in every lane at every point.
/// Not a cell `pre` and the inner header define more than once between
/// them: lanes run those part by part, a whole lane at a time, so a
/// later lane would read lane 0's last def where its own earlier one
/// stands.
pub(super) fn lane_shared(func: &Function, p: &Plan) -> HashSet<ValueId> {
    let blocks: Vec<BlockId> = p.pre_order().into_iter().chain(p.inner_order()).collect();
    let insts = || blocks.iter().flat_map(|&b| blk(func, b).insts.iter());
    let mut defs_before_body: HashMap<ValueId, u32> = HashMap::new();
    for b in p.pre_order().into_iter().chain([p.inner_header]) {
        for r in blk(func, b).insts.iter().filter_map(|i| i.result) {
            *defs_before_body.entry(r).or_insert(0) += 1;
        }
    }
    let mut variant: HashSet<ValueId> = HashSet::from([p.iv]);
    loop {
        let mut grew = false;
        for inst in insts() {
            let Some(r) = inst.result else { continue };
            let mut reads = false;
            visit_value_operands(&inst.kind, |v| reads |= variant.contains(&v));
            if reads && variant.insert(r) {
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    insts()
        .filter_map(|i| i.result)
        .filter(|r| !variant.contains(r) && defs_before_body.get(r).is_none_or(|&n| n < 2))
        .collect()
}
