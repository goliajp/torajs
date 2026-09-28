//! `Object.hasOwn(obj, key)` / `Reflect.has(obj, key)` compile-time
//! fold pulled out of [`crate::ssa_lower::lower_expr_inner`]
//! `Expr::Call` dispatch as chunk-24 of the `Expr::Call` god-arm
//! decomp (chunks 1-23 = Arr higher-order + Map dispatch + Set
//! dispatch + Arr.push + Number instance methods + bare-name globals
//! + Str regex methods + Number namespace + Array.from + Arr
//! predicate iter + Arr.flatMap + Object.entries + fn-indirect +
//! Number/String/Boolean coercion + universal methods + closure-local
//! + Object.values + Object.keys + Object.getPrototypeOf +
//! Object.assign + Bun runtime cluster + Reflect.get +
//! Symbol.for/keyFor).
//!
//! v0.2 #3 — `Object.hasOwn(obj, key)` (ES §20.1.2.4) folds to a
//! constant Bool when the target lowers to `Type::Obj(sid)` and the
//! key arg is a string literal: the field is either declared on the
//! struct or it's not. Own-only is exactly what §20.1.2.4 asks, so
//! the fold stays sound however the prototype chain grows.
//!
//! `Reflect.has(obj, key)` (§28.1.9) used to ride that same fold on
//! the reasoning that "tr has no prototype chain, so `key in target`
//! collapses to the same own-property check". It has one now, and
//! §28.1.9 is §13.10.1 verbatim, so the surface hands off to
//! `ssa_lower_call_in_op::lower_has_property` and only `Object.hasOwn`
//! is answered here.
//!
//! Variable-key paths + non-struct targets are deferred (caller falls
//! through to the generic Call lowering): returning `None` here lets
//! the surrounding match handle them.
//!
//! Trailing args past `args[1]` are spec-lowered for side-effect
//! parity per the S272 idiom (check.rs S257 already typecheck-drops
//! them). `args[1]` is a String literal (no side effect) and is not
//! re-lowered.

use crate::ast::{Expr, ExprId, PropKey};
use crate::ssa::{IPred, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let (ns_id, m_name) = match ctx.ast.get_expr(callee) {
        Expr::Member { obj, name } => (*obj, name.clone()),
        _ => return None,
    };
    let Expr::Ident(ns) = ctx.ast.get_expr(ns_id) else {
        return None;
    };
    if ns == "Object" && m_name == "__forinKeyLive" && args.len() == 2 {
        return Some(lower_forin_key_live(ctx, args[0], args[1]));
    }
    let is_reflect_has = m_name == "has" && ns == "Reflect";
    if !(m_name == "hasOwn" && ns == "Object") && !is_reflect_has {
        return None;
    }
    if args.len() < 2 {
        return None;
    }
    if is_reflect_has {
        // §28.1.9 steps 1-3 are §13.10.1 steps 5-6 verbatim: throw on
        // a non-Object target, ToPropertyKey, [[HasProperty]]. So
        // `Reflect.has(o, k)` IS `k in o`, and it now emits as one.
        //
        // It used to alias to the own-property probe below instead,
        // on the reasoning that "tr has no prototype chain so own ==
        // all and the spec gap with `in` is empty". That was true
        // when written and stopped being true; the copy could not
        // notice, because it lived in the lowering and the chain grew
        // in the runtime. Three ways it was wrong, all silent:
        // `Reflect.has(new Map(), "get")` answered false (chain),
        // `Reflect.has(42, "x")` answered false where step 1 throws,
        // and every non-struct receiver died at "unsupported member
        // call shape" while `k in o` on the same value worked.
        //
        // Argument order is the one thing the two surfaces do NOT
        // share — `in` evaluates the key first — so the operands are
        // lowered here, in call order, and handed over lowered.
        let obj_op = ctx.lower_expr(args[0]);
        let key_op = ctx.lower_expr(args[1]);
        // S302 — trailing args eval-then-discard, before the call.
        for &a in args.iter().skip(2) {
            let _ = ctx.lower_expr(a);
        }
        return Some(crate::ssa_lower_call_in_op::lower_has_property(
            ctx, key_op, args[1], obj_op,
        ));
    }
    let key_lit: Option<PropKey> = match ctx.ast.get_expr(args[1]) {
        Expr::String(k) => Some(PropKey::from(k.clone())),
        _ => None,
    };
    // Borrow-only read of the obj — lower_expr loads the local slot
    // but ownership stays with the caller's scope (which will drop on
    // exit). No emit_drop_value here.
    let obj_op = ctx.lower_expr(args[0]);
    let obj_ty = ctx.operand_ty(&obj_op);
    // A receiver whose shape is known statically still has its
    // own-property face only at run time — a Map's bag, an array's
    // indices and `length`, a string's indices, a closure's `name`.
    // `any_prop_has` below already answers every one of them
    // correctly (each probed against bun, not assumed); the refusal
    // was this lowering's alone, and it read "unsupported member call
    // shape: hasOwn" while the same value spelled `any` answered.
    // `Type::Obj(_)` stays out: a struct's own face IS its
    // compile-time layout, so the fold at the bottom is both exact
    // and free. `FnSig` stays out for the reason `in` gives — a bare
    // function-pointer value has no cell to box.
    let (obj_op, obj_ty) = if matches!(obj_ty, Type::Obj(_) | Type::Any | Type::FnSig(_)) {
        (obj_op, obj_ty)
    } else {
        (ctx.box_to_any(obj_op), Type::Any)
    };
    // chunk D-1 (RFC 20260711) — Any receiver: runtime own-property
    // probe through `any_prop_has` (B3 substrate). Static key names
    // intern; dynamic keys lower + coerce to a Str (Substr temps are
    // owned and dropped — same ledger as `ssa_lower_delete`).
    if matches!(obj_ty, Type::Any) {
        // A key resolved through §7.1.19 at run time is owned and of
        // unknowable kind, so it releases through the kind-aware
        // dropper rather than the Str ledger below.
        let mut resolved_key = false;
        let (key_v, owned_temp) = if let Some(key) = &key_lit {
            (ctx.intern_string_literal(key), None)
        } else {
            let k_raw = ctx.lower_expr(args[1]);
            let k_ty = ctx.operand_ty(&k_raw);
            // §20.1.3.4 step 1 is ToPropertyKey, not ToString, so
            // §7.1.19 step 2 hands a Symbol key straight through — the
            // own-property probe keys off the cell's own tag. Coercing
            // it would hit §7.1.17's "cannot convert a Symbol to a
            // string" TypeError on a call that must simply answer true.
            if k_ty == Type::Any {
                // The kind is the run time's to decide; stringifying
                // would ask for "Symbol(x)" and answer false about a
                // property that is there.
                let k = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.anyv_to_property_key, vec![k_raw.clone()]),
                    Type::Ptr,
                    None,
                );
                ctx.release_owned_temp(args[1], &k_raw);
                ctx.emit_throw_check(None);
                resolved_key = true;
                (k, None)
            } else {
                let key_op = if k_ty == Type::Symbol {
                    k_raw.clone()
                } else {
                    ctx.coerce_to_str(k_raw.clone(), k_ty)
                };
                let Operand::Value(key_v) = key_op else {
                    panic!("ssa-lower: hasOwn key lowered to a non-value operand");
                };
                (key_v, Some((key_op, k_ty == Type::Substr, k_raw, k_ty)))
            }
        };
        for &a in args.iter().skip(2) {
            let _ = ctx.lower_expr(a);
        }
        let ans = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.any_prop_has,
                vec![obj_op, Operand::Value(key_v)],
            ),
            Type::I64,
            None,
        );
        ctx.emit_throw_check(None);
        if resolved_key {
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.anyv_property_key_drop,
                    vec![Operand::Value(key_v)],
                ),
            );
        }
        if let Some((key_op, coerce_owned, k_raw, k_ty)) = owned_temp {
            if coerce_owned {
                ctx.emit_drop_value(key_op, Type::Str);
            }
            if k_ty.is_refcounted() && ctx.expr_transfers_ownership(args[1]) {
                ctx.emit_drop_value(k_raw, k_ty);
            }
        }
        let b = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Ne, Operand::Value(ans), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        return Some(Operand::Value(b));
    }
    let Some(key) = key_lit else {
        // Typed target + dynamic key: deferred — caller falls through.
        return None;
    };
    // S302 — lower-and-drop trailing args[2..] per S272 idiom so
    // step()-style side-effect exprs fire per ES eval-then-discard
    // (check.rs S257 already typecheck-dropped). args[1] is a String
    // literal (no side effect) gated above.
    for &a in args.iter().skip(2) {
        let _ = ctx.lower_expr(a);
    }
    let Type::Obj(sid) = obj_ty else {
        // Non-struct target: deferred substrate — caller falls through.
        return None;
    };
    // An accessor property (RFC 20260714-objlit-accessor) lives in the
    // layout under its synthetic half-slot spelling — either half makes
    // the name an own property (§10.4). The synthetic spellings
    // themselves are NOT properties (the runtime lane guards them via
    // accessor_name_kind; mirror that here so the internal name never
    // leaks onto the user-visible surface).
    let is_internal_spelling =
        crate::check_type_of_object_lit::accessor_slot(key.as_wtf8()).is_some();
    let has = !is_internal_spelling
        && ctx.struct_layouts[sid.0 as usize].iter().any(|(n, _)| {
            n == &key
                || n.strip_prefix("__getter_")
                    .is_some_and(|p| p == key.as_wtf8())
                || n.strip_prefix("__setter_")
                    .is_some_and(|p| p == key.as_wtf8())
        });
    Some(Operand::ConstBool(has))
}

/// `Object.__forinKeyLive(obj, key)` — the for-in mid-loop-delete guard
/// a generator machine body spells as a call (`desugar_generators_forin`);
/// a receiver whose key set is fixed answers `true`.
fn lower_forin_key_live(ctx: &mut LowerCtx<'_>, obj: ExprId, key: ExprId) -> Operand {
    let obj_op = ctx.lower_expr(obj);
    let key_op = ctx.lower_expr(key);
    let Some(has) = crate::ssa_lower_stmt_for_of::forin_key_live(ctx, obj_op, key_op) else {
        return Operand::ConstBool(true);
    };
    let live = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(has), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    Operand::Value(live)
}
