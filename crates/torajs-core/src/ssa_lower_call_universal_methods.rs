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
    // ES §23.1.3.34 — `arr.valueOf()` returns the Array itself
    // (identity). Narrow: handled here BEFORE the prim/obj fold so
    // other Array.toString / Array.hasOwnProperty dispatch arms
    // further down remain reachable for non-valueOf calls.
    if matches!(recv_ty, Type::Arr(_)) && m_name == "valueOf" {
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
    // valueOf returns the receiver as-is (identity).
    if m_name == "valueOf" {
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
        return Some(recv_op);
    }
    if m_name == "toString" {
        if is_obj {
            // A class that declares OR inherits an own `toString` must
            // dispatch to its `__cm_<C>__toString`, NOT the `[object
            // Object]` default — this is the injected
            // `Error.prototype.toString` (§20.5.3.4, declared on the
            // root `Error` and inherited by every NativeError subclass)
            // and any user class overriding toString. `toString` is a
            // universal-method name, so unlike an ordinary inherited
            // call (`sub.greet()`) desugar never rewrites the call site
            // — we resolve the nearest ancestor's `__cm_<A>__toString`
            // here and emit the static call directly.
            if let Some(cname) = crate::ssa_lower_member_obj_field::class_name_of_expr(ctx, recv_id)
                && let Some(op) = try_dispatch_class_tostring(ctx, &cname, recv_op, args)
            {
                return Some(op);
            }
            // S304 — lower-and-drop trailing args per S272 idiom so
            // step()-style side-effect exprs fire (toString is 0-useful
            // on a struct instance with no own toString; "[object
            // Object]" const independent of args, ES §19.1.3.6).
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
        return None;
    }
    // hasOwnProperty / propertyIsEnumerable / isPrototypeOf.
    if let Type::Obj(_) = recv_ty
        && matches!(m_name.as_str(), "hasOwnProperty" | "propertyIsEnumerable")
        && let Some(arg_eid) = args.first()
    {
        return Some(emit_obj_has_own_property(ctx, recv_ty, *arg_eid, args));
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

/// Dispatch `recv.toString()` to the nearest `__cm_<C>__toString` in the
/// receiver class's ancestor chain (self first), emitting the static
/// call with the already-lowered `recv_op` as the receiver arg. Trailing
/// args are lowered-and-dropped (§20.5.3.4 toString ignores arguments).
/// Returns `None` when no class in the chain declares an own `toString`
/// (the caller then folds to the `[object Object]` default) — in that
/// case no args are lowered here, so the caller's fold lowers them once.
fn try_dispatch_class_tostring(
    ctx: &mut LowerCtx<'_>,
    cname: &str,
    recv_op: Operand,
    args: &[ExprId],
) -> Option<Operand> {
    let mut cur = Some(cname.to_string());
    while let Some(c) = cur {
        let fn_name = format!("__cm_{c}__toString");
        if let Some(&fid) = ctx.fn_table.get(&fn_name) {
            // S304 — lower-and-drop trailing args (toString is 0-arg).
            for &a in args.iter() {
                let _ = ctx.lower_expr(a);
            }
            let ret_ty = ctx.f_ret_type_hint(fid);
            let cur_block = ctx.cur_block;
            let v = ctx
                .f
                .append_inst(cur_block, InstKind::Call(fid, vec![recv_op]), ret_ty, None);
            if ctx.may_throw_fns.contains(&fn_name) {
                ctx.emit_throw_check(None);
            }
            return Some(Operand::Value(v));
        }
        cur = ctx.ast.class_parents.get(&c).and_then(|p| p.clone());
    }
    None
}

/// Emit `obj.hasOwnProperty(k)` / `obj.propertyIsEnumerable(k)` on a
/// `Type::Obj` receiver. Literal key → compile-time fold against
/// struct_layouts. Runtime key (V3-18 m2.g) → inline str_eq chain over
/// the struct's field names (zero-alloc — each name interned as
/// literal Str).
fn emit_obj_has_own_property(
    ctx: &mut LowerCtx<'_>,
    recv_ty: Type,
    arg_eid: ExprId,
    args: &[ExprId],
) -> Operand {
    let Type::Obj(sid) = recv_ty else {
        unreachable!("emit_obj_has_own_property called with non-Obj receiver");
    };
    if let Expr::String(key) = ctx.ast.get_expr(arg_eid) {
        // Literal key — compile-time fold. The lowered literal is a
        // static cell (rc no-op); nothing to release.
        let layout = &ctx.struct_layouts[sid.0 as usize];
        let result = layout.iter().any(|(fname, _)| fname == key);
        let arg_val = ctx.lower_expr(arg_eid);
        ctx.release_owned_temp(arg_eid, &arg_val);
        // S304 — lower-and-drop trailing args per S272 idiom
        // (hasOwnProperty / propertyIsEnumerable useful arity 1).
        for &a in args.iter().skip(1) {
            let _ = ctx.lower_expr(a);
        }
        return Operand::ConstBool(result);
    }
    // Runtime key — emit inline str_eq chain.
    let layout = ctx.struct_layouts[sid.0 as usize].clone();
    let key_op = ctx.lower_expr(arg_eid);
    let key_ty = ctx.operand_ty(&key_op);
    let mut acc: Operand = Operand::ConstBool(false);
    for (fname, _) in &layout {
        let lit = ctx.intern_string_literal(fname);
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
        let eq_op = Operand::Value(eq);
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
