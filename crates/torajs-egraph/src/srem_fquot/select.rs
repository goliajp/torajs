//! The divisibility test selects read, once select_form has
//! if-converted its branch (`s = n % d === 0 ? s + d : s`). Nothing
//! is mispredicted any more, and when no select result flows back into
//! the remainder's operands inside the loop the quotient's longer
//! latency sits on no loop-carried chain: the loop is bound by how
//! often a divider issues, and the f64 one issues twice as often. Runs
//! after select_form, on facts the lattice recomputes for a function
//! holding such a test (the passes since the pipeline's own run have
//! rewritten instructions).

use crate::id_hash::HashSet;

use torajs_core::ssa::{
    BinOp, BlockId, Function, IPred, InstKind, Module, Operand, ValueId, visit_value_operands,
};

use super::{Readers, Site, SremFquotStats, Uses, rewrite_function, zero_tests_only};
use crate::fact_folds::LoopShapes;
use crate::interval::transfer::op_const_int;

pub fn float_quotient_select_srems(module: &mut Module) -> SremFquotStats {
    let names: Vec<String> = module.funcs.iter().map(|f| f.name.clone()).collect();
    let mut stats = SremFquotStats::default();
    let mut shapes = LoopShapes::new(module.funcs.len());
    for (fi, func) in module.funcs.iter_mut().enumerate() {
        if func.is_declaration() || !has_select_test(func) {
            continue;
        }
        let facts = crate::interval::analyze_function_with(func, &names);
        rewrite_function(func, &facts, &mut stats, Readers::Selects, &mut shapes, fi);
    }
    stats
}

/// Some select decides on `srem x, y == 0` (or `!=`) with `y` a value.
fn has_select_test(func: &Function) -> bool {
    let insts = || func.blocks.iter().flat_map(|b| &b.insts);
    let conds: HashSet<ValueId> = insts()
        .filter_map(|i| match &i.kind {
            InstKind::Select(_, Operand::Value(c), _, _) => Some(*c),
            _ => None,
        })
        .collect();
    if conds.is_empty() {
        return false;
    }
    let srems: HashSet<ValueId> = insts()
        .filter(|i| matches!(i.kind, InstKind::BinOp(BinOp::SRem, _, Operand::Value(_))))
        .filter_map(|i| i.result)
        .collect();
    insts().any(|i| {
        i.result.is_some_and(|r| conds.contains(&r))
            && matches!(&i.kind, InstKind::ICmp(IPred::Eq | IPred::Ne, Operand::Value(a), z)
                if srems.contains(a) && op_const_int(z) == Some(0))
    })
}

/// `r`'s zero compares are each read only as the condition of selects,
/// and no select result reaches `x` or `y` within the loop `body`.
pub(super) fn selects_only(func: &Function, uses: &Uses, s: &Site, body: &[BlockId]) -> bool {
    let insts = &func.blocks[s.bi].insts;
    // the selects reading the compare at `i`, if they are its only readers
    let selects = |i: usize| -> Option<Vec<ValueId>> {
        let c = insts[i].result?;
        let cv = Operand::Value(c);
        uses.reads
            .get(&c)?
            .iter()
            .map(|&(b, j)| {
                let inst = func.blocks[b].insts.get(j)?;
                match &inst.kind {
                    InstKind::Select(_, cond, a, e) if *cond == cv && *a != cv && *e != cv => {
                        inst.result
                    }
                    _ => None,
                }
            })
            .collect()
    };
    if !zero_tests_only(func, uses, s, |i| selects(i).is_some()) {
        return false;
    }
    let results: Vec<ValueId> = uses.reads[&s.r]
        .iter()
        .flat_map(|&(_, i)| selects(i).unwrap_or_default())
        .collect();
    !results.is_empty() && !reaches_operands(func, body, &results, s)
}

/// Whether a value computed in `body` from `from` (through any chain
/// of defs, cells included) is `s.x` or `s.y`.
fn reaches_operands(func: &Function, body: &[BlockId], from: &[ValueId], s: &Site) -> bool {
    let mut reached: HashSet<ValueId> = from.iter().copied().collect();
    loop {
        let mut grew = false;
        for &b in body {
            for inst in &func.blocks[b.0 as usize].insts {
                let Some(r) = inst.result else { continue };
                let mut hit = false;
                visit_value_operands(&inst.kind, |v| hit |= reached.contains(&v));
                if hit && reached.insert(r) {
                    grew = true;
                }
            }
        }
        if !grew {
            break;
        }
    }
    [s.x, s.y]
        .iter()
        .any(|op| matches!(op, Operand::Value(v) if reached.contains(v)))
}
