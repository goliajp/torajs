//! The split-count lane's read side: which operands are a measured
//! split product held as its token count.
//!
//! The census (`ast::split_length_only`) lists the one-argument split
//! calls whose product is only ever measured; `lower_split` emits
//! `__torajs_str_split_count` for them when the receiver and separator
//! are strings, so the call's value is an `I64`, not an array. Two
//! places then have to answer from that number: the `.length` read
//! (`ssa_lower_member::lower`) and the binding it lands in
//! (`ssa_lower_stmt_let_decl::finalize_and_bind`).

use crate::ast::{Expr, ExprId};
use crate::ssa::Type;
use crate::ssa_lower::LowerCtx;

/// `eid` is a one-argument split call the census listed.
pub(crate) fn is_counted_split(ctx: &LowerCtx<'_>, eid: ExprId) -> bool {
    matches!(
        ctx.ast.get_expr(eid),
        Expr::Call { args, .. }
            if args.len() == 1 && ctx.ast.split_length_only_seps.contains(&args[0])
    )
}

/// `obj`, lowered to an operand of type `obj_ty`, is a counted split
/// product: the call itself, or a binding that holds one.
///
/// A binding is recognised by a pair — the checker types the name an
/// array while its slot holds an `I64` — rather than by a side set:
/// only `finalize_and_bind` ever binds an array-typed name to an
/// integer slot, for a counted split or a measured match
/// (`ssa_lower_match_len`), and a binding takes the slot only when all
/// its uses are its own lane's, whose use sets do not meet (`x.length`
/// here). `ctx.locals` answers the binding currently in scope, so an
/// inner declaration that shadows the name keeps its own reading.
pub(crate) fn operand_is_split_count(ctx: &LowerCtx<'_>, obj: ExprId, obj_ty: &Type) -> bool {
    if *obj_ty != Type::I64 {
        return false;
    }
    match ctx.ast.get_expr(obj) {
        Expr::Ident(n) => {
            matches!(ctx.expr_types.get(&obj), Some(crate::check::Type::Array(_)))
                && ctx.locals.get(n).is_some_and(|l| l.ty == Type::I64)
        }
        _ => is_counted_split(ctx, obj),
    }
}
