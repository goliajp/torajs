//! Typed-receiver Member-access cluster for primitive / Symbol /
//! Arr / Map / Set / Closure / FnSig pulled out of
//! [`crate::ssa_lower::lower_expr_inner`]'s `Expr::Member` god-arm
//! as chunk-64 of the decomp (chunks 1-63 = ... + RegExp accessor
//! cluster).
//!
//! Five sub-arms tried in source order:
//!
//! - **V3-18 m2.c** — `<prim>.constructor` returns `ConstPtrNull`
//!   for `Type::{I64|F64|I32|Bool|Str|Substr|BigInt|Symbol}`. tora
//!   doesn't have first-class function refs for namespaces; `typeof`
//!   on the result still routes through the typeof-Member path
//!   which returns `"function"`. `obj_val` is intentionally dropped
//!   (primitive borrows have no side effect to flush).
//! - **V3-18 m1.h.47** — `Symbol.prototype.description`. Returns the
//!   desc str the Symbol was created with (or null for `Symbol()`
//!   no-arg). Runtime helper bumps the desc's refcount so the caller
//!   can drop independently of the Symbol's lifetime.
//! - **Phase 2A** — `xs.length` on `Type::Arr(_)` reads u64 len at
//!   `ARR_LEN_OFF` of the array header.
//! - **P6.1 / P6.2** — `m.size` / `s.size` accessor property per ES
//!   §23.1.3.10 / §24.2.3.9. Both route through
//!   `__torajs_map_size` since Set storage shares the Map runtime.
//! - **T-27.c** — `f.length` / `f.name` for `Type::Closure(_)` /
//!   `Type::FnSig(_)`. Compile-time fold from the fn's static
//!   signature (param count for length; lifted-`__env` hidden Ptr
//!   excluded for Closure; FnSig signatures don't carry env so the
//!   raw param count is reported). `.name` for FnSig recovers the
//!   ident text from the call-site `obj` (top-level FnDecl reuse);
//!   `__closure_N` synthetic names yield `""` per ES spec for
//!   anonymous fn expressions.
//!
//! Returns `Some(op)` on hit; `None` on miss (receiver type +
//! Member name combo not in the allowlist). Caller falls through to
//! the generic Member path.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    obj: ExprId,
    obj_val: Operand,
    obj_ty: Type,
    name: &str,
) -> Option<Operand> {
    if name == "constructor" && is_prim_for_constructor(obj_ty) {
        let _ = obj_val;
        return Some(Operand::ConstPtrNull);
    }
    if obj_ty == Type::Symbol && name == "description" {
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.symbol_description, vec![obj_val]),
            Type::Str,
            None,
        );
        return Some(Operand::Value(v));
    }
    if matches!(obj_ty, Type::Arr(_)) && name == "length" {
        // RC-4 F1a — a nullable-arr receiver (exec/match result)
        // may be null on miss; guard before the inline len load.
        crate::ssa_lower_nullable_guard::emit_nullable_arr_guard(ctx, obj, &obj_val);
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Load(Type::I64, obj_val, ARR_LEN_OFF),
            Type::I64,
            None,
        );
        return Some(Operand::Value(v));
    }
    if matches!(obj_ty, Type::Map | Type::Set) && name == "size" {
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.map_size, vec![obj_val]),
            Type::I64,
            None,
        );
        return Some(Operand::Value(v));
    }
    if (matches!(obj_ty, Type::Closure(_)) || matches!(obj_ty, Type::FnSig(_)))
        && (name == "length" || name == "name")
    {
        return Some(lower_fn_length_or_name(ctx, obj, obj_ty, name));
    }
    None
}

fn is_prim_for_constructor(obj_ty: Type) -> bool {
    matches!(
        obj_ty,
        Type::I64
            | Type::F64
            | Type::I32
            | Type::Bool
            | Type::Str
            | Type::Substr
            | Type::BigInt
            | Type::Symbol
    )
}

fn lower_fn_length_or_name(
    ctx: &mut LowerCtx<'_>,
    obj: ExprId,
    obj_ty: Type,
    name: &str,
) -> Operand {
    let sig_id = match obj_ty {
        Type::Closure(s) | Type::FnSig(s) => s,
        _ => unreachable!(),
    };
    if name == "length" {
        let (params, _ret) = &ctx.fn_sigs[sig_id.0 as usize];
        let visible =
            if matches!(obj_ty, Type::Closure(_)) && !params.is_empty() && params[0] == Type::Ptr {
                params.len() - 1
            } else {
                params.len()
            };
        return Operand::ConstI64(visible as i64);
    }
    let fn_name = if let Expr::Ident(n) = ctx.ast.get_expr(obj) {
        n.clone()
    } else {
        String::new()
    };
    let s = ctx.intern_string_literal(&fn_name);
    Operand::Value(s)
}
