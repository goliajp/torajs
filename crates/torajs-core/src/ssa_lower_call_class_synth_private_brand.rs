//! RFC 20260916-private-element-brand slice 2a — the two synthetic
//! calls `ast/private_brand.rs` mints, arms of
//! [`crate::ssa_lower_call_class_synth`].
//!
//! - `__torajs_priv_install(obj, from, to)` — §7.3.33
//!   PrivateMethodOrAccessorAdd / §7.3.32 PrivateFieldAdd for a run of
//!   one class's elements: the object's install cursor moves from
//!   `from` to `to` (inline for a typed instance, the kernel
//!   otherwise), and a cursor already elsewhere is the duplicate-add
//!   TypeError.
//! - `__torajs_priv_brand(obj, "C", pos)` — §7.3.31 / §7.3.32 step 1
//!   for a private method or accessor of `C`, answering `obj` itself
//!   (it replaces the receiver of the access): the object is an
//!   instance of `C` (the `instanceof` lowering, typed tag chain or
//!   any dispatch) and its cursor is past `C`'s position. A typed
//!   instance reads the cursor inline from the high half of the +8
//!   slot; anything else asks the kernel, which answers -1 for a value
//!   that is not a class instance cell.

use crate::ast::{Expr, ExprId};
use crate::ssa::{BinOp, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{LowerCtx, OBJ_CLASS_TAG_OFF};

pub(crate) fn try_lower(ctx: &mut LowerCtx<'_>, name: &str, args: &[ExprId]) -> Option<Operand> {
    match name {
        crate::ast::private_brand::INSTALL => lower_install(ctx, args),
        crate::ast::private_brand::CHECK => lower_check(ctx, args),
        _ => None,
    }
}

fn const_u32(ctx: &LowerCtx<'_>, eid: ExprId) -> Option<i64> {
    match ctx.ast.get_expr(eid) {
        Expr::Number(n) => Some(*n as i64),
        _ => None,
    }
}

fn lower_install(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    let [obj, from, to] = args else { return None };
    let (from, to) = (const_u32(ctx, *from)?, const_u32(ctx, *to)?);
    let recv = ctx.lower_expr(*obj);
    if matches!(ctx.operand_ty(&recv), Type::Obj(_)) {
        lower_install_inline(ctx, recv, from, to);
        return Some(Operand::ConstI64(0));
    }
    let v = if matches!(ctx.operand_ty(&recv), Type::Any) {
        recv
    } else {
        ctx.box_to_any(recv)
    };
    let f = ctx.intrinsics.priv_install;
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(f, vec![v, Operand::ConstI64(from), Operand::ConstI64(to)]),
    );
    ctx.emit_throw_check(None);
    Some(Operand::ConstI64(0))
}

/// A class instance's cursor sits in the high half of its +8 slot:
/// standing at `from` it moves by `to - from`; anywhere else the kernel
/// raises the duplicate-add TypeError.
fn lower_install_inline(ctx: &mut LowerCtx<'_>, recv: Operand, from: i64, to: i64) {
    let slot = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, recv.clone(), OBJ_CLASS_TAG_OFF),
        Type::I64,
        None,
    );
    let cursor = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(BinOp::LShr, Operand::Value(slot), Operand::ConstI64(32)),
        Type::I64,
        None,
    );
    let at = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, Operand::Value(cursor), Operand::ConstI64(from)),
        Type::Bool,
        None,
    );
    let (fast, slow, cont) = (ctx.f.add_block(), ctx.f.add_block(), ctx.f.add_block());
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(at),
            then_blk: fast,
            else_blk: slow,
        },
    );
    ctx.cur_block = fast;
    let moved = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(
            BinOp::Add,
            Operand::Value(slot),
            Operand::ConstI64((to - from) << 32),
        ),
        Type::I64,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(moved), recv.clone(), OBJ_CLASS_TAG_OFF),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(cont));
    ctx.cur_block = slow;
    let boxed = ctx.box_to_any(recv);
    let f = ctx.intrinsics.priv_install;
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            f,
            vec![boxed, Operand::ConstI64(from), Operand::ConstI64(to)],
        ),
    );
    ctx.emit_throw_check(None);
    if ctx.cur_open() {
        ctx.f.set_term(ctx.cur_block, Terminator::Br(cont));
    }
    ctx.cur_block = cont;
}

fn lower_check(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    let [obj, class, pos] = args else { return None };
    let Expr::String(class) = ctx.ast.get_expr(*class) else {
        return None;
    };
    let class = class.to_string_lossy_owned();
    let pos = const_u32(ctx, *pos)?;
    let recv = ctx.lower_expr(*obj);
    let is_instance =
        crate::ssa_lower_instanceof::lower_class_membership(ctx, recv.clone(), &class);
    let cursor = if matches!(ctx.operand_ty(&recv), Type::Obj(_)) {
        let slot = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, recv.clone(), OBJ_CLASS_TAG_OFF),
            Type::I64,
            None,
        );
        ctx.f.append_inst(
            ctx.cur_block,
            InstKind::BinOp(BinOp::LShr, Operand::Value(slot), Operand::ConstI64(32)),
            Type::I64,
            None,
        )
    } else {
        let boxed = if matches!(ctx.operand_ty(&recv), Type::Any) {
            recv.clone()
        } else {
            ctx.box_to_any(recv.clone())
        };
        let f = ctx.intrinsics.priv_cursor;
        ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(f, vec![boxed]),
            Type::I64,
            None,
        )
    };
    let past = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Sgt, Operand::Value(cursor), Operand::ConstI64(pos)),
        Type::Bool,
        None,
    );
    let ok = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(BinOp::And, is_instance, Operand::Value(past)),
        Type::Bool,
        None,
    );
    let fail = ctx.f.add_block();
    let cont = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(ok),
            then_blk: cont,
            else_blk: fail,
        },
    );
    ctx.cur_block = fail;
    let f = ctx.intrinsics.priv_brand_fail;
    ctx.f.append_void(ctx.cur_block, InstKind::Call(f, vec![]));
    ctx.emit_throw_check(None);
    if ctx.cur_open() {
        ctx.f.set_term(ctx.cur_block, Terminator::Br(cont));
    }
    ctx.cur_block = cont;
    // The check answers its receiver with the receiver's own
    // ownership (`LowerCtx::expr_owned_shape` looks through it).
    Some(recv)
}
