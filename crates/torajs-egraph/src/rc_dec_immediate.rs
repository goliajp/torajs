//! Immediate-box release elision (r503).
//!
//! `__torajs_anyv_rc_dec(v)` where `v = __torajs_anyv_box_from_pair
//! (<const tag>, _)` and the tag is not the heap tag is a no-op by the
//! kernel's own definition (`nanbox_ffi::__torajs_anyv_rc_dec` only
//! ever reaches `value_drop_heap` through `is_cell`), and once its
//! last use is that release, the box itself packs bits nobody reads.
//! The shape ssa_lower mints: a `new C()` site boxes `undefined` as
//! the ctor's `__new_target` and releases it after the call returns.
//!
//! Left in place, that release is the one edge from a class program's
//! user main into the generic any-drop — which roots the cycle
//! collector, the weak registry and the value-drop walker — so the
//! elision is what lets the link judgment strip those worlds.
//!
//! Same-fn only: the mint and the release must both be visible, so
//! the pass runs after inlining. A release whose operand reaches it
//! any other way (a param, a load, a select) is kept — the safe
//! direction.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{
    ANY_SLOT_TAG_HEAP, InstKind, Module, Operand, Terminator, ValueId, visit_value_operands,
};

const RC_DEC: &str = "__torajs_anyv_rc_dec";
const BOX_FROM_PAIR: &str = "__torajs_anyv_box_from_pair";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RcDecImmediateStats {
    /// Releases of an immediate box removed.
    pub decs_elided: u32,
    /// Immediate boxes left without a use by that removal, removed.
    pub boxes_elided: u32,
}

/// Remove every `anyv_rc_dec` of an immediate-tagged `box_from_pair`
/// result, then every such box whose last use that was.
pub fn elide_immediate_rc_decs(module: &mut Module) -> RcDecImmediateStats {
    let mut stats = RcDecImmediateStats::default();
    let fid = |name: &str| {
        module
            .funcs
            .iter()
            .position(|f| f.name == name)
            .map(|i| i as u32)
    };
    let (Some(dec), Some(bx)) = (fid(RC_DEC), fid(BOX_FROM_PAIR)) else {
        return stats;
    };
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        let immediates: HashSet<ValueId> = func
            .blocks
            .iter()
            .flat_map(|b| b.insts.iter())
            .filter_map(|i| match (&i.kind, i.result) {
                (InstKind::Call(f, args), Some(r))
                    if f.0 == bx
                        && matches!(args.first(), Some(Operand::ConstI64(t)) if *t != ANY_SLOT_TAG_HEAP) =>
                {
                    Some(r)
                }
                _ => None,
            })
            .collect();
        if immediates.is_empty() {
            continue;
        }
        let mut removed = 0u32;
        for block in func.blocks.iter_mut() {
            block.insts.retain(|i| {
                let hit = i.result.is_none()
                    && matches!(&i.kind, InstKind::Call(f, args)
                        if f.0 == dec
                            && matches!(args.as_slice(), [Operand::Value(v)] if immediates.contains(v)));
                removed += u32::from(hit);
                !hit
            });
        }
        if removed == 0 {
            continue;
        }
        stats.decs_elided += removed;
        let mut uses: HashMap<ValueId, u32> = immediates.iter().map(|v| (*v, 0)).collect();
        let mut count = |v: ValueId| {
            if let Some(n) = uses.get_mut(&v) {
                *n += 1;
            }
        };
        for block in &func.blocks {
            for i in &block.insts {
                visit_value_operands(&i.kind, &mut count);
            }
            match &block.term {
                Terminator::Ret(Some(Operand::Value(v)))
                | Terminator::CondBr {
                    cond: Operand::Value(v),
                    ..
                } => count(*v),
                _ => {}
            }
        }
        let dead: HashSet<ValueId> = uses
            .into_iter()
            .filter(|(_, n)| *n == 0)
            .map(|(v, _)| v)
            .collect();
        for block in func.blocks.iter_mut() {
            block.insts.retain(|i| {
                let hit = i.result.is_some_and(|r| dead.contains(&r));
                stats.boxes_elided += u32::from(hit);
                !hit
            });
        }
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, BlockId, FuncId, Function, Inst, Terminator, Type, ValueInfo};

    const DEC: FuncId = FuncId(0);
    const BOX: FuncId = FuncId(1);
    const OTHER: FuncId = FuncId(2);

    fn declaration(name: &str) -> Function {
        Function {
            name: name.into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![],
            values: vec![],
            current_origin: None,
        }
    }

    fn module(insts: Vec<Inst>, n_values: usize) -> Module {
        let body = Function {
            name: "main".into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![Block {
                id: BlockId(0),
                insts,
                term: Terminator::Ret(None),
            }],
            values: vec![
                ValueInfo {
                    ty: Type::Any,
                    name: None,
                };
                n_values
            ],
            current_origin: None,
        };
        Module {
            funcs: vec![
                declaration(RC_DEC),
                declaration(BOX_FROM_PAIR),
                declaration("__torajs_anyv_ctor_register"),
                body,
            ],
            ..Default::default()
        }
    }

    fn boxed(r: u32, tag: i64) -> Inst {
        Inst {
            result: Some(ValueId(r)),
            kind: InstKind::Call(BOX, vec![Operand::ConstI64(tag), Operand::ConstI64(0)]),
            origin: None,
        }
    }
    fn dec(v: u32) -> Inst {
        Inst {
            result: None,
            kind: InstKind::Call(DEC, vec![Operand::Value(ValueId(v))]),
            origin: None,
        }
    }
    fn other(v: u32) -> Inst {
        Inst {
            result: None,
            kind: InstKind::Call(OTHER, vec![Operand::Value(ValueId(v))]),
            origin: None,
        }
    }

    fn kinds(m: &Module) -> Vec<String> {
        m.funcs[3].blocks[0]
            .insts
            .iter()
            .map(|i| match &i.kind {
                InstKind::Call(f, _) => m.funcs[f.0 as usize].name.clone(),
                other => format!("{other:?}"),
            })
            .collect()
    }

    #[test]
    fn undefined_box_and_its_release_both_go() {
        let mut m = module(vec![boxed(0, 5), dec(0)], 1);
        let st = elide_immediate_rc_decs(&mut m);
        assert_eq!(
            st,
            RcDecImmediateStats {
                decs_elided: 1,
                boxes_elided: 1
            }
        );
        assert!(kinds(&m).is_empty());
    }

    #[test]
    fn heap_tagged_box_keeps_its_release() {
        let mut m = module(vec![boxed(0, ANY_SLOT_TAG_HEAP), dec(0)], 1);
        let st = elide_immediate_rc_decs(&mut m);
        assert_eq!(st, RcDecImmediateStats::default());
        assert_eq!(kinds(&m), [BOX_FROM_PAIR, RC_DEC]);
    }

    #[test]
    fn box_with_another_use_survives_its_release() {
        let mut m = module(vec![boxed(0, 2), other(0), dec(0)], 1);
        let st = elide_immediate_rc_decs(&mut m);
        assert_eq!(
            st,
            RcDecImmediateStats {
                decs_elided: 1,
                boxes_elided: 0
            }
        );
        assert_eq!(kinds(&m), [BOX_FROM_PAIR, "__torajs_anyv_ctor_register"]);
    }

    #[test]
    fn release_of_a_non_box_value_is_kept() {
        // %0 is a box that reaches its dec through nothing we track;
        // %1 is not a box at all.
        let mut m = module(vec![boxed(0, 5), other(1), dec(1)], 2);
        let st = elide_immediate_rc_decs(&mut m);
        assert_eq!(st, RcDecImmediateStats::default());
        assert_eq!(kinds(&m).len(), 3);
    }
}
