//! Scalar replacement of stack cells merged through a copy — the
//! generator step object `{ value, done }` after heap_to_stack.
//!
//! Each `return { value, done }` arm of an inlined `next()` fills its
//! own `alloca_bytes` cell and hands it to the caller through one
//! multi-def Copy `%r`; the caller reads `%r + 32` and `%r + 40`. The
//! slot passes never see these cells as scalars, because the cell
//! pointer itself is what flows into `%r`. This is C2's
//! `ReduceAllocationMerges` / `reduce_phi_on_field_access` (and LLVM
//! SROA's `speculatePHINodeLoads`): every field the merge is read at
//! becomes its own register cell, set in each arm from the value the
//! arm stored there, and the cells, their stores and the merge go away.
//!
//! A merge `%r` qualifies when
//! - every definition of `%r` is `copy <cell>` of an `alloca_bytes`
//!   whose only other uses are constant-offset stores in that same
//!   block, before the copy;
//! - every use of `%r` is a constant-offset load or store, or an
//!   equality test against null or a global (never equal: the cells
//!   are stack slots);
//! - each offset read through `%r` is written in every member, and
//!   every access at that offset carries the same value type, with no
//!   other write overlapping it.
//!
//! Then each read offset gets a cell `c`: a member store there becomes
//! `c = copy v`, a store through `%r` likewise, a load through `%r`
//! becomes `copy c`; the null / global tests answer as constants to
//! their readers (`const_bool_fold`, which also folds what those
//! readers become); every other member store, the members and the
//! merge copies are deleted.
//!
//! `TORAJS_MERGE_SROA_OFF=1` skips, `TORAJS_MERGE_SROA_STATS=1` counts.

use std::collections::BTreeMap;

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    Function, IPred, Inst, InstKind, Module, Operand, Terminator, Type, ValueId, ValueInfo,
    visit_value_operands,
};

use crate::cell_access::{fits, load_width, operand_ty, store_width};
use crate::const_bool_fold::{ConstBoolFold, fold_constant_bools};

#[derive(Debug, Default)]
pub struct MergeSroaStats {
    pub merges_replaced: u32,
    pub fields_promoted: u32,
    pub stores_removed: u32,
    /// What the constant answers of the null / global tests folded.
    pub tests_folded: ConstBoolFold,
}

pub fn replace_merged_cells(module: &mut Module) -> MergeSroaStats {
    let mut stats = MergeSroaStats::default();
    for func in module.funcs.iter_mut() {
        if !func.is_declaration() {
            replace_fn(func, &mut stats);
        }
    }
    stats
}

/// Where each value is defined (block, index), all definitions kept.
fn defs_of(func: &Function) -> HashMap<ValueId, Vec<(usize, usize)>> {
    let mut defs: HashMap<ValueId, Vec<(usize, usize)>> = HashMap::default();
    for (b, block) in func.blocks.iter().enumerate() {
        for (i, inst) in block.insts.iter().enumerate() {
            if let Some(r) = inst.result {
                defs.entry(r).or_default().push((b, i));
            }
        }
    }
    defs
}

fn replace_fn(func: &mut Function, stats: &mut MergeSroaStats) {
    let defs = defs_of(func);
    let mut merges: Vec<ValueId> = defs
        .iter()
        .filter(|(_, ds)| ds.len() > 1)
        .filter(|(_, ds)| {
            ds.iter().all(|&(b, i)| {
                matches!(
                    func.blocks[b].insts[i].kind,
                    InstKind::Copy(Type::Obj(_), Operand::Value(_))
                )
            })
        })
        .map(|(v, _)| *v)
        .collect();
    merges.sort_by_key(|v| v.0);
    let mut answers: HashMap<ValueId, Operand> = HashMap::default();
    for m in merges {
        if let Some(plan) = plan_merge(func, &defs, m) {
            apply(func, m, &plan, stats, &mut answers);
        }
    }
    if !answers.is_empty() {
        let folded = fold_constant_bools(func, answers);
        stats.tests_folded.ops += folded.ops;
        stats.tests_folded.branches += folded.branches;
        stats.tests_folded.blocks += folded.blocks;
    }
}

struct Plan {
    /// Member cells (the `alloca_bytes` results).
    members: HashSet<ValueId>,
    /// Read offset → the type every access there carries.
    fields: BTreeMap<u64, Type>,
}

fn plan_merge(
    func: &Function,
    defs: &HashMap<ValueId, Vec<(usize, usize)>>,
    m: ValueId,
) -> Option<Plan> {
    // members: `copy <cell>` of an alloca_bytes defined once
    let mut members: HashMap<ValueId, usize> = HashMap::default();
    for &(b, i) in &defs[&m] {
        let InstKind::Copy(_, Operand::Value(cell)) = func.blocks[b].insts[i].kind else {
            return None;
        };
        let cd = defs.get(&cell)?;
        if cd.len() != 1
            || cd[0].0 != b
            || !matches!(func.blocks[b].insts[cd[0].1].kind, InstKind::AllocaBytes(_))
        {
            return None;
        }
        members.insert(cell, b);
    }

    // every mention of a member or of the merge, classified
    let mut fields: BTreeMap<u64, Type> = BTreeMap::new();
    let mut writes: Vec<(u64, u64, Option<Type>, bool)> = Vec::new(); // (off, width, ty, through merge)
    let mut member_store_offs: HashMap<ValueId, HashSet<u64>> = HashMap::default();
    for (b, block) in func.blocks.iter().enumerate() {
        for (i, inst) in block.insts.iter().enumerate() {
            let mut named = false;
            visit_value_operands(&inst.kind, |v| {
                named |= v == m || members.contains_key(&v);
            });
            if !named {
                continue;
            }
            match &inst.kind {
                InstKind::AllocaBytes(_) => {}
                InstKind::Copy(_, Operand::Value(c)) if inst.result == Some(m) => {
                    // the copy closes its member's block
                    if members.get(c) != Some(&b) || i + 1 != block.insts.len() {
                        return None;
                    }
                }
                InstKind::Store(val, Operand::Value(p), off)
                    if (*p == m || members.get(p) == Some(&b))
                        && !matches!(val, Operand::Value(x) if *x == m || members.contains_key(x)) =>
                {
                    writes.push((*off, store_width(func, val), operand_ty(func, val), *p == m));
                    if *p != m {
                        member_store_offs.entry(*p).or_default().insert(*off);
                    }
                }
                InstKind::Load(ty, Operand::Value(p), off) if *p == m => {
                    match fields.insert(*off, *ty) {
                        Some(t) if t != *ty => return None,
                        _ => {}
                    }
                }
                InstKind::ICmp(IPred::Eq | IPred::Ne, a, b2)
                    if is_merge_vs_nonstack(func, defs, a, b2, m) => {}
                _ => return None,
            }
        }
    }
    for block in &func.blocks {
        let mentions = match &block.term {
            Terminator::CondBr {
                cond: Operand::Value(v),
                ..
            }
            | Terminator::Ret(Some(Operand::Value(v))) => *v == m || members.contains_key(v),
            _ => false,
        };
        if mentions {
            return None;
        }
    }
    if fields.is_empty() {
        return None;
    }
    for (&off, &ty) in &fields {
        let (lo, hi) = (off, off + load_width(ty));
        for &(o, w, _, _) in &writes {
            if o != off && o < hi && lo < o + w {
                return None;
            }
        }
        for member in members.keys() {
            if !member_store_offs
                .get(member)
                .is_some_and(|s| s.contains(&off))
            {
                return None;
            }
        }
    }
    // every write at a read offset carries the field's type
    for (b, block) in func.blocks.iter().enumerate() {
        for inst in &block.insts {
            if let InstKind::Store(val, Operand::Value(p), off) = &inst.kind
                && (*p == m || members.get(p) == Some(&b))
                && let Some(&ty) = fields.get(off)
                && !fits(func, val, ty)
            {
                return None;
            }
        }
    }
    Some(Plan {
        members: members.keys().copied().collect(),
        fields,
    })
}

/// `icmp eq|ne m, x` (either order) where `x` can never be one of the
/// stack cells: null, or a global's address.
fn is_merge_vs_nonstack(
    func: &Function,
    defs: &HashMap<ValueId, Vec<(usize, usize)>>,
    a: &Operand,
    b: &Operand,
    m: ValueId,
) -> bool {
    let other = match (a, b) {
        (Operand::Value(x), o) | (o, Operand::Value(x)) if *x == m => o,
        _ => return false,
    };
    match other {
        Operand::ConstPtrNull => true,
        Operand::Value(g) => defs.get(g).is_some_and(|ds| {
            ds.len() == 1
                && matches!(
                    func.blocks[ds[0].0].insts[ds[0].1].kind,
                    InstKind::GlobalRef(_)
                )
        }),
        _ => false,
    }
}

/// `answers` collects the constant each null / global test answers,
/// folded through its readers once every merge is replaced.
fn apply(
    func: &mut Function,
    m: ValueId,
    plan: &Plan,
    stats: &mut MergeSroaStats,
    answers: &mut HashMap<ValueId, Operand>,
) {
    let cells: BTreeMap<u64, ValueId> = plan
        .fields
        .iter()
        .map(|(&off, &ty)| {
            let v = ValueId(func.values.len() as u32);
            func.values.push(ValueInfo { ty, name: None });
            (off, v)
        })
        .collect();
    let named =
        |op: &Operand| matches!(op, Operand::Value(v) if *v == m || plan.members.contains(v));
    for block in func.blocks.iter_mut() {
        let old = std::mem::take(&mut block.insts);
        let mut out: Vec<Inst> = Vec::with_capacity(old.len());
        for inst in old {
            match &inst.kind {
                InstKind::AllocaBytes(_)
                    if inst.result.is_some_and(|r| plan.members.contains(&r)) => {}
                InstKind::Copy(_, _) if inst.result == Some(m) => {}
                InstKind::Store(val, p, off) if named(p) => match cells.get(off) {
                    Some(&c) => out.push(Inst {
                        result: Some(c),
                        kind: InstKind::Copy(plan.fields[off], *val),
                        origin: inst.origin,
                    }),
                    None => stats.stores_removed += 1,
                },
                InstKind::Load(ty, p, off) if named(p) => out.push(Inst {
                    kind: InstKind::Copy(*ty, Operand::Value(cells[off])),
                    ..inst
                }),
                InstKind::ICmp(pred, a, b) if named(a) || named(b) => {
                    if let Some(r) = inst.result {
                        answers.insert(r, Operand::ConstBool(*pred == IPred::Ne));
                    }
                }
                _ => out.push(inst),
            }
        }
        block.insts = out;
    }
    stats.merges_replaced += 1;
    stats.fields_promoted += cells.len() as u32;
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, BlockId, FuncId, StructId};

    fn inst(result: Option<u32>, kind: InstKind) -> Inst {
        Inst {
            result: result.map(ValueId),
            kind,
            origin: None,
        }
    }
    fn v(i: u32) -> Operand {
        Operand::Value(ValueId(i))
    }
    fn arm(cell: u32, value: Operand, done: bool) -> Vec<Inst> {
        vec![
            inst(Some(cell), InstKind::AllocaBytes(48)),
            inst(None, InstKind::Store(Operand::ConstI32(1), v(cell), 0)),
            inst(None, InstKind::Store(value, v(cell), 32)),
            inst(None, InstKind::Store(Operand::ConstBool(done), v(cell), 40)),
            inst(Some(9), InstKind::Copy(Type::Obj(StructId(0)), v(cell))),
        ]
    }

    /// b0: cond → b1 | b2; b1 / b2 fill a step cell each and merge it
    /// into %9; b3 reads `done` and `value`, tests %9 against null and
    /// branches on the test to b4 (returns `value`) or b5 (returns 0).
    fn merged(join_extra: Vec<Inst>) -> Module {
        let mut join = vec![
            inst(Some(10), InstKind::Load(Type::Bool, v(9), 40)),
            inst(Some(11), InstKind::Load(Type::I64, v(9), 32)),
            inst(
                Some(12),
                InstKind::ICmp(IPred::Eq, v(9), Operand::ConstPtrNull),
            ),
        ];
        join.extend(join_extra);
        let blocks = vec![
            Block {
                id: BlockId(0),
                insts: vec![],
                term: Terminator::CondBr {
                    cond: Operand::ConstBool(true),
                    then_blk: BlockId(1),
                    else_blk: BlockId(2),
                },
            },
            Block {
                id: BlockId(1),
                insts: arm(1, Operand::ConstI64(7), false),
                term: Terminator::Br(BlockId(3)),
            },
            Block {
                id: BlockId(2),
                insts: arm(2, Operand::ConstI64(0), true),
                term: Terminator::Br(BlockId(3)),
            },
            Block {
                id: BlockId(3),
                insts: join,
                term: Terminator::CondBr {
                    cond: v(12),
                    then_blk: BlockId(5),
                    else_blk: BlockId(4),
                },
            },
            Block {
                id: BlockId(4),
                insts: vec![],
                term: Terminator::Ret(Some(v(11))),
            },
            Block {
                id: BlockId(5),
                insts: vec![],
                term: Terminator::Ret(Some(Operand::ConstI64(0))),
            },
        ];
        let ty = |t| ValueInfo { ty: t, name: None };
        let mut values: Vec<ValueInfo> = (0..9).map(|_| ty(Type::Ptr)).collect();
        values.extend([
            ty(Type::Obj(StructId(0))),
            ty(Type::Bool),
            ty(Type::I64),
            ty(Type::Bool),
        ]);
        Module {
            funcs: vec![
                Function {
                    name: "sink".into(),
                    params: vec![],
                    ret: Type::Void,
                    blocks: vec![],
                    values: vec![],
                    current_origin: None,
                },
                Function {
                    name: "main".into(),
                    params: vec![],
                    ret: Type::I64,
                    blocks,
                    values,
                    current_origin: None,
                },
            ],
            ..Default::default()
        }
    }

    fn mentions(f: &Function, x: u32) -> bool {
        f.blocks.iter().flat_map(|b| &b.insts).any(|i| {
            let mut hit = i.result == Some(ValueId(x));
            visit_value_operands(&i.kind, |u| hit |= u == ValueId(x));
            hit
        })
    }

    #[test]
    fn a_merged_step_object_becomes_field_cells() {
        let mut m = merged(vec![]);
        let st = replace_merged_cells(&mut m);
        assert_eq!((st.merges_replaced, st.fields_promoted), (1, 2));
        // the +0 header store is only written, never read
        assert_eq!(st.stores_removed, 2);
        let f = &m.funcs[1];
        for x in [1, 2, 9] {
            assert!(!mentions(f, x), "%{x} still mentioned");
        }
        // b0 branched on a constant too: b2 is gone and b3 is now the
        // third block
        let join = &f.blocks[2].insts;
        assert!(matches!(
            join[0].kind,
            InstKind::Copy(Type::Bool, Operand::Value(_))
        ));
        assert!(matches!(
            join[1].kind,
            InstKind::Copy(Type::I64, Operand::Value(_))
        ));
        // the null test answered false: its compare, its branch and
        // the arm only that branch reached are gone
        assert_eq!(join.len(), 2);
        assert!(matches!(f.blocks[2].term, Terminator::Br(BlockId(3))));
        assert_eq!(f.blocks.len(), 4);
        assert_eq!(
            st.tests_folded,
            ConstBoolFold {
                ops: 0,
                branches: 2,
                blocks: 2
            }
        );
        assert!(
            f.blocks[1]
                .insts
                .iter()
                .all(|i| matches!(i.kind, InstKind::Copy(..)))
        );
    }

    #[test]
    fn a_merge_handed_to_a_call_stays_in_memory() {
        let call = inst(None, InstKind::Call(FuncId(0), vec![v(9)]));
        let mut m = merged(vec![call]);
        assert_eq!(replace_merged_cells(&mut m).merges_replaced, 0);
    }

    #[test]
    fn a_member_that_never_writes_a_read_field_stays_in_memory() {
        let mut m = merged(vec![]);
        m.funcs[1].blocks[2]
            .insts
            .retain(|i| !matches!(i.kind, InstKind::Store(_, _, 32)));
        assert_eq!(replace_merged_cells(&mut m).merges_replaced, 0);
    }
}
