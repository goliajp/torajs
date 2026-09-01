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
//! v0.2 #3 — both `Object.hasOwn(obj, key)` (ES §20.1.2.4) and
//! `Reflect.has(obj, key)` (ES §28.1.9) fold to a constant Bool when
//! the target lowers to `Type::Obj(sid)` and the key arg is a string
//! literal: the field is either declared on the struct or it's not.
//!
//! tr has no prototype chain, so `Reflect.has` (`key in target` per
//! spec, walking the proto chain) collapses to the same own-property
//! check as `Object.hasOwn` — the spec gap is empty for tr's typed
//! struct subset.
//!
//! Variable-key paths + non-struct targets are deferred (caller falls
//! through to the generic Call lowering): returning `None` here lets
//! the surrounding match handle them.
//!
//! Trailing args past `args[1]` are spec-lowered for side-effect
//! parity per the S272 idiom (check.rs S257 already typecheck-drops
//! them). `args[1]` is a String literal (no side effect) and is not
//! re-lowered.

use crate::ast::{Expr, ExprId};
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
    let matches = (m_name == "hasOwn" && ns == "Object")
        // `Reflect.has(obj, key)` aliases to the same emit — tr has
        // no prototype chain so own == all and the spec gap with `in`
        // is empty.
        || (m_name == "has" && ns == "Reflect");
    if !matches {
        return None;
    }
    if args.len() < 2 {
        return None;
    }
    let key_lit: Option<String> = match ctx.ast.get_expr(args[1]) {
        Expr::String(k) => Some(k.to_string_lossy_owned()),
        _ => None,
    };
    // Borrow-only read of the obj — lower_expr loads the local slot
    // but ownership stays with the caller's scope (which will drop on
    // exit). No emit_drop_value here.
    let obj_op = ctx.lower_expr(args[0]);
    let obj_ty = ctx.operand_ty(&obj_op);
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
    let is_internal_spelling = key.starts_with("__getter_") || key.starts_with("__setter_");
    let has = !is_internal_spelling
        && ctx.struct_layouts[sid.0 as usize].iter().any(|(n, _)| {
            n == &key
                || n.strip_prefix("__getter_") == Some(key.as_str())
                || n.strip_prefix("__setter_") == Some(key.as_str())
        });
    Some(Operand::ConstBool(has))
}
