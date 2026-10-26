//! Where each expression's value goes: into which expression, binding,
//! return or nowhere, and what an expression does with its child's
//! value (`super::samename`).

use std::collections::HashMap;

use crate::ast::desugar_with::walk::{expr_node_children, stmt_children_ref, stmt_exprs};
use crate::ast::{Ast, BinOp, Expr, ExprId, Stmt};
use crate::check::Type;

/// Where an expression's value goes.
pub(super) enum Place {
    /// into another expression
    Expr(ExprId),
    /// into a binding or a parameter written with this annotation
    Bind(Option<String>),
    /// out of a function declared to return this
    Ret(Option<String>),
    /// nowhere: a statement's value, a condition
    Drop,
    /// somewhere else (a throw, an iteration)
    Other,
}

/// The place of every expression the statements of `ast` reach.
pub(super) fn places(ast: &Ast) -> HashMap<ExprId, Place> {
    let mut out = HashMap::new();
    for s in &ast.stmts {
        walk(ast, s, None, &mut out);
    }
    out
}

fn walk(ast: &Ast, s: &Stmt, ret: Option<&String>, out: &mut HashMap<ExprId, Place>) {
    if let Stmt::FnDecl {
        params,
        return_type,
        body,
        ..
    } = s
    {
        for p in params {
            if let Some(d) = p.default {
                root(ast, d, Place::Bind(p.type_ann.clone()), out);
            }
        }
        for s in body {
            walk(ast, s, return_type.as_ref(), out);
        }
        return;
    }
    for e in stmt_exprs(s) {
        let place = match s {
            Stmt::Expr(_) | Stmt::If { .. } | Stmt::While { .. } | Stmt::DoWhile { .. } => {
                Place::Drop
            }
            Stmt::For { .. } | Stmt::Switch { .. } => Place::Drop,
            Stmt::LetDecl { type_ann, .. } => Place::Bind(type_ann.clone()),
            Stmt::Return(_) => Place::Ret(ret.cloned()),
            _ => Place::Other,
        };
        root(ast, e, place, out);
    }
    for c in stmt_children_ref(s) {
        walk(ast, c, ret, out);
    }
}

fn root(ast: &Ast, e: ExprId, place: Place, out: &mut HashMap<ExprId, Place>) {
    out.insert(e, place);
    let mut todo = vec![e];
    while let Some(p) = todo.pop() {
        for c in expr_node_children(ast.get_expr(p)) {
            out.insert(c, Place::Expr(p));
            todo.push(c);
        }
    }
}

/// What the expression `p` does with its child `c`.
pub(super) enum Into<'a> {
    /// reads it, compares it, or calls it
    Read,
    /// hands it to a slot of this type, or of a type it cannot read
    Slot(Option<&'a Type>),
    /// converts it to the type this annotation spells
    Ann(&'a str),
    /// its own value is `c`'s
    Through,
    /// hands it to a method of a builtin value typed so
    Builtin(&'a Type),
    /// somewhere it cannot read
    Unknown,
}

pub(super) fn into<'a>(
    ast: &'a Ast,
    types: &'a HashMap<ExprId, Type>,
    p: ExprId,
    c: ExprId,
) -> Into<'a> {
    match ast.get_expr(p) {
        Expr::Member { .. }
        | Expr::OptChain { .. }
        | Expr::Index { .. }
        | Expr::OptIndex { .. }
        | Expr::Unary { .. }
        | Expr::TypeOf { .. }
        | Expr::Delete { .. }
        | Expr::InstanceOf { .. } => Into::Read,
        Expr::BinOp {
            op: BinOp::LAnd | BinOp::LOr,
            ..
        } => Into::Through,
        Expr::BinOp { .. } => Into::Read,
        Expr::Ternary { cond, .. } | Expr::Sequence { left: cond, .. } if *cond == c => Into::Read,
        Expr::Ternary { .. } | Expr::Sequence { .. } | Expr::Nullish { .. } => Into::Through,
        Expr::Assign { target, .. } if *target == c => Into::Read,
        // an assignment is typed as its target, a literal by its members
        Expr::Assign { .. } | Expr::ObjectLit { .. } => Into::Slot(types.get(&p)),
        Expr::Array(_) => Into::Slot(match types.get(&p) {
            Some(Type::Array(e)) => Some(e),
            t => t,
        }),
        Expr::As { ty_ann, .. } => Into::Ann(ty_ann),
        Expr::Call { callee, args } | Expr::OptCall { callee, args } => {
            into_call(ast, types, *callee, args, c)
        }
        _ => Into::Unknown,
    }
}

/// What a call of `callee` with `args` does with its child `c`.
fn into_call<'a>(
    ast: &'a Ast,
    types: &'a HashMap<ExprId, Type>,
    callee: ExprId,
    args: &[ExprId],
    c: ExprId,
) -> Into<'a> {
    if callee == c {
        return Into::Read;
    }
    let (Some(Type::Function(ps, _)), Some(i)) =
        (types.get(&callee), args.iter().position(|a| *a == c))
    else {
        return match ast.get_expr(callee) {
            Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => match types.get(obj) {
                Some(t) if builtin(t) => Into::Builtin(t),
                _ => Into::Unknown,
            },
            _ => Into::Unknown,
        };
    };
    match ps
        .get(i)
        .or_else(|| ps.last().filter(|t| matches!(t, Type::Rest(_))))
    {
        Some(Type::Rest(t)) => Into::Slot(Some(t)),
        Some(t) => Into::Slot(Some(t)),
        None => Into::Unknown,
    }
}

/// Whether a value typed `t` is a builtin one, whose methods are the
/// runtime's: what one is handed reaches a slot its own type spells.
fn builtin(t: &Type) -> bool {
    !matches!(
        t,
        Type::ClassRef(_)
            | Type::Struct(_)
            | Type::Any
            | Type::TypeVar(_)
            | Type::Union(_)
            | Type::Nullable(_)
            | Type::Function(..)
    )
}
