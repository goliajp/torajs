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
        // RFC 20260707 chunk 625 — an array literal returned from an
        // `Arr<Any>`-ret fn takes the annotation-consuming widen
        // (mirror of lower_let_init_val / assign_member's chunk 614
        // arm): without it the literal lowered through the typed
        // fast path, the block never got FLAG_ARR_ANY, and every
        // kind-aware Arr<Any> reader saw UNSET (undefined) while its
        // raw scalar slots misread as NaN-boxes elsewhere.
        let v = if let Expr::Array(els) = ctx.ast.get_expr(eid)
            && let Type::Arr(arr_id) = ctx.f.ret
            && ctx.arr_layouts[arr_id.0 as usize] == Type::Any
        {
            let ids: Vec<crate::ast::ExprId> = els.clone();
            ctx.lower_array_any_literal(&ids)
        } else {
            ctx.lower_expr(eid)
        };
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
        // RFC 20260708-closure-argv-face (generalized chunk 674) —
        // `return a[i]` where `a` is a local Arr<Any> hands back a
        // box BORROWING the array's elem stake (get_any_boxed is a
        // borrow read). The historical blanket fix marked `a` moved
        // — skipping its scope drop and stranding one array per
        // call (probe ag8: named fn `return a[0]` leaked; the
        // materialized `__torajs_arguments` was the same shape).
        // Retain the payload instead: the box leaves self-owned,
        // the array keeps its scope drop, and only the index
        // sub-expression feeds the consume walk. Gated on the
        // receiver's static Arr<Any> type — other Any-producing
        // index lanes may answer owned boxes where a retain would
        // leak.
        if let Expr::Index { obj, index } = ctx.ast.get_expr(eid)
            && let Expr::Ident(obj_name) = ctx.ast.get_expr(*obj)
            && ctx.operand_ty(&v) == Type::Any
            && ctx.locals.get(obj_name).is_some_and(|info| {
                matches!(info.ty, Type::Arr(id)
                    if ctx.arr_layouts[id.0 as usize] == Type::Any)
            })
        {
            let retained = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.anyv_retain, vec![v.clone()]),
                Type::Any,
                None,
            );
            ctx.consume_all_idents_in_return(*index);
            return Operand::Value(retained);
        }
        // Chunk 752 — a Copy-typed result (scalar) cannot alias any
        // local's heap, so no binding needs the moved mark; the
        // blanket walk stranded every non-Copy local the expression
        // touched (`return v.length` skipped v's scope drop and
        // leaked one concat cell per call — probe vJ 15.97MB vs
        // 6.37MB flat; the L3b #6 any-alias framing was refuted by
        // the typed-string variant leaking identically).
        if !ctx.operand_ty(&v).is_copy() {
            ctx.consume_all_idents_in_return(eid);
        }
        v
    });
    // §7.4.9 on the way out of every enclosing for-of. `break` gets
    // this from the loop's exit block; a return never reaches that
    // block, so the iterator stayed open AND its slot stake stayed
    // held. Emitted after the return value is in hand — the value can
    // read the loop variable — and before the finally hand-off below.
    //
    // With a `finally` INSIDE the loop body the spec order is
    // finally-then-close and this emits close-then-finally; observable
    // only if the finally block touches the iterator. Recorded as
    // backlog rather than silently narrowed: not closing at all was
    // the strictly worse answer this replaces.
    crate::ssa_lower_for_of_teardown::emit_all_for_return(ctx);
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
            // Chunk 806 — the expr-aware variant tags an Undefined-
            // typed source ANY_UNDEF; the plain box encoded its
            // ConstPtrNull payload as ANY_NULL, so `return undefined`
            // (and void-call returns) printed `null` at the caller.
            if let Some(eid) = maybe {
                return ctx.box_to_any_from_expr(eid, op);
            }
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
            // Pending-throw propagation — a user toString can throw
            // (0-check audit, rotation 130 L3b).
            ctx.emit_throw_check(None);
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
