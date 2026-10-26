//! `Stmt::ForOf` arm of `LowerCtx::lower_stmt` extracted from
//! [`crate::ssa_lower`] (chunk 147).
//!
//! Pre-extract this arm was 319 LOC inline inside `lower_stmt`.
//! Body verbatim moved here as a free fn taking `&mut LowerCtx`;
//! lower_stmt's match arm delegates with one line.
//!
//! Lowers `for (let v of <src>) body` over four source shapes:
//!
//! 1. **Map / Set / MapIter / ArrIter** (P6.4b/c) → routes through
//!    `lower_for_of_map_like` (sibling, chunk 137).
//! 2. **User class with `[Symbol.iterator]()`** (P5.3 Phase B) →
//!    `lower_for_of_iter_protocol` (sibling, chunk 143). Lookup
//!    the class name in `aliases`, resolve
//!    `__cm_<C>____sym_Symbol_iterator__` in fn_table; panic if
//!    the iter method is missing.
//! 3. **String** (P11.4) → `lower_for_of_str` (still inline). Per-iter
//!    code-point Substr views, advance 1 or 2 code units via
//!    `__torajs_str_code_point_at` per ES §22.1.5.
//!    3b. **Any** (RFC 20260704 S5+) → `lower_for_of_any_iter`
//!    (sibling): unified runtime iteration protocol
//!    (`__torajs_any_iter_next` tag-dispatch over strings / arrays /
//!    MapIter / ArrIter cells).
//! 4. **Array<T>** — the fast path. Hoist length read; build
//!    header/body/step/after blocks; per-iter element load via
//!    Expr::Index (lowers boxing for Type::Any correctly); bind
//!    `var_name` as moved+borrowed (alias-init from src[i],
//!    refcount belongs to source slot, per-iter drop skips to
//!    avoid double-dec of array slot child rc).
//!
//! Hole Z — `for await (decl of iter)` no longer wraps elem_expr in
//! a `.value` Member (that conflated the await unwrap with a real
//! user member and killed Struct elements on member lookup). The
//! stmt's `is_await` flag drives the await: a Promise-typed element
//! (checker verdict in `expr_types`) loads through
//! `promise_get_value`; every other element awaits to itself per
//! §27.2, so the plain Index load stands.

use crate::ast::{Expr, Stmt};
use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LocalInfo, LowerCtx};
use forin_guard::emit_forin_guard;
pub(crate) use forin_guard::forin_key_live;

mod element;
mod forin_guard;
mod lane;
mod source_class;
use element::element_value;

pub(crate) fn lower(
    ctx: &mut LowerCtx,
    var_name: &str,
    i_ident: &str,
    elem_expr: crate::ast::ExprId,
    body: &Stmt,
    forin_obj: Option<crate::ast::ExprId>,
    is_await: bool,
) {
    let src_ref_eid = if let Expr::Index { obj, .. } = ctx.ast.get_expr(elem_expr) {
        *obj
    } else {
        panic!(
            "for-of: elem_expr must be Expr::Index (parser pre-builds `src[i]`), got {:?}",
            ctx.ast.get_expr(elem_expr)
        );
    };
    // a head some store handed a value of another face
    // (`check_stmt_let_decl::fn_face_slot`) calls through the boxed entry
    if ctx.ast.face_mismatch_lets.contains(&elem_expr) {
        ctx.variadic_locals.insert(var_name.to_string());
    }
    let src_ptr_op = ctx.lower_expr(src_ref_eid);
    let src_ty = ctx.operand_ty(&src_ptr_op);
    // §14.7.5.6 GetIterator throws on a nullish source
    crate::ssa_lower_nullable_guard::emit_slot_guard(ctx, src_ref_eid, &src_ptr_op);
    if lane::lower_non_array_source(
        ctx,
        src_ref_eid,
        src_ptr_op,
        src_ty,
        (var_name, i_ident),
        body,
        is_await,
    ) {
        return;
    }
    // §14.7.5.6 iterates an Array through `Array.prototype
    // [Symbol.iterator]`, which a module that can name that prototype
    // may replace or delete — or install on one array directly. Such a
    // module stands the index loop down to the any lane's runtime
    // GetIterator — the same compile-time stand-down the typed method
    // tier takes; a program that spells neither keeps the index loop.
    if matches!(src_ty, Type::Arr(_)) && ctx.scans.proto_shadow.shadows_iteration("Array") {
        ctx.emit_arr_mark_kind(&src_ptr_op);
        let boxed = ctx.box_to_any(src_ptr_op);
        crate::ssa_lower_for_of_any_iter::lower(ctx, boxed, var_name, body, is_await);
        return;
    }
    if !matches!(src_ty, Type::Arr(_)) {
        panic!(
            "ssa-lower: for-of source type {src_ty:?} not yet supported (P5.3 subset — Array<T> + user-class iterable only)"
        );
    }

    ctx.scope_stack.push(Vec::new());
    ctx.shadow_stack.push(Vec::new());

    let i_slot = ctx.alloca(Type::I64, Some(i_ident));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(0), Operand::Value(i_slot), 0),
    );
    bind_scoped_local(ctx, i_ident, i_slot, Type::I64, false, false, false);

    let src_ptr = match src_ptr_op {
        Operand::Value(v) => v,
        _ => panic!("for-of: src ident must lower to a value operand"),
    };
    let end_val = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(src_ptr), ARR_LEN_OFF),
        Type::I64,
        None,
    );

    let header = ctx.f.add_block();
    let body_blk = ctx.f.add_block();
    let step_blk = ctx.f.add_block();
    let after = ctx.f.add_block();
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header));
    // a body that suspends awaits each step
    let steps_await = is_await && ctx.suspend_body;
    let (exhausted, brk) = if steps_await {
        crate::ssa_lower_fn::suspend::sync_iter_exits(ctx, after)
    } else {
        (after, after)
    };

    ctx.cur_block = header;
    let i_now = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    let cond_val = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Slt, Operand::Value(i_now), Operand::Value(end_val)),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(cond_val),
            then_blk: body_blk,
            else_blk: exhausted,
        },
    );

    ctx.cur_block = body_blk;
    ctx.scope_stack.push(Vec::new());
    ctx.shadow_stack.push(Vec::new());
    let v_val = element_value(ctx, elem_expr, is_await, steps_await);
    let v_ty = ctx.operand_ty(&v_val);
    if let Some(obj_eid) = forin_obj {
        emit_forin_guard(ctx, obj_eid, &v_val, step_blk);
    }
    crate::ssa_lower_for_of_written::note_sentinel_loop_var(ctx, var_name, v_ty, src_ref_eid);
    // an awaited step answers its value owned
    let init = if steps_await {
        crate::ssa_lower_for_of_written::LoopVarInit::Owned
    } else {
        crate::ssa_lower_for_of_written::LoopVarInit::Alias
    };
    crate::ssa_lower_for_of_written::bind_loop_var(ctx, var_name, v_val, v_ty, body, init);
    // a rejected element's close runs nothing (an array iterator has no
    // `return`); the body's exits close
    if steps_await {
        ctx.for_of_teardown_stack
            .push(crate::ssa_lower_for_of_teardown::ForOfTeardown::AsyncFromSync);
    }
    // RFC 20260901-scope-exit-drops — the body frame (already pushed,
    // holding the loop variable) is only closed by `close_body_scope`
    // on fall-through, so a jump out owes it too: depth = its index.
    ctx.loop_stack
        .push(crate::ssa_lower_scope_exit::LoopTargets {
            cont: Some(step_blk),
            brk,
            scope_depth: ctx.scope_stack.len() - 1,
            teardown_depth: ctx.for_of_teardown_stack.len(),
        });
    ctx.lower_stmt(body);
    let body_open_at_end = ctx.cur_open();
    ctx.loop_stack.pop();
    if steps_await {
        ctx.for_of_teardown_stack.pop();
    }

    close_body_scope(ctx, step_blk, body_open_at_end);

    ctx.cur_block = step_blk;
    emit_index_step(ctx, i_slot, header);

    ctx.cur_block = after;
    let i_frame = ctx.scope_stack.pop().expect("for-of i scope");
    let i_shadows = ctx.shadow_stack.pop().expect("shadow frame");
    for n in &i_frame {
        ctx.locals.remove(n);
    }
    for (n, prev) in i_shadows {
        ctx.locals.insert(n, prev);
    }
}

/// The loop's step block: `i += 1`, then back to the `header` test.
fn emit_index_step(ctx: &mut LowerCtx, i_slot: crate::ssa::ValueId, header: crate::ssa::BlockId) {
    let i_cur = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    let i_next = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(SsaBinOp::Add, Operand::Value(i_cur), Operand::ConstI64(1)),
        Type::I64,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(i_next), Operand::Value(i_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header));
}

/// Register `name` in the innermost scope frame, shadow-saving any
/// outer binding of the same name — the for-of `i` and element
/// bindings share this shape; only ty / moved / borrowed differ.
pub(crate) fn bind_scoped_local(
    ctx: &mut LowerCtx,
    name: &str,
    slot: crate::ssa::ValueId,
    ty: Type,
    moved: bool,
    borrowed: bool,
    boxed: bool,
) {
    let cur_depth = ctx.scope_stack.len() - 1;
    if let Some(prev) = ctx.locals.get(name).copied()
        && prev.scope_depth < cur_depth
    {
        ctx.shadow_stack
            .last_mut()
            .expect("shadow frame")
            .push((name.to_string(), prev));
    }
    ctx.locals.insert(
        name.to_string(),
        LocalInfo {
            slot,
            ty,
            moved,
            borrowed,
            boxed,
            scope_depth: cur_depth,
        },
    );
    ctx.scope_stack
        .last_mut()
        .expect("scope frame")
        .push(name.to_string());
}

/// Pop the body scope frame — drop-walk still-owned locals and
/// branch to the step block when the body fell through open, then
/// restore shadowed bindings.
pub(crate) fn close_body_scope(
    ctx: &mut LowerCtx,
    step_blk: crate::ssa::BlockId,
    body_open_at_end: bool,
) {
    let body_frame = ctx.scope_stack.pop().expect("for-of body scope");
    let body_shadows = ctx.shadow_stack.pop().expect("shadow frame");
    if body_open_at_end {
        // RFC 20260901-scope-exit-drops — the block-close protocol,
        // shared with every early exit out of this frame.
        ctx.emit_frame_drops(&body_frame);
        ctx.f.set_term(ctx.cur_block, Terminator::Br(step_blk));
    }
    for n in &body_frame {
        ctx.locals.remove(n);
    }
    for (n, prev) in body_shadows {
        ctx.locals.insert(n, prev);
    }
}
