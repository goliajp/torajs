//! Collect the capture lists of every lifted `Expr::Closure` nested
//! anywhere inside one expression tree.
//!
//! The forward-binding hoists (checker `check_hoist_closure_lets`,
//! lowering `hoist_forward_boxes`) used to look only at let-inits
//! that ARE a closure, so a closure minted deeper in the init —
//! an object-literal method, an array element, a `new` argument —
//! never registered its captures, and a binding declared later in
//! the same list stayed unresolvable ("references unknown
//! identifier" / "closure capture not in scope") even though ES
//! hoists the `let` and the body only runs after the declaration:
//!
//! ```text
//! let o = { next() { return iterator.next(); } };
//! let iterator = Iterator.concat(o);
//! ```
//!
//! Bodies are already lifted to FnDecls by the time either pass
//! runs, so a `Closure` node is a leaf here — its capture list
//! already carries the names that were free in the (transitively
//! lifted) body. A residual `ArrowFn` (pre-lift transitional shape)
//! is deliberately not entered: neither hoist pass ever saw inside
//! one, and this walker only widens where captures are ground truth.

use super::ast_def::Ast;
use super::expr::Expr;
use crate::ast::ExprId;

/// Push every nested closure's capture names (in encounter order,
/// duplicates included — callers use set semantics). Borrows the
/// names straight off the `Closure` nodes so both hoist passes can
/// key their AST-lifetime sets without cloning.
pub(crate) fn collect<'a>(ast: &'a Ast, eid: ExprId, out: &mut Vec<&'a str>) {
    match ast.get_expr(eid) {
        Expr::Closure { captures, .. } => {
            out.extend(captures.iter().map(|c| c.as_str()));
        }
        Expr::Ident(_)
        | Expr::String(_)
        | Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::Bool(_)
        | Expr::Uninit
        | Expr::Regex { .. }
        | Expr::Null
        | Expr::This
        | Expr::NewTarget
        | Expr::Elision
        | Expr::ArrowFn { .. } => {}
        Expr::BinOp { left, right, .. }
        | Expr::Sequence { left, right }
        | Expr::Nullish {
            lhs: left,
            rhs: right,
        }
        | Expr::Assign {
            target: left,
            value: right,
        }
        | Expr::Index {
            obj: left,
            index: right,
        }
        | Expr::OptIndex {
            obj: left,
            index: right,
        } => {
            collect(ast, *left, out);
            collect(ast, *right, out);
        }
        Expr::Unary { expr, .. }
        | Expr::Member { obj: expr, .. }
        | Expr::OptChain { obj: expr, .. }
        | Expr::As { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Delete { expr }
        | Expr::Spread { expr }
        | Expr::PostIncr { target: expr, .. } => collect(ast, *expr, out),
        Expr::InstanceOf { expr, rhs } => {
            collect(ast, *expr, out);
            collect(ast, *rhs, out);
        }
        Expr::Call { callee, args }
        | Expr::NewDynamic { callee, args }
        | Expr::OptCall { callee, args } => {
            collect(ast, *callee, out);
            for a in args {
                collect(ast, *a, out);
            }
        }
        Expr::New { args, .. } | Expr::Super { args } => {
            for a in args {
                collect(ast, *a, out);
            }
        }
        Expr::Array(els) => {
            for e in els {
                collect(ast, *e, out);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, e) in fields {
                collect(ast, *e, out);
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            collect(ast, *cond, out);
            collect(ast, *then_branch, out);
            collect(ast, *else_branch, out);
        }
    }
}
