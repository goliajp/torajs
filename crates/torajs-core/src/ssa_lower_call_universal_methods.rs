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
        return Some(recv_op);
    }
    if !(is_prim || is_obj) {
        return None;
    }
    // valueOf returns the receiver as-is (identity).
    if m_name == "valueOf" {
        // S290 — primitive valueOf trailing-arg ignore; lower-and-drop
        // before identity return (S272 idiom).
        for &a in args.iter() {
            let _ = ctx.lower_expr(a);
        }
        return Some(recv_op);
    }
    if m_name == "toString" {
        if is_obj {
            // S304 — lower-and-drop trailing args per S272 idiom so
            // step()-style side-effect exprs fire (toString is 0-useful
            // on struct instance; "[object Object]" const independent
            // of args).
            for &a in args.iter() {
                let _ = ctx.lower_expr(a);
            }
            let v = ctx.intern_string_literal("[object Object]");
            return Some(Operand::Value(v));
        }
        // For primitives, fall through to allow existing arms
        // (toFixed/toString) to take over.
        return None;
    }
    // hasOwnProperty / propertyIsEnumerable / isPrototypeOf.
    if let Type::Obj(_) = recv_ty
        && matches!(m_name.as_str(), "hasOwnProperty" | "propertyIsEnumerable")
        && let Some(arg_eid) = args.first()
    {
        return Some(emit_obj_has_own_property(ctx, recv_ty, *arg_eid, args));
    }
    // Fallback (primitives, isPrototypeOf, no args): drop arg + return
    // false.
    if !args.is_empty() {
        let arg_val = ctx.lower_expr(args[0]);
        let arg_ty = ctx.operand_ty(&arg_val);
        ctx.consume_if_ident(args[0]);
        ctx.emit_drop_value(arg_val, arg_ty);
    }
    // S304 — lower-and-drop trailing args (isPrototypeOf useful=1;
    // primitive fallback also covers stray trailing).
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    Some(Operand::ConstBool(false))
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
        // Literal key — compile-time fold.
        let layout = &ctx.struct_layouts[sid.0 as usize];
        let result = layout.iter().any(|(fname, _)| fname == key);
        let arg_val = ctx.lower_expr(arg_eid);
        let arg_ty = ctx.operand_ty(&arg_val);
        ctx.consume_if_ident(arg_eid);
        ctx.emit_drop_value(arg_val, arg_ty);
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
    ctx.consume_if_ident(arg_eid);
    ctx.emit_drop_value(key_op, key_ty);
    // S304 — lower-and-drop trailing args (runtime-key path;
    // same useful=1).
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    acc
}
