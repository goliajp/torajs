//! `__torajs_obj_rest(src, "a,b", [k1, k2])` — the object-rest
//! destructuring form whose excluded set is not all known at compile
//! time (§13.15.5.4 RestDestructuringAssignmentEvaluation).
//!
//! Without a computed sibling key the desugar keeps the sentinel
//! object-literal form (`__spread_omit__:a,b`), whose value the
//! checker can type exactly: the source's struct minus the named
//! keys. A computed key has no name to subtract, so the rest object's
//! shape is not a static answer at all — this call is the honest
//! spelling of that, and it answers `any`.
//!
//! The two halves reach the kernel as they are: the spelled names as
//! the one comma-separated Str cell the sentinel already used, the
//! computed ones as an `Array<Any>` of the values §13.15.5.5 put
//! through ToPropertyKey at their own position in the pattern. Doing
//! the conversion there rather than here is what keeps a key's
//! `toString` from running a second time — the pattern's own load
//! reads through the same converted key.
//!
//! The kernel skips an excluded key BEFORE [[Get]], which is the
//! point of carrying the list at all: copying first and deleting
//! after would answer with the right properties while running the
//! excluded key's getter, a side effect the program can see
//! (test262 `object-rest-proxy-gopd-not-called-on-excluded-keys`).

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// `(source, names, keys)`. `names` is a string literal (empty when
/// the pattern spells no plain key); `keys` is an array literal of
/// the computed keys, already property keys.
pub(crate) fn try_lower(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    if args.len() != 3 {
        return None;
    }
    let src_raw = ctx.lower_expr(args[0]);
    let src = if matches!(ctx.operand_ty(&src_raw), Type::Any) {
        src_raw.clone()
    } else {
        ctx.box_to_any(src_raw.clone())
    };
    let names = match ctx.ast.get_expr(args[1]) {
        crate::ast::Expr::String(s) if s.is_empty() => Operand::ConstPtrNull,
        _ => ctx.lower_expr(args[1]),
    };
    let keys_raw = ctx.lower_expr(args[2]);
    let keys_ty = ctx.operand_ty(&keys_raw);
    let keys = if matches!(keys_ty, Type::Any) {
        keys_raw.clone()
    } else {
        ctx.box_to_any(keys_raw.clone())
    };
    let out = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.anyv_obj_rest, vec![src, names, keys]),
        Type::Any,
        None,
    );
    ctx.emit_drop_value(keys_raw, keys_ty);
    ctx.release_owned_temp(args[0], &src_raw);
    ctx.emit_throw_check_owned(None, Operand::Value(out), Type::Any);
    Some(Operand::Value(out))
}
