//! Map lookup sharing — a `__torajs_map_find_str(M, K)` that repeats an
//! earlier find on the same Map and key, with nothing between that can
//! write a Map, reuses the earlier entry index; a later
//! `__torajs_map_set_str(M, K, -1, …)` takes an earlier index as its
//! hint even across calls, because the kernel checks a hint against
//! the entry before trusting it.
//!
//! `if (m.has(k)) m.set(k, m.get(k)! + 1)` is the counting idiom, and
//! each call used to hash, probe and compare the key again. JSC's DFG
//! folds it through `MapHash` / `MapGet` CSE (`DFGClobberize.h`: the
//! lookup defines a heap location keyed by map and key, killed by any
//! write). This is the same analysis over our IR, kept to the shape
//! that matters and is cheap to prove:
//!
//! * the earlier find is found by walking backward from the later call
//!   through its block and then through unique-predecessor blocks only,
//!   so it dominates the later call and every instruction on the walk is
//!   on every path between them;
//! * the walk stops at any redefinition of `M` or `K` (post-φ loop
//!   cells are multi-def) and, for a find, at any call outside
//!   [`TRANSPARENT`] (anything else may write a Map or run user code).
//!
//! Entry indices stay valid while no Map is written: only insert,
//! delete, clear and rehash move entries, and each of those is a call.
//!
//! Runs in the pipeline tail before loop rotation.
//! `TORAJS_MAP_FIND_CSE_OFF=1` skips, `TORAJS_MAP_FIND_CSE_STATS=1`
//! counts.

use std::collections::HashMap;

use torajs_core::ssa::{BlockId, FuncId, Function, InstKind, Module, Operand, Terminator, ValueId};

use crate::slot_forward::rewrite_operands;

const FIND: &str = "__torajs_map_find_str";
const SET: &str = "__torajs_map_set_str";

/// Calls that read Maps or build / take apart value pairs, and never
/// write a Map or run user code.
const TRANSPARENT: [&str; 9] = [
    "__torajs_map_find_str",
    "__torajs_map_entry_get",
    "__torajs_map_entry_get_f64",
    "__torajs_map_get_f64",
    "__torajs_map_size",
    "__torajs_anyv_box_from_pair",
    "__torajs_anyv_unbox_tag",
    "__torajs_anyv_unbox_value",
    "__torajs_rc_inc",
];

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MapFindCseStats {
    /// Repeated finds replaced by an earlier index.
    pub finds_shared: u32,
    /// `set_str` calls handed an earlier index as their hint.
    pub set_hints: u32,
}

pub fn share_map_finds(module: &mut Module) -> MapFindCseStats {
    let mut stats = MapFindCseStats::default();
    let find = |name: &str| {
        module
            .funcs
            .iter()
            .position(|f| f.name == name)
            .map(|i| FuncId(i as u32))
    };
    let (Some(find_fid), Some(set_fid)) = (find(FIND), find(SET)) else {
        return stats;
    };
    let transparent: Vec<FuncId> = TRANSPARENT.iter().filter_map(|n| find(n)).collect();
    let fids = Fids {
        find: find_fid,
        set: set_fid,
        transparent,
    };
    for func in module.funcs.iter_mut() {
        if !func.is_declaration() {
            share_in_function(func, &fids, &mut stats);
        }
    }
    stats
}

struct Fids {
    find: FuncId,
    set: FuncId,
    transparent: Vec<FuncId>,
}

fn unique_preds(func: &Function) -> HashMap<BlockId, Option<BlockId>> {
    let mut preds: HashMap<BlockId, Option<BlockId>> = HashMap::new();
    let mut add = |to: BlockId, from: BlockId| {
        preds
            .entry(to)
            .and_modify(|p| {
                if *p != Some(from) {
                    *p = None;
                }
            })
            .or_insert(Some(from));
    };
    for block in &func.blocks {
        match &block.term {
            Terminator::Br(t) => add(*t, block.id),
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => {
                add(*then_blk, block.id);
                add(*else_blk, block.id);
            }
            Terminator::Ret(_) | Terminator::Unreachable => {}
        }
    }
    preds
}

fn share_in_function(func: &mut Function, fids: &Fids, stats: &mut MapFindCseStats) {
    let preds = unique_preds(func);
    let mut replace: HashMap<ValueId, Operand> = HashMap::new();
    let mut dead: Vec<(usize, usize)> = Vec::new();
    let mut hints: Vec<(usize, usize, ValueId)> = Vec::new();
    for bi in 0..func.blocks.len() {
        for ii in 0..func.blocks[bi].insts.len() {
            let inst = &func.blocks[bi].insts[ii];
            let InstKind::Call(fid, args) = &inst.kind else {
                continue;
            };
            let (m, k) = match (args.first(), args.get(1)) {
                (Some(m), Some(k)) => (m, k),
                _ => continue,
            };
            let is_find = *fid == fids.find && inst.result.is_some();
            let is_unhinted_set = *fid == fids.set && args.get(2) == Some(&Operand::ConstI64(-1));
            if !(is_find || is_unhinted_set) {
                continue;
            }
            let Some(prev) = earlier_find(func, &preds, fids, (bi, ii), (m, k), is_find) else {
                continue;
            };
            if is_find {
                let r = inst.result.expect("checked above");
                replace.insert(r, Operand::Value(prev));
                dead.push((bi, ii));
                stats.finds_shared += 1;
            } else {
                hints.push((bi, ii, prev));
                stats.set_hints += 1;
            }
        }
    }
    for (bi, ii, prev) in hints {
        if let InstKind::Call(_, args) = &mut func.blocks[bi].insts[ii].kind {
            args[2] = Operand::Value(prev);
        }
    }
    // a shared find's own result may be the earlier index of a later
    // one; resolve chains before rewriting
    let resolved: HashMap<ValueId, Operand> = replace
        .keys()
        .map(|v| {
            let mut op = Operand::Value(*v);
            while let Operand::Value(x) = op
                && let Some(next) = replace.get(&x)
            {
                op = next.clone();
            }
            (*v, op)
        })
        .collect();
    for (bi, ii) in dead.into_iter().rev() {
        func.blocks[bi].insts.remove(ii);
    }
    for block in func.blocks.iter_mut() {
        for inst in block.insts.iter_mut() {
            rewrite_operands(&mut inst.kind, &resolved);
        }
        match &mut block.term {
            Terminator::CondBr { cond: op, .. } | Terminator::Ret(Some(op)) => {
                if let Operand::Value(v) = op
                    && let Some(n) = resolved.get(v)
                {
                    *op = n.clone();
                }
            }
            _ => {}
        }
    }
}

/// The result of a find on `(m, k)` reached walking backward from
/// `at` through transparent instructions and unique predecessors.
fn earlier_find(
    func: &Function,
    preds: &HashMap<BlockId, Option<BlockId>>,
    fids: &Fids,
    at: (usize, usize),
    (m, k): (&Operand, &Operand),
    stop_at_calls: bool,
) -> Option<ValueId> {
    let redefines =
        |r: Option<ValueId>| r.is_some_and(|r| *m == Operand::Value(r) || *k == Operand::Value(r));
    let (mut bi, mut end) = at;
    let mut seen = 0usize;
    loop {
        for inst in func.blocks[bi].insts[..end].iter().rev() {
            if let InstKind::Call(fid, args) = &inst.kind {
                if *fid == fids.find && args.first() == Some(m) && args.get(1) == Some(k) {
                    return inst.result;
                }
                if stop_at_calls && !fids.transparent.contains(fid) {
                    return None;
                }
            } else if stop_at_calls
                && matches!(
                    inst.kind,
                    InstKind::CallIndirect(..) | InstKind::RcDecAny(_)
                )
            {
                return None;
            }
            if redefines(inst.result) {
                return None;
            }
        }
        let pred = (*preds.get(&func.blocks[bi].id)?)?;
        seen += 1;
        // a unique-predecessor cycle has no entry; stop rather than spin
        if seen > func.blocks.len() {
            return None;
        }
        bi = pred.0 as usize;
        end = func.blocks[bi].insts.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, IPred, Inst, Type, ValueInfo};

    // FuncIds in the test module: 0 find, 1 set, 2 opaque call.
    const FIND_F: FuncId = FuncId(0);
    const SET_F: FuncId = FuncId(1);
    const OPAQUE_F: FuncId = FuncId(2);

    fn fids() -> Fids {
        Fids {
            find: FIND_F,
            set: SET_F,
            transparent: vec![FIND_F],
        }
    }
    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }
    fn call(r: Option<u32>, f: FuncId, args: Vec<Operand>) -> Inst {
        Inst {
            result: r.map(ValueId),
            kind: InstKind::Call(f, args),
            origin: None,
        }
    }

    /// bb0: %2 = find(%0, %1); %3 = icmp sge %2, 0; cond_br %3 → bb1 / bb2
    /// bb1: <middle>; %4 = find(%0, %1); set(%0, %1, -1, 2, 7); ret
    fn counting(middle: Vec<Inst>) -> Function {
        let mut bb1 = middle;
        bb1.push(call(Some(4), FIND_F, vec![v(0), v(1)]));
        bb1.push(call(
            None,
            SET_F,
            vec![
                v(0),
                v(1),
                Operand::ConstI64(-1),
                Operand::ConstI64(2),
                Operand::ConstI64(7),
            ],
        ));
        let ty = |t| ValueInfo { ty: t, name: None };
        Function {
            name: "f".into(),
            params: vec![ValueId(0), ValueId(1)],
            ret: Type::Void,
            blocks: vec![
                Block {
                    id: BlockId(0),
                    insts: vec![
                        call(Some(2), FIND_F, vec![v(0), v(1)]),
                        Inst {
                            result: Some(ValueId(3)),
                            kind: InstKind::ICmp(IPred::Sge, v(2), Operand::ConstI64(0)),
                            origin: None,
                        },
                    ],
                    term: Terminator::CondBr {
                        cond: v(3),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                },
                Block {
                    id: BlockId(1),
                    insts: bb1,
                    term: Terminator::Ret(None),
                },
                Block {
                    id: BlockId(2),
                    insts: vec![],
                    term: Terminator::Ret(None),
                },
            ],
            values: (0..6).map(|_| ty(Type::I64)).collect(),
            current_origin: None,
        }
    }

    #[test]
    fn repeated_find_in_the_successor_reuses_the_index() {
        let mut f = counting(vec![]);
        let mut stats = MapFindCseStats::default();
        share_in_function(&mut f, &fids(), &mut stats);
        assert_eq!(
            stats,
            MapFindCseStats {
                finds_shared: 1,
                set_hints: 1
            }
        );
        let bb1 = &f.blocks[1].insts;
        assert_eq!(bb1.len(), 1);
        assert_eq!(
            bb1[0].kind,
            InstKind::Call(
                SET_F,
                vec![v(0), v(1), v(2), Operand::ConstI64(2), Operand::ConstI64(7)]
            )
        );
    }

    #[test]
    fn an_opaque_call_keeps_the_find_but_not_the_hint() {
        let mut f = counting(vec![call(None, OPAQUE_F, vec![])]);
        let mut stats = MapFindCseStats::default();
        share_in_function(&mut f, &fids(), &mut stats);
        // the set's hint is checked by the kernel, so it may cross the call
        assert_eq!(
            stats,
            MapFindCseStats {
                finds_shared: 0,
                set_hints: 1
            }
        );
        assert_eq!(f.blocks[1].insts.len(), 3);
    }

    #[test]
    fn a_redefined_key_stops_the_walk() {
        let redefine = Inst {
            result: Some(ValueId(1)),
            kind: InstKind::Copy(Type::I64, v(5)),
            origin: None,
        };
        let mut f = counting(vec![redefine]);
        let mut stats = MapFindCseStats::default();
        share_in_function(&mut f, &fids(), &mut stats);
        // the find after the redefinition cannot reach the first one;
        // the set still takes that later find, which reads the new key
        assert_eq!(
            stats,
            MapFindCseStats {
                finds_shared: 0,
                set_hints: 1
            }
        );
        let InstKind::Call(_, args) = &f.blocks[1].insts[2].kind else {
            panic!("set call expected");
        };
        assert_eq!(args[2], v(4));
    }
}
