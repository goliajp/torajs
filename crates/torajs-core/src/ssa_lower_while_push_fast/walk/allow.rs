//! The allowlist a walk's body has to stay on, and the appends a trip
//! through it can make (see the parent module).

use crate::ast::{Ast, Expr, ExprId, Stmt, UnaryOp};
use crate::check::Type as CType;
use crate::ssa::Type;
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_push_loop_detect::is_inert_binop;

/// The names the allowlist protects.
pub(super) struct Names<'n> {
    pub(super) counter: &'n str,
    pub(super) arr: &'n str,
    /// the string the loop's test reads the length of, which is no
    /// longer nullish once the body runs
    pub(super) src: &'n str,
    /// the bindings the body declares, which the lowering has not
    /// met yet
    pub(super) decls: Vec<&'n str>,
}

pub(super) fn first_push_receiver(ast: &Ast, s: &Stmt) -> Option<String> {
    match s {
        Stmt::Expr(e) => match ast.get_expr(*e) {
            Expr::Call { callee, .. } => match ast.get_expr(*callee) {
                Expr::Member { obj, name } if name == "push" => match ast.get_expr(*obj) {
                    Expr::Ident(n) => Some(n.clone()),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        },
        Stmt::Block(v) | Stmt::Multi(v) => v.iter().find_map(|s| first_push_receiver(ast, s)),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => first_push_receiver(ast, then_branch).or_else(|| {
            else_branch
                .as_deref()
                .and_then(|b| first_push_receiver(ast, b))
        }),
        _ => None,
    }
}

/// The most appends to `names.arr` one trip through `s` can make, or
/// `None` when `s` is off the allowlist. `depth` counts the inner loops
/// around `s`, which may not push and own their `break` / `continue`.
pub(super) fn walk_stmt(
    ctx: &LowerCtx<'_>,
    s: &Stmt,
    names: &Names<'_>,
    depth: u32,
) -> Option<i64> {
    match s {
        Stmt::Expr(e) => walk_expr_stmt(ctx, *e, names, depth),
        Stmt::LetDecl {
            name,
            type_ann,
            init,
            is_var: false,
            ..
        } => (name != names.counter
            && name != names.arr
            && type_ann
                .as_deref()
                .is_none_or(|t| t == "number" || t == "boolean")
            && expr_ok(ctx, names, *init))
        .then_some(0),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            if !expr_ok(ctx, names, *cond) {
                return None;
            }
            let t = walk_stmt(ctx, then_branch, names, depth)?;
            let e = match else_branch {
                Some(b) => walk_stmt(ctx, b, names, depth)?,
                None => 0,
            };
            Some(t.max(e))
        }
        Stmt::Block(v) | Stmt::Multi(v) => v
            .iter()
            .try_fold(0, |n, s| Some(n + walk_stmt(ctx, s, names, depth)?)),
        Stmt::While { cond, body } => (expr_ok(ctx, names, *cond)
            && walk_stmt(ctx, body, names, depth + 1)? == 0)
            .then_some(0),
        Stmt::Break(None) | Stmt::Continue(None) if depth > 0 => Some(0),
        _ => None,
    }
}

fn walk_expr_stmt(ctx: &LowerCtx<'_>, e: ExprId, names: &Names<'_>, depth: u32) -> Option<i64> {
    let ast = ctx.ast;
    // a write before the declaration raises as a read does (§9.1.1.1.5)
    let ident = |t: ExprId| match ast.get_expr(t) {
        Expr::Ident(n) if !ast.tdz_reads.contains(&t) => Some(n.as_str()),
        _ => None,
    };
    match ast.get_expr(e) {
        Expr::Call { callee, args } => {
            let Expr::Member { obj, name } = ast.get_expr(*callee) else {
                return None;
            };
            (name == "push"
                && depth == 0
                && args.len() == 1
                && ident(*obj) == Some(names.arr)
                && push_arg_ok(ctx, names, args[0]))
            .then_some(1)
        }
        // the counter included: the reserve is a forecast, which no
        // step can make unsound (see the parent module)
        Expr::Assign { target, value } => {
            let t = ident(*target)?;
            (t != names.arr && primitive_slot(ctx, names, t) && expr_ok(ctx, names, *value))
                .then_some(0)
        }
        Expr::PostIncr { target, .. } => {
            let t = ident(*target)?;
            (t != names.arr && primitive_slot(ctx, names, t)).then_some(0)
        }
        _ => None,
    }
}

/// A binding whose slot holds a number or a boolean. A name the body
/// declares is one of its own (the allowlist admits only number and
/// boolean declarations); one it shares with a binding outside the
/// body could name either, so that one has to qualify too.
fn primitive_slot(ctx: &LowerCtx<'_>, names: &Names<'_>, name: &str) -> bool {
    let ty = match ctx.locals.get(name) {
        Some(l) => l.ty,
        None => match ctx.globals.get(name) {
            Some(t) => *t,
            None => return names.decls.contains(&name),
        },
    };
    matches!(ty, Type::I64 | Type::I32 | Type::F64 | Type::Bool)
}

pub(super) fn body_decls<'s>(s: &'s Stmt, out: &mut Vec<&'s str>) {
    match s {
        Stmt::LetDecl { name, .. } => out.push(name),
        Stmt::Block(v) | Stmt::Multi(v) => v.iter().for_each(|s| body_decls(s, out)),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            body_decls(then_branch, out);
            if let Some(b) = else_branch {
                body_decls(b, out);
            }
        }
        Stmt::While { body, .. } => body_decls(body, out),
        _ => {}
    }
}

fn push_arg_ok(ctx: &LowerCtx<'_>, names: &Names<'_>, a: ExprId) -> bool {
    match crate::ssa_lower::inline_push::construction_args(ctx, a) {
        Some(args) => args.iter().all(|x| expr_ok(ctx, names, *x)),
        None => expr_ok(ctx, names, a),
    }
}

/// A number or boolean that cannot call, write, raise or read the
/// array: literals, locals past their declaration, arithmetic and
/// comparison over them, and the `length` and `charCodeAt` of a string
/// that cannot be nullish.
fn expr_ok(ctx: &LowerCtx<'_>, names: &Names<'_>, e: ExprId) -> bool {
    if !matches!(
        ctx.expr_types.get(&e),
        Some(CType::Number) | Some(CType::Boolean)
    ) {
        return false;
    }
    match ctx.ast.get_expr(e) {
        Expr::Number(_) | Expr::Bool(_) => true,
        // a read before its declaration raises (§9.1.1.1.6 GetBindingValue)
        Expr::Ident(_) => !ctx.ast.tdz_reads.contains(&e),
        Expr::BinOp { op, left, right } => {
            is_inert_binop(*op) && expr_ok(ctx, names, *left) && expr_ok(ctx, names, *right)
        }
        Expr::Unary { op, expr } => !matches!(op, UnaryOp::Plus) && expr_ok(ctx, names, *expr),
        Expr::Member { obj, name } => name == "length" && sure_string(ctx, names, *obj),
        Expr::Call { callee, args } => {
            matches!(ctx.ast.get_expr(*callee), Expr::Member { obj, name }
                if name == "charCodeAt" && sure_string(ctx, names, *obj))
                && args.len() == 1
                && expr_ok(ctx, names, args[0])
        }
        _ => false,
    }
}

/// A string the body reads that cannot be nullish: the one the loop's
/// test has read, or one the lowering would not test.
fn sure_string(ctx: &LowerCtx<'_>, names: &Names<'_>, e: ExprId) -> bool {
    is_string_ident(ctx, e)
        && (matches!(ctx.ast.get_expr(e), Expr::Ident(n) if n == names.src)
            || !crate::ssa_lower_nullable_guard::is_nullable_str_source(ctx, e))
}

pub(super) fn is_string_ident(ctx: &LowerCtx<'_>, e: ExprId) -> bool {
    matches!(ctx.ast.get_expr(e), Expr::Ident(_))
        && !ctx.ast.tdz_reads.contains(&e)
        && matches!(ctx.expr_types.get(&e), Some(CType::String))
}
