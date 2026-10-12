//! The group temp's source, lowered for the lane that reads it: boxed
//! for the iterator walk, or split by code point for a `string` — the
//! split `[...s]` and `Array.from(s)` share.

use crate::ast::ExprId;
use crate::ssa::{BinOp, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{LowerCtx, intern_arr_layout};

/// Lower the source expression and hand the kernel an AnyValue, saying
/// whether a box had to be minted for it. A concrete source (a
/// `Generator` binding, a `Set`) boxes: the box takes a reference of its
/// own, so a borrow-shaped init (an Ident) retains first and an owned
/// one (a `gen()` call) transfers the reference it already holds. An
/// `any` source is already in the kernel's shape and passes through.
pub(super) fn lower_src_to_any(ctx: &mut LowerCtx, init: ExprId) -> (Operand, bool) {
    let src = ctx.lower_expr(init);
    let src_ty = ctx.operand_ty(&src);
    if src_ty == Type::Any {
        return (src, false);
    }
    let is_borrow = !ctx.expr_transfers_ownership(init);
    (ctx.box_owned_from_expr(init, src, is_borrow), true)
}

/// A binding pattern over a `string` source the checker typed
/// `Array<String>` (`check_stmt_let_decl_destr_lane::steps_string_typed`):
/// the String iterator's steps are the string's code points (§22.1.5.1),
/// so the group is the string split by code point, at most `limit` of
/// them — the pattern never observes the tail, and a string iterator
/// has nothing to close. Answers the fresh `Array<String>`, or `None`
/// for any other group.
///
/// A `string` slot can still hold null or its undefined sentinel at run
/// time (a missed capture, a `string[]` read past the end); §7.4.2
/// GetIterator throws a TypeError on either, as the walk lane did.
pub(super) fn try_split_string(
    ctx: &mut LowerCtx,
    name: &str,
    init: ExprId,
    limit: i64,
) -> Option<(Operand, Type)> {
    if ctx.expr_types.get(&init) != Some(&crate::check::Type::String)
        || !crate::check_stmt_let_decl_destr_lane::steps_string_typed(
            ctx.ast,
            &ctx.scans.proto_shadow,
            name,
            init,
        )
    {
        return None;
    }
    let v = ctx.lower_expr(init);
    // A view copies out first (the undefined sentinel keeps its
    // identity); the copy is ours to release, a borrowed view is not.
    let (s, minted) = if ctx.operand_ty(&v) == Type::Substr {
        let owned = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.substr_to_owned, vec![v]),
            Type::Str,
            None,
        );
        if ctx.join_arm_mints(init) {
            ctx.emit_drop_value(v, Type::Substr);
        }
        (Operand::Value(owned), true)
    } else {
        (v, false)
    };
    emit_nullish_throw(ctx, s);
    let split = ctx.emit_arr_from_string(s, limit);
    if minted {
        ctx.emit_drop_value(s, Type::Str);
    } else {
        ctx.release_owned_temp(init, &v);
    }
    Some(split)
}

impl LowerCtx<'_> {
    /// Split the `Str` `s` by code point into a fresh `Array<String>`,
    /// at most `limit` elements when `limit >= 0`
    /// (`__torajs_arr_from_string`). Borrows `s`.
    pub(crate) fn emit_arr_from_string(&mut self, s: Operand, limit: i64) -> (Operand, Type) {
        let arr_ty = Type::Arr(intern_arr_layout(self.arr_layouts, Type::Str));
        let arr = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.arr_from_string,
                vec![s, Operand::ConstI64(limit)],
            ),
            arr_ty,
            None,
        );
        (Operand::Value(arr), arr_ty)
    }
}

/// Branch to a TypeError when the `Str` source is null or the undefined
/// sentinel; the live path continues in a fresh block.
fn emit_nullish_throw(ctx: &mut LowerCtx, s: Operand) {
    let eq_null = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, s, Operand::ConstPtrNull),
        Type::Bool,
        None,
    );
    let sentinel = ctx
        .str_undef_sentinel_for(Type::Str)
        .expect("a Str slot has an undefined sentinel");
    let eq_undef = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, s, sentinel),
        Type::Bool,
        None,
    );
    let nullish = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(BinOp::Or, Operand::Value(eq_null), Operand::Value(eq_undef)),
        Type::Bool,
        None,
    );
    let throw_blk = ctx.f.add_block();
    let live_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(nullish),
            then_blk: throw_blk,
            else_blk: live_blk,
        },
    );
    ctx.cur_block = throw_blk;
    let msg = ctx.intern_string_literal("value is not iterable");
    let raiser = ctx.intrinsics.throw_type_error_str;
    let _ = ctx.f.append_inst(
        throw_blk,
        InstKind::Call(raiser, vec![Operand::Value(msg)]),
        Type::Void,
        None,
    );
    ctx.emit_throw_check(None);
    ctx.f.set_term(ctx.cur_block, Terminator::Br(live_blk));
    ctx.cur_block = live_blk;
}
