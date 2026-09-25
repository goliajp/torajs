//! Heap-to-stack promotion of non-escaping header cells — the SSA
//! analogue of LLVM Attributor's HeapToStack / HotSpot's scalar
//! replacement entry step. After inlining + devirt a closure env or
//! object literal whose every use sits in one function (the
//! `loopSum` env once `add` is inlined) still pays `obj_alloc` +
//! `obj_drop_sized`, and — worse — its captured fields reach the loop
//! as opaque heap loads, so the interval analysis sees `offset` as
//! Top and float demotion refuses the accumulator. Turning the cell
//! into an `alloca_bytes` hands its fields to the slot passes
//! (`slot_forward` / `mem2reg` / `phi_promote`), which forward the
//! stored constant into the loop.
//!
//! A candidate is `%b = call obj_alloc(N)` with every use of `%b` in
//! this allowlist (anything else keeps the heap cell):
//!
//! * address position of a constant-offset `Load` / `Store` (never
//!   the stored value)
//! * `icmp eq|ne %b, x` against anything but `%b` — a fresh
//!   allocation equals no other pointer, so it folds to a constant
//! * the paired `obj_drop_sized(%b, N)` — the free itself, deleted
//! * a runtime seam that is a no-op for a cell the runtime never saw:
//!   the unbuffer seams check `FLAG_BUFFERED` (only `cycle_buffer`
//!   sets it, and `%b` never reaches it), `weakref_target_dying`
//!   looks up a registry `%b` was never entered into, and the props
//!   seams release the expando bag at +24 — admitted only when every
//!   store into that word is a null constant
//!
//! The refcount header stays in memory: the inlined rc_dec still
//! counts it down and runs the field drops, only the final free is
//! gone. Frame slots are per-instruction, so an allocation inside a
//! loop reuses one slot — sound because `%b` is never copied, so no
//! previous iteration's cell is reachable.
//!
//! Runs after inlining + devirt (which expose the single-function
//! shape) and before `slot_forward`. `TORAJS_HEAP_TO_STACK_OFF=1`
//! skips (bisect gate).

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{FuncId, Function, IPred, InstKind, Module, Operand, Terminator, ValueId};

use crate::rc_peephole::visit_value_operands;
use crate::slot_forward::{resolve, rewrite_operands};

const OBJ_ALLOC: &str = "__torajs_obj_alloc";
const OBJ_DROP_SIZED: &str = "__torajs_obj_drop_sized";
/// Seams that only touch runtime state `%b` cannot be in.
const INERT_SEAMS: &[&str] = &[
    "__torajs_closure_unbuffer_slow",
    "__torajs_obj_unbuffer_slow",
    "__torajs_weakref_target_dying",
];
/// Seams that release the expando bag at `PROPS_OFF`.
const PROPS_SEAMS: &[&str] = &[
    "__torajs_closure_drop_props_slow",
    "__torajs_obj_props_drop_slow",
    "__torajs_obj_buffer_slow",
];
/// `OBJ_PROPS_OFF` / `CLOSURE_PROPS_OFF` in torajs-core `ssa_lower.rs`.
const PROPS_OFF: u64 = 24;
/// Frame budget per promoted cell (LLVM HeapToStack's default is 128;
/// a closure env with a few captures or a small literal fits).
const MAX_BYTES: i64 = 256;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct HeapToStackStats {
    /// `obj_alloc` calls rewritten to `alloca_bytes`.
    pub promoted: u32,
    /// Allocation candidates kept on the heap by a use outside the
    /// allowlist.
    pub rejected: u32,
    /// Pointer comparisons folded to constants.
    pub cmps_folded: u32,
    /// `obj_drop_sized` / seam calls deleted.
    pub calls_removed: u32,
}

struct Fids {
    alloc: FuncId,
    drop_sized: Option<FuncId>,
    inert: HashSet<FuncId>,
    props: HashSet<FuncId>,
}

pub fn promote_module(module: &mut Module) -> HeapToStackStats {
    let mut stats = HeapToStackStats::default();
    let fid = |name: &str| {
        module
            .funcs
            .iter()
            .position(|f| f.name == name)
            .map(|i| FuncId(i as u32))
    };
    let Some(alloc) = fid(OBJ_ALLOC) else {
        return stats;
    };
    let fids = Fids {
        alloc,
        drop_sized: fid(OBJ_DROP_SIZED),
        inert: INERT_SEAMS.iter().filter_map(|n| fid(n)).collect(),
        props: PROPS_SEAMS.iter().filter_map(|n| fid(n)).collect(),
    };
    for func in module.funcs.iter_mut() {
        if !func.is_declaration() {
            promote_in_function(func, &fids, &mut stats);
        }
    }
    stats
}

fn promote_in_function(func: &mut Function, fids: &Fids, stats: &mut HeapToStackStats) {
    let mut cands: HashMap<ValueId, i64> = HashMap::new();
    for block in &func.blocks {
        for inst in &block.insts {
            if let (InstKind::Call(f, args), Some(r)) = (&inst.kind, inst.result)
                && *f == fids.alloc
                && let [Operand::ConstI64(n)] = args.as_slice()
                && (1..=MAX_BYTES).contains(n)
            {
                cands.insert(r, *n);
            }
        }
    }
    if cands.is_empty() {
        return;
    }
    let mut bad: HashSet<ValueId> = HashSet::new();
    let mut props_seam_users: HashSet<ValueId> = HashSet::new();
    let mut props_dirty: HashSet<ValueId> = HashSet::new();
    let mark = |op: &Operand, bad: &mut HashSet<ValueId>| {
        if let Operand::Value(v) = op
            && cands.contains_key(v)
        {
            bad.insert(*v);
        }
    };
    for block in &func.blocks {
        for inst in &block.insts {
            match &inst.kind {
                InstKind::Load(_, _, _) => {}
                InstKind::Store(val, ptr, off) => {
                    mark(val, &mut bad);
                    if let Operand::Value(b) = ptr
                        && cands.contains_key(b)
                        && touches_props(*off)
                        && !(*off == PROPS_OFF && is_null(val))
                    {
                        props_dirty.insert(*b);
                    }
                }
                InstKind::ICmp(IPred::Eq | IPred::Ne, a, c) if a != c => {}
                InstKind::Call(f, args) => match args.as_slice() {
                    [Operand::Value(b), Operand::ConstI64(n)]
                        if Some(*f) == fids.drop_sized && cands.get(b) == Some(n) => {}
                    [Operand::Value(_)] if fids.inert.contains(f) => {}
                    [Operand::Value(b)] if fids.props.contains(f) => {
                        props_seam_users.insert(*b);
                    }
                    _ => args.iter().for_each(|a| mark(a, &mut bad)),
                },
                other => visit_value_operands(other, |v| mark(&Operand::Value(v), &mut bad)),
            }
        }
        match &block.term {
            Terminator::Ret(Some(op)) => mark(op, &mut bad),
            Terminator::CondBr { cond, .. } => mark(cond, &mut bad),
            Terminator::Br(_) | Terminator::Ret(None) | Terminator::Unreachable => {}
        }
    }
    bad.extend(props_seam_users.intersection(&props_dirty));
    stats.rejected += bad.len() as u32;
    cands.retain(|v, _| !bad.contains(v));
    if cands.is_empty() {
        return;
    }
    apply(func, &cands, fids, stats);
}

fn apply(
    func: &mut Function,
    cands: &HashMap<ValueId, i64>,
    fids: &Fids,
    stats: &mut HeapToStackStats,
) {
    let is_cand = |op: &Operand| matches!(op, Operand::Value(v) if cands.contains_key(v));
    let mut replace: HashMap<ValueId, Operand> = HashMap::new();
    for block in func.blocks.iter_mut() {
        block.insts.retain_mut(|inst| match &inst.kind {
            InstKind::Call(f, args) if *f == fids.alloc => {
                if let (Some(r), [Operand::ConstI64(n)]) = (inst.result, args.as_slice())
                    && cands.contains_key(&r)
                {
                    inst.kind = InstKind::AllocaBytes(*n as u64);
                    stats.promoted += 1;
                }
                true
            }
            InstKind::Call(_, args) if args.first().is_some_and(is_cand) => {
                stats.calls_removed += 1;
                false
            }
            InstKind::ICmp(pred, a, b) if is_cand(a) || is_cand(b) => {
                if let Some(r) = inst.result {
                    replace.insert(r, Operand::ConstBool(*pred == IPred::Ne));
                }
                stats.cmps_folded += 1;
                false
            }
            _ => true,
        });
    }
    if replace.is_empty() {
        return;
    }
    for block in func.blocks.iter_mut() {
        for inst in block.insts.iter_mut() {
            rewrite_operands(&mut inst.kind, &replace);
        }
        match &mut block.term {
            Terminator::Ret(Some(op)) => *op = resolve(op, &replace),
            Terminator::CondBr { cond, .. } => *cond = resolve(cond, &replace),
            _ => {}
        }
    }
}

/// An 8-byte store at `off` writes some byte of the props word.
fn touches_props(off: u64) -> bool {
    off + 8 > PROPS_OFF && off < PROPS_OFF + 8
}

fn is_null(op: &Operand) -> bool {
    matches!(op, Operand::ConstPtrNull | Operand::ConstI64(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, BlockId, Inst, Type, ValueInfo};

    fn decl(name: &str) -> Function {
        Function {
            name: name.into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![],
            values: vec![],
            current_origin: None,
        }
    }

    fn val(r: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(r)),
            kind,
            origin: None,
        }
    }

    fn void(kind: InstKind) -> Inst {
        Inst {
            result: None,
            kind,
            origin: None,
        }
    }

    const B: Operand = Operand::Value(ValueId(0));

    /// fids: 0 obj_alloc · 1 obj_drop_sized · 2 closure_drop_props_slow
    /// · 3 closure_unbuffer_slow · 4 an unrelated callee
    fn module_with(body: Vec<Inst>) -> Module {
        let main = Function {
            name: "main".into(),
            params: vec![],
            ret: Type::I64,
            blocks: vec![Block {
                id: BlockId(0),
                insts: body,
                term: Terminator::Ret(Some(Operand::Value(ValueId(1)))),
            }],
            values: vec![
                ValueInfo {
                    ty: Type::Ptr,
                    name: None
                };
                3
            ],
            current_origin: None,
        };
        Module {
            funcs: vec![
                decl(OBJ_ALLOC),
                decl(OBJ_DROP_SIZED),
                decl("__torajs_closure_drop_props_slow"),
                decl("__torajs_closure_unbuffer_slow"),
                decl("__torajs_other"),
                main,
            ],
            ..Default::default()
        }
    }

    fn cell(props: Operand, tail: Vec<Inst>) -> Vec<Inst> {
        let mut v = vec![
            val(0, InstKind::Call(FuncId(0), vec![Operand::ConstI64(56)])),
            void(InstKind::Store(Operand::ConstI32(1), B, 0)),
            void(InstKind::Store(props, B, PROPS_OFF)),
            void(InstKind::Store(Operand::ConstI64(2), B, 48)),
            val(1, InstKind::Load(Type::I64, B, 48)),
            val(2, InstKind::ICmp(IPred::Eq, B, Operand::ConstPtrNull)),
            void(InstKind::Call(FuncId(3), vec![B])),
            void(InstKind::Call(FuncId(2), vec![B])),
            void(InstKind::Call(FuncId(1), vec![B, Operand::ConstI64(56)])),
        ];
        v.extend(tail);
        v
    }

    #[test]
    fn closure_env_shape_promotes() {
        let mut m = module_with(cell(Operand::ConstPtrNull, vec![]));
        let stats = promote_module(&mut m);
        assert_eq!(stats.promoted, 1);
        assert_eq!(stats.cmps_folded, 1);
        assert_eq!(stats.calls_removed, 3);
        let insts = &m.funcs[5].blocks[0].insts;
        assert!(matches!(insts[0].kind, InstKind::AllocaBytes(56)));
        assert!(!insts.iter().any(|i| matches!(i.kind, InstKind::Call(..))));
    }

    #[test]
    fn pointer_handed_to_other_call_stays_on_heap() {
        let tail = vec![void(InstKind::Call(FuncId(4), vec![B]))];
        let mut m = module_with(cell(Operand::ConstPtrNull, tail));
        let stats = promote_module(&mut m);
        assert_eq!((stats.promoted, stats.rejected), (0, 1));
    }

    #[test]
    fn live_props_bag_keeps_its_release() {
        // a non-null bag at +24 means the props seam has work to do
        let mut m = module_with(cell(Operand::Value(ValueId(2)), vec![]));
        let stats = promote_module(&mut m);
        assert_eq!((stats.promoted, stats.rejected), (0, 1));
    }
}
