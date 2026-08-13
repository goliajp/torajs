//! The shapes of the tree the `with` desugar walks: which statements
//! hold which children, which expressions a statement roots, and where
//! a nested function body hides.
//!
//! Written here rather than borrowed from a sibling pass because no
//! shared walk exists — the passes that traverse the AST each carry
//! their own arm set, tuned to what they are looking for. What this
//! one is looking for is every place a `with` marker block or a free
//! identifier can sit, which is *both* halves of the tree: statements
//! and the statement lists that hang off arena expressions.

use crate::ast::{Ast, Expr, ExprId, Stmt};

/// The `&mut Stmt` children the depth-first rewrite descends into,
/// so an inner `with` is finished before the outer one starts.
pub(crate) fn stmt_children(s: &mut Stmt) -> Vec<&mut Stmt> {
    match s {
        Stmt::Block(v) | Stmt::Multi(v) => v.iter_mut().collect(),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            let mut v: Vec<&mut Stmt> = vec![then_branch.as_mut()];
            if let Some(e) = else_branch.as_mut() {
                v.push(e.as_mut());
            }
            v
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::Labeled { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::ForOfSplitIter { body, .. } => vec![body.as_mut()],
        Stmt::For { init, body, .. } => {
            let mut v: Vec<&mut Stmt> = Vec::new();
            if let Some(i) = init.as_mut() {
                v.push(i.as_mut());
            }
            v.push(body.as_mut());
            v
        }
        Stmt::Switch { cases, default, .. } => {
            let mut v: Vec<&mut Stmt> = Vec::new();
            for c in cases.iter_mut() {
                v.extend(c.body.iter_mut());
            }
            if let Some(d) = default.as_mut() {
                v.extend(d.iter_mut());
            }
            v
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            let mut v: Vec<&mut Stmt> = body.iter_mut().collect();
            v.extend(catch_body.iter_mut());
            if let Some(f) = finally_body.as_mut() {
                v.extend(f.iter_mut());
            }
            v
        }
        Stmt::FnDecl { body, .. } => body.iter_mut().collect(),
        _ => Vec::new(),
    }
}

/// Read-only twin of [`stmt_children`] — the collect walk needs the
/// same shape without the mutable borrow. It deliberately stops at a
/// `FnDecl` body: a nested function is its own scope, entered on
/// purpose by the caller rather than fallen into.
pub(crate) fn stmt_children_ref(s: &Stmt) -> Vec<&Stmt> {
    match s {
        Stmt::Block(v) | Stmt::Multi(v) => v.iter().collect(),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            let mut v: Vec<&Stmt> = vec![then_branch.as_ref()];
            if let Some(e) = else_branch.as_ref() {
                v.push(e.as_ref());
            }
            v
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::Labeled { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::ForOfSplitIter { body, .. } => vec![body.as_ref()],
        Stmt::For { init, body, .. } => {
            let mut v: Vec<&Stmt> = Vec::new();
            if let Some(i) = init.as_ref() {
                v.push(i.as_ref());
            }
            v.push(body.as_ref());
            v
        }
        Stmt::Switch { cases, default, .. } => {
            let mut v: Vec<&Stmt> = Vec::new();
            for c in cases {
                v.extend(c.body.iter());
            }
            if let Some(d) = default.as_ref() {
                v.extend(d.iter());
            }
            v
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            let mut v: Vec<&Stmt> = body.iter().collect();
            v.extend(catch_body.iter());
            if let Some(f) = finally_body.as_ref() {
                v.extend(f.iter());
            }
            v
        }
        _ => Vec::new(),
    }
}

/// The expression roots this statement owns — not its children's.
/// Paired with [`stmt_children_ref`] it covers every expression in a
/// statement tree exactly once.
pub(crate) fn stmt_exprs(s: &Stmt) -> Vec<ExprId> {
    match s {
        Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Yield(e) => vec![*e],
        Stmt::LetDecl { init, .. } | Stmt::UsingDecl { init, .. } => vec![*init],
        Stmt::YieldInto { value, .. } => vec![*value],
        Stmt::Return(o) => o.iter().copied().collect(),
        Stmt::If { cond, .. } | Stmt::While { cond, .. } | Stmt::DoWhile { cond, .. } => {
            vec![*cond]
        }
        Stmt::Switch {
            scrutinee, cases, ..
        } => {
            let mut v = vec![*scrutinee];
            v.extend(cases.iter().map(|c| c.value));
            v
        }
        Stmt::For { cond, step, .. } => cond.iter().chain(step.iter()).copied().collect(),
        Stmt::ForOf { elem_expr, .. } => vec![*elem_expr],
        Stmt::ForOfSplitIter { parent, sep, .. } => vec![*parent, *sep],
        _ => Vec::new(),
    }
}

/// Every child ExprId of one node. An `ArrowFn`'s body is NOT a child
/// here — it is a statement list, reached through [`arrow_nodes`].
pub(crate) fn expr_children(ast: &Ast, eid: ExprId) -> Vec<ExprId> {
    match ast.get_expr(eid) {
        Expr::BinOp { left, right, .. }
        | Expr::Sequence { left, right }
        | Expr::Index {
            obj: left,
            index: right,
        }
        | Expr::OptIndex {
            obj: left,
            index: right,
        }
        | Expr::Nullish {
            lhs: left,
            rhs: right,
        }
        | Expr::InstanceOf {
            expr: left,
            rhs: right,
        }
        | Expr::Assign {
            target: left,
            value: right,
        } => vec![*left, *right],
        Expr::Unary { expr, .. }
        | Expr::As { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Delete { expr }
        | Expr::Spread { expr } => vec![*expr],
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => vec![*obj],
        Expr::PostIncr { target, .. } => vec![*target],
        Expr::Call { callee, args } | Expr::OptCall { callee, args } => {
            let mut v = vec![*callee];
            v.extend(args.iter().copied());
            v
        }
        Expr::NewDynamic { callee, args, .. } => {
            let mut v = vec![*callee];
            v.extend(args.iter().copied());
            v
        }
        Expr::New { args, .. } | Expr::Super { args, .. } => args.clone(),
        Expr::Array(items) => items.clone(),
        Expr::ObjectLit { fields } => fields.iter().map(|(_, e)| *e).collect(),
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => vec![*cond, *then_branch, *else_branch],
        _ => Vec::new(),
    }
}

/// The `ArrowFn` nodes rooted at this statement's own expressions —
/// which is where a function EXPRESSION's body lives (the parser gives
/// `function () { … }` and `() => …` the same node).
///
/// [`stmt_children`] cannot reach those bodies: they hang off an arena
/// expression, not off a statement. 刀 1 walked statements only, so a
/// `with` written inside a function expression was never rewritten at
/// all and its free names resolved lexically — `var f = function () {
/// with (o) { x } }` answered the outer `x` where bun answers `o.x`.
/// Silently, which is the one outcome this pass is not allowed to
/// produce.
///
/// The walk stops at each arrow rather than entering it: the caller
/// re-enters the statement rewrite on the body, and that finds any
/// arrow nested deeper on its own.
pub(crate) fn arrow_nodes(ast: &Ast, s: &Stmt) -> Vec<ExprId> {
    let mut out = Vec::new();
    for root in stmt_exprs(s) {
        collect_arrows(ast, root, &mut out);
    }
    out
}

fn collect_arrows(ast: &Ast, eid: ExprId, out: &mut Vec<ExprId>) {
    if matches!(ast.get_expr(eid), Expr::ArrowFn { .. }) {
        out.push(eid);
        return;
    }
    for c in expr_children(ast, eid) {
        collect_arrows(ast, c, out);
    }
}
