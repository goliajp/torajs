//! Indirect-call promotion (devirtualization) — rewrite a
//! `CallIndirect` whose function pointer provably resolves to a single
//! static `FnAddr` into a direct `Call`, so the inliner can take over.
//! This is the textbook LLVM IndirectCallPromotion shape restricted to
//! its statically-provable sub-case (no profile data, no speculation —
//! a site is either proven single-target or left untouched).
//!
//! Three resolvable callee-pointer shapes, all whole-function:
//!
//! 1. **Direct** — the pointer operand's def is `FnAddr(F)`.
//! 2. **Fn-pointer slot** — `%p = load <fnsig-slot>` where the slot is
//!    a non-escaped alloca whose single non-null dominating store is a
//!    `FnAddr(F)` value. (Exposed by the inliner splicing a fn-typed
//!    param into the caller, e.g. `applyAll(xs, add1)`.)
//! 3. **Closure env field** — `%q = load ptr, %env +8|+16` where
//!    `%env = load closure, <slot>` from a non-escaped closure-typed
//!    alloca whose single dominating store is an `__torajs_obj_alloc`
//!    result, and that object's `+8` (code ptr) / `+16` (drop ptr)
//!    field has exactly one in-function store, a `FnAddr(F)`.
//!
//! Soundness leans on two facts:
//!
//! * A non-escaped alloca (address used only as a `Load`/`Store`
//!   address — `rc_peephole::collect_unescaped_slots`) cannot be
//!   written through an alias, so its unique non-null store is the
//!   only value a dominated load can observe.
//! * The closure env code/drop fn-ptr fields are written exactly once,
//!   at construction (`ssa_lower` closure layout, `CLOSURE_FN_ADDR_OFF`
//!   / `CLOSURE_DROP_FN_OFF`); no lowering path mutates them
//!   afterwards, so the env object escaping (it is passed to the call
//!   itself) cannot invalidate the binding. A `StoreDyn` through the
//!   env object disqualifies it anyway (cheap, conservative).
//!
//! Every guard errs toward leaving the site indirect: a missed
//! promotion costs one indirect branch, a wrong one calls the wrong
//! function. Signatures must match exactly (`Module::signature(sig)`
//! vs the target's params/ret) or the site is skipped.
//!
//! After a rewrite the dangling pointer-load chain (`load`/`fn_addr`
//! whose only consumer was the promoted site) is swept so the hot loop
//! does not keep paying for loads that feed nothing.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{
    FuncId, Function, InstKind, Module, Operand, SigId, Terminator, Type, ValueId,
};
use torajs_core::ssa_lower::{CLOSURE_DROP_FN_OFF, CLOSURE_FN_ADDR_OFF};

use crate::dominator::DominatorTree;
use crate::rc_peephole::{collect_unescaped_slots, visit_value_operands};

/// Allocator intrinsic backing closure env blocks. A slot value whose
/// def is a call to this (reached through a closure-typed slot) is a
/// closure env object.
const OBJ_ALLOC: &str = "__torajs_obj_alloc";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DevirtStats {
    /// `CallIndirect` sites seen across the module.
    pub scanned: u32,
    /// Sites rewritten to a direct `Call`.
    pub rewritten: u32,
    /// Sites whose pointer chain did not resolve to a unique `FnAddr`.
    pub skipped_unresolved: u32,
    /// Resolved sites dropped because the interned `SigId` signature
    /// does not exactly match the target function's params/ret.
    pub skipped_sig_mismatch: u32,
    /// Dead `Load`/`FnAddr` insts removed from promoted pointer chains.
    pub dead_chain_removed: u32,
}

/// One store into a tracked location: position + stored operand.
#[derive(Debug, Clone, Copy)]
struct StoreSite {
    blk: usize,
    idx: usize,
    val: Operand,
}

/// Read-only per-function fact tables the resolver queries.
struct FnIndex {
    /// Module-level fid of `__torajs_obj_alloc`, if declared.
    obj_alloc: Option<FuncId>,
    /// Non-escaped alloca slots (address only ever a load/store addr).
    slots: HashSet<ValueId>,
    /// def position of every result-bearing inst.
    defs: HashMap<ValueId, (usize, usize)>,
    /// Stores at offset 0 into a tracked slot.
    slot_stores: HashMap<ValueId, Vec<StoreSite>>,
    /// Slots with a store at a non-zero offset — reaching-value logic
    /// below only models whole-slot stores, so these are off-limits.
    slot_off_disq: HashSet<ValueId>,
    /// Stores into non-slot bases, keyed by (base, byte offset).
    obj_stores: HashMap<(ValueId, u64), Vec<StoreSite>>,
    /// Bases written through `StoreDyn` (dynamic offset — could hit
    /// any field, disqualifies the base).
    dyn_written: HashSet<ValueId>,
    dom: DominatorTree,
}

impl FnIndex {
    fn build(func: &Function, obj_alloc: Option<FuncId>) -> Self {
        let slots = collect_unescaped_slots(func);
        let mut defs = HashMap::new();
        let mut slot_stores: HashMap<ValueId, Vec<StoreSite>> = HashMap::new();
        let mut slot_off_disq = HashSet::new();
        let mut obj_stores: HashMap<(ValueId, u64), Vec<StoreSite>> = HashMap::new();
        let mut dyn_written = HashSet::new();
        for (bi, block) in func.blocks.iter().enumerate() {
            for (ii, inst) in block.insts.iter().enumerate() {
                if let Some(v) = inst.result {
                    defs.insert(v, (bi, ii));
                }
                match &inst.kind {
                    InstKind::Store(val, Operand::Value(base), off) => {
                        let site = StoreSite {
                            blk: bi,
                            idx: ii,
                            val: *val,
                        };
                        if slots.contains(base) {
                            if *off == 0 {
                                slot_stores.entry(*base).or_default().push(site);
                            } else {
                                slot_off_disq.insert(*base);
                            }
                        } else {
                            obj_stores.entry((*base, *off)).or_default().push(site);
                        }
                    }
                    InstKind::StoreDyn(_, Operand::Value(base), _) => {
                        dyn_written.insert(*base);
                    }
                    _ => {}
                }
            }
        }
        Self {
            obj_alloc,
            slots,
            defs,
            slot_stores,
            slot_off_disq,
            obj_stores,
            dyn_written,
            dom: DominatorTree::compute(func),
        }
    }

    /// `store` must execute before `use_pos` on every path: same block
    /// and earlier, or its block strictly dominates the use's block.
    fn store_dominates(&self, func: &Function, s: &StoreSite, use_pos: (usize, usize)) -> bool {
        if s.blk == use_pos.0 {
            return s.idx < use_pos.1;
        }
        self.dom
            .dominates(func.blocks[s.blk].id, func.blocks[use_pos.0].id)
    }

    /// The single value a load at `use_pos` can observe, given
    /// `sites` = every store to the loaded location. Exactly one
    /// non-null store is allowed and it must dominate the load; null
    /// stores are tolerated only as the lowering's init pattern (same
    /// block as, and earlier than, the real store).
    fn unique_reaching_value(
        &self,
        func: &Function,
        sites: &[StoreSite],
        use_pos: (usize, usize),
    ) -> Option<ValueId> {
        let (nulls, real): (Vec<_>, Vec<_>) = sites
            .iter()
            .partition(|s| matches!(s.val, Operand::ConstPtrNull));
        let [only] = real.as_slice() else { return None };
        if !self.store_dominates(func, only, use_pos) {
            return None;
        }
        if !nulls.iter().all(|n| n.blk == only.blk && n.idx < only.idx) {
            return None;
        }
        match only.val {
            Operand::Value(v) => Some(v),
            _ => None,
        }
    }

    fn def_kind<'f>(&self, func: &'f Function, v: ValueId) -> Option<&'f InstKind> {
        let (bi, ii) = *self.defs.get(&v)?;
        Some(&func.blocks[bi].insts[ii].kind)
    }

    /// Resolve a `CallIndirect` pointer operand to a unique static
    /// target, or None to leave the site untouched.
    fn resolve_callee(&self, func: &Function, ptr: ValueId) -> Option<FuncId> {
        let (pb, pi) = *self.defs.get(&ptr)?;
        match &func.blocks[pb].insts[pi].kind {
            // shape 1 — direct
            InstKind::FnAddr(f) => Some(*f),
            // shape 2 — fn-pointer slot
            InstKind::Load(_, Operand::Value(base), 0)
                if self.slots.contains(base) && !self.slot_off_disq.contains(base) =>
            {
                let sites = self.slot_stores.get(base)?;
                let v = self.unique_reaching_value(func, sites, (pb, pi))?;
                match self.def_kind(func, v)? {
                    InstKind::FnAddr(f) => Some(*f),
                    _ => None,
                }
            }
            // shape 3 — closure env code/drop field
            InstKind::Load(_, Operand::Value(env), off)
                if *off == CLOSURE_FN_ADDR_OFF || *off == CLOSURE_DROP_FN_OFF =>
            {
                let obj = self.resolve_closure_env(func, *env)?;
                let sites = self.obj_stores.get(&(obj, *off))?;
                let v = self.unique_reaching_value(func, sites, (pb, pi))?;
                match self.def_kind(func, v)? {
                    InstKind::FnAddr(f) => Some(*f),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// `env` must be a closure-typed load from a non-escaped slot whose
    /// unique reaching value is an `__torajs_obj_alloc` result not
    /// dynamically written. The closure-typed slot is the gate that
    /// imports the construction-immutability invariant for +8/+16.
    fn resolve_closure_env(&self, func: &Function, env: ValueId) -> Option<ValueId> {
        let alloc = self.obj_alloc?;
        let (eb, ei) = *self.defs.get(&env)?;
        let InstKind::Load(ty, Operand::Value(slot), 0) = &func.blocks[eb].insts[ei].kind else {
            return None;
        };
        if !matches!(ty, Type::Closure(_)) {
            return None;
        }
        if !self.slots.contains(slot) || self.slot_off_disq.contains(slot) {
            return None;
        }
        let sites = self.slot_stores.get(slot)?;
        let obj = self.unique_reaching_value(func, sites, (eb, ei))?;
        if self.dyn_written.contains(&obj) {
            return None;
        }
        match self.def_kind(func, obj)? {
            InstKind::Call(fid, _) if *fid == alloc => Some(obj),
            _ => None,
        }
    }
}

/// Exact-match signature check: the interned `SigId` signature must
/// equal the target's param types and return type. ABI-compatible but
/// non-identical types (e.g. `ptr` vs `closure`) are rejected — the
/// `skipped_sig_mismatch` counter exposes them for a deliberate
/// follow-up widening if they ever hold real traffic.
fn sig_matches(module: &Module, target: FuncId, sig: SigId) -> bool {
    let f = &module.funcs[target.0 as usize];
    let (params, ret) = module.signature(sig);
    f.params.len() == params.len()
        && f.ret == *ret
        && f.params
            .iter()
            .zip(params)
            .all(|(vid, ty)| f.values[vid.0 as usize].ty == *ty)
}

/// Run indirect-call promotion over every function body. Pure SSA →
/// SSA rewrite; behaviour-preserving by construction (only provably
/// single-target sites are touched).
pub fn devirtualize_module(module: &mut Module) -> DevirtStats {
    let mut stats = DevirtStats::default();
    let obj_alloc = module
        .funcs
        .iter()
        .position(|f| f.name == OBJ_ALLOC)
        .map(|i| FuncId(i as u32));
    for f_idx in 0..module.funcs.len() {
        if module.funcs[f_idx].is_declaration() {
            continue;
        }
        // plan (read-only) — collect (blk, idx, target, old ptr)
        let mut rewrites: Vec<(usize, usize, FuncId, ValueId)> = Vec::new();
        {
            let func = &module.funcs[f_idx];
            let index = FnIndex::build(func, obj_alloc);
            for (bi, block) in func.blocks.iter().enumerate() {
                for (ii, inst) in block.insts.iter().enumerate() {
                    let InstKind::CallIndirect(sig, Operand::Value(ptr), _) = &inst.kind else {
                        continue;
                    };
                    stats.scanned += 1;
                    match index.resolve_callee(func, *ptr) {
                        Some(target) => {
                            if sig_matches(module, target, *sig) {
                                rewrites.push((bi, ii, target, *ptr));
                            } else {
                                stats.skipped_sig_mismatch += 1;
                            }
                        }
                        None => stats.skipped_unresolved += 1,
                    }
                }
            }
        }
        if rewrites.is_empty() {
            continue;
        }
        // apply + sweep (mutating)
        let func = &mut module.funcs[f_idx];
        let mut chain_roots: Vec<ValueId> = Vec::new();
        for (bi, ii, target, ptr) in rewrites {
            let inst = &mut func.blocks[bi].insts[ii];
            let InstKind::CallIndirect(_, _, args) = &inst.kind else {
                continue;
            };
            inst.kind = InstKind::Call(target, args.clone());
            stats.rewritten += 1;
            chain_roots.push(ptr);
        }
        stats.dead_chain_removed += sweep_dead_chain(func, chain_roots);
    }
    stats
}

/// Count uses of `v` across every inst operand and terminator.
fn count_uses(func: &Function, v: ValueId) -> u32 {
    let mut n = 0;
    for block in &func.blocks {
        for inst in &block.insts {
            visit_value_operands(&inst.kind, |u| {
                if u == v {
                    n += 1;
                }
            });
        }
        let term_op = match &block.term {
            Terminator::Ret(Some(op)) => Some(op),
            Terminator::CondBr { cond, .. } => Some(cond),
            _ => None,
        };
        if let Some(Operand::Value(u)) = term_op {
            if *u == v {
                n += 1;
            }
        }
    }
    n
}

/// Remove now-dead `Load` / `FnAddr` insts left behind by promotion,
/// walking each pointer chain toward its base (a swept load exposes
/// its base address value as the next candidate). Only pure,
/// side-effect-free kinds are eligible, so removal can never change
/// behaviour.
fn sweep_dead_chain(func: &mut Function, roots: Vec<ValueId>) -> u32 {
    let mut removed = 0;
    let mut frontier = roots;
    while let Some(v) = frontier.pop() {
        if count_uses(func, v) != 0 {
            continue;
        }
        let mut found: Option<(usize, usize, Option<ValueId>)> = None;
        'scan: for (bi, block) in func.blocks.iter().enumerate() {
            for (ii, inst) in block.insts.iter().enumerate() {
                if inst.result == Some(v) {
                    match &inst.kind {
                        InstKind::Load(_, Operand::Value(base), _) => {
                            found = Some((bi, ii, Some(*base)));
                        }
                        InstKind::FnAddr(_) => found = Some((bi, ii, None)),
                        _ => {}
                    }
                    break 'scan;
                }
            }
        }
        if let Some((bi, ii, base)) = found {
            func.blocks[bi].insts.remove(ii);
            removed += 1;
            if let Some(b) = base {
                frontier.push(b);
            }
        }
    }
    removed
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

    /// A defined target fn whose params/ret match `sig_params`/`ret`.
    fn target(name: &str, sig_params: &[Type], ret: Type) -> Function {
        Function {
            name: name.into(),
            params: (0..sig_params.len() as u32).map(ValueId).collect(),
            ret,
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![],
                term: Terminator::Ret(None),
            }],
            values: sig_params
                .iter()
                .map(|ty| ValueInfo {
                    ty: *ty,
                    name: None,
                })
                .collect(),
            current_origin: None,
        }
    }

    fn val(result: u32, ty: Type, kind: InstKind, values: &mut Vec<ValueInfo>) -> Inst {
        assert_eq!(values.len(), result as usize);
        values.push(ValueInfo { ty, name: None });
        Inst {
            result: Some(ValueId(result)),
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

    /// Module layout: funcs[0] = obj_alloc decl, funcs[1] = target fn
    /// `cb`, funcs[2] = single-block body under test.
    /// signatures[0] = (sig_params, sig_ret).
    fn module_with(
        insts: Vec<Inst>,
        values: Vec<ValueInfo>,
        cb_params: &[Type],
        cb_ret: Type,
        sig_params: Vec<Type>,
        sig_ret: Type,
    ) -> Module {
        Module {
            funcs: vec![
                decl(OBJ_ALLOC),
                target("cb", cb_params, cb_ret),
                Function {
                    name: "main".into(),
                    params: vec![],
                    ret: Type::Void,
                    blocks: vec![Block {
                        id: BlockId(0),
                        insts,
                        term: Terminator::Ret(None),
                    }],
                    values,
                    current_origin: None,
                },
            ],
            signatures: vec![(sig_params, sig_ret)],
            ..Default::default()
        }
    }

    const ALLOC: FuncId = FuncId(0);
    const CB: FuncId = FuncId(1);
    const SIG: SigId = SigId(0);

    fn body(m: &Module) -> &[Inst] {
        &m.funcs[2].blocks[0].insts
    }

    #[test]
    fn direct_fnaddr_promoted() {
        let mut vals = vec![];
        let insts = vec![
            val(0, Type::FnSig(SIG), InstKind::FnAddr(CB), &mut vals),
            val(
                1,
                Type::I64,
                InstKind::CallIndirect(SIG, Operand::Value(ValueId(0)), vec![Operand::ConstI64(7)]),
                &mut vals,
            ),
        ];
        let mut m = module_with(
            insts,
            vals,
            &[Type::I64],
            Type::I64,
            vec![Type::I64],
            Type::I64,
        );
        let stats = devirtualize_module(&mut m);
        assert_eq!(stats.rewritten, 1);
        // fn_addr swept (its only use was the promoted site)
        assert_eq!(stats.dead_chain_removed, 1);
        assert!(matches!(body(&m)[0].kind, InstKind::Call(CB, _)));
    }

    #[test]
    fn fnsig_slot_promoted() {
        let mut vals = vec![];
        let insts = vec![
            val(0, Type::Ptr, InstKind::Alloca(Type::FnSig(SIG)), &mut vals),
            val(1, Type::FnSig(SIG), InstKind::FnAddr(CB), &mut vals),
            void(InstKind::Store(
                Operand::Value(ValueId(1)),
                Operand::Value(ValueId(0)),
                0,
            )),
            val(
                2,
                Type::FnSig(SIG),
                InstKind::Load(Type::FnSig(SIG), Operand::Value(ValueId(0)), 0),
                &mut vals,
            ),
            val(
                3,
                Type::I64,
                InstKind::CallIndirect(SIG, Operand::Value(ValueId(2)), vec![Operand::ConstI64(1)]),
                &mut vals,
            ),
        ];
        let mut m = module_with(
            insts,
            vals,
            &[Type::I64],
            Type::I64,
            vec![Type::I64],
            Type::I64,
        );
        let stats = devirtualize_module(&mut m);
        assert_eq!(stats.rewritten, 1);
        // the slot load is dead after promotion; the FnAddr is still
        // live through the store, so exactly one inst goes.
        assert_eq!(stats.dead_chain_removed, 1);
        assert!(
            body(&m)
                .iter()
                .all(|i| !matches!(i.kind, InstKind::CallIndirect(..)))
        );
    }

    #[test]
    fn closure_env_field_promoted() {
        // mirrors ssa_lower closure construction: env obj from
        // obj_alloc, fn_addr stored at +8, obj parked in a
        // closure-typed slot, loop-side load slot → load +8 → call.
        let mut vals = vec![];
        let insts = vec![
            val(
                0,
                Type::Ptr,
                InstKind::Alloca(Type::Closure(SIG)),
                &mut vals,
            ),
            val(
                1,
                Type::Ptr,
                InstKind::Call(ALLOC, vec![Operand::ConstI64(48)]),
                &mut vals,
            ),
            val(2, Type::FnSig(SIG), InstKind::FnAddr(CB), &mut vals),
            void(InstKind::Store(
                Operand::Value(ValueId(2)),
                Operand::Value(ValueId(1)),
                CLOSURE_FN_ADDR_OFF,
            )),
            void(InstKind::Store(
                Operand::ConstPtrNull,
                Operand::Value(ValueId(0)),
                0,
            )),
            void(InstKind::Store(
                Operand::Value(ValueId(1)),
                Operand::Value(ValueId(0)),
                0,
            )),
            val(
                3,
                Type::Closure(SIG),
                InstKind::Load(Type::Closure(SIG), Operand::Value(ValueId(0)), 0),
                &mut vals,
            ),
            val(
                4,
                Type::Ptr,
                InstKind::Load(Type::Ptr, Operand::Value(ValueId(3)), CLOSURE_FN_ADDR_OFF),
                &mut vals,
            ),
            val(
                5,
                Type::I64,
                InstKind::CallIndirect(
                    SIG,
                    Operand::Value(ValueId(4)),
                    vec![Operand::Value(ValueId(3)), Operand::ConstI64(7)],
                ),
                &mut vals,
            ),
        ];
        let mut m = module_with(
            insts,
            vals,
            &[Type::Ptr, Type::I64],
            Type::I64,
            vec![Type::Ptr, Type::I64],
            Type::I64,
        );
        let stats = devirtualize_module(&mut m);
        assert_eq!(stats.rewritten, 1);
        // the +8 load dies; the env load (%3) stays live as call arg.
        assert_eq!(stats.dead_chain_removed, 1);
        let call = body(&m)
            .iter()
            .find(|i| matches!(i.kind, InstKind::Call(CB, _)))
            .expect("promoted call");
        let InstKind::Call(_, args) = &call.kind else {
            unreachable!()
        };
        assert_eq!(args.len(), 2);
        assert!(matches!(args[0], Operand::Value(ValueId(3))));
    }

    #[test]
    fn two_stores_block_promotion() {
        let mut vals = vec![];
        let insts = vec![
            val(0, Type::Ptr, InstKind::Alloca(Type::FnSig(SIG)), &mut vals),
            val(1, Type::FnSig(SIG), InstKind::FnAddr(CB), &mut vals),
            val(2, Type::FnSig(SIG), InstKind::FnAddr(FuncId(0)), &mut vals),
            void(InstKind::Store(
                Operand::Value(ValueId(1)),
                Operand::Value(ValueId(0)),
                0,
            )),
            void(InstKind::Store(
                Operand::Value(ValueId(2)),
                Operand::Value(ValueId(0)),
                0,
            )),
            val(
                3,
                Type::FnSig(SIG),
                InstKind::Load(Type::FnSig(SIG), Operand::Value(ValueId(0)), 0),
                &mut vals,
            ),
            val(
                4,
                Type::I64,
                InstKind::CallIndirect(SIG, Operand::Value(ValueId(3)), vec![Operand::ConstI64(1)]),
                &mut vals,
            ),
        ];
        let mut m = module_with(
            insts,
            vals,
            &[Type::I64],
            Type::I64,
            vec![Type::I64],
            Type::I64,
        );
        let stats = devirtualize_module(&mut m);
        assert_eq!(stats.rewritten, 0);
        assert_eq!(stats.skipped_unresolved, 1);
    }

    #[test]
    fn escaped_slot_blocks_promotion() {
        // slot address passed to a call → escape analysis drops it.
        let mut vals = vec![];
        let insts = vec![
            val(0, Type::Ptr, InstKind::Alloca(Type::FnSig(SIG)), &mut vals),
            val(1, Type::FnSig(SIG), InstKind::FnAddr(CB), &mut vals),
            void(InstKind::Store(
                Operand::Value(ValueId(1)),
                Operand::Value(ValueId(0)),
                0,
            )),
            void(InstKind::Call(ALLOC, vec![Operand::Value(ValueId(0))])),
            val(
                2,
                Type::FnSig(SIG),
                InstKind::Load(Type::FnSig(SIG), Operand::Value(ValueId(0)), 0),
                &mut vals,
            ),
            val(
                3,
                Type::I64,
                InstKind::CallIndirect(SIG, Operand::Value(ValueId(2)), vec![Operand::ConstI64(1)]),
                &mut vals,
            ),
        ];
        let mut m = module_with(
            insts,
            vals,
            &[Type::I64],
            Type::I64,
            vec![Type::I64],
            Type::I64,
        );
        let stats = devirtualize_module(&mut m);
        assert_eq!(stats.rewritten, 0);
        assert_eq!(stats.skipped_unresolved, 1);
    }

    #[test]
    fn sig_mismatch_skipped() {
        let mut vals = vec![];
        let insts = vec![
            val(0, Type::FnSig(SIG), InstKind::FnAddr(CB), &mut vals),
            val(
                1,
                Type::I64,
                InstKind::CallIndirect(SIG, Operand::Value(ValueId(0)), vec![Operand::ConstI64(7)]),
                &mut vals,
            ),
        ];
        // target ret I64 but interned sig says Void → reject.
        let mut m = module_with(
            insts,
            vals,
            &[Type::I64],
            Type::I64,
            vec![Type::I64],
            Type::Void,
        );
        let stats = devirtualize_module(&mut m);
        assert_eq!(stats.rewritten, 0);
        assert_eq!(stats.skipped_sig_mismatch, 1);
        assert!(matches!(body(&m)[1].kind, InstKind::CallIndirect(..)));
    }

    #[test]
    fn dyn_written_env_blocks_promotion() {
        let mut vals = vec![];
        let insts = vec![
            val(
                0,
                Type::Ptr,
                InstKind::Alloca(Type::Closure(SIG)),
                &mut vals,
            ),
            val(
                1,
                Type::Ptr,
                InstKind::Call(ALLOC, vec![Operand::ConstI64(48)]),
                &mut vals,
            ),
            val(2, Type::FnSig(SIG), InstKind::FnAddr(CB), &mut vals),
            void(InstKind::Store(
                Operand::Value(ValueId(2)),
                Operand::Value(ValueId(1)),
                CLOSURE_FN_ADDR_OFF,
            )),
            void(InstKind::Store(
                Operand::Value(ValueId(1)),
                Operand::Value(ValueId(0)),
                0,
            )),
            void(InstKind::StoreDyn(
                Operand::ConstI64(0),
                Operand::Value(ValueId(1)),
                Operand::ConstI64(8),
            )),
            val(
                3,
                Type::Closure(SIG),
                InstKind::Load(Type::Closure(SIG), Operand::Value(ValueId(0)), 0),
                &mut vals,
            ),
            val(
                4,
                Type::Ptr,
                InstKind::Load(Type::Ptr, Operand::Value(ValueId(3)), CLOSURE_FN_ADDR_OFF),
                &mut vals,
            ),
            val(
                5,
                Type::I64,
                InstKind::CallIndirect(
                    SIG,
                    Operand::Value(ValueId(4)),
                    vec![Operand::Value(ValueId(3)), Operand::ConstI64(7)],
                ),
                &mut vals,
            ),
        ];
        let mut m = module_with(
            insts,
            vals,
            &[Type::Ptr, Type::I64],
            Type::I64,
            vec![Type::Ptr, Type::I64],
            Type::I64,
        );
        let stats = devirtualize_module(&mut m);
        assert_eq!(stats.rewritten, 0);
        assert_eq!(stats.skipped_unresolved, 1);
    }
}
