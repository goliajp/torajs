//! `lower_for_of_str` + private helpers extracted from
//! [`crate::ssa_lower`] (chunk 157).
//!
//! Pre-extract `lower_for_of_str` (205 LOC) + `emit_for_of_str_advance`
//! (26 LOC) + `emit_for_of_str_step` (13 LOC) lived on `LowerCtx`.
//! All three move here as a sibling module; `LowerCtx::lower_for_of_str`
//! becomes a thin pub(crate) wrapper since the [`Stmt::ForOf`] arm
//! (sibling `ssa_lower_stmt_for_of`, chunk 147) calls it as a method.
//!
//! P11.4 — `for (let v of <string>)` iterates by Unicode code point
//! per ES §22.1.5. Layout:
//!
//! ```text
//!   alloc i_slot = 0; alloc length_slot = src.length
//!   br header
//! header:
//!   i < length?  → body | after
//! body:
//!   (c, adv) = (substr_create(src, i, adv), code_point_at > 0xFFFF ? 2 : 1)
//!   bind var_name = c (Substr, owned with rc=1)
//!   <body>; per-iter drop owned locals
//!   br step
//! step:
//!   recompute adv = code_point_at(src, i) > 0xFFFF ? 2 : 1
//!   i += adv
//!   br header
//! after:
//!   close i scope, fall through
//! ```
//!
//! Step block recomputes adv because the body's adv value isn't
//! available across a `continue` jump (no phi nodes in this IR).
//! Cost is one extra code_point_at + cmp per iter — acceptable for
//! a language-construct loop.

use crate::ast::Stmt;
use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{LocalInfo, LowerCtx};

pub(crate) fn lower(
    ctx: &mut LowerCtx,
    src_op: Operand,
    i_ident: &str,
    var_name: &str,
    body: &Stmt,
) {
    ctx.scope_stack.push(Vec::new());
    ctx.shadow_stack.push(Vec::new());
    let i_slot = ctx.alloca(Type::I64, Some(i_ident));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(0), Operand::Value(i_slot), 0),
    );
    {
        let cur_depth = ctx.scope_stack.len() - 1;
        if let Some(prev) = ctx.locals.get(i_ident).copied()
            && prev.scope_depth < cur_depth
        {
            ctx.shadow_stack
                .last_mut()
                .expect("shadow frame")
                .push((i_ident.to_string(), prev));
        }
        ctx.locals.insert(
            i_ident.to_string(),
            LocalInfo {
                slot: i_slot,
                ty: Type::I64,
                moved: false,
                borrowed: false,
                scope_depth: cur_depth,
            },
        );
        ctx.scope_stack
            .last_mut()
            .expect("scope frame")
            .push(i_ident.to_string());
    }

    let length_op = crate::ssa_lower_str::load_str_or_substr_length(ctx, src_op.clone(), Type::Str);
    let length_slot = ctx.alloca(Type::I64, Some("__forof_str_len"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(length_op, Operand::Value(length_slot), 0),
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
    let len_now = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(length_slot), 0),
        Type::I64,
        None,
    );
    let cond_val = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Slt, Operand::Value(i_now), Operand::Value(len_now)),
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
    let i_body = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    let (c_val, _adv_body) = emit_step(ctx, src_op.clone(), Operand::Value(i_body));
    let v_slot = ctx.alloca(Type::Substr, Some(var_name));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(c_val), Operand::Value(v_slot), 0),
    );
    {
        let cur_depth = ctx.scope_stack.len() - 1;
        if let Some(prev) = ctx.locals.get(var_name).copied()
            && prev.scope_depth < cur_depth
        {
            ctx.shadow_stack
                .last_mut()
                .expect("shadow frame")
                .push((var_name.to_string(), prev));
        }
        ctx.locals.insert(
            var_name.to_string(),
            LocalInfo {
                slot: v_slot,
                ty: Type::Substr,
                moved: false,
                borrowed: false,
                scope_depth: cur_depth,
            },
        );
        ctx.scope_stack
            .last_mut()
            .expect("scope frame")
            .push(var_name.to_string());
    }

    // RFC 20260901-scope-exit-drops — body frame already pushed and
    // only closed on fall-through: a jump out owes it (depth = index).
    ctx.loop_stack
        .push(crate::ssa_lower_scope_exit::LoopTargets {
            cont: step_blk,
            brk: after,
            scope_depth: ctx.scope_stack.len() - 1,
        });
    ctx.lower_stmt(body);
    let body_open_at_end = ctx.cur_open();
    ctx.loop_stack.pop();

    let body_frame = ctx.scope_stack.pop().expect("for-of str body scope");
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

    ctx.cur_block = step_blk;
    let i_step = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    let adv_step = emit_advance(ctx, src_op.clone(), Operand::Value(i_step));
    let i_next = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(
            SsaBinOp::Add,
            Operand::Value(i_step),
            Operand::Value(adv_step),
        ),
        Type::I64,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(i_next), Operand::Value(i_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header));

    ctx.cur_block = after;
    let i_frame = ctx.scope_stack.pop().expect("for-of str i scope");
    let i_shadows = ctx.shadow_stack.pop().expect("shadow frame");
    for n in &i_frame {
        ctx.locals.remove(n);
    }
    for (n, prev) in i_shadows {
        ctx.locals.insert(n, prev);
    }
}

/// P11.4 helper — compute `adv = (code_point_at(src, i) > 0xFFFF) ? 2 : 1`
/// in the current block. `i_val` must be the already-loaded i64
/// index value (NOT an i_slot pointer — that was an earlier bug
/// that surfaced as `load i64, <i64-value>` and tripped inkwell's
/// PointerValue verifier). Returns the i64 SSA value for `adv`.
fn emit_advance(ctx: &mut LowerCtx, src_op: Operand, i_val: Operand) -> ValueId {
    let cp = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.str_code_point_at, vec![src_op, i_val]),
        Type::I64,
        None,
    );
    let is_supp = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Sgt, Operand::Value(cp), Operand::ConstI64(0xFFFF)),
        Type::Bool,
        None,
    );
    let supp_i = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ZExtBoolToI64(Operand::Value(is_supp)),
        Type::I64,
        None,
    );
    ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(SsaBinOp::Add, Operand::ConstI64(1), Operand::Value(supp_i)),
        Type::I64,
        None,
    )
}

/// P11.4 helper — body-side: compute adv, alloc Substr. `i_val` is
/// the already-loaded current index. Returns (c_val, adv_val).
fn emit_step(ctx: &mut LowerCtx, src_op: Operand, i_val: Operand) -> (ValueId, ValueId) {
    let adv = emit_advance(ctx, src_op.clone(), i_val.clone());
    let c = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.substr_create,
            vec![src_op, i_val, Operand::Value(adv)],
        ),
        Type::Substr,
        None,
    );
    (c, adv)
}
