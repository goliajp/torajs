//! 628-05 — `__torajs_class_computed_field_register("<C>", "<after>",
//! __ccmk_<O>_<n>)`: hand the runtime the own-key ORDER anchor of one
//! computed instance field of class `C` (the layout row named
//! `<after>` it follows, `""` = before every row) and the key global
//! it was stored under. Emitted by `desugar_classes_computed_fields`
//! at the class-decl position; the class is registered by then, so
//! the runtime resolves the row at once.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower_register(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    if args.len() != 3 {
        return None;
    }
    let Expr::String(cname) = ctx.ast.get_expr(args[0]) else {
        return None;
    };
    let cname = cname.to_string_lossy_owned();
    // An unregistered name (a generic class's erased original, a
    // declined capture) has no tag to anchor under — the key global
    // still evaluated at its let, nothing else to do here.
    let Some(tag) = ctx.class_name_to_tag.get(cname.as_str()).copied() else {
        return Some(Operand::ConstI64(0));
    };
    let after_op = ctx.lower_expr(args[1]);
    let key_op = ctx.lower_expr(args[2]);
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.class_computed_field_register,
            vec![Operand::ConstI64(tag as i64), after_op, key_op],
        ),
    );
    // The kernel keeps its own share of the key and reads the anchor
    // name once; the lowered Str literal is a caller-owned temp.
    let ty = ctx.operand_ty(&after_op);
    ctx.emit_drop_value(after_op, ty);
    Some(Operand::ConstI64(0))
}
