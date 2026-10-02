//! T-22 Stmt::ForOfSplitIter — "x-safe" analysis: verify no
//! reference to the split-iter user-named `x` binding appears
//! outside the exact `Index(Ident(x), Ident(i))` sequential-access
//! shape, plus a plain-ident-use probe.
//!
//! Chunk 348 — extracted from ast.rs. Paired with chunks 347's
//! `sfi_rewrite` sibling. Both wrappers (`sfi_body_x_safe` +
//! `sfi_stmt_uses_ident`) are called from the SFI desugar pass in
//! ast.rs (lines ~5203/5219/5225); the two recursive walkers
//! (`sfi_stmt_x_safe` + `sfi_expr_x_safe`) stay sibling-private.

use super::{Ast, Expr, ExprId, Stmt};

/// Walk body, return false if any reference to `x_name` appears that
/// is NOT exactly `Index(Ident(x_name), Ident(i_name))` (sequential
/// access). Conservative — false on any uncertain shape.
pub(super) fn sfi_body_x_safe(ast: &Ast, body: &Stmt, x_name: &str, i_name: &str) -> bool {
    sfi_stmt_x_safe(ast, body, x_name, XShape::SeqIndex(i_name))
}

/// True when every reference to the split loop's variable `v_name` in
/// `body` is a view read the lowering answers from the loop's own
/// registers: the receiver of a `.charCodeAt(pos?)` call, or `.length`.
/// Then the part never needs to exist as a Substr in memory.
pub(crate) fn split_var_view_reads_only(ast: &Ast, body: &Stmt, v_name: &str) -> bool {
    sfi_stmt_x_safe(ast, body, v_name, XShape::ViewReads)
}

/// Which references to X a walk accepts.
#[derive(Clone, Copy)]
enum XShape<'a> {
    /// `X[I]` for the loop counter `I`, nothing else.
    SeqIndex(&'a str),
    /// `X.charCodeAt(pos?)` as a call, and `X.length`.
    ViewReads,
}

fn sfi_stmt_x_safe(ast: &Ast, s: &Stmt, x_name: &str, shape: XShape<'_>) -> bool {
    match s {
        Stmt::Expr(eid) | Stmt::Throw(eid) | Stmt::Yield(eid) => {
            sfi_expr_x_safe(ast, *eid, x_name, shape)
        }
        Stmt::Return(Some(eid)) => sfi_expr_x_safe(ast, *eid, x_name, shape),
        Stmt::Return(None) | Stmt::Break(_) | Stmt::Continue(_) => true,
        Stmt::Labeled { body, .. } => sfi_stmt_x_safe(ast, body, x_name, shape),
        Stmt::LetDecl { init, .. } | Stmt::UsingDecl { init, .. } => {
            sfi_expr_x_safe(ast, *init, x_name, shape)
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            sfi_expr_x_safe(ast, *cond, x_name, shape)
                && sfi_stmt_x_safe(ast, then_branch, x_name, shape)
                && else_branch
                    .as_deref()
                    .is_none_or(|eb| sfi_stmt_x_safe(ast, eb, x_name, shape))
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            sfi_expr_x_safe(ast, *cond, x_name, shape) && sfi_stmt_x_safe(ast, body, x_name, shape)
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            init.as_deref()
                .is_none_or(|i| sfi_stmt_x_safe(ast, i, x_name, shape))
                && cond.is_none_or(|c| sfi_expr_x_safe(ast, c, x_name, shape))
                && step.is_none_or(|st| sfi_expr_x_safe(ast, st, x_name, shape))
                && sfi_stmt_x_safe(ast, body, x_name, shape)
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            sfi_expr_x_safe(ast, *parent, x_name, shape)
                && sfi_expr_x_safe(ast, *sep, x_name, shape)
                && sfi_stmt_x_safe(ast, body, x_name, shape)
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => {
            sfi_expr_x_safe(ast, *elem_expr, x_name, shape)
                && sfi_stmt_x_safe(ast, body, x_name, shape)
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
            ..
        } => {
            sfi_expr_x_safe(ast, *scrutinee, x_name, shape)
                && cases.iter().all(|c| {
                    sfi_expr_x_safe(ast, c.value, x_name, shape)
                        && c.body
                            .iter()
                            .all(|s| sfi_stmt_x_safe(ast, s, x_name, shape))
                })
                && default
                    .as_ref()
                    .is_none_or(|db| db.iter().all(|s| sfi_stmt_x_safe(ast, s, x_name, shape)))
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            body.iter().all(|s| sfi_stmt_x_safe(ast, s, x_name, shape))
                && catch_body
                    .iter()
                    .all(|s| sfi_stmt_x_safe(ast, s, x_name, shape))
                && finally_body
                    .as_ref()
                    .is_none_or(|fb| fb.iter().all(|s| sfi_stmt_x_safe(ast, s, x_name, shape)))
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            stmts.iter().all(|s| sfi_stmt_x_safe(ast, s, x_name, shape))
        }
        Stmt::YieldInto { value, .. } => sfi_expr_x_safe(ast, *value, x_name, shape),
        Stmt::FnDecl { .. }
        | Stmt::TypeDecl { .. }
        | Stmt::ClassDecl { .. }
        | Stmt::ImportDecl { .. } => true,
        Stmt::ExportDecl { inner, .. } => inner
            .as_deref()
            .is_none_or(|s| sfi_stmt_x_safe(ast, s, x_name, shape)),
    }
}

/// Returns true iff every reference to `x_name` in `eid` is the safe
/// shape `Index(Ident(x_name), Ident(i_name))`. Conservative — any
/// shape we can't analyze cleanly returns false.
fn sfi_expr_x_safe(ast: &Ast, eid: ExprId, x_name: &str, shape: XShape<'_>) -> bool {
    match ast.get_expr(eid) {
        Expr::Elision => true,
        Expr::Ident(n) => n != x_name,
        Expr::Index { obj, index } => {
            if let Expr::Ident(n) = ast.get_expr(*obj)
                && n == x_name
            {
                return match shape {
                    XShape::SeqIndex(i_name) => {
                        matches!(ast.get_expr(*index), Expr::Ident(in_) if in_ == i_name)
                    }
                    XShape::ViewReads => false,
                };
            }
            sfi_expr_x_safe(ast, *obj, x_name, shape) && sfi_expr_x_safe(ast, *index, x_name, shape)
        }
        Expr::Member { obj, name } => {
            if let Expr::Ident(n) = ast.get_expr(*obj)
                && n == x_name
            {
                return matches!(shape, XShape::ViewReads) && name == "length";
            }
            sfi_expr_x_safe(ast, *obj, x_name, shape)
        }
        Expr::Call { callee, args } => {
            let view_read = matches!(shape, XShape::ViewReads)
                && args.len() <= 1
                && matches!(ast.get_expr(*callee), Expr::Member { obj, name }
                    if name == "charCodeAt"
                        && matches!(ast.get_expr(*obj), Expr::Ident(n) if n == x_name));
            (view_read || sfi_expr_x_safe(ast, *callee, x_name, shape))
                && args.iter().all(|a| sfi_expr_x_safe(ast, *a, x_name, shape))
        }
        Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::String(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Uninit
        | Expr::This
        | Expr::NewTarget
        | Expr::Regex { .. } => true,
        Expr::Array(els) => els.iter().all(|e| sfi_expr_x_safe(ast, *e, x_name, shape)),
        Expr::ObjectLit { fields } => fields
            .iter()
            .all(|(_, e)| sfi_expr_x_safe(ast, *e, x_name, shape)),
        Expr::Spread { expr } => sfi_expr_x_safe(ast, *expr, x_name, shape),
        Expr::BinOp { left, right, .. } => {
            sfi_expr_x_safe(ast, *left, x_name, shape)
                && sfi_expr_x_safe(ast, *right, x_name, shape)
        }
        Expr::Assign { target, value } => {
            sfi_expr_x_safe(ast, *target, x_name, shape)
                && sfi_expr_x_safe(ast, *value, x_name, shape)
        }
        Expr::Unary { expr, .. } => sfi_expr_x_safe(ast, *expr, x_name, shape),
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            sfi_expr_x_safe(ast, *cond, x_name, shape)
                && sfi_expr_x_safe(ast, *then_branch, x_name, shape)
                && sfi_expr_x_safe(ast, *else_branch, x_name, shape)
        }
        Expr::TypeOf { expr } | Expr::Delete { expr } => sfi_expr_x_safe(ast, *expr, x_name, shape),
        Expr::InstanceOf { expr, rhs } => {
            sfi_expr_x_safe(ast, *expr, x_name, shape) && sfi_expr_x_safe(ast, *rhs, x_name, shape)
        }
        Expr::As { expr, .. } => sfi_expr_x_safe(ast, *expr, x_name, shape),
        Expr::Sequence { left, right } => {
            sfi_expr_x_safe(ast, *left, x_name, shape)
                && sfi_expr_x_safe(ast, *right, x_name, shape)
        }
        // A closure may run after the iteration it was made in, so no
        // shape of X is safe inside one: any mention disqualifies.
        Expr::ArrowFn { body, .. } => !body.iter().any(|s| sfi_stmt_uses_ident(ast, s, x_name)),
        Expr::Closure { captures, .. } => !captures.iter().any(|c| c == x_name),
        Expr::Super { args } => args.iter().all(|a| sfi_expr_x_safe(ast, *a, x_name, shape)),
        Expr::New { args, .. } => args.iter().all(|a| sfi_expr_x_safe(ast, *a, x_name, shape)),
        Expr::NewDynamic { callee, args } => {
            sfi_expr_x_safe(ast, *callee, x_name, shape)
                && args.iter().all(|a| sfi_expr_x_safe(ast, *a, x_name, shape))
        }
        Expr::Nullish { lhs, rhs } => {
            sfi_expr_x_safe(ast, *lhs, x_name, shape) && sfi_expr_x_safe(ast, *rhs, x_name, shape)
        }
        Expr::OptChain { obj, .. } => {
            if let Expr::Ident(n) = ast.get_expr(*obj)
                && n == x_name
            {
                return false;
            }
            sfi_expr_x_safe(ast, *obj, x_name, shape)
        }
        Expr::OptIndex { obj, index } => {
            if let Expr::Ident(n) = ast.get_expr(*obj)
                && n == x_name
            {
                return false;
            }
            sfi_expr_x_safe(ast, *obj, x_name, shape) && sfi_expr_x_safe(ast, *index, x_name, shape)
        }
        Expr::OptCall { callee, args } => {
            sfi_expr_x_safe(ast, *callee, x_name, shape)
                && args.iter().all(|a| sfi_expr_x_safe(ast, *a, x_name, shape))
        }
        Expr::PostIncr { target, .. } => sfi_expr_x_safe(ast, *target, x_name, shape),
    }
}

pub(super) fn sfi_stmt_uses_ident(ast: &Ast, s: &Stmt, name: &str) -> bool {
    !sfi_stmt_x_safe(ast, s, name, XShape::SeqIndex("<<UNUSED>>"))
}
