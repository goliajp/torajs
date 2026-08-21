//! Self-tail-call elimination — rewrite `return f(args)` self-recursion
//! into parameter rebinding + a branch back to the function header, so
//! 100k-deep tail recursion runs in O(1) stack (ES2015
//! sec-static-semantics-hascallintailposition, restricted to the
//! self-call subset; the LuaJIT `rec_call_setup` self-tail-call shape).
//!
//! The pass never proves at compile time that the callee IS the current
//! function. It recognizes the *shape* ssa_lower emits for a named
//! function expression calling itself through its self-slot —
//!
//! ```text
//! %cell = load closure, %__env +K      ; self cell from own env
//! %fp   = load ptr, %cell +8           ; fn ptr out of the cell
//! %r    = call_indirect <sig> %fp(%cell, ARGC, a1..am)
//! %t    = call __torajs_throw_check()
//! %c    = icmp ne %t, 0
//! cond_br %c, bbThrow, bbOk            ; bbOk: dec(args)* ; ret %r
//! ```
//!
//! — and guards the rewrite with a single runtime compare
//! `%cell == %__env`: for the named-fn-expr self slot the loaded cell
//! IS the executing closure's own cell, so the guard is always true
//! there; for anything else (mutual recursion, a rebound cell) it is
//! false and the original call path runs, costing one perfectly
//! predicted compare.
//!
//! Ownership protocol (params become loop-carried):
//! * pre block: every Any param is `rc_inc`'d (borrowed → owned), so
//!   the header sees an owned value on every iteration uniformly.
//! * every `ret` decs the current Any param values. `return x` (param)
//!   already retains before ret, so the dec never drops the returned
//!   box to zero.
//! * rebind: copy args to fresh temps (parallel-move safe), inc the
//!   borrowed Any args (owned call temps just transfer — their
//!   post-call dec is on the not-taken call path), dec the old param
//!   values, then copy temps into the param vars. Missing trailing
//!   args are handled by re-running the header's argc normalization
//!   (branch-shaped: the undefined arm never reads the stale slot).
//!
//! The "bbOk decs are exactly call args" requirement doubles as the
//! scope-drop guard: a function with live owned locals at the tail
//! `ret` has extra drops in bbOk (lower emits scope closes before
//! every ret), which fails the match — no separate escape analysis
//! needed.

use std::collections::HashMap;

use torajs_core::ssa::{
    Block, BlockId, FuncId, IPred, Inst, InstKind, Module, Operand, Terminator, Type, ValueId,
    ValueInfo,
};

use crate::slot_forward::rewrite_operands;

/// Retain / release / pending-throw runtime symbols the rewrite emits
/// or matches. All are immediate-safe no-ops on non-cell Any values.
const ANYV_INC: &str = "__torajs_anyv_rc_inc";
const ANYV_DEC: &str = "__torajs_anyv_rc_dec";
const THROW_CHECK: &str = "__torajs_throw_check";

mod match_shape;
use match_shape::{TailSite, match_sites, operand_eq};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SelfTailCallStats {
    /// Functions that had at least one tail site rewritten.
    pub fns_rewritten: u32,
    /// Tail sites rewritten (guard + rebind loop installed).
    pub sites_rewritten: u32,
    /// Candidate call_indirect sites that failed the shape match.
    pub sites_skipped: u32,
    /// Functions with matching sites skipped for a non-{Any,I64,F64,Bool}
    /// param type (v1 rc-typed param boundary).
    pub fns_skipped_param_ty: u32,
}

pub fn eliminate_self_tail_calls(module: &mut Module) -> SelfTailCallStats {
    let mut stats = SelfTailCallStats::default();
    // Legacy probe shape only; the inline `GlobalRef` + `Load` probe
    // needs no fid (match_shape::tail_probe).
    let throw_fid = find_func(module, THROW_CHECK);
    let inc_fid = find_func(module, ANYV_INC);
    let dec_fid = find_func(module, ANYV_DEC);
    for fi in 0..module.funcs.len() {
        rewrite_function(module, fi, throw_fid, inc_fid, dec_fid, &mut stats);
    }
    stats
}

fn find_func(module: &Module, name: &str) -> Option<FuncId> {
    module
        .funcs
        .iter()
        .position(|f| f.name == name)
        .map(|i| FuncId(i as u32))
}

/// `(env, argc)` param pair check: params[0] named `__env` (ptr) and
/// params[1] named `__torajs_argc` (i64) — the S1 hidden-argc closure
/// entry shape. Returns the user params (index 2..).
fn closure_entry_params(module: &Module, fi: usize) -> Option<Vec<ValueId>> {
    let func = &module.funcs[fi];
    if func.params.len() < 2 || func.is_declaration() {
        return None;
    }
    let named = |v: ValueId, want: &str| func.values[v.0 as usize].name.as_deref() == Some(want);
    if !named(func.params[0], "__env") || !named(func.params[1], "__torajs_argc") {
        return None;
    }
    Some(func.params[2..].to_vec())
}

fn rewrite_function(
    module: &mut Module,
    fi: usize,
    throw_fid: Option<FuncId>,
    inc_fid: Option<FuncId>,
    dec_fid: Option<FuncId>,
    stats: &mut SelfTailCallStats,
) {
    let Some(user_params) = closure_entry_params(module, fi) else {
        return;
    };
    let sites = match_sites(module, fi, throw_fid, dec_fid, &user_params, stats);
    if sites.is_empty() {
        return;
    }
    // v1 param-type boundary: Any is the only rc-carrying type the
    // inc/dec protocol below handles; other rc types skip the fn.
    let param_tys: Vec<Type> = {
        let func = &module.funcs[fi];
        user_params
            .iter()
            .map(|p| func.values[p.0 as usize].ty)
            .collect()
    };
    if !param_tys
        .iter()
        .all(|t| matches!(t, Type::Any | Type::I64 | Type::F64 | Type::Bool))
    {
        stats.fns_skipped_param_ty += 1;
        return;
    }
    let has_any = param_tys.iter().any(|t| matches!(t, Type::Any));
    if has_any && (inc_fid.is_none() || dec_fid.is_none()) {
        stats.fns_skipped_param_ty += 1;
        return;
    }

    let func = &mut module.funcs[fi];
    let env = func.params[0];
    let argc_param = func.params[1];

    // Loop-carried vars: one per user param + one for argc. The pre
    // block seeds them from the raw params; rebind blocks overwrite
    // them — the multi-def `Copy` virtual-register shape codegen's
    // liveness already merges.
    let mint = |func: &mut torajs_core::ssa::Function, ty: Type, name: String| {
        func.values.push(ValueInfo {
            ty,
            name: Some(name),
        });
        ValueId((func.values.len() - 1) as u32)
    };
    let argc_var = mint(func, Type::I64, "__tc_argc".to_string());
    let param_vars: Vec<ValueId> = user_params
        .iter()
        .zip(&param_tys)
        .map(|(p, ty)| {
            let base = func.values[p.0 as usize]
                .name
                .clone()
                .unwrap_or_else(|| format!("p{}", p.0));
            mint(func, *ty, format!("__tc_{base}"))
        })
        .collect();

    // Redirect every param use to its var (env stays — it is loop
    // invariant and the guard compares against it directly).
    let mut replace: HashMap<ValueId, Operand> = HashMap::new();
    replace.insert(argc_param, Operand::Value(argc_var));
    for (p, v) in user_params.iter().zip(&param_vars) {
        replace.insert(*p, Operand::Value(*v));
    }
    for block in func.blocks.iter_mut() {
        for inst in block.insts.iter_mut() {
            rewrite_operands(&mut inst.kind, &replace);
        }
        match &mut block.term {
            Terminator::Ret(Some(Operand::Value(v))) => {
                if let Some(Operand::Value(nv)) = replace.get(v) {
                    *v = *nv;
                }
            }
            Terminator::CondBr {
                cond: Operand::Value(v),
                ..
            } => {
                if let Some(Operand::Value(nv)) = replace.get(v) {
                    *v = *nv;
                }
            }
            _ => {}
        }
    }

    // Move the original entry body (argc normalization included) into a
    // fresh header block; the entry keeps position 0 (BlockId.0 ==
    // position invariant) and becomes the pre block seeding the vars.
    let header = BlockId(func.blocks.len() as u32);
    let entry = &mut func.blocks[0];
    let moved_insts = std::mem::take(&mut entry.insts);
    let moved_term = std::mem::replace(&mut entry.term, Terminator::Br(header));
    let mut pre_insts = vec![copy_inst(Type::I64, argc_var, Operand::Value(argc_param))];
    for ((p, v), ty) in user_params.iter().zip(&param_vars).zip(&param_tys) {
        pre_insts.push(copy_inst(*ty, *v, Operand::Value(*p)));
        if matches!(ty, Type::Any) {
            pre_insts.push(void_call(inc_fid.unwrap(), vec![Operand::Value(*v)]));
        }
    }
    func.blocks[0].insts = pre_insts;
    func.blocks.push(Block {
        id: header,
        insts: moved_insts,
        term: moved_term,
    });

    // Rewrite each site: split the call tail off behind a cell==env
    // guard, with the rebind loop on the true edge.
    for site in &sites {
        let blk = if site.blk == 0 {
            header.0 as usize
        } else {
            site.blk
        };
        rewrite_site(
            func,
            blk,
            site,
            header,
            env,
            argc_var,
            &param_vars,
            &param_tys,
            inc_fid,
            dec_fid,
        );
        stats.sites_rewritten += 1;
    }

    // Borrowed→owned accounting: every ret path releases the Any param
    // vars the pre block retained.
    for block in func.blocks.iter_mut() {
        if matches!(block.term, Terminator::Ret(_)) {
            for (v, ty) in param_vars.iter().zip(&param_tys) {
                if matches!(ty, Type::Any) {
                    block
                        .insts
                        .push(void_call(dec_fid.unwrap(), vec![Operand::Value(*v)]));
                }
            }
        }
    }
    stats.fns_rewritten += 1;
}

/// Split one matched site: `B: [prefix, call, tc, cmp] + CondBr`
/// becomes `B: [prefix, guard] + CondBr(guard, rebind, callblk)`, with
/// the original tail moved verbatim into `callblk` and the rebind loop
/// (parallel-move temps + rc handoff) branching back to the header.
#[allow(clippy::too_many_arguments)]
fn rewrite_site(
    func: &mut torajs_core::ssa::Function,
    blk: usize,
    site: &TailSite,
    header: BlockId,
    env: ValueId,
    argc_var: ValueId,
    param_vars: &[ValueId],
    param_tys: &[Type],
    inc_fid: Option<FuncId>,
    dec_fid: Option<FuncId>,
) {
    let call_blk = BlockId(func.blocks.len() as u32);
    let rebind_blk = BlockId(func.blocks.len() as u32 + 1);

    let tail: Vec<Inst> = func.blocks[blk].insts.split_off(site.call_at);
    let InstKind::CallIndirect(_, _, args) = &tail[0].kind else {
        unreachable!("matched site lost its call");
    };
    let argc_const = args[1];
    let user_args: Vec<Operand> = args[2..].to_vec();
    // Owned call temps (dec'd on the original ok path) transfer into
    // the loop vars; everything else is borrowed and needs a retain.
    let ok_decs: Vec<Operand> = match &func.blocks[blk].term {
        Terminator::CondBr { else_blk, .. } => func.blocks[else_blk.0 as usize]
            .insts
            .iter()
            .filter_map(|i| match &i.kind {
                InstKind::Call(f, a) if Some(*f) == dec_fid => Some(a[0]),
                _ => None,
            })
            .collect(),
        _ => unreachable!("matched site lost its cond_br"),
    };
    let old_term = std::mem::replace(
        &mut func.blocks[blk].term,
        Terminator::CondBr {
            cond: Operand::ConstBool(true), // patched below once guard exists
            then_blk: rebind_blk,
            else_blk: call_blk,
        },
    );

    // guard: %g = icmp eq cell, env
    func.values.push(ValueInfo {
        ty: Type::Bool,
        name: Some("__tc_self".to_string()),
    });
    let guard = ValueId((func.values.len() - 1) as u32);
    func.blocks[blk].insts.push(Inst {
        result: Some(guard),
        kind: InstKind::ICmp(IPred::Eq, Operand::Value(site.cell), Operand::Value(env)),
        origin: None,
    });
    if let Terminator::CondBr { cond, .. } = &mut func.blocks[blk].term {
        *cond = Operand::Value(guard);
    }

    func.blocks.push(Block {
        id: call_blk,
        insts: tail,
        term: old_term,
    });

    // Rebind: temps first (parallel move), retain borrowed Any args,
    // release the old param values, then land the new ones.
    let mut insts = Vec::new();
    let mut temps = Vec::new();
    for (j, arg) in user_args.iter().enumerate() {
        func.values.push(ValueInfo {
            ty: param_tys[j],
            name: Some(format!("__tc_t{j}")),
        });
        let t = ValueId((func.values.len() - 1) as u32);
        insts.push(copy_inst(param_tys[j], t, *arg));
        if matches!(param_tys[j], Type::Any) && !ok_decs.iter().any(|d| operand_eq(d, arg)) {
            insts.push(void_call(inc_fid.unwrap(), vec![Operand::Value(t)]));
        }
        temps.push(t);
    }
    for (v, ty) in param_vars.iter().zip(param_tys) {
        if matches!(ty, Type::Any) {
            insts.push(void_call(dec_fid.unwrap(), vec![Operand::Value(*v)]));
        }
    }
    for (j, t) in temps.iter().enumerate() {
        insts.push(copy_inst(param_tys[j], param_vars[j], Operand::Value(*t)));
    }
    insts.push(copy_inst(Type::I64, argc_var, argc_const));
    func.blocks.push(Block {
        id: rebind_blk,
        insts,
        term: Terminator::Br(header),
    });
}

fn copy_inst(ty: Type, dst: ValueId, src: Operand) -> Inst {
    Inst {
        result: Some(dst),
        kind: InstKind::Copy(ty, src),
        origin: None,
    }
}

fn void_call(fid: FuncId, args: Vec<Operand>) -> Inst {
    Inst {
        result: None,
        kind: InstKind::Call(fid, args),
        origin: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::SigId;

    fn extern_fn(name: &str) -> torajs_core::ssa::Function {
        torajs_core::ssa::Function {
            name: name.into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![],
            values: vec![],
            current_origin: None,
        }
    }

    fn vi(ty: Type, name: &str) -> ValueInfo {
        ValueInfo {
            ty,
            name: Some(name.into()),
        }
    }

    /// Minimal self-tail shape: `bb0: cell=load env+56; fp=load cell+8;
    /// r=call_indirect(cell,1,n); t=throw_check; c=icmp ne t,0;
    /// cond_br c, throw, ok` / `throw: ret 0` / `ok: ret r`.
    fn self_tail_module() -> Module {
        let mut m = Module::default();
        m.funcs.push(extern_fn(THROW_CHECK)); // FuncId(0)
        m.funcs.push(extern_fn(ANYV_INC)); // FuncId(1)
        m.funcs.push(extern_fn(ANYV_DEC)); // FuncId(2)
        m.signatures
            .push((vec![Type::Ptr, Type::I64, Type::Any], Type::Any)); // SigId(0)
        let sig = SigId(0);
        let values = vec![
            vi(Type::Ptr, "__env"),         // v0
            vi(Type::I64, "__torajs_argc"), // v1
            vi(Type::Any, "n"),             // v2
            vi(Type::Closure(sig), "cell"), // v3
            vi(Type::Ptr, "fp"),            // v4
            vi(Type::Any, "r"),             // v5
            vi(Type::I64, "t"),             // v6
            vi(Type::Bool, "c"),            // v7
        ];
        let v = |i: u32| Operand::Value(ValueId(i));
        let blocks = vec![
            Block {
                id: BlockId(0),
                insts: vec![
                    Inst {
                        result: Some(ValueId(3)),
                        kind: InstKind::Load(Type::Closure(sig), v(0), 56),
                        origin: None,
                    },
                    Inst {
                        result: Some(ValueId(4)),
                        kind: InstKind::Load(Type::Ptr, v(3), 8),
                        origin: None,
                    },
                    Inst {
                        result: Some(ValueId(5)),
                        kind: InstKind::CallIndirect(
                            sig,
                            v(4),
                            vec![v(3), Operand::ConstI64(1), v(2)],
                        ),
                        origin: None,
                    },
                    Inst {
                        result: Some(ValueId(6)),
                        kind: InstKind::Call(FuncId(0), vec![]),
                        origin: None,
                    },
                    Inst {
                        result: Some(ValueId(7)),
                        kind: InstKind::ICmp(IPred::Ne, v(6), Operand::ConstI64(0)),
                        origin: None,
                    },
                ],
                term: Terminator::CondBr {
                    cond: v(7),
                    then_blk: BlockId(1),
                    else_blk: BlockId(2),
                },
            },
            Block {
                id: BlockId(1),
                insts: vec![],
                term: Terminator::Ret(Some(Operand::ConstI64(0))),
            },
            Block {
                id: BlockId(2),
                insts: vec![],
                term: Terminator::Ret(Some(v(5))),
            },
        ];
        m.funcs.push(torajs_core::ssa::Function {
            name: "__closure_0".into(),
            params: vec![ValueId(0), ValueId(1), ValueId(2)],
            ret: Type::Any,
            blocks,
            values,
            current_origin: None,
        });
        m
    }

    #[test]
    fn rewrites_minimal_self_tail_shape() {
        let mut m = self_tail_module();
        let stats = eliminate_self_tail_calls(&mut m);
        assert_eq!(stats.fns_rewritten, 1);
        assert_eq!(stats.sites_rewritten, 1);
        assert_eq!(stats.sites_skipped, 0);
        let f = &m.funcs[3];
        // 3 original + header + call_blk + rebind_blk
        assert_eq!(f.blocks.len(), 6);
        // entry became the pre block: seeds vars, incs the Any param,
        // branches to the moved header.
        assert!(matches!(f.blocks[0].term, Terminator::Br(_)));
        assert!(
            f.blocks[0]
                .insts
                .iter()
                .any(|i| matches!(&i.kind, InstKind::Call(fid, _) if *fid == FuncId(1)))
        );
        // the header (site block after the move) ends on the cell==env
        // guard feeding a CondBr into rebind/call blocks.
        let header = &f.blocks[3];
        assert!(matches!(
            header.insts.last().map(|i| &i.kind),
            Some(InstKind::ICmp(IPred::Eq, _, _))
        ));
        // both ret blocks got the Any-param dec appended.
        for bi in [1usize, 2] {
            assert!(
                f.blocks[bi]
                    .insts
                    .iter()
                    .any(|i| matches!(&i.kind, InstKind::Call(fid, _) if *fid == FuncId(2)))
            );
        }
        // rebind block branches back to the header.
        let rebind = f.blocks.last().unwrap();
        assert!(matches!(rebind.term, Terminator::Br(b) if b.0 == 3));
    }

    #[test]
    fn skips_when_ok_block_has_non_arg_dec() {
        let mut m = self_tail_module();
        // a scope drop in the ok block (dec of a value that is not a
        // call arg) must fail the match — that dec is lower's scope
        // close, which the rebind path would skip.
        m.funcs[3].blocks[2].insts.push(Inst {
            result: None,
            kind: InstKind::Call(FuncId(2), vec![Operand::Value(ValueId(3))]),
            origin: None,
        });
        let stats = eliminate_self_tail_calls(&mut m);
        assert_eq!(stats.fns_rewritten, 0);
        assert_eq!(stats.sites_rewritten, 0);
        assert_eq!(stats.sites_skipped, 1);
    }

    #[test]
    fn skips_over_application() {
        let mut m = self_tail_module();
        // call passes more user args than the fn has user params —
        // there is no slot to rebind the extra into.
        let f = &mut m.funcs[3];
        f.values.push(vi(Type::Any, "extra"));
        if let InstKind::CallIndirect(_, _, args) = &mut f.blocks[0].insts[2].kind {
            args.push(Operand::Value(ValueId(2)));
        }
        let stats = eliminate_self_tail_calls(&mut m);
        assert_eq!(stats.sites_rewritten, 0);
        assert_eq!(stats.sites_skipped, 1);
    }
}
