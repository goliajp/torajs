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
//! 4. **Array<T>** — the fast path. Hoist length read; build
//!    header/body/step/after blocks; per-iter element load via
//!    Expr::Index (lowers boxing for Type::Any correctly); bind
//!    `var_name` as moved+borrowed (alias-init from src[i],
//!    refcount belongs to source slot, per-iter drop skips to
//!    avoid double-dec of array slot child rc).
//!
//! P10.3-A1 — `for await (decl of iter)` desugar wraps elem_expr in
//! a `.value` Member access (await desugar); we strip the wrapper
//! to find the underlying Index for src resolution, but per-iter
//! lowering still uses the wrapped elem_expr so await semantics
//! (`promise_get_value`) fire naturally.

use crate::ast::{Expr, Stmt};
use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LocalInfo, LowerCtx};

pub(crate) fn lower(
    ctx: &mut LowerCtx,
    var_name: &str,
    i_ident: &str,
    elem_expr: crate::ast::ExprId,
    body: &Stmt,
) {
    let index_eid = resolve_index_eid(ctx, elem_expr);
    let src_ref_eid = if let Expr::Index { obj, .. } = ctx.ast.get_expr(index_eid) {
        *obj
    } else {
        unreachable!("index_eid resolution above guarantees Expr::Index");
    };
    let src_ptr_op = ctx.lower_expr(src_ref_eid);
    let src_ty = ctx.operand_ty(&src_ptr_op);
    if matches!(
        src_ty,
        Type::Map | Type::Set | Type::MapIter | Type::ArrIter
    ) {
        crate::ssa_lower_for_of_map_like::lower_for_of_map_like(
            ctx, src_ptr_op, src_ty, var_name, body,
        );
        return;
    }
    if let Type::Obj(sid) = src_ty {
        let mut cname: Option<String> = None;
        for (n, ty) in ctx.aliases.iter() {
            if matches!(ty, Type::Obj(s) if s.0 == sid.0) && ctx.ast.class_parents.contains_key(n) {
                cname = Some(n.clone());
                break;
            }
        }
        if let Some(cname) = cname {
            let iter_fn = format!("__cm_{cname}____sym_Symbol_iterator__");
            if let Some(&iter_fid) = ctx.fn_table.get(&iter_fn) {
                crate::ssa_lower_for_of_iter_protocol::lower_for_of_iter_protocol(
                    ctx, src_ptr_op, iter_fid, var_name, body, &cname,
                );
                return;
            }
            panic!(
                "ssa-lower: for-of on class `{cname}` requires a `[Symbol.iterator](): SomeIter` method (P5.2 syntax, P5.3 Phase B dispatch) — fn `{iter_fn}` not registered"
            );
        }
        panic!(
            "ssa-lower: for-of source type Type::Obj(sid={}) is not a registered class (subset — iterator protocol only fires for user-class iterables; inline-struct iteration not yet supported)",
            sid.0
        );
    }
    if src_ty == Type::Str {
        crate::ssa_lower_for_of_str::lower(ctx, src_ptr_op, i_ident, var_name, body);
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
    bind_scoped_local(ctx, i_ident, i_slot, Type::I64, false, false);

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
            else_blk: after,
        },
    );

    ctx.cur_block = body_blk;
    ctx.scope_stack.push(Vec::new());
    ctx.shadow_stack.push(Vec::new());
    let v_val = ctx.lower_expr(elem_expr);
    let v_ty = ctx.operand_ty(&v_val);
    let v_slot = ctx.alloca(v_ty, Some(var_name));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(v_val, Operand::Value(v_slot), 0),
    );
    bind_scoped_local(ctx, var_name, v_slot, v_ty, true, true);
    ctx.loop_stack.push((step_blk, after));
    ctx.lower_stmt(body);
    let body_open_at_end = ctx.cur_open();
    ctx.loop_stack.pop();

    close_body_scope(ctx, step_blk, body_open_at_end);

    ctx.cur_block = step_blk;
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

/// Peel the for-await `.value` Member wrapper (P10.3-A1) to the
/// underlying `Expr::Index` used for src resolution.
fn resolve_index_eid(ctx: &LowerCtx, elem_expr: crate::ast::ExprId) -> crate::ast::ExprId {
    match ctx.ast.get_expr(elem_expr) {
        Expr::Index { .. } => elem_expr,
        Expr::Member { obj, name } if name == "value" => {
            if matches!(ctx.ast.get_expr(*obj), Expr::Index { .. }) {
                *obj
            } else {
                panic!(
                    "for-of: for-await wrapper expects Member.value over Index, got {:?}",
                    ctx.ast.get_expr(*obj)
                );
            }
        }
        other => panic!(
            "for-of: elem_expr must be Expr::Index or for-await Member.value-over-Index wrapper, got {:?}",
            other
        ),
    }
}

/// Register `name` in the innermost scope frame, shadow-saving any
/// outer binding of the same name — the for-of `i` and element
/// bindings share this shape; only ty / moved / borrowed differ.
fn bind_scoped_local(
    ctx: &mut LowerCtx,
    name: &str,
    slot: crate::ssa::ValueId,
    ty: Type,
    moved: bool,
    borrowed: bool,
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
fn close_body_scope(ctx: &mut LowerCtx, step_blk: crate::ssa::BlockId, body_open_at_end: bool) {
    let body_frame = ctx.scope_stack.pop().expect("for-of body scope");
    let body_shadows = ctx.shadow_stack.pop().expect("shadow frame");
    if body_open_at_end {
        for name in &body_frame {
            let info = match ctx.locals.get(name) {
                Some(i) => *i,
                None => continue,
            };
            if info.moved || info.ty.is_copy() || ctx.stack_alloced_locals.contains(name) {
                continue;
            }
            let val = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(info.ty, Operand::Value(info.slot), 0),
                info.ty,
                None,
            );
            ctx.emit_drop_value(Operand::Value(val), info.ty);
        }
        ctx.f.set_term(ctx.cur_block, Terminator::Br(step_blk));
    }
    for n in &body_frame {
        ctx.locals.remove(n);
    }
    for (n, prev) in body_shadows {
        ctx.locals.insert(n, prev);
    }
}
