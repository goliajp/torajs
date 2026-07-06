//! `Stmt::Return` arm of `LowerCtx::lower_stmt` extracted from
//! [`crate::ssa_lower`] (chunk 149).
//!
//! Pre-extract this arm was 168 LOC inline inside `lower_stmt`.
//! Body verbatim moved here as a free fn; lower_stmt's match arm
//! delegates with one line.
//!
//! Pipeline:
//!
//! 1. **Lower the optional return value** with borrowed-ident
//!    retain (Swift ARC +0-parameter / +1-result) — `return s` for
//!    a non-Copy borrowed local takes a +1 stake so the caller's
//!    scope-end drop doesn't release the owner's reference.
//! 2. **Mark every non-Copy local touched by the return expression
//!    as moved** via `consume_all_idents_in_return` so the end-of-fn
//!    drop walk doesn't dangle the returned pointer.
//! 3. **Try-with-finally routing** (review #0001) — if a finally is
//!    active, stash the value in a lazy-alloc'd `__pending_ret`
//!    slot + set `__pending_flag`, then `br` to the innermost
//!    finally. The finally tail dispatches: still-wrapped → br next
//!    outer; outermost → load + ret.
//! 4. **Boundary coercions** (no finally on the stack):
//!    - Any-typed return slot with concrete value → `box_to_any`
//!      (P0.9 — ABI).
//!    - Any-typed value returned where declared return is numeric
//!      → `coerce_any_to_number` (P7.2b — fixes B-throw-2's
//!      pointer-as-primitive corruption).
//!    - i64 ↔ f64 promotion / narrowing per declared return.
//!    - Substr → Str materialization (caller may rely on declared
//!      return type for slot layout — e.g. flatMap's dst_elem_ty).
//!    - Array<Substr> → Array<Str> materialization (symmetric).
//! 5. **Void-return guard** — arrow expression-body desugar wraps
//!    a trailing void Call in `Stmt::Return(Some(eid))`; the dummy
//!    operand must not feed `Terminator::Ret` if the fn is void
//!    (LLVM verify rejects `ret i64 0` from a void fn).
//! 6. Emit owned-local drops + set the block terminator to
//!    `Ret(coerced)`.

use crate::ast::Expr;
use crate::ssa::{InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower(ctx: &mut LowerCtx, maybe: Option<crate::ast::ExprId>) {
    let ret_operand = maybe.map(|eid| {
        let v = ctx.lower_expr(eid);
        let needs_retain = if let Expr::Ident(name) = ctx.ast.get_expr(eid) {
            ctx.locals
                .get(name)
                .is_some_and(|info| info.borrowed && info.ty.is_refcounted())
        } else {
            false
        };
        if needs_retain {
            ctx.emit_rc_inc(v.clone());
        }
        ctx.consume_all_idents_in_return(eid);
        v
    });
    if !ctx.try_finally_stack.is_empty() {
        let target = *ctx.try_finally_stack.last().unwrap();
        let ret_ty = ctx.f.ret;
        let slot = match ctx.pending_return_slot {
            Some(s) => s,
            None => {
                let s = ctx.alloca(ret_ty, Some("__pending_ret"));
                ctx.pending_return_slot = Some(s);
                s
            }
        };
        let flag = match ctx.pending_return_flag {
            Some(f) => f,
            None => {
                let f = ctx.alloca(Type::Bool, Some("__pending_flag"));
                ctx.pending_return_flag = Some(f);
                f
            }
        };
        if let Some(v) = ret_operand {
            ctx.f
                .append_void(ctx.cur_block, InstKind::Store(v, Operand::Value(slot), 0));
        }
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::ConstBool(true), Operand::Value(flag), 0),
        );
        let cb = ctx.cur_block;
        ctx.f.set_term(cb, Terminator::Br(target));
        return;
    }
    let coerced = ret_operand.map(|op| {
        let actual = ctx.operand_ty(&op);
        if ctx.f.ret == Type::Any && actual != Type::Any {
            return ctx.box_to_any(op);
        }
        if actual == Type::Any && matches!(ctx.f.ret, Type::I64 | Type::F64) {
            return ctx.coerce_any_to_number(op, ctx.f.ret);
        }
        if actual == Type::Any && ctx.f.ret == Type::Str {
            // RC-4 — Any-typed value returned where the declared
            // return is Str: `anyv_to_str` materializes (fresh
            // owned; a short-str immediate box would otherwise be
            // deref'd as a Str pointer by the caller — heap results
            // only survived because a cell box IS the raw pointer).
            // The Any box's own stake settles via the box drop.
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.any_to_str_box, vec![op]),
                Type::Str,
                None,
            );
            ctx.emit_drop_value(op, Type::Any);
            return Operand::Value(v);
        }
        if ctx.f.ret == Type::F64 && actual == Type::I64 {
            ctx.coerce_to_f64(op)
        } else if ctx.f.ret == Type::I64 && actual == Type::F64 {
            ctx.coerce_to_i64(op)
        } else if ctx.f.ret == Type::Str && actual == Type::Substr {
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.substr_to_owned, vec![op]),
                Type::Str,
                None,
            );
            ctx.emit_drop_value(op, Type::Substr);
            Operand::Value(v)
        } else if let (Type::Arr(want_id), Type::Arr(got_id)) = (ctx.f.ret, actual)
            && ctx.arr_layouts[want_id.0 as usize] == Type::Str
            && ctx.arr_layouts[got_id.0 as usize] == Type::Substr
        {
            ctx.materialize_arr_substr_to_str(op, ctx.f.ret)
        } else {
            op
        }
    });
    let coerced = if ctx.f.ret == Type::Void {
        None
    } else {
        coerced
    };
    ctx.emit_drops_for_owned_locals();
    let cb = ctx.cur_block;
    ctx.f.set_term(cb, Terminator::Ret(coerced));
}
