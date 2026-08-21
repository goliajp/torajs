//! Call-fn-value dispatch helpers for `LowerCtx<'a>` extracted from
//! `ssa_lower.rs` chunk 398 — Path A.3-batch19.
//!
//! Groups the M6.2 "call a Closure or FnSig value with a list of args"
//! dispatch layer used inside `Array.map/filter/reduce/forEach` and
//! peer higher-order fn bodies. Public methods:
//!
//! - `call_fn_value(fn_val, fn_ty, args, sig_skip)` — indirect dispatch, walks
//!   the FnSig for Substr → Str boundary materialization + drops.
//! - `call_fn_value_devirt(known_fid, fn_val, fn_ty, args, sig_skip)` — v0.6+1
//!   perf checkpoint direct-Call variant when the callee FuncId is
//!   statically known so LLVM value-prop can inline through
//!   `xs.map(cb)` loops.
//!
//! Private helpers used only from within this module:
//!
//! - `sig_param_tys(fn_ty)` — pull the param-type vec from a
//!   FnSig / Closure type; None for non-callable.
//! - `materialize_call_args(fn_ty, args, sig_skip)` — Phase Substr.B boundary
//!   materialization: allocate an owned Str via `substr_to_owned`
//!   when the callee expects `Type::Str` but the operand is
//!   `Type::Substr`. Returns `(materialized_args, drops)`.
//! - `call_fn_value_raw(fn_val, fn_ty, args, sig_skip, spec_argc)` —
//!   the raw indirect dispatch (Closure = load fn_ptr @ +8 +
//!   CallIndirect with env and the S1 spec argc prepended; FnSig =
//!   plain CallIndirect).

use crate::ssa::{FuncId, InstKind, Operand, Type, ValueId};
use crate::ssa_lower::{CLOSURE_FN_ADDR_OFF, LowerCtx};
use crate::ssa_lower_call_arg_conv::ArgConv;
use crate::ssa_lower_interners::intern_fn_sig;

impl<'a> LowerCtx<'a> {
    /// M6.2 — call a Closure or FnSig value with a list of args. Used
    /// inside Array.map/filter/reduce/forEach loop bodies (and is the
    /// mirror of the existing inline call-via-Closure / call-via-FnSig
    /// dispatch, packaged for re-use).
    /// Look up a sig's param types from a callable type. Returns None for
    /// non-callable types — callers should already have validated.
    pub(crate) fn sig_param_tys(&self, fn_ty: Type) -> Option<Vec<Type>> {
        let sig_id = match fn_ty {
            Type::FnSig(s) | Type::Closure(s) => s,
            _ => return None,
        };
        Some(self.fn_sigs[sig_id.0 as usize].0.clone())
    }

    /// RC-1 (RFC 20260706-test262-bug-corpus) — a callable's declared
    /// return type. The HO array arms use this to detect Void-returning
    /// callbacks: their call produces no value, so the consumers fold
    /// the use to the JS `undefined` semantic (ToBoolean → false /
    /// boxed ANY_UNDEF) instead of reading the void call's result.
    pub(crate) fn callback_ret_ty(&self, fn_ty: Type) -> Option<Type> {
        let sig_id = match fn_ty {
            Type::FnSig(s) | Type::Closure(s) => s,
            _ => return None,
        };
        Some(self.fn_sigs[sig_id.0 as usize].1)
    }

    /// Phase Substr.B — boundary materialization. If the callee expects
    /// `Type::Str` for an arg position and the actual operand is
    /// `Type::Substr`, allocate an owned Str via substr_to_owned and
    /// return the materialized operand; the caller drops it after the
    /// call. An `Any` param position with a concrete arg boxes it via
    /// `box_to_any` — the CallIndirect otherwise passes the raw slot
    /// bits into a box-shaped ABI lane (SIGSEGV when an
    /// inference-defaulted `(x: any, y: any)` comparator derefs a raw
    /// i64; RFC 20260705 chunk 549). Heap payloads take an explicit
    /// rc_inc before boxing — chunk 753 made `box_to_any` a pure
    /// encode for non-Str heap values (only the Str-slot helper still
    /// incs), and every caller here hands a BORROWED elem (HO-loop
    /// LoadDyn slot); the boxed +1 joins the post-call drop list so
    /// the pair self-balances (rotation 184 — the stale "the box
    /// helper incs" premise here was a use-after-free: the drop
    /// stole the array's own element stake).
    /// Other type pairs pass through unchanged. Returns the
    /// (possibly-rewritten) args plus the (value, type) drops to emit
    /// after the call returns.
    ///
    /// `sig_skip` — count of leading argv entries that are NOT in the
    /// callable's sig: a promoted receiver-first callback (fnexpr-this
    /// knife 4) prepends the boxed `__this` while the SSA sig sheds it
    /// (ssa_lower_closure), so positional alignment starts after those.
    /// Skipped args pass through untouched.
    fn materialize_call_args(
        &mut self,
        fn_ty: Type,
        args: Vec<Operand>,
        sig_skip: usize,
    ) -> (Vec<Operand>, Vec<(Operand, Type)>) {
        let Some(param_tys) = self.sig_param_tys(fn_ty) else {
            return (args, Vec::new());
        };
        let mut out = Vec::with_capacity(args.len());
        let mut drops = Vec::new();
        for (i, a) in args.into_iter().enumerate() {
            let actual = self.operand_ty(&a);
            let expected = i
                .checked_sub(sig_skip)
                .and_then(|pi| param_tys.get(pi).copied());
            // RFC 20260707 chunk 626 — typed array into an Arr<Any>
            // param marks the block's elem kind (self-gating).
            if let Some(exp) = expected {
                self.mark_arr_arg_for_any_param(exp, &a);
            }
            // A split product into an owned-string parameter: this
            // lane holds only operands, so it cannot tell a fresh
            // product from an alias and copies out either way; the
            // copy is released after the call (rotation 469, 469-01).
            if let Some(exp) = expected
                && crate::ssa_lower_assign_slot_fit::arr_views_into_owned_slot(self, exp, actual)
            {
                let copy = crate::ssa_lower_assign_slot_fit::copy_views_out(self, exp, a);
                out.push(copy.clone());
                drops.push((copy, exp));
                continue;
            }
            // The shared contract decides; this lane only owns the
            // bookkeeping. Beyond the sig's arity there is no
            // parameter to reach, so nothing converts.
            let conv = match expected {
                Some(exp) => crate::ssa_lower_call_arg_conv::arg_conv(exp, actual),
                None => ArgConv::None,
            };
            match conv {
                ArgConv::None => out.push(a),
                // Both number lanes. This lane used to carry neither,
                // and three higher-order call stations patched the
                // I64 → F64 half back in on their own side before
                // handing their argv here; they no longer need to.
                ArgConv::ToF64 => {
                    let v = self.coerce_to_f64(a);
                    out.push(v);
                }
                ArgConv::ToI64 => {
                    let v = self.coerce_to_i64(a);
                    out.push(v);
                }
                ArgConv::BoxAny => {
                    // Str included: anyv_box_str_slot is rc-neutral too
                    // (box_void_ptr is a pure encode; the sentinel/null
                    // shapes carry no rc at all).
                    if actual.is_refcounted() {
                        self.emit_rc_inc(a.clone());
                    }
                    let boxed = self.box_to_any(a);
                    out.push(boxed);
                    drops.push((boxed, Type::Any));
                }
                // Any arg into a typed param — unbox at the call
                // boundary. Without this the CallIndirect passes the
                // NaN-box bits raw into the typed lane: `Map<string,
                // number>.forEach((v: number) => ...)` re-boxes
                // entries as Any, and the typed callback read the box
                // bits as an i64 (silent-wrong arithmetic).
                ArgConv::AnyToNumber(t) => {
                    let v = self.coerce_any_to_number(a, t);
                    out.push(v);
                }
                ArgConv::AnyToBool => {
                    let v = self.coerce_to_bool(a);
                    out.push(v);
                }
                ArgConv::AnyToStr => {
                    let s = self.coerce_to_str(a, Type::Any);
                    out.push(s);
                    drops.push((s, Type::Str));
                }
                ArgConv::AnyToBigInt => {
                    let b = self.coerce_any_to_bigint(a);
                    out.push(b);
                    drops.push((b, Type::BigInt));
                }
                ArgConv::SubstrToStr => {
                    let v = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(self.intrinsics.substr_to_owned, vec![a]),
                        Type::Str,
                        None,
                    );
                    out.push(Operand::Value(v));
                    drops.push((Operand::Value(v), Type::Str));
                }
            }
        }
        (out, drops)
    }

    /// `spec_argc` — the S1 hidden-argc value for this call. Every
    /// caller of this family is a builtin higher-order lane whose
    /// spec fixes the callback's argument count (map/filter/forEach
    /// pass «kValue, k, O» = 3 regardless of what the callback
    /// declares), while the lane only materializes the slots the
    /// callback's sig can receive. Deriving argc from the physical
    /// arg list understated it — `[10,20].map(function(){ return
    /// arguments.length })` answered 1 where the spec observes 3 —
    /// so the lane states the spec count and the physical list stays
    /// an unobservable transport optimization.
    pub(crate) fn call_fn_value(
        &mut self,
        fn_val: Operand,
        fn_ty: Type,
        args: Vec<Operand>,
        sig_skip: usize,
        spec_argc: i64,
    ) -> ValueId {
        let (args, drops) = self.materialize_call_args(fn_ty, args, sig_skip);
        let ret = self.call_fn_value_raw(fn_val, fn_ty, args, sig_skip, spec_argc);
        for (d, ty) in drops {
            self.emit_drop_value(d, ty);
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
        sig_skip: usize,
        spec_argc: i64,
    ) -> ValueId {
        let (args, drops) = self.materialize_call_args(fn_ty, args, sig_skip);
        let ret_ty = match fn_ty {
            Type::Closure(sig_id) | Type::FnSig(sig_id) => self.fn_sigs[sig_id.0 as usize].1,
            other => panic!("call_fn_value_devirt: expected Closure/FnSig, got {other:?}"),
        };
        let mut argv: Vec<Operand> = match fn_ty {
            Type::Closure(_) => {
                /* Closure ABI: env_ptr, S1 argc (the spec count — see
                 * call_fn_value's doc), then user args. */
                let mut a = Vec::with_capacity(args.len() + 2);
                a.push(fn_val);
                a.push(Operand::ConstI64(spec_argc));
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
        for (d, ty) in drops {
            self.emit_drop_value(d, ty);
        }
        ret
    }

    fn call_fn_value_raw(
        &mut self,
        fn_val: Operand,
        fn_ty: Type,
        args: Vec<Operand>,
        sig_skip: usize,
        spec_argc: i64,
    ) -> ValueId {
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
                let mut env_first = Vec::with_capacity(user_params.len() + 2 + sig_skip);
                env_first.push(Type::Ptr);
                // S1 (RFC 20260810-indirect-argc-abi) — hidden I64
                // argc at ABI position 1.
                env_first.push(Type::I64);
                // Skipped leading argv entries (a promoted callback's
                // boxed `__this`) are in the native ABI but shed from
                // the user sig — the indirect call's own sig must
                // carry their slots or the CallIndirect type drops
                // them (the devirt lane never sees this: a direct
                // Call takes its type from the fn definition).
                for _ in 0..sig_skip {
                    env_first.push(Type::Any);
                }
                env_first.extend(user_params);
                let env_first_sig = intern_fn_sig(self.fn_sigs, env_first, ret_ty);
                let mut argv = Vec::with_capacity(args.len() + 2);
                argv.push(Operand::Value(env_ptr));
                argv.push(Operand::ConstI64(spec_argc));
                argv.extend(args);
                self.f.append_inst(
                    self.cur_block,
                    InstKind::CallIndirect(env_first_sig, Operand::Value(fn_ptr), argv),
                    ret_ty,
                    None,
                )
            }
            Type::FnSig(sig_id) => {
                // A bare fn value never rides the fnexpr-this
                // promotion (no env, no receiver slot) — a skip here
                // would silently mis-type the indirect call.
                assert_eq!(sig_skip, 0, "sig_skip on a FnSig callee");
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
