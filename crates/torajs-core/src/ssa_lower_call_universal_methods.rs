//! V3-18 m2.a / m2.d — `Object.prototype` methods dispatched on
//! primitives (auto-boxing) AND struct instances (class instance
//! objects). Pulled out of [`crate::ssa_lower::lower_expr_inner`]
//! `Expr::Call` as chunk-15 of the `Expr::Call` god-arm decomp
//! (chunks 1-14 done).
//!
//! Covers five method names: `valueOf` / `hasOwnProperty` /
//! `propertyIsEnumerable` / `isPrototypeOf` / `toString`. Must catch
//! BEFORE any other Member-method dispatch since these names overlap
//! nothing else.
//!
//! Receiver-type routes:
//! - `Type::Arr(_) + valueOf` — identity (ES §23.1.3.34). Handled
//!   first to keep other Array.toString / Array.hasOwnProperty
//!   dispatch arms downstream reachable for non-valueOf calls.
//! - primitives (F64/I64/I32/Str/Substr/Bool/BigInt/Symbol) or
//!   `Type::Obj(_)`:
//!   - `valueOf` — identity.
//!   - `toString` on Obj — `"[object Object]"` const (subset stub
//!     matching bun for non-overridden toString). Primitives fall
//!     through to the existing m1.h.27 / m1.h.47 toFixed/toString
//!     arms.
//!   - `hasOwnProperty` / `propertyIsEnumerable` on Obj with literal
//!     key — compile-time field-name fold (struct_layouts lookup).
//!   - Same with runtime key — V3-18 m2.g inline str_eq chain over
//!     the struct's field names (each name interned as literal Str).
//!   - `isPrototypeOf` on Obj, primitive variants of any predicate,
//!     no args — drop+return false.
//!
//! S290 + S304 trailing-arg drops preserved (lower-and-drop per S272
//! idiom so step()-style side-effect exprs fire).
//!
//! Returns `Some(result)` when dispatched; `None` falls through to the
//! caller's downstream arms (vtable virtual-dispatch / NS toString /
//! ...).

use crate::ast::{Expr, ExprId};
use crate::ssa::{BinOp as SsaBinOp, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Try to lower a `<recv>.{valueOf | hasOwnProperty |
/// propertyIsEnumerable | isPrototypeOf | toString}(...)` call.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let (recv_id, m_name) = match ctx.ast.get_expr(callee) {
        Expr::Member { obj, name } => (*obj, name.clone()),
        _ => return None,
    };
    if !matches!(
        m_name.as_str(),
        "valueOf" | "hasOwnProperty" | "propertyIsEnumerable" | "isPrototypeOf" | "toString"
    ) {
        return None;
    }
    // RFC 20260719-fn-tostring-source B6 — `Math.max.toString()`:
    // the namespace-static builtin fn member has no value form, so
    // the eager receiver lower below would reject it. Bail first;
    // the fn-tostring wedge downstream folds the JSC named native
    // form.
    if crate::ssa_lower_call_fn_tostring::namespace_static_native_form(ctx, recv_id).is_some() {
        return None;
    }
    // A class that declares one of these five names answers with its
    // own method, not with the Object.prototype default this arm
    // folds. Usually that never reaches here: a name with a single
    // owner is rewritten to a direct `__cm_<C>__<M>` call during
    // desugar. But when unrelated classes declare the SAME name,
    // desugar leaves every call Member-shaped for the sibling-class
    // lane to resolve by the receiver's static class — and that lane
    // sits behind this one, so two classes with a `toString` each made
    // both of them answer "[object Object]". Decline on exactly the
    // pair of conditions that lane requires, so the two cannot
    // disagree about who owns the call.
    if ctx.ast.method_owners.contains_key(&m_name)
        && let Some(cname) = crate::ssa_lower_member_obj_field::class_name_of_expr(ctx, recv_id)
        && crate::ssa_lower_call_sibling_class_dispatch::declaring_class_fn(ctx, &cname, &m_name)
            .is_some()
    {
        return None;
    }
    let recv_op = ctx.lower_expr(recv_id);
    let recv_ty = ctx.operand_ty(&recv_op);
    let is_prim = matches!(
        recv_ty,
        Type::F64
            | Type::I64
            | Type::I32
            | Type::Str
            | Type::Substr
            | Type::Bool
            | Type::BigInt
            | Type::Symbol
    );
    let is_obj = matches!(recv_ty, Type::Obj(_));
    if m_name == "valueOf" {
        return try_value_of(ctx, recv_id, recv_op, recv_ty, is_prim, is_obj, args);
    }
    if !(is_prim || is_obj) {
        // RFC 20260705 chunk 555 — the receiver is already lowered;
        // park the operand for the next cascade arm (single-eval).
        ctx.redispatch_lowered = Some((recv_id, recv_op));
        return None;
    }
    if m_name == "toString" {
        return try_to_string(ctx, recv_id, recv_op, is_obj, args);
    }
    // hasOwnProperty / propertyIsEnumerable / isPrototypeOf.
    if let Type::Obj(_) = recv_ty
        && matches!(m_name.as_str(), "hasOwnProperty" | "propertyIsEnumerable")
        && let Some(arg_eid) = args.first()
    {
        return Some(emit_obj_has_own_property(
            ctx, recv_id, &recv_op, recv_ty, &m_name, *arg_eid, args,
        ));
    }
    if matches!(recv_ty, Type::Str | Type::Substr)
        && matches!(m_name.as_str(), "hasOwnProperty" | "propertyIsEnumerable")
        && let Some(arg_eid) = args.first()
    {
        return Some(try_str_prop_check(
            ctx, recv_op, recv_ty, &m_name, *arg_eid, args,
        ));
    }
    // Fallback (primitives, isPrototypeOf, no args): release an owned
    // temp arg + return false. Ident args are borrows — the old
    // consume+drop pair destroyed the source's stake while later reads
    // still used it (reuse-window probe read filler bytes).
    if !args.is_empty() {
        let arg_val = ctx.lower_expr(args[0]);
        ctx.release_owned_temp(args[0], &arg_val);
    }
    // S304 — lower-and-drop trailing args (isPrototypeOf useful=1;
    // primitive fallback also covers stray trailing).
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    Some(Operand::ConstBool(false))
}

/// `<recv>.valueOf(...)` — three receiver families share the identity
/// semantics: `Type::Arr` (ES §23.1.3.34), primitives, and Type::Obj.
/// Handles the Arr arm BEFORE the prim/obj gate so `arr.toString` /
/// `arr.hasOwnProperty` cascade arms further down remain reachable
/// for non-valueOf Array calls.
fn try_value_of(
    ctx: &mut LowerCtx<'_>,
    recv_id: ExprId,
    recv_op: Operand,
    recv_ty: Type,
    is_prim: bool,
    is_obj: bool,
    args: &[ExprId],
) -> Option<Operand> {
    if matches!(recv_ty, Type::Arr(_)) {
        // S290 — trailing-arg ignore per ES §23.1.3.34; arr.valueOf
        // is identity. Lower-and-drop args[..] before the recv
        // return so step()-style side-effect exprs fire (S272 idiom).
        for &a in args.iter() {
            let _ = ctx.lower_expr(a);
        }
        // RFC 20260705 owned-result invariant: identity result
        // carries its own ref.
        ctx.emit_rc_inc(recv_op.clone());
        return Some(recv_op);
    }
    if !(is_prim || is_obj) {
        // RFC 20260705 chunk 555 — the receiver is already lowered;
        // park the operand for the next cascade arm (single-eval).
        ctx.redispatch_lowered = Some((recv_id, recv_op));
        return None;
    }
    // S290 — primitive valueOf trailing-arg ignore; lower-and-drop
    // before identity return (S272 idiom).
    for &a in args.iter() {
        let _ = ctx.lower_expr(a);
    }
    // RFC 20260705 owned-result invariant: refcounted identity
    // results (Str / Substr / BigInt / Symbol) carry their own
    // ref; Copy primitives need none.
    if recv_ty.is_refcounted() {
        ctx.emit_rc_inc(recv_op.clone());
    }
    Some(recv_op)
}

/// `<recv>.toString(...)` — Type::Obj arm dispatches Error-derived
/// class instances through the `error_tostring_dispatch` runtime
/// entry (monkey-patch probe + §20.5.3.4 formatter; kept a runtime
/// helper, not an injected class method, so `toString` never
/// enters `method_owners` and pollutes the checker's resolution of
/// a plain `x.toString()` on a primitive / any / unrelated class);
/// non-Error Obj folds to `"[object Object]"` per ES §19.1.3.6.
/// Primitives park the lowered receiver + fall through to
/// number_methods so `toFixed / toString(radix)` etc. take over
/// (chunk 555 single-eval).
fn try_to_string(
    ctx: &mut LowerCtx<'_>,
    recv_id: ExprId,
    recv_op: Operand,
    is_obj: bool,
    args: &[ExprId],
) -> Option<Operand> {
    if is_obj {
        if let Some(cname) = crate::ssa_lower_member_obj_field::class_name_of_expr(ctx, recv_id)
            && ctx.class_is_error_derived(&cname)
        {
            // S304 — lower-and-drop trailing args (toString is 0-arg).
            for &a in args.iter() {
                let _ = ctx.lower_expr(a);
            }
            let cur_block = ctx.cur_block;
            // rotation 141 — the dispatch entry (not the formatter
            // direct): a monkey-patched `Error.prototype.toString`
            // wins on typed receivers too; NULL = pending throw
            // (override threw / non-string boundary), divert.
            let v = ctx.f.append_inst(
                cur_block,
                InstKind::Call(ctx.intrinsics.error_tostring_dispatch, vec![recv_op]),
                Type::Str,
                None,
            );
            ctx.emit_throw_check(None);
            return Some(Operand::Value(v));
        }
        // S304 — lower-and-drop trailing args per S272 idiom so
        // step()-style side-effect exprs fire (toString is 0-useful
        // on a non-error struct instance; "[object Object]" const
        // independent of args, ES §19.1.3.6).
        for &a in args.iter() {
            let _ = ctx.lower_expr(a);
        }
        let v = ctx.intern_string_literal("[object Object]");
        return Some(Operand::Value(v));
    }
    // For primitives, fall through to allow existing arms
    // (toFixed/toString) to take over. Chunk 555 — park the
    // lowered receiver so the accepting arm reuses it
    // (`getNum().toString()` evaluated its call twice pre-555).
    ctx.redispatch_lowered = Some((recv_id, recv_op));
    None
}

/// RFC 20260716 刀 13 / 刀 20 — typed-Str / -Substr
/// `.hasOwnProperty(key)` + `.propertyIsEnumerable(key)` per ES §22.1.4
/// String Exotic Object. `"length"` and every canonical index
/// `[0, [[StringData]].length)` are own; every other key is not. The
/// pair differs only in `"length"` handling (non-enumerable per
/// §22.1.5.1): `str_prop_enumerable` returns 0 for it while
/// `str_prop_has` returns 1. Canonical indices are 1 on both.
/// Pre-fix the primitive fallback below folded to `ConstBool(false)`
/// for every key, missing bun-visible spec behavior on typed-Str
/// receivers. Substr materializes through `substr_to_owned` — the
/// runtime helper reads a plain Str layout only.
fn try_str_prop_check(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    recv_ty: Type,
    m_name: &str,
    arg_eid: ExprId,
    args: &[ExprId],
) -> Operand {
    let key_op = ctx.lower_expr(arg_eid);
    let key_ty = ctx.operand_ty(&key_op);
    let key_str = if key_ty == Type::Substr {
        let owned = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.substr_to_owned, vec![key_op.clone()]),
            Type::Str,
            None,
        );
        Operand::Value(owned)
    } else if key_ty == Type::Str {
        key_op.clone()
    } else {
        // Non-Str key (Number literal etc.): coerce first so
        // the runtime helper sees a live Str block. Uses the
        // shared `coerce_to_str` (Substr/I64/F64/Bool/Any all
        // covered). A borrowed Str stays borrowed; a fresh
        // owned Str drops after the call.
        ctx.coerce_to_str(key_op.clone(), key_ty)
    };
    let recv_str = if recv_ty == Type::Substr {
        let owned = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.substr_to_owned, vec![recv_op.clone()]),
            Type::Str,
            None,
        );
        Operand::Value(owned)
    } else {
        recv_op.clone()
    };
    let intrinsic = if m_name == "propertyIsEnumerable" {
        ctx.intrinsics.str_prop_enumerable
    } else {
        ctx.intrinsics.str_prop_has
    };
    let hit_i64 = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(intrinsic, vec![recv_str.clone(), key_str.clone()]),
        Type::I64,
        None,
    );
    // Drop the coerced temps we newly own. The Substr materialize
    // path always mints a fresh Str; the non-Str-key coerce path
    // does too. A same-tag borrow (Str + Str) uses `key_op`
    // directly — `release_owned_temp` handles the Ident-vs-owned
    // split.
    if recv_ty == Type::Substr {
        ctx.emit_drop_value(recv_str, Type::Str);
    }
    if !matches!(key_ty, Type::Str) {
        ctx.emit_drop_value(key_str, Type::Str);
    }
    ctx.release_owned_temp(arg_eid, &key_op);
    // Lower trailing args for side effects (S304 idiom).
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    let hit_bool = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(
            crate::ssa::IPred::Ne,
            Operand::Value(hit_i64),
            Operand::ConstI64(0),
        ),
        Type::Bool,
        None,
    );
    Operand::Value(hit_bool)
}

/// Emit `obj.hasOwnProperty(k)` / `obj.propertyIsEnumerable(k)` on a
/// `Type::Obj` receiver. Literal key → compile-time fold against
/// struct_layouts. Runtime key (V3-18 m2.g) → inline str_eq chain over
/// the struct's field names (zero-alloc — each name interned as
/// literal Str).
///
/// All three of an Error-derived receiver's injected slots carry
/// runtime state a layout list cannot express, and each differently:
///
/// - `message` (§20.5.6.1.1) — propertyIsEnumerable is constantly
///   false ([[Enumerable]]: false); hasOwnProperty reads the
///   own-absence sentinel through `__torajs_error_message_present`.
/// - `name` (§20.5.3.2) — lives on `<C>.prototype`, so an own one
///   exists only where user code assigned it, and that assignment is
///   an ordinary enumerable data property. Both probes therefore ask
///   the same question and share `__torajs_error_name_present`.
/// - `stack` — same [[Enumerable]]: false as `message`, but written
///   by every construction, so its own-ness needs no runtime probe
///   and only the enumerable answer moves.
fn emit_obj_has_own_property(
    ctx: &mut LowerCtx<'_>,
    recv_id: ExprId,
    recv_op: &Operand,
    recv_ty: Type,
    m_name: &str,
    arg_eid: ExprId,
    args: &[ExprId],
) -> Operand {
    let Type::Obj(sid) = recv_ty else {
        unreachable!("emit_obj_has_own_property called with non-Obj receiver");
    };
    let is_err_recv = crate::ssa_lower_member_obj_field::class_name_of_expr(ctx, recv_id)
        .is_some_and(|c| ctx.class_is_error_derived(&c));
    let is_enumerable_probe = m_name == "propertyIsEnumerable";
    let emit_present = |ctx: &mut LowerCtx<'_>, key: &str| {
        let target = if key == "message" {
            ctx.intrinsics.error_message_present
        } else {
            ctx.intrinsics.error_name_present
        };
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(target, vec![recv_op.clone()]),
            Type::Bool,
            None,
        );
        Operand::Value(v)
    };
    if let Expr::String(key) = ctx.ast.get_expr(arg_eid) {
        // Literal key — compile-time fold. The lowered literal is a
        // static cell (rc no-op); nothing to release.
        let key = key.clone();
        let layout = &ctx.struct_layouts[sid.0 as usize];
        // An accessor member rides the layout under its synthetic
        // `__getter_<k>` / `__setter_<k>` name — it IS the own
        // property `<k>` (test262 8.12.1-1_20: `{get foo(){}}`
        // hasOwnProperty("foo") is true).
        let getter = format!("__getter_{key}");
        let setter = format!("__setter_{key}");
        let result = layout
            .iter()
            .any(|(fname, _)| *fname == key || *fname == getter || *fname == setter);
        let arg_val = ctx.lower_expr(arg_eid);
        ctx.release_owned_temp(arg_eid, &arg_val);
        // S304 — lower-and-drop trailing args per S272 idiom
        // (hasOwnProperty / propertyIsEnumerable useful arity 1).
        for &a in args.iter().skip(1) {
            let _ = ctx.lower_expr(a);
        }
        if result && is_err_recv {
            if key == "message" {
                if is_enumerable_probe {
                    return Operand::ConstBool(false);
                }
                return emit_present(ctx, "message");
            }
            // `name` answers the SAME probe either way: an own one
            // only exists because user code assigned it, and that
            // assignment is an ordinary enumerable data property.
            if key == "name" {
                return emit_present(ctx, "name");
            }
            // `stack` shares msgDesc's [[Enumerable]]: false but is
            // unconditionally own, so only the enumerable probe moves.
            if key == "stack" && is_enumerable_probe {
                return Operand::ConstBool(false);
            }
        }
        return Operand::ConstBool(result);
    }
    // Runtime key — emit inline str_eq chain.
    let layout = ctx.struct_layouts[sid.0 as usize].clone();
    let key_op = ctx.lower_expr(arg_eid);
    let key_ty = ctx.operand_ty(&key_op);
    let mut acc: Operand = Operand::ConstBool(false);
    for (fname, _) in &layout {
        // Accessor synthetic names compare under their public key.
        let public = fname
            .strip_prefix("__getter_")
            .or_else(|| fname.strip_prefix("__setter_"))
            .unwrap_or(fname);
        // Error-derived `message` runtime attributes (see fn doc):
        // the enumerable probe never matches it; the hasOwnProperty
        // chain gates its eq on the own-presence probe.
        if is_err_recv && is_enumerable_probe && (public == "message" || public == "stack") {
            continue;
        }
        let lit = ctx.intern_string_literal(public);
        let cmp_target = if key_ty == Type::Substr {
            ctx.intrinsics.substr_eq_str
        } else {
            ctx.intrinsics.str_eq
        };
        let eq = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(cmp_target, vec![key_op, Operand::Value(lit)]),
            Type::Bool,
            None,
        );
        let mut eq_op = Operand::Value(eq);
        if is_err_recv && (public == "message" || public == "name") {
            let present = emit_present(ctx, public);
            let and = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BinOp(SsaBinOp::And, eq_op, present),
                Type::Bool,
                None,
            );
            eq_op = Operand::Value(and);
        }
        if matches!(acc, Operand::ConstBool(false)) {
            acc = eq_op;
        } else {
            let or = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BinOp(SsaBinOp::Or, acc, eq_op),
                Type::Bool,
                None,
            );
            acc = Operand::Value(or);
        }
    }
    // The str_eq chain only reads the key — an Ident key keeps its
    // stake and its scope drop; owned temp keys release here (the old
    // consume+drop pair destroyed the source's stake, reuse-window UAF).
    ctx.release_owned_temp(arg_eid, &key_op);
    // S304 — lower-and-drop trailing args (runtime-key path;
    // same useful=1).
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    acc
}
