//! How the lift sees one expression node: which children it evaluates,
//! in what order, and which of them only sometimes.

use crate::ast::desugar_async_sm::{await_inner, expr_has_await};
use crate::ast::{Ast, BinOp, Expr, ExprId};

/// How one node's children are evaluated.
pub(super) enum Shape {
    /// No children of ours (a nested function's awaits are its own).
    Leaf,
    /// Children in evaluation order, all unconditional.
    Ordered(Vec<ExprId>),
    /// `first` always evaluates; the rest only sometimes.
    Guarded(ExprId, Vec<ExprId>),
    /// A child position the lift never rewrites.
    Refuse(Vec<ExprId>),
}

pub(super) fn shape(ast: &Ast, eid: ExprId) -> Shape {
    match ast.get_expr(eid) {
        Expr::Ident(_)
        | Expr::String(_)
        | Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::Bool(_)
        | Expr::Uninit
        | Expr::Regex { .. }
        | Expr::Null
        | Expr::Closure { .. }
        | Expr::This
        | Expr::NewTarget
        | Expr::Elision
        | Expr::ArrowFn { .. } => Shape::Leaf,
        Expr::BinOp {
            op: BinOp::LAnd | BinOp::LOr,
            left,
            right,
        } => Shape::Guarded(*left, vec![*right]),
        Expr::Nullish { lhs, rhs } => Shape::Guarded(*lhs, vec![*rhs]),
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => Shape::Guarded(*cond, vec![*then_branch, *else_branch]),
        Expr::BinOp { left, right, .. } | Expr::Sequence { left, right } => {
            Shape::Ordered(vec![*left, *right])
        }
        Expr::Unary { expr, .. }
        | Expr::As { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::InstanceOf { expr, .. }
        | Expr::Spread { expr } => Shape::Ordered(vec![*expr]),
        Expr::Member { obj, .. } => Shape::Ordered(vec![*obj]),
        Expr::Index { obj, index } => Shape::Ordered(vec![*obj, *index]),
        // A member callee is the receiver's read — it stays with the
        // call (module doc); only the arguments are children here.
        Expr::Call { callee, args } | Expr::NewDynamic { callee, args } => {
            let mut kids = Vec::with_capacity(args.len() + 1);
            if !matches!(ast.get_expr(*callee), Expr::Member { .. }) {
                kids.push(*callee);
            }
            kids.extend(args.iter().copied());
            Shape::Ordered(kids)
        }
        Expr::New { args, .. } | Expr::Super { args } | Expr::Array(args) => {
            Shape::Ordered(args.clone())
        }
        Expr::ObjectLit { fields } => Shape::Ordered(fields.iter().map(|(_, v)| *v).collect()),
        // The target reference is evaluated before the value: a member
        // / index target's parts hoist like operands; a bare name
        // resolves to the same binding either way.
        Expr::Assign { target, value } => match ast.get_expr(*target) {
            Expr::Ident(_) => Shape::Ordered(vec![*value]),
            Expr::Member { obj, .. } => Shape::Ordered(vec![*obj, *value]),
            Expr::Index { obj, index } => Shape::Ordered(vec![*obj, *index, *value]),
            _ => Shape::Refuse(vec![*target, *value]),
        },
        Expr::OptChain { obj, .. } => Shape::Refuse(vec![*obj]),
        Expr::OptIndex { obj, index } => Shape::Refuse(vec![*obj, *index]),
        Expr::OptCall { callee, args } => {
            let mut kids = vec![*callee];
            kids.extend(args.iter().copied());
            Shape::Refuse(kids)
        }
        Expr::Delete { expr } => Shape::Refuse(vec![*expr]),
        Expr::PostIncr { target, .. } => Shape::Refuse(vec![*target]),
    }
}

/// Does every await under `eid` sit at a position the lift takes?
pub(in crate::ast) fn can_lift(ast: &Ast, eid: ExprId) -> bool {
    if let Some(inner) = await_inner(ast, eid) {
        return can_lift(ast, inner);
    }
    match shape(ast, eid) {
        Shape::Leaf => true,
        Shape::Ordered(kids) => kids.iter().all(|k| can_lift(ast, *k)),
        Shape::Guarded(first, rest) => {
            can_lift(ast, first) && rest.iter().all(|r| can_lift(ast, *r))
        }
        Shape::Refuse(kids) => kids.iter().all(|k| !expr_has_await(ast, *k)),
    }
}
