//! `Array.isArray(value)` — compile-time static check (ES §23.1.2.2)
//! pulled out of [`crate::ssa_lower::lower_expr_inner`]'s `Expr::Call`
//! god-arm as chunk-44 of the decomp (chunks 1-43 = ... + T-24 vtable
//! dispatch).
//!
//! Three layers, in order:
//!
//! 1. **S204** — empty arg list → `false` (spec step 1: missing arg
//!    defaults to undefined, undefined is not an Array). Short-circuit
//!    before touching `args[0]` to dodge the index-out-of-bounds panic
//!    the inline path would otherwise hit.
//! 2. **S267** — eval-and-drop trailing args after the first
//!    (silent-ignore per spec).
//! 3. Argument dispatch:
//!    - **T-38** — namespace idents (`Math` / `JSON` / `Array` / ...)
//!      referenced as runtime values short-circuit to `false`. They
//!      have no Operand representation (typecheck-only
//!      `Type::Object(<name>)` markers); without this, `lower_expr`
//!      hits the catch-all `unknown ident` panic.
//!    - Static type fast paths:
//!      - `Type::Arr(_)` → `true`.
//!      - `Type::Any` → `__torajs_any_is_arr` runtime tag dispatch.
//!      - any other concrete typed value → `false`.
//!
//! Returns `Some(op)` on hit; `None` on miss (callee not the
//! `Array.isArray` Member-Ident shape).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ctx.ast.get_expr(callee)
    else {
        return None;
    };
    if m_name != "isArray" {
        return None;
    }
    let ns_id = *ns_id;
    let Expr::Ident(ns) = ctx.ast.get_expr(ns_id) else {
        return None;
    };
    if ns != "Array" {
        return None;
    }
    if args.is_empty() {
        return Some(Operand::ConstBool(false));
    }
    for a in args.iter().skip(1) {
        let _ = ctx.lower_expr(*a);
    }
    if let Expr::Ident(arg_name) = ctx.ast.get_expr(args[0])
        && matches!(
            arg_name.as_str(),
            "Math"
                | "JSON"
                | "Array"
                | "Object"
                | "Number"
                | "String"
                | "Boolean"
                | "Date"
                | "RegExp"
                | "Symbol"
                | "console"
                | "globalThis"
        )
    {
        return Some(Operand::ConstBool(false));
    }
    let arg_op = ctx.lower_expr(args[0]);
    let arg_ty = ctx.operand_ty(&arg_op);
    // RECORDED GAP (517-06) — §7.2.2 step 2 wants an Array EXOTIC
    // object, and an arguments object is not one. tr desugars
    // `arguments` into a plain `__torajs_arguments` array local, so
    // both the static type here and the runtime tag say "array"; the
    // header bit that would have separated them is shared with
    // FLAG_SPLIT_BLOCK. Both lanes answer true until that identity
    // has a signal of its own.
    if matches!(arg_ty, Type::Arr(_)) {
        return Some(Operand::ConstBool(true));
    }
    if matches!(arg_ty, Type::Any) {
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.any_is_arr, vec![arg_op]),
            Type::Bool,
            None,
        );
        // §7.2.2 step 3.a — a revoked proxy throws instead of
        // answering, so this call has a throw channel now.
        ctx.emit_throw_check(None);
        return Some(Operand::Value(v));
    }
    Some(Operand::ConstBool(false))
}
