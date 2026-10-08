//! The pointer-to-closure direction of the argument contract
//! ([`crate::ssa_lower_call_arg_conv::ArgConv::FnSigToClosure`]): a
//! function value a call hands a closure parameter as a bare pointer
//! becomes the closure cell that stands for it.

use crate::ast::{Expr, ExprId};
use crate::ssa::{BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;

/// The closure cell for a function value the call hands a closure
/// parameter as a bare pointer (+1, released by the calling lane after
/// the call). A top-level function's value is the canonical cell of
/// its forwarder shell, which every value read of the function
/// answers; the collector mints the shell for a function named in a
/// member call's argument. A choice among such functions
/// (`c ? f1 : f2`, `f1 || f2`) picks the cell of the function the
/// pointer is.
pub(crate) fn fn_value_cell(
    ctx: &mut LowerCtx<'_>,
    expected: Type,
    arg: ExprId,
    op: Operand,
) -> Operand {
    static_fn_value_cell(ctx, expected, arg, op).unwrap_or_else(|| {
        panic!(
            "ssa-lower: a function value with no static origin reaches a closure-typed \
             parameter; only a top-level function, or a choice among them, converts to its \
             closure cell here"
        )
    })
}

/// [`fn_value_cell`] for a lane that holds only the operand: the
/// expression the operand was lowered from stands for the argument.
/// None when that expression is not a static choice of functions.
pub(crate) fn operand_fn_value_cell(
    ctx: &mut LowerCtx<'_>,
    expected: Type,
    op: Operand,
) -> Option<Operand> {
    let Operand::Value(v) = op else {
        return None;
    };
    let origin = ctx
        .f
        .blocks
        .iter()
        .flat_map(|b| &b.insts)
        .find(|i| i.result == Some(v))
        .and_then(|i| i.origin)?;
    static_fn_value_cell(ctx, expected, origin, op)
}

fn static_fn_value_cell(
    ctx: &mut LowerCtx<'_>,
    expected: Type,
    arg: ExprId,
    op: Operand,
) -> Option<Operand> {
    let mut leaves: Vec<(String, String)> = Vec::new();
    if !static_fn_leaves(ctx, arg, &mut leaves) {
        return None;
    }
    let ((_, last), rest) = leaves.split_last()?;
    if rest.is_empty() {
        return Some(crate::ssa_lower_closure::lower(
            ctx,
            last.clone(),
            Vec::new(),
        ));
    }
    let slot = ctx.alloca_in_entry(expected, Some("__fncell"));
    let after = ctx.f.add_block();
    let sig = ctx.operand_ty(&op);
    for (f, shell) in rest {
        let fid = ctx.fn_table[f.as_str()];
        let addr = ctx
            .f
            .append_inst(ctx.cur_block, InstKind::FnAddr(fid), sig, None);
        let eq = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Eq, op, Operand::Value(addr)),
            Type::Bool,
            None,
        );
        let hit = ctx.f.add_block();
        let next = ctx.f.add_block();
        let cond = Operand::Value(eq);
        ctx.f.set_term(
            ctx.cur_block,
            Terminator::CondBr {
                cond,
                then_blk: hit,
                else_blk: next,
            },
        );
        ctx.cur_block = hit;
        store_cell(ctx, shell.clone(), slot, after);
        ctx.cur_block = next;
    }
    store_cell(ctx, last.clone(), slot, after);
    ctx.cur_block = after;
    let load = InstKind::Load(expected, Operand::Value(slot), 0);
    Some(Operand::Value(ctx.f.append_inst(
        ctx.cur_block,
        load,
        expected,
        None,
    )))
}

fn store_cell(ctx: &mut LowerCtx<'_>, shell: String, slot: ValueId, after: BlockId) {
    let cell = crate::ssa_lower_closure::lower(ctx, shell, Vec::new());
    let store = InstKind::Store(cell, Operand::Value(slot), 0);
    ctx.f.append_void(ctx.cur_block, store);
    ctx.f.set_term(ctx.cur_block, Terminator::Br(after));
}

/// Collect `(function, forwarder shell)` for every value `e` may
/// answer, through the operators that hand an operand straight
/// through. False when one of them is not a top-level function with
/// a shell.
fn static_fn_leaves(ctx: &LowerCtx<'_>, e: ExprId, out: &mut Vec<(String, String)>) -> bool {
    match ctx.ast.get_expr(ctx.peel_as_wrappers(e)) {
        Expr::Ident(f) if !ctx.locals.contains_key(f) && ctx.fn_table.contains_key(f) => {
            let shell = crate::ast::shell_names(f)
                .into_iter()
                .find(|n| ctx.fn_table.contains_key(n.as_str()));
            let Some(shell) = shell else {
                return false;
            };
            if !out.iter().any(|(g, _)| g == f) {
                out.push((f.clone(), shell));
            }
            true
        }
        Expr::Ternary {
            then_branch,
            else_branch,
            ..
        } => static_fn_leaves(ctx, *then_branch, out) && static_fn_leaves(ctx, *else_branch, out),
        Expr::Nullish { lhs: a, rhs: b }
        | Expr::BinOp {
            op: crate::ast::BinOp::LAnd | crate::ast::BinOp::LOr,
            left: a,
            right: b,
        } => static_fn_leaves(ctx, *a, out) && static_fn_leaves(ctx, *b, out),
        Expr::Sequence { right, .. } => static_fn_leaves(ctx, *right, out),
        _ => false,
    }
}
