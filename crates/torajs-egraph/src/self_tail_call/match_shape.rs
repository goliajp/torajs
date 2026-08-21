//! Shape matching for the self-tail-call pass — scans a closure body
//! for the `cell = load env+K; fp = load cell+8; call_indirect(cell,
//! ARGC, args…); throw_check; icmp; cond_br(throw, ok)` tail shape and
//! the ok-block `dec(args)*; ret r` epilogue. Split out of the parent
//! when the pass file crossed the 500-line prod limit; the rewrite
//! machinery stays in `self_tail_call.rs`.

use std::collections::HashMap;

use torajs_core::ssa::{
    FuncId, IPred, Inst, InstKind, Module, Operand, THROW_ACTIVE_SYM, Terminator, Type, ValueId,
};

use super::SelfTailCallStats;

/// Structural operand equality — `Operand` carries an f64 payload so
/// it derives no `PartialEq`; the match only ever compares the
/// bit-exact variants below and treats everything else as unequal
/// (conservative: a missed match keeps the original call).
pub(super) fn operand_eq(a: &Operand, b: &Operand) -> bool {
    match (a, b) {
        (Operand::Value(x), Operand::Value(y)) => x == y,
        (Operand::ConstI64(x), Operand::ConstI64(y)) => x == y,
        (Operand::ConstBool(x), Operand::ConstBool(y)) => x == y,
        (Operand::ConstPtrNull, Operand::ConstPtrNull) => true,
        _ => false,
    }
}

/// A matched tail site, located before any rewriting moves blocks.
pub(super) struct TailSite {
    pub(super) blk: usize,
    /// Index of the call_indirect inst inside the block.
    pub(super) call_at: usize,
    /// ValueId of the loaded self cell (call's env argument).
    pub(super) cell: ValueId,
}

/// Scan a function for the tail-call shape documented in the module
/// header. Sites are matched against the pre-rewrite SSA (param uses
/// still raw), which is fine: the match only anchors on the env param
/// (never rebound) and block-local structure.
pub(super) fn match_sites(
    module: &Module,
    fi: usize,
    throw_fid: Option<FuncId>,
    dec_fid: Option<FuncId>,
    user_params: &[ValueId],
    stats: &mut SelfTailCallStats,
) -> Vec<TailSite> {
    let func = &module.funcs[fi];
    let env = func.params[0];
    // Single-def kind lookup for cell / fn-ptr provenance checks.
    let mut defs: HashMap<ValueId, &InstKind> = HashMap::new();
    for block in &func.blocks {
        for inst in &block.insts {
            if let Some(r) = inst.result {
                defs.insert(r, &inst.kind);
            }
        }
    }
    let mut sites = Vec::new();
    for (bi, block) in func.blocks.iter().enumerate() {
        let n = block.insts.len();
        if n < 3 {
            continue;
        }
        // `call → <throw probe> → icmp`; the probe is one inst
        // (legacy `call __torajs_throw_check()`) or two (inline
        // `GlobalRef` + `Load`), so the call sits at n-3 or n-4.
        let probe = tail_probe(&block.insts, throw_fid);
        let call = &block.insts[probe.map_or(n - 3, |(ci, _)| ci)];
        let cmp = &block.insts[n - 1];
        let InstKind::CallIndirect(sig, Operand::Value(fp), args) = &call.kind else {
            continue;
        };
        // From here on this is a candidate; any bail is a counted skip.
        let matched = (|| {
            // call_indirect through `load ptr, cell+8` where cell came
            // off our own env (one-hop named-fn-expr self slot).
            let Some(InstKind::Load(Type::Ptr, Operand::Value(cell), 8)) = defs.get(fp) else {
                return None;
            };
            let cell = *cell;
            match defs.get(&cell) {
                Some(InstKind::Load(Type::Closure(_), Operand::Value(e), _)) if *e == env => {}
                _ => return None,
            }
            // args: (cell, const argc, user args…) with a signature
            // whose param/ret types prefix-match our own entry.
            match args.first() {
                Some(op) if operand_eq(op, &Operand::Value(cell)) => {}
                _ => return None,
            }
            let Some(Operand::ConstI64(_)) = args.get(1) else {
                return None;
            };
            if args.len() < 2 || args.len() - 2 > user_params.len() {
                return None;
            }
            let (sig_params, sig_ret) = &module.signatures[sig.0 as usize];
            if *sig_ret != func.ret || sig_params.len() != args.len() {
                return None;
            }
            for (sp, arg_ty) in sig_params.iter().enumerate().skip(2) {
                if *arg_ty != func.values[user_params[sp - 2].0 as usize].ty {
                    return None;
                }
            }
            // call → throw probe → icmp ne → cond_br(throw, ok)
            let r = call.result?;
            let (_, t) = probe?;
            match (&cmp.kind, cmp.result) {
                (InstKind::ICmp(IPred::Ne, Operand::Value(x), Operand::ConstI64(0)), Some(_))
                    if *x == t => {}
                _ => return None,
            }
            let Terminator::CondBr {
                cond: Operand::Value(c),
                else_blk: ok,
                ..
            } = &block.term
            else {
                return None;
            };
            if Some(*c) != cmp.result {
                return None;
            }
            // ok block: dec(arg)* then `ret %r` — decs restricted to
            // call args is also the no-pending-scope-drops proof.
            let ok_block = &func.blocks[ok.0 as usize];
            for inst in &ok_block.insts {
                match &inst.kind {
                    InstKind::Call(f, a)
                        if Some(*f) == dec_fid
                            && a.len() == 1
                            && args[2..].iter().any(|x| operand_eq(x, &a[0])) => {}
                    _ => return None,
                }
            }
            match &ok_block.term {
                Terminator::Ret(Some(Operand::Value(rv))) if *rv == r => {}
                _ => return None,
            }
            Some(TailSite {
                blk: bi,
                call_at: probe.map_or(n - 3, |(ci, _)| ci),
                cell,
            })
        })();
        match matched {
            Some(site) => sites.push(site),
            None => stats.sites_skipped += 1,
        }
    }
    sites
}

/// `(call index, throw-flag value)` when the tail of `insts` is a
/// throw probe followed by one more inst (the icmp). The probe is
/// either the inline `GlobalRef(___torajs_throw_active)` + `Load`
/// pair (`ssa_lower_emit_throw_check`, rotation 470) or the legacy
/// `call __torajs_throw_check()`; the call under test sits just
/// before it.
fn tail_probe(insts: &[Inst], throw_fid: Option<FuncId>) -> Option<(usize, ValueId)> {
    let n = insts.len();
    if n >= 4 {
        let (g, ld) = (&insts[n - 3], &insts[n - 2]);
        if let (InstKind::GlobalRef(sym), Some(gv)) = (&g.kind, g.result) {
            if sym == THROW_ACTIVE_SYM {
                if let (InstKind::Load(Type::I64, Operand::Value(p), 0), Some(t)) =
                    (&ld.kind, ld.result)
                {
                    if *p == gv {
                        return Some((n - 4, t));
                    }
                }
            }
        }
    }
    if n >= 3 {
        let tc = &insts[n - 2];
        if let (InstKind::Call(f, a), Some(t)) = (&tc.kind, tc.result) {
            if Some(*f) == throw_fid && a.is_empty() {
                return Some((n - 3, t));
            }
        }
    }
    None
}
