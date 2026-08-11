//! RC elide peephole — remove provably-redundant retain/release pairs.
//!
//! After the Cluster E inliner splices callee bodies into callers (and
//! after ssa_lower's share-on-alias convention emits a binding-point
//! `rc_inc` for same-scope `let t = s` copies), the SSA stream often
//! carries the shape
//!
//! ```text
//! %2 = load str, %s               ; %s = alloca stack slot
//! call __torajs_rc_inc(%2)
//! ...stack traffic only...
//! %5 = load str, %s
//! call __torajs_str_drop(%5)      ; or substr/arr drop
//! ```
//!
//! within a single block. The pair is a net refcount no-op on the
//! *same runtime pointer*: the inc raises its count to ≥ 2, so the
//! drop can never reach zero and free — deleting both leaves every
//! observable state identical. This is the textbook ARC pairing
//! optimization (LLVM `ObjCARCOpts.cpp` retain/release pairing with
//! its provenance analysis; Swift's ARCSequenceOpts) restricted to
//! its safest sub-case.
//!
//! Same-pointer proof — two shapes, both block-local:
//!
//! * the inc and drop operands are the **same `ValueId`**, or
//! * both operands are `Load`s from the **same non-escaped alloca
//!   slot** at the same offset with the same store-generation (no
//!   `Store` to that slot between the two loads). Escape analysis is
//!   whole-function: a slot whose pointer appears anywhere other than
//!   a `Load`/`Store` address position is dropped from tracking.
//!
//! Guards (wrong elision is a use-after-free; missed elision is only
//! a lost cycle — every guard errs toward keeping the pair):
//!
//! * inc and drop must sit in the **same block**.
//! * every instruction between them must be rc-transparent: pure
//!   (`classify_pure`), an `Alloca`, a `Load`/`Store` whose
//!   address is a tracked stack slot, or a call to a read-only
//!   print intrinsic (`READONLY_INTRINSICS` — LLVM ObjCARC's
//!   `CanAlterRefCount == false` call class). Anything that could
//!   dec/free the object or observe its refcount (other calls,
//!   heap loads, heap stores) closes the window.
//! * only the inc-before-drop direction is elided; drop-before-inc
//!   would let the object die mid-gap.
//! * both insts must be result-less (`append_void` shape) so removal
//!   can never orphan a `ValueId` use.
//! * the drop must be one of the single-inst NULL-safe runtime drop
//!   helpers (`DROP_INTRINSICS`). Obj drops are inline CFG expansions
//!   and Closure drops are `CallIndirect` — both out of scope.
//!
//! The scan runs to a fixpoint so nested pairs
//! (`inc inc drop drop`) collapse fully.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{Function, Inst, InstKind, Module, Operand, Terminator, ValueId};

use crate::optimize::classify_pure;

/// Name of the retain intrinsic emitted by `ssa_lower::emit_rc_inc`.
const RC_INC: &str = "__torajs_rc_inc";

/// Single-inst void drop helpers whose semantics are exactly
/// "dec refcount, free on zero, NULL passes through". Inline-expanded
/// drops (Obj) and indirect drops (Closure) are intentionally absent.
const DROP_INTRINSICS: [&str; 4] = [
    "__torajs_str_drop",
    "__torajs_substr_drop",
    "__torajs_arr_drop",
    "__torajs_arr_drop_any",
];

/// Runtime intrinsics that provably cannot change any refcount,
/// free any object, or read a refcount field: they only read
/// payload bytes plus header length/encoding bits and write to
/// stdio (verified against `torajs-str/src/print.rs` and
/// `torajs-io/src/sink.rs`). A call to one of these is
/// window-transparent: nothing inside a transparent window can
/// decrement a count, so every object alive at the inc stays alive
/// across the whole window even with the pair removed.
///
/// The io sink-switch pair joined with RFC 20260812-console-sink
/// knife 2: `console.error(str)` now lowers as
/// `sink_to_stderr; str_print; sink_to_stdout`, and without the
/// pair in this set the switch calls would opacify a window that
/// was transparent when the lowering emitted the single
/// `str_print_err` call. They touch only the io buffers and an
/// AtomicBool — no rc traffic.
const READONLY_INTRINSICS: [&str; 4] = [
    "__torajs_str_print",
    "__torajs_substr_print",
    "__torajs_io_sink_to_stderr",
    "__torajs_io_sink_to_stdout",
];

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RcPeepholeStats {
    /// Number of inc/drop pairs removed (each pair = 2 insts).
    pub pairs_elided: u32,
}

/// Abstract identity of a refcounted pointer operand. Two operands
/// with equal keys hold the same runtime pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PtrKey {
    /// The SSA value itself (no provenance info needed).
    Direct(ValueId),
    /// Loaded from a tracked stack slot: (slot, byte offset,
    /// store-generation of the slot at load time).
    SlotLoad(ValueId, u64, u32),
}

/// Run the retain/release elide peephole over every function body in
/// `module`. Returns the number of pairs removed.
pub fn elide_rc_pairs(module: &mut Module) -> RcPeepholeStats {
    let mut stats = RcPeepholeStats::default();
    let Some(inc_fid) = find_func(module, RC_INC) else {
        return stats; // program uses no refcounted values at all
    };
    let drop_fids: HashSet<u32> = DROP_INTRINSICS
        .iter()
        .filter_map(|name| find_func(module, name))
        .collect();
    if drop_fids.is_empty() {
        return stats;
    }
    let readonly_fids: HashSet<u32> = READONLY_INTRINSICS
        .iter()
        .filter_map(|name| find_func(module, name))
        .collect();
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        let slots = collect_unescaped_slots(func);
        for block in func.blocks.iter_mut() {
            loop {
                let removed = elide_in_insts(
                    &mut block.insts,
                    inc_fid,
                    &drop_fids,
                    &readonly_fids,
                    &slots,
                );
                if removed == 0 {
                    break;
                }
                stats.pairs_elided += removed;
            }
        }
    }
    stats
}

fn find_func(module: &Module, name: &str) -> Option<u32> {
    module
        .funcs
        .iter()
        .position(|f| f.name == name)
        .map(|i| i as u32)
}

/// Whole-function escape analysis over stack slots. A slot stays
/// tracked only if every use of its pointer is a `Load` address or a
/// `Store` address. Any other appearance (call argument, stored as a
/// value, returned, branched on, dynamic addressing) escapes it —
/// after that, stores through unknown pointers could alias it and the
/// block-local store-generation counting below would be unsound.
pub(crate) fn collect_unescaped_slots(func: &Function) -> HashSet<ValueId> {
    let mut slots: HashSet<ValueId> = HashSet::new();
    for block in &func.blocks {
        for inst in &block.insts {
            if matches!(inst.kind, InstKind::Alloca(_) | InstKind::AllocaBytes(_)) {
                if let Some(v) = inst.result {
                    slots.insert(v);
                }
            }
        }
    }
    if slots.is_empty() {
        return slots;
    }
    let mut escaped: HashSet<ValueId> = HashSet::new();
    let mark = |op: &Operand, escaped: &mut HashSet<ValueId>| {
        if let Operand::Value(v) = op {
            if slots.contains(v) {
                escaped.insert(*v);
            }
        }
    };
    for block in &func.blocks {
        for inst in &block.insts {
            match &inst.kind {
                // address-position uses keep the slot tracked
                InstKind::Load(_, _ptr, _) => {}
                InstKind::Store(val, _ptr, _) => mark(val, &mut escaped),
                // exhaustive operand walk for everything else: any
                // slot appearing anywhere escapes (conservative)
                other => visit_value_operands(other, |v| {
                    if slots.contains(&v) {
                        escaped.insert(v);
                    }
                }),
            }
        }
        match &block.term {
            Terminator::Ret(Some(op)) => mark(op, &mut escaped),
            Terminator::CondBr { cond, .. } => mark(cond, &mut escaped),
            Terminator::Br(_) | Terminator::Ret(None) | Terminator::Unreachable => {}
        }
    }
    slots.retain(|s| !escaped.contains(s));
    slots
}

/// Invoke `f` on every `Operand::Value` inside `kind`. Mirrors the
/// exhaustive operand enumeration of `inliner/rewrite.rs`; `Load` /
/// `Store` callers special-case their address positions before
/// reaching this.
// Hoisted to torajs-core (ssa/visit.rs) so codegen's branch-fuse
// use-count shares the one exhaustive walker; re-exported here to
// keep the long-standing `crate::rc_peephole::visit_value_operands`
// import path of the sibling passes working.
pub(crate) use torajs_core::ssa::visit_value_operands;

/// One scan pass over a block's instruction list. Removes every
/// provable pair found in this pass and returns the count; callers
/// loop to fixpoint so pairs revealed by a removal (nested
/// `inc inc drop drop`) collapse on the next pass.
fn elide_in_insts(
    insts: &mut Vec<Inst>,
    inc_fid: u32,
    drop_fids: &HashSet<u32>,
    readonly_fids: &HashSet<u32>,
    slots: &HashSet<ValueId>,
) -> u32 {
    // pass 1 — linear walk assigning each value loaded from a tracked
    // slot its provenance key (slot, offset, store-generation)
    let mut slot_gen: HashMap<ValueId, u32> = HashMap::new();
    let mut prov: HashMap<ValueId, PtrKey> = HashMap::new();
    for inst in insts.iter() {
        match &inst.kind {
            InstKind::Load(_, Operand::Value(p), off) if slots.contains(p) => {
                if let Some(v) = inst.result {
                    let g = *slot_gen.get(p).unwrap_or(&0);
                    prov.insert(v, PtrKey::SlotLoad(*p, *off, g));
                }
            }
            InstKind::Store(_, Operand::Value(p), _) if slots.contains(p) => {
                *slot_gen.entry(*p).or_insert(0) += 1;
            }
            _ => {}
        }
    }
    let key_of = |v: ValueId| prov.get(&v).copied().unwrap_or(PtrKey::Direct(v));

    // pass 2 — pair matching with rc-transparent windows
    let mut to_remove: HashSet<usize> = HashSet::new();
    'inc_scan: for i in 0..insts.len() {
        if to_remove.contains(&i) {
            continue;
        }
        let Some(a) = match_void_call_on_value(&insts[i], |fid| fid == inc_fid) else {
            continue;
        };
        let a_key = key_of(a);
        for (j, inst) in insts.iter().enumerate().skip(i + 1) {
            if to_remove.contains(&j) {
                // already claimed by an earlier pair; its effect is
                // gone, treat as transparent
                continue;
            }
            if let Some(b) = match_void_call_on_value(inst, |fid| drop_fids.contains(&fid)) {
                if key_of(b) == a_key {
                    to_remove.insert(i);
                    to_remove.insert(j);
                    continue 'inc_scan;
                }
                // a drop of a DIFFERENT pointer is still a call that
                // could free shared state — stop the window
                break;
            }
            if !rc_transparent(&inst.kind, readonly_fids, slots) {
                break;
            }
        }
    }
    if to_remove.is_empty() {
        return 0;
    }
    let pairs = (to_remove.len() / 2) as u32;
    let mut idx = 0;
    insts.retain(|_| {
        let keep = !to_remove.contains(&idx);
        idx += 1;
        keep
    });
    pairs
}

/// Can `kind` sit between a paired inc and drop without invalidating
/// the elision? True only for instructions that provably cannot
/// change any refcount, free any object, or read a refcount field:
/// pure ops, stack-slot allocation, loads/stores whose address is a
/// tracked (non-escaped) stack slot, and calls to the read-only
/// print intrinsics.
fn rc_transparent(kind: &InstKind, readonly_fids: &HashSet<u32>, slots: &HashSet<ValueId>) -> bool {
    if classify_pure(kind) {
        return true;
    }
    match kind {
        InstKind::Alloca(_) | InstKind::AllocaBytes(_) => true,
        InstKind::Load(_, Operand::Value(p), _) | InstKind::Store(_, Operand::Value(p), _) => {
            slots.contains(p)
        }
        // register-level move (mem2reg φ destruction product) — pure
        // bit-move, provably cannot touch a refcount or heap header.
        // Not in classify_pure because its multi-def shape must never
        // be GVN-deduplicated.
        InstKind::Copy(_, _) => true,
        // rc-neutral readonly runtime call (READONLY_INTRINSICS)
        InstKind::Call(fid, _) => readonly_fids.contains(&fid.0),
        _ => false,
    }
}

/// Match `call <fid>(%v)` with no result and a single
/// `Operand::Value` argument; return the value.
fn match_void_call_on_value(inst: &Inst, fid_ok: impl Fn(u32) -> bool) -> Option<ValueId> {
    if inst.result.is_some() {
        return None;
    }
    let InstKind::Call(fid, args) = &inst.kind else {
        return None;
    };
    if !fid_ok(fid.0) {
        return None;
    }
    let [Operand::Value(v)] = args.as_slice() else {
        return None;
    };
    Some(*v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{
        BinOp, Block, BlockId, FuncId, Function, Inst, InstKind, Operand, Terminator, Type,
        ValueId, ValueInfo,
    };

    fn void_inst(kind: InstKind) -> Inst {
        Inst {
            result: None,
            kind,
            origin: None,
        }
    }

    fn val_inst(result: ValueId, kind: InstKind) -> Inst {
        Inst {
            result: Some(result),
            kind,
            origin: None,
        }
    }

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

    /// Module layout: funcs[0] = rc_inc decl, funcs[1] = str_drop
    /// decl, funcs[2] = body under test with `n_values` Str values.
    fn module_with_body(insts: Vec<Inst>, n_values: usize) -> Module {
        module_with_term(insts, n_values, Terminator::Ret(None))
    }

    fn module_with_term(insts: Vec<Inst>, n_values: usize, term: Terminator) -> Module {
        let body = Function {
            name: "main".into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![Block {
                id: BlockId(0),
                insts,
                term,
            }],
            values: vec![
                ValueInfo {
                    ty: Type::Str,
                    name: None,
                };
                n_values
            ],
            current_origin: None,
        };
        Module {
            funcs: vec![
                declaration("__torajs_rc_inc"),
                declaration("__torajs_str_drop"),
                body,
            ],
            ..Default::default()
        }
    }

    const INC: FuncId = FuncId(0);
    const DROP: FuncId = FuncId(1);
    const PRINT: FuncId = FuncId(3);
    const OTHER: FuncId = FuncId(4);

    /// Like `module_with_body` but with two extra declarations:
    /// funcs[3] = a readonly print intrinsic (window-transparent),
    /// funcs[4] = an arbitrary non-whitelisted runtime call.
    fn module_with_calls(insts: Vec<Inst>, n_values: usize) -> Module {
        let mut m = module_with_body(insts, n_values);
        m.funcs.push(declaration("__torajs_str_print"));
        m.funcs
            .push(declaration("__torajs_microtask_run_until_idle"));
        m
    }

    fn print_str(v: u32) -> Inst {
        void_inst(InstKind::Call(PRINT, vec![Operand::Value(ValueId(v))]))
    }
    fn other_call() -> Inst {
        void_inst(InstKind::Call(OTHER, vec![]))
    }

    fn inc(v: u32) -> Inst {
        void_inst(InstKind::Call(INC, vec![Operand::Value(ValueId(v))]))
    }
    fn drop_str(v: u32) -> Inst {
        void_inst(InstKind::Call(DROP, vec![Operand::Value(ValueId(v))]))
    }
    fn pure_add(result: u32) -> Inst {
        val_inst(
            ValueId(result),
            InstKind::BinOp(BinOp::Add, Operand::ConstI64(1), Operand::ConstI64(2)),
        )
    }
    fn alloca(result: u32) -> Inst {
        val_inst(ValueId(result), InstKind::Alloca(Type::Str))
    }
    fn load_slot(result: u32, slot: u32) -> Inst {
        val_inst(
            ValueId(result),
            InstKind::Load(Type::Str, Operand::Value(ValueId(slot)), 0),
        )
    }
    fn store_slot(val: u32, slot: u32) -> Inst {
        void_inst(InstKind::Store(
            Operand::Value(ValueId(val)),
            Operand::Value(ValueId(slot)),
            0,
        ))
    }

    fn body_insts(m: &Module) -> &[Inst] {
        &m.funcs[2].blocks[0].insts
    }

    #[test]
    fn adjacent_direct_pair_elided() {
        let mut m = module_with_body(vec![inc(0), drop_str(0)], 1);
        let stats = elide_rc_pairs(&mut m);
        assert_eq!(stats.pairs_elided, 1);
        assert!(body_insts(&m).is_empty());
    }

    #[test]
    fn pair_across_pure_insts_elided() {
        let mut m = module_with_body(vec![inc(0), pure_add(1), pure_add(2), drop_str(0)], 3);
        let stats = elide_rc_pairs(&mut m);
        assert_eq!(stats.pairs_elided, 1);
        assert_eq!(body_insts(&m).len(), 2);
    }

    #[test]
    fn heap_store_between_blocks_elision() {
        // store through a non-slot pointer (value 1 is not an alloca
        // result) could write a heap header — window must close
        let mut m = module_with_body(
            vec![
                inc(0),
                void_inst(InstKind::Store(
                    Operand::ConstI64(0),
                    Operand::Value(ValueId(1)),
                    0,
                )),
                drop_str(0),
            ],
            2,
        );
        let stats = elide_rc_pairs(&mut m);
        assert_eq!(stats.pairs_elided, 0);
        assert_eq!(body_insts(&m).len(), 3);
    }

    #[test]
    fn call_between_blocks_elision() {
        // an unrelated drop call of a different pointer sits between —
        // it could free shared state, so the window must close
        let mut m = module_with_body(vec![inc(0), drop_str(1), drop_str(0)], 2);
        let stats = elide_rc_pairs(&mut m);
        assert_eq!(stats.pairs_elided, 0);
        assert_eq!(body_insts(&m).len(), 3);
    }

    #[test]
    fn different_value_not_paired() {
        let mut m = module_with_body(vec![inc(0), drop_str(1)], 2);
        let stats = elide_rc_pairs(&mut m);
        assert_eq!(stats.pairs_elided, 0);
    }

    #[test]
    fn drop_before_inc_not_paired() {
        let mut m = module_with_body(vec![drop_str(0), inc(0)], 1);
        let stats = elide_rc_pairs(&mut m);
        assert_eq!(stats.pairs_elided, 0);
        assert_eq!(body_insts(&m).len(), 2);
    }

    #[test]
    fn nested_pairs_collapse_to_fixpoint() {
        let mut m = module_with_body(vec![inc(0), inc(0), drop_str(0), drop_str(0)], 1);
        let stats = elide_rc_pairs(&mut m);
        assert_eq!(stats.pairs_elided, 2);
        assert!(body_insts(&m).is_empty());
    }

    #[test]
    fn two_independent_pairs_one_pass() {
        let mut m = module_with_body(vec![inc(0), drop_str(0), inc(1), drop_str(1)], 2);
        let stats = elide_rc_pairs(&mut m);
        assert_eq!(stats.pairs_elided, 2);
        assert!(body_insts(&m).is_empty());
    }

    #[test]
    fn pair_across_readonly_print_elided() {
        let mut m = module_with_calls(vec![inc(0), print_str(0), drop_str(0)], 1);
        let stats = elide_rc_pairs(&mut m);
        assert_eq!(stats.pairs_elided, 1);
        assert_eq!(body_insts(&m).len(), 1);
    }

    #[test]
    fn alias_double_copy_shape_collapses_to_fixpoint() {
        // alias-002 shape: `const t = s; const u = s;` + three logs —
        // 2 incs, 3 same-pointer prints, 3 drops. Both pairs elide
        // across the readonly prints, leaving prints + final drop.
        let mut m = module_with_calls(
            vec![
                inc(0),
                inc(0),
                print_str(0),
                print_str(0),
                print_str(0),
                drop_str(0),
                drop_str(0),
                drop_str(0),
            ],
            1,
        );
        let stats = elide_rc_pairs(&mut m);
        assert_eq!(stats.pairs_elided, 2);
        assert_eq!(body_insts(&m).len(), 4);
    }

    #[test]
    fn non_whitelisted_call_still_closes_window() {
        let mut m = module_with_calls(vec![inc(0), other_call(), drop_str(0)], 1);
        let stats = elide_rc_pairs(&mut m);
        assert_eq!(stats.pairs_elided, 0);
        assert_eq!(body_insts(&m).len(), 3);
    }

    #[test]
    fn module_without_rc_inc_is_noop() {
        let body = Function {
            name: "main".into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![pure_add(0)],
                term: Terminator::Ret(None),
            }],
            values: vec![ValueInfo {
                ty: Type::I64,
                name: None,
            }],
            current_origin: None,
        };
        let mut m = Module {
            funcs: vec![body],
            ..Default::default()
        };
        let stats = elide_rc_pairs(&mut m);
        assert_eq!(stats.pairs_elided, 0);
    }

    #[test]
    fn call_with_result_not_matched() {
        // a value-producing call must never be deleted even if its
        // callee name matched (would orphan the result ValueId)
        let mut insts = vec![inc(0), drop_str(0)];
        insts[1].result = Some(ValueId(0));
        let mut m = module_with_body(insts, 1);
        let stats = elide_rc_pairs(&mut m);
        assert_eq!(stats.pairs_elided, 0);
    }

    /// The ssa_lower share-on-alias shape (`let t = s; return t`):
    /// inc on a load of slot %s, drop on a *different* load of %s,
    /// stack-only traffic between. Same provenance → elided.
    #[test]
    fn slot_load_pair_elided() {
        // %1 = alloca (slot s), %2 = alloca (slot t)
        // store %0, s ; %3 = load s ; inc(%3)
        // store %3, t ; %4 = load t ; %5 = load s ; drop(%5)
        let insts = vec![
            alloca(1),
            alloca(2),
            store_slot(0, 1),
            load_slot(3, 1),
            inc(3),
            store_slot(3, 2),
            load_slot(4, 2),
            load_slot(5, 1),
            drop_str(5),
        ];
        let mut m = module_with_body(insts, 6);
        let stats = elide_rc_pairs(&mut m);
        assert_eq!(stats.pairs_elided, 1);
        assert_eq!(body_insts(&m).len(), 7);
        assert!(
            !body_insts(&m)
                .iter()
                .any(|i| matches!(i.kind, InstKind::Call(..)))
        );
    }

    /// A store to the tracked slot between the two loads bumps the
    /// generation — the loads may now hold different pointers.
    #[test]
    fn slot_store_between_loads_blocks_pairing() {
        let insts = vec![
            alloca(1),
            store_slot(0, 1),
            load_slot(2, 1),
            inc(2),
            store_slot(3, 1), // slot overwritten
            load_slot(4, 1),
            drop_str(4),
        ];
        let mut m = module_with_body(insts, 5);
        let stats = elide_rc_pairs(&mut m);
        assert_eq!(stats.pairs_elided, 0);
    }

    /// Loads at different offsets within one slot are different
    /// pointers.
    #[test]
    fn different_offset_loads_not_paired() {
        let mut insts = vec![alloca(1), store_slot(0, 1), load_slot(2, 1)];
        insts.push(val_inst(
            ValueId(3),
            InstKind::Load(Type::Str, Operand::Value(ValueId(1)), 8),
        ));
        insts.push(inc(2));
        insts.push(drop_str(3));
        let mut m = module_with_body(insts, 4);
        let stats = elide_rc_pairs(&mut m);
        assert_eq!(stats.pairs_elided, 0);
    }

    /// A slot whose pointer is passed to a call escapes — stores
    /// through unknown pointers could alias it, so its loads get no
    /// provenance and the pair is kept.
    #[test]
    fn escaped_slot_not_tracked() {
        let insts = vec![
            alloca(1),
            store_slot(0, 1),
            load_slot(2, 1),
            inc(2),
            // slot pointer escapes into a call
            void_inst(InstKind::Call(DROP, vec![Operand::Value(ValueId(1))])),
            load_slot(3, 1),
            drop_str(3),
        ];
        let mut m = module_with_body(insts, 4);
        let stats = elide_rc_pairs(&mut m);
        assert_eq!(stats.pairs_elided, 0);
    }

    /// A slot returned from the function escapes via the terminator.
    #[test]
    fn slot_escaping_via_ret_not_tracked() {
        let insts = vec![
            alloca(1),
            store_slot(0, 1),
            load_slot(2, 1),
            inc(2),
            load_slot(3, 1),
            drop_str(3),
        ];
        let mut m = module_with_term(insts, 4, Terminator::Ret(Some(Operand::Value(ValueId(1)))));
        let stats = elide_rc_pairs(&mut m);
        assert_eq!(stats.pairs_elided, 0);
    }
}
