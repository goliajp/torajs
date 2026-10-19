//! What a split resume function must be: no suspension and no throw
//! signal left, every return bare, and every value defined on every
//! path to each of its uses. A value read after a landing that the
//! split failed to carry would otherwise compile to a register read
//! of whatever the last caller left there; here it stops the build
//! with the function and the value named.

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::id_hash::HashMap;
use torajs_core::ssa::{Function, InstKind, Operand, Terminator, ValueId, visit_value_operands};

pub(super) fn verify(f: &Function) {
    let mut def: HashMap<ValueId, (usize, usize)> = HashMap::default();
    for (bi, b) in f.blocks.iter().enumerate() {
        for (ii, i) in b.insts.iter().enumerate() {
            assert!(
                !matches!(i.kind, InstKind::Suspend(..) | InstKind::ThrowSignal),
                "internal error: `{}` keeps a {:?} after the split",
                f.name,
                i.kind
            );
            if let Some(r) = i.result {
                def.insert(r, (bi, ii));
            }
        }
        assert!(
            !matches!(b.term, Terminator::Ret(Some(_))),
            "internal error: `{}` returns a value after the split",
            f.name
        );
    }
    let dom = DominatorTree::compute(f);
    let check = |v: ValueId, bi: usize, at: usize| {
        if f.params.contains(&v) {
            return;
        }
        let Some(&(db, di)) = def.get(&v) else {
            panic!(
                "internal error: `{}` reads %{} that nothing defines",
                f.name, v.0
            )
        };
        let ok = if db == bi {
            di < at
        } else {
            dom.dominates(f.blocks[db].id, f.blocks[bi].id)
        };
        assert!(
            ok || !dom.is_reachable(f.blocks[bi].id),
            "internal error: `{}` reads %{} in bb{} where its definition in bb{} does not reach",
            f.name,
            v.0,
            f.blocks[bi].id.0,
            f.blocks[db].id.0
        );
    };
    for (bi, b) in f.blocks.iter().enumerate() {
        for (ii, i) in b.insts.iter().enumerate() {
            visit_value_operands(&i.kind, |v| check(v, bi, ii));
        }
        if let Terminator::CondBr {
            cond: Operand::Value(v),
            ..
        } = &b.term
        {
            check(*v, bi, b.insts.len());
        }
    }
}
