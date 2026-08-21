//! The split-product census the annotated promotion lane consults —
//! carved out of the parent under the 500-line file discipline the
//! moment the walker pushed it to 647.
//!
//! Two questions, both about a top-level `const X: string[] = <init>`:
//!
//! - [`init_is_static_string_split`] — is the init the exact shape
//!   `lower_split` sends to the `(Str, Str)` kernel, i.e. will the
//!   slot be filled with substring VIEWS rather than owned strings?
//! - [`binding_written_through`] — does anything in the program write
//!   an owned value INTO that array (push / sort / `X[i] = v` / …)?
//!
//! Together they decide whether the promoted slot may take
//! `Arr<Substr>` (views, nothing written) or must not be promoted at
//! all (views, but written — neither layout is right for a global, so
//! the binding stays the main-local the `let` spelling already is).
//! See the parent's `annotated_slot_ty` arm and RFC 20260821 §5.10.

use std::collections::HashMap;

use crate::ast::{Ast, Expr, ExprId, Stmt};

/// True for the `<string>.split(<string>)` shape that `lower_split`
/// lowers through the `(Str, Str)` kernel — whose product is an array
/// of Substr views. Mirrors that dispatcher's admit: a string-typed
/// receiver, a first argument the checker typed `string` (so never an
/// `any` slot, a RegExp, a scalar, or a missing separator — those
/// route to the Any-product or fresh-Str kernels). Anything the
/// dispatcher would NOT send to the view kernel answers false here,
/// so the slot keeps the annotation's layout for it.
pub(super) fn init_is_static_string_split(
    init: ExprId,
    ast: &Ast,
    expr_types: &HashMap<ExprId, crate::check::Type>,
) -> bool {
    let Expr::Call { callee, args } = ast.get_expr(init) else {
        return false;
    };
    let Expr::Member { obj, name } = ast.get_expr(*callee) else {
        return false;
    };
    if name != "split" {
        return false;
    }
    let Some(&sep) = args.first() else {
        return false;
    };
    matches!(expr_types.get(obj), Some(crate::check::Type::String))
        && matches!(expr_types.get(&sep), Some(crate::check::Type::String))
}

/// True when any statement in the program writes THROUGH the binding:
/// a mutator method call on it (`X.push(v)` / `unshift` / `splice` /
/// `sort` / `reverse` / `fill` / `copyWithin` / `pop` / `shift`) or an
/// assignment to one of its elements or to its `length`. An
/// `Arr<Substr>` slot cannot receive an owned Str (a view cannot be
/// minted from a fresh string), so a split product that is written
/// through keeps the annotation's `Arr<Str>` layout — the same
/// decision the `let` lane's `(Str, Substr)` push arm encodes from the
/// other side. Bias: any shape this walk does not recognize as
/// definitely-not-a-write counts as a write (a false "written" only
/// keeps today's layout; a false "not written" would store an owned
/// cell into a view-typed slot).
pub(super) fn binding_written_through(ast: &Ast, name: &str) -> bool {
    fn expr_writes(ast: &Ast, eid: ExprId, x: &str) -> bool {
        let is_x = |e: ExprId| matches!(ast.get_expr(e), Expr::Ident(n) if n == x);
        match ast.get_expr(eid) {
            Expr::Call { callee, args } => {
                if let Expr::Member { obj, name } = ast.get_expr(*callee)
                    && is_x(*obj)
                    && matches!(
                        name.as_str(),
                        "push"
                            | "unshift"
                            | "splice"
                            | "sort"
                            | "reverse"
                            | "fill"
                            | "copyWithin"
                            | "pop"
                            | "shift"
                    )
                {
                    return true;
                }
                expr_writes(ast, *callee, x) || args.iter().any(|a| expr_writes(ast, *a, x))
            }
            Expr::Assign { target, value } => {
                let target_is_through_x = match ast.get_expr(*target) {
                    Expr::Index { obj, .. } => is_x(*obj),
                    Expr::Member { obj, .. } => is_x(*obj),
                    _ => false,
                };
                target_is_through_x || expr_writes(ast, *target, x) || expr_writes(ast, *value, x)
            }
            Expr::PostIncr { target, .. } => {
                matches!(ast.get_expr(*target), Expr::Index { obj, .. } if is_x(*obj))
                    || expr_writes(ast, *target, x)
            }
            // Anything that can hide a write we have not modelled
            // counts: a lifted closure that captured the binding, or
            // an arrow whose body is not walked here.
            Expr::Closure { captures, .. } => captures.iter().any(|c| c == x),
            Expr::ArrowFn { .. } => true,
            // Pure recursion over every other variant — the same
            // catalogue `ast::escape_analyze::eal_expr_safe` walks.
            Expr::Member { obj, .. } => expr_writes(ast, *obj, x),
            Expr::Index { obj, index } => expr_writes(ast, *obj, x) || expr_writes(ast, *index, x),
            Expr::BinOp { left, right, .. } | Expr::Sequence { left, right } => {
                expr_writes(ast, *left, x) || expr_writes(ast, *right, x)
            }
            Expr::Nullish { lhs, rhs } => expr_writes(ast, *lhs, x) || expr_writes(ast, *rhs, x),
            Expr::Unary { expr, .. }
            | Expr::TypeOf { expr }
            | Expr::Delete { expr }
            | Expr::As { expr, .. }
            | Expr::Spread { expr } => expr_writes(ast, *expr, x),
            Expr::InstanceOf { expr, rhs } => {
                expr_writes(ast, *expr, x) || expr_writes(ast, *rhs, x)
            }
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                expr_writes(ast, *cond, x)
                    || expr_writes(ast, *then_branch, x)
                    || expr_writes(ast, *else_branch, x)
            }
            Expr::Array(els) => els.iter().any(|e| expr_writes(ast, *e, x)),
            Expr::ObjectLit { fields } => fields.iter().any(|(_, e)| expr_writes(ast, *e, x)),
            Expr::OptChain { obj, .. } => expr_writes(ast, *obj, x),
            Expr::OptIndex { obj, index } => {
                expr_writes(ast, *obj, x) || expr_writes(ast, *index, x)
            }
            Expr::OptCall { callee, args } => {
                expr_writes(ast, *callee, x) || args.iter().any(|a| expr_writes(ast, *a, x))
            }
            Expr::New { args, .. } | Expr::Super { args } => {
                args.iter().any(|a| expr_writes(ast, *a, x))
            }
            Expr::NewDynamic { callee, args } => {
                expr_writes(ast, *callee, x) || args.iter().any(|a| expr_writes(ast, *a, x))
            }
            Expr::Ident(_)
            | Expr::Elision
            | Expr::This
            | Expr::NewTarget
            | Expr::Number(_)
            | Expr::BigInt { .. }
            | Expr::String(_)
            | Expr::Bool(_)
            | Expr::Null
            | Expr::Uninit
            | Expr::Regex { .. } => false,
        }
    }
    fn stmt_writes(ast: &Ast, s: &Stmt, x: &str) -> bool {
        match s {
            Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Yield(e) | Stmt::YieldInto { value: e, .. } => {
                expr_writes(ast, *e, x)
            }
            Stmt::Return(e) => e.is_some_and(|e| expr_writes(ast, e, x)),
            Stmt::Break(_) | Stmt::Continue(_) => false,
            Stmt::LetDecl { init, .. } | Stmt::UsingDecl { init, .. } => expr_writes(ast, *init, x),
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                expr_writes(ast, *cond, x)
                    || stmt_writes(ast, then_branch, x)
                    || else_branch
                        .as_deref()
                        .is_some_and(|e| stmt_writes(ast, e, x))
            }
            Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
                expr_writes(ast, *cond, x) || stmt_writes(ast, body, x)
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                init.as_deref().is_some_and(|i| stmt_writes(ast, i, x))
                    || cond.is_some_and(|c| expr_writes(ast, c, x))
                    || step.is_some_and(|st| expr_writes(ast, st, x))
                    || stmt_writes(ast, body, x)
            }
            Stmt::ForOfSplitIter {
                parent, sep, body, ..
            } => {
                expr_writes(ast, *parent, x)
                    || expr_writes(ast, *sep, x)
                    || stmt_writes(ast, body, x)
            }
            Stmt::ForOf {
                elem_expr, body, ..
            } => expr_writes(ast, *elem_expr, x) || stmt_writes(ast, body, x),
            Stmt::Switch {
                scrutinee,
                cases,
                default,
            } => {
                expr_writes(ast, *scrutinee, x)
                    || cases.iter().any(|c| {
                        expr_writes(ast, c.value, x)
                            || c.body.iter().any(|s| stmt_writes(ast, s, x))
                    })
                    || default
                        .as_ref()
                        .is_some_and(|db| db.iter().any(|s| stmt_writes(ast, s, x)))
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                body.iter().any(|s| stmt_writes(ast, s, x))
                    || catch_body.iter().any(|s| stmt_writes(ast, s, x))
                    || finally_body
                        .as_ref()
                        .is_some_and(|fb| fb.iter().any(|s| stmt_writes(ast, s, x)))
            }
            Stmt::Block(stmts) | Stmt::Multi(stmts) => stmts.iter().any(|s| stmt_writes(ast, s, x)),
            Stmt::Labeled { body, .. } => stmt_writes(ast, body, x),
            // A named fn body CAN reach a promoted top-level binding
            // (that is why it was promoted); walk it. Class methods
            // likewise.
            Stmt::FnDecl { body, .. } => body.iter().any(|s| stmt_writes(ast, s, x)),
            Stmt::ClassDecl { methods, .. } => methods
                .iter()
                .any(|m| m.body.iter().any(|s| stmt_writes(ast, s, x))),
            Stmt::ExportDecl { inner, .. } => {
                inner.as_deref().is_some_and(|s| stmt_writes(ast, s, x))
            }
            Stmt::TypeDecl { .. } | Stmt::ImportDecl { .. } => false,
        }
    }
    ast.stmts.iter().any(|s| stmt_writes(ast, s, name))
}
