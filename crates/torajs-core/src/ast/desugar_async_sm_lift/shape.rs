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
    /// The first ones always evaluate, in order; the rest only
    /// sometimes.
    Guarded(Vec<ExprId>, Vec<ExprId>),
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
        } => Shape::Guarded(vec![*left], vec![*right]),
        Expr::Nullish { lhs, rhs } => Shape::Guarded(vec![*lhs], vec![*rhs]),
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => Shape::Guarded(vec![*cond], vec![*then_branch, *else_branch]),
        Expr::BinOp { left, right, .. } | Expr::Sequence { left, right } => {
            Shape::Ordered(vec![*left, *right])
        }
        Expr::Unary { expr, .. }
        | Expr::As { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::InstanceOf { expr, .. }
        | Expr::Spread { expr } => Shape::Ordered(vec![*expr]),
        // `o?.a.b` / `o?.p[k]` / `o?.m(a)`: a nullish base is the whole
        // chain's undefined, the keys and arguments past it unevaluated
        // (§13.3.9.1)
        Expr::Member { obj, .. } => match chain_parts(ast, *obj) {
            Some((base, keys)) => Shape::Guarded(vec![base], keys),
            None => Shape::Ordered(vec![*obj]),
        },
        Expr::Index { obj, index } => match chain_parts(ast, *obj) {
            Some((base, mut keys)) => {
                keys.push(*index);
                Shape::Guarded(vec![base], keys)
            }
            None => Shape::Ordered(vec![*obj, *index]),
        },
        Expr::Call { callee, args } => match chain_parts(ast, *callee) {
            Some((base, mut keys)) => {
                keys.extend(args.iter().copied());
                Shape::Guarded(vec![base], keys)
            }
            // `(o?.a[k])(x)`: the keys past the link are guarded, the
            // arguments are not (`paren_callee`)
            None => match paren_chain_parts(ast, *callee) {
                Some((base, mut keys))
                    if keys.iter().chain(args).any(|k| expr_has_await(ast, *k)) =>
                {
                    keys.extend(args.iter().copied());
                    Shape::Guarded(vec![base], keys)
                }
                _ => call_kids(ast, *callee, args),
            },
        },
        Expr::NewDynamic { callee, args } => call_kids(ast, *callee, args),
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
            // a destructuring pattern reads its references only after
            // the value is evaluated (§13.15.2 step 2)
            Expr::Array(_) | Expr::ObjectLit { .. } if !expr_has_await(ast, *target) => {
                Shape::Ordered(vec![*value])
            }
            _ => Shape::Refuse(vec![*target, *value]),
        },
        // the base of `o?.p` is evaluated whatever it turns out to be
        Expr::OptChain { obj, .. } => Shape::Ordered(vec![*obj]),
        Expr::OptIndex { obj, index } => Shape::Guarded(vec![*obj], vec![*index]),
        // the callee's reference is evaluated, then tested (a member
        // callee keeps its receiver)
        Expr::OptCall { callee, args } => {
            let args = args.clone();
            match ast.get_expr(*callee) {
                Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
                    Shape::Guarded(vec![*obj], args)
                }
                Expr::Index { obj, index } => Shape::Guarded(vec![*obj, *index], args),
                Expr::OptIndex { obj, index } => {
                    Shape::Guarded(vec![*obj], std::iter::once(*index).chain(args).collect())
                }
                _ => Shape::Guarded(vec![*callee], args),
            }
        }
        // a member or index reference is evaluated part by part like an
        // operand; anything else is not a reference to update
        Expr::Delete { expr: r } | Expr::PostIncr { target: r, .. }
            if matches!(ast.get_expr(*r), Expr::Member { .. } | Expr::Index { .. }) =>
        {
            Shape::Ordered(vec![*r])
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
        Shape::Guarded(first, rest) => first.iter().chain(&rest).all(|k| can_lift(ast, *k)),
        Shape::Refuse(kids) => kids.iter().all(|k| !expr_has_await(ast, *k)),
    }
}

/// The optional link `eid` hangs from, through the `.name` and `[key]`
/// steps an optional chain goes on with (`o?.a[k]`): the link's base,
/// and the keys evaluated only past its test, in order. `None` when
/// there is none, or parentheses end the chain on the way (`(o?.a).b`
/// reads `.b` off the chain's value).
pub(super) fn chain_parts(ast: &Ast, eid: ExprId) -> Option<(ExprId, Vec<ExprId>)> {
    chain_walk(ast, eid, None)
}

/// [`chain_parts`] of a parenthesized chain itself (`(o?.a[k])`, a
/// call's callee): the walk starts inside its parentheses.
pub(super) fn paren_chain_parts(ast: &Ast, eid: ExprId) -> Option<(ExprId, Vec<ExprId>)> {
    if !ast.paren_optchains.contains(&eid) {
        return None;
    }
    chain_walk(ast, eid, Some(eid))
}

fn chain_walk(ast: &Ast, eid: ExprId, inside: Option<ExprId>) -> Option<(ExprId, Vec<ExprId>)> {
    let mut keys = Vec::new();
    let mut link = eid;
    let base = loop {
        if Some(link) != inside && ast.paren_optchains.contains(&link) {
            return None;
        }
        match ast.get_expr(link) {
            Expr::Member { obj, .. } => link = *obj,
            Expr::Index { obj, index } => {
                keys.push(*index);
                link = *obj;
            }
            Expr::OptChain { obj, .. } => break *obj,
            Expr::OptIndex { obj, index } => {
                keys.push(*index);
                break *obj;
            }
            _ => return None,
        }
    };
    keys.reverse();
    Some((base, keys))
}

/// A call's children: a member callee is the receiver's read, which
/// stays with the call (module doc), so only the arguments are — and
/// the receiver's own operand when it awaits (`(await p).m(x)`), which
/// a hoist keeps as the receiver; an index callee's object and key. A
/// parenthesized optional callee whose keys and arguments do not await
/// (`((await p)?.m)(x)`) is a member one too: only its base is lifted.
fn call_kids(ast: &Ast, callee: ExprId, args: &[ExprId]) -> Shape {
    let mut kids = Vec::with_capacity(args.len() + 1);
    match ast.get_expr(callee) {
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } | Expr::OptIndex { obj, .. }
            if expr_has_await(ast, *obj) =>
        {
            kids.push(*obj)
        }
        Expr::Member { .. } | Expr::OptChain { .. } | Expr::OptIndex { .. } => {}
        // hoisted part by part, the call stays an index call
        Expr::Index { obj, index } => kids.extend([*obj, *index]),
        _ => kids.push(callee),
    }
    kids.extend(args.iter().copied());
    Shape::Ordered(kids)
}
