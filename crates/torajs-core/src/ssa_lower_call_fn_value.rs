//! Call-fn-value dispatch helpers for `LowerCtx<'a>` extracted from
//! `ssa_lower.rs` chunk 398 — Path A.3-batch19.
//!
//! Groups the M6.2 "call a Closure or FnSig value with a list of args"
//! dispatch layer used inside `Array.map/filter/reduce/forEach` and
//! peer higher-order fn bodies. Public methods:
//!
//! - `call_fn_value(fn_val, fn_ty, args)` — indirect dispatch, walks
//!   the FnSig for Substr → Str boundary materialization + drops.
//! - `call_fn_value_devirt(known_fid, fn_val, fn_ty, args)` — v0.6+1
//!   perf checkpoint direct-Call variant when the callee FuncId is
//!   statically known so LLVM value-prop can inline through
//!   `xs.map(cb)` loops.
//!
//! Private helpers used only from within this module:
//!
//! - `sig_param_tys(fn_ty)` — pull the param-type vec from a
//!   FnSig / Closure type; None for non-callable.
//! - `materialize_call_args(fn_ty, args)` — Phase Substr.B boundary
//!   materialization: allocate an owned Str via `substr_to_owned`
//!   when the callee expects `Type::Str` but the operand is
//!   `Type::Substr`. Returns `(materialized_args, drops)`.
//! - `call_fn_value_raw(fn_val, fn_ty, args)` — the raw indirect
//!   dispatch (Closure = load fn_ptr @ +8 + CallIndirect with env
//!   prepended; FnSig = plain CallIndirect).

use crate::ssa::{FuncId, InstKind, Operand, Type, ValueId};
use crate::ssa_lower::{CLOSURE_FN_ADDR_OFF, LowerCtx};
use crate::ssa_lower_interners::intern_fn_sig;

impl<'a> LowerCtx<'a> {
    /// M6.2 — call a Closure or FnSig value with a list of args. Used
    /// inside Array.map/filter/reduce/forEach loop bodies (and is the
    /// mirror of the existing inline call-via-Closure / call-via-FnSig
    /// dispatch, packaged for re-use).
    /// Look up a sig's param types from a callable type. Returns None for
    /// non-callable types — callers should already have validated.
    fn sig_param_tys(&self, fn_ty: Type) -> Option<Vec<Type>> {
        let sig_id = match fn_ty {
            Type::FnSig(s) | Type::Closure(s) => s,
            _ => return None,
        };
        Some(self.fn_sigs[sig_id.0 as usize].0.clone())
    }

    /// Phase Substr.B — boundary materialization. If the callee expects
    /// `Type::Str` for an arg position and the actual operand is
    /// `Type::Substr`, allocate an owned Str via substr_to_owned and
    /// return the materialized operand; the caller drops it after the
    /// call. Other type pairs pass through unchanged. Returns the
    /// (possibly-rewritten) args plus a list of Str values to drop after
    /// the call returns.
    fn materialize_call_args(
        &mut self,
        fn_ty: Type,
        args: Vec<Operand>,
    ) -> (Vec<Operand>, Vec<Operand>) {
        let Some(param_tys) = self.sig_param_tys(fn_ty) else {
            return (args, Vec::new());
        };
        let mut out = Vec::with_capacity(args.len());
        let mut drops = Vec::new();
        for (i, a) in args.into_iter().enumerate() {
            let actual = self.operand_ty(&a);
            let expected = param_tys.get(i).copied();
            if expected == Some(Type::Str) && actual == Type::Substr {
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.substr_to_owned, vec![a]),
                    Type::Str,
                    None,
                );
                out.push(Operand::Value(v));
                drops.push(Operand::Value(v));
            } else {
                out.push(a);
            }
        }
        (out, drops)
    }

    pub(crate) fn call_fn_value(
        &mut self,
        fn_val: Operand,
        fn_ty: Type,
        args: Vec<Operand>,
    ) -> ValueId {
        let (args, drops) = self.materialize_call_args(fn_ty, args);
        let ret = self.call_fn_value_raw(fn_val, fn_ty, args);
        for d in drops {
            self.emit_drop_value(d, Type::Str);
        }
        ret
    }

    /// v0.6+1 perf checkpoint — devirt variant of `call_fn_value`.
    /// When the callable's underlying FuncId is statically known
    /// (caller resolved Expr::Closure / Expr::Ident at SSA-lower
    /// time), emit a direct `Call(fid, ...)` instead of the env+8
    /// fn_ptr load + CallIndirect dance. LLVM value-prop then sees
    /// a constant call target and can inline the body — a 10M-elem
    /// `xs.map((x) => x + k)` loop devirts every iteration's
    /// closure call so the optimizer can vectorize the lot.
    ///
    /// `fn_val` still threads through for its env-pointer side
    /// effect (Closure args take env_ptr as the first param). For
    /// Type::FnSig (no env), env arg is omitted.
    pub(crate) fn call_fn_value_devirt(
        &mut self,
        known_fid: FuncId,
        fn_val: Operand,
        fn_ty: Type,
        args: Vec<Operand>,
    ) -> ValueId {
        let (args, drops) = self.materialize_call_args(fn_ty, args);
        let ret_ty = match fn_ty {
            Type::Closure(sig_id) | Type::FnSig(sig_id) => self.fn_sigs[sig_id.0 as usize].1,
            other => panic!("call_fn_value_devirt: expected Closure/FnSig, got {other:?}"),
        };
        let mut argv: Vec<Operand> = match fn_ty {
            Type::Closure(_) => {
                /* Closure ABI: first arg is env_ptr, then user args. */
                let mut a = Vec::with_capacity(args.len() + 1);
                a.push(fn_val);
                a.extend(args);
                a
            }
            Type::FnSig(_) => args, // raw fn ptr — no env arg
            _ => unreachable!(),
        };
        let _ = &mut argv;
        let ret = self.f.append_inst(
            self.cur_block,
            InstKind::Call(known_fid, argv),
            ret_ty,
            None,
        );
        for d in drops {
            self.emit_drop_value(d, Type::Str);
        }
        ret
    }

    fn call_fn_value_raw(&mut self, fn_val: Operand, fn_ty: Type, args: Vec<Operand>) -> ValueId {
        match fn_ty {
            Type::Closure(user_sig_id) => {
                let env_ptr = match fn_val {
                    Operand::Value(v) => v,
                    _ => unreachable!("closure value is SSA"),
                };
                let fn_ptr = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::Ptr, Operand::Value(env_ptr), CLOSURE_FN_ADDR_OFF),
                    Type::Ptr,
                    None,
                );
                let (user_params, ret_ty) = self.fn_sigs[user_sig_id.0 as usize].clone();
                let mut env_first = Vec::with_capacity(user_params.len() + 1);
                env_first.push(Type::Ptr);
                env_first.extend(user_params);
                let env_first_sig = intern_fn_sig(self.fn_sigs, env_first, ret_ty);
                let mut argv = Vec::with_capacity(args.len() + 1);
                argv.push(Operand::Value(env_ptr));
                argv.extend(args);
                self.f.append_inst(
                    self.cur_block,
                    InstKind::CallIndirect(env_first_sig, Operand::Value(fn_ptr), argv),
                    ret_ty,
                    None,
                )
            }
            Type::FnSig(sig_id) => {
                let fn_ptr_val = match fn_val {
                    Operand::Value(v) => v,
                    _ => unreachable!("fnsig value is SSA"),
                };
                let ret_ty = self.fn_sigs[sig_id.0 as usize].1;
                self.f.append_inst(
                    self.cur_block,
                    InstKind::CallIndirect(sig_id, Operand::Value(fn_ptr_val), args),
                    ret_ty,
                    None,
                )
            }
            other => panic!("ssa-lower: call_fn_value: expected Closure or FnSig, got {other:?}"),
        }
    }
}
