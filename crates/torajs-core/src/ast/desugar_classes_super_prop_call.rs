//! The call-shaped arms of the SuperProperty rewrite
//! (`desugar_classes_super_prop`): `super[k](…)`, `super.m?.(…)` /
//! `super[k]?.(…)`, the `super.x++` family and the dynamic-base
//! write. Each site travels WHOLE to one runtime kernel that reads
//! the base with §13.3.7's receiver and then acts with it — the
//! reason none of them is a rewrite of the Member alone is in the
//! parent's module doc. Split out under the 500-line / 200-line
//! rules when the update and optional-call arms landed (rotation
//! 628); bodies verbatim from the parent's match arms.

use super::desugar_classes_super_prop_mint::{SiteCtx, update_fn};
use super::{Ast, Expr, ExprId};

/// `super[k](args)` → `__torajs_super_prop_call(<base>, k, <receiver>,
/// [args…])`.
pub(super) fn rewrite_call_index(
    ast: &mut Ast,
    cx: &SiteCtx<'_>,
    call: ExprId,
    index_expr: ExprId,
) {
    let Expr::Call { args, .. } = ast.get_expr(call) else {
        return;
    };
    let args = args.clone();
    let Expr::Index { index, .. } = ast.get_expr(index_expr) else {
        return;
    };
    let key = *index;
    let base = cx.base(ast);
    let recv = cx.receiver(ast);
    // The pack is an array literal, the protocol
    // `__torajs_super_value` already uses: one dense
    // `Arr<Any>` carries a spread element without the
    // kernel needing a spread protocol of its own.
    let pack = ast.add_expr(Expr::Array(args));
    let callee = ast.add_expr(Expr::Ident("__torajs_super_prop_call".to_string()));
    ast.exprs[call.0 as usize] = Expr::Call {
        callee,
        args: vec![base, key, recv, pack],
    };
    // The Index node the marker hung off is now orphaned
    // — same posture as the markers themselves.
    ast.exprs[index_expr.0 as usize] = Expr::Null;
}

/// `super.m?.(args)` / `super[k]?.(args)` →
/// `__torajs_super_prop_opt_call(<base>, k, <receiver>, [args…])`.
pub(super) fn rewrite_call_opt(ast: &mut Ast, cx: &SiteCtx<'_>, call: ExprId, callee: ExprId) {
    let Expr::OptCall { args, .. } = ast.get_expr(call) else {
        return;
    };
    let args = args.clone();
    let key = match ast.get_expr(callee) {
        Expr::Member { name, .. } => {
            let name = name.clone();
            ast.add_expr(Expr::String(name.into()))
        }
        Expr::Index { index, .. } => *index,
        _ => return,
    };
    let base = cx.base(ast);
    let recv = cx.receiver(ast);
    let pack = ast.add_expr(Expr::Array(args));
    let kernel = ast.add_expr(Expr::Ident("__torajs_super_prop_opt_call".to_string()));
    ast.exprs[call.0 as usize] = Expr::Call {
        callee: kernel,
        args: vec![base, key, recv, pack],
    };
    ast.exprs[callee.0 as usize] = Expr::Null;
}

/// `super.x++` and its spellings → one of the four update kernel
/// names over `(<base>, k, <receiver>)`.
pub(super) fn rewrite_update(
    ast: &mut Ast,
    cx: &SiteCtx<'_>,
    node: ExprId,
    target: ExprId,
    is_inc: bool,
    is_prefix: bool,
) {
    let key = match ast.get_expr(target) {
        Expr::Member { name, .. } => {
            let name = name.clone();
            ast.add_expr(Expr::String(name.into()))
        }
        Expr::Index { index, .. } => *index,
        _ => return,
    };
    let base = cx.base(ast);
    let recv = cx.receiver(ast);
    let callee = ast.add_expr(Expr::Ident(update_fn(is_inc, is_prefix).to_string()));
    ast.exprs[node.0 as usize] = Expr::Call {
        callee,
        args: vec![base, key, recv],
    };
    // The Member / Index the marker hung off is orphaned
    // — same posture as the markers themselves.
    ast.exprs[target.0 as usize] = Expr::Null;
}

/// The dynamic-base write — the whole Assign becomes
/// `__torajs_super_prop_set(<base>, key, value, <receiver>)`, which
/// answers the assignment's value like the Assign did.
pub(super) fn rewrite_dynamic_write(
    ast: &mut Ast,
    cx: &SiteCtx<'_>,
    assign: ExprId,
    key: ExprId,
    value: ExprId,
) {
    let base = cx.base(ast);
    let recv = cx.receiver(ast);
    let callee = ast.add_expr(Expr::Ident("__torajs_super_prop_set".to_string()));
    ast.exprs[assign.0 as usize] = Expr::Call {
        callee,
        args: vec![base, key, value, recv],
    };
}
