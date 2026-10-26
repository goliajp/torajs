//! The syntactic family of an init or an assign's right-hand side.

use crate::ast::{Ast, Expr, ExprId};

/// Syntactic family of an init / assign-rhs expression. `None` =
/// unclassifiable (never participates in widening).
#[derive(PartialEq, Eq, Clone)]
pub(super) enum Family {
    New(String),
    Num,
    Str,
    Bool,
    Null,
    /// The `undefined` literal (spelled `Ident("undefined")` in the
    /// arena; nothing else in the pipeline treats it as a binding).
    Undef,
    Arr,
    Obj,
    /// `Ns.method(...)` where `Ns` is a known global namespace
    /// (`Iterator.from`, `Array.of`, `Object.create`, ...): the
    /// return type is namespace-determined and virtually never the
    /// init's class.
    NsCall(String, String),
}

pub(super) fn classify(ast: &Ast, e: ExprId) -> Option<Family> {
    match ast.get_expr(strip_as(ast, e)) {
        Expr::New { class_name, .. } => Some(Family::New(class_name.clone())),
        Expr::Number(_) => Some(Family::Num),
        Expr::String(_) => Some(Family::Str),
        Expr::Bool(_) => Some(Family::Bool),
        Expr::Null => Some(Family::Null),
        Expr::Ident(n) if n == "undefined" => Some(Family::Undef),
        Expr::Array(_) => Some(Family::Arr),
        Expr::ObjectLit { .. } => Some(Family::Obj),
        Expr::Call { callee, .. } => match ast.get_expr(*callee) {
            Expr::Member { obj, name } => match ast.get_expr(*obj) {
                Expr::Ident(ns) if crate::ast::free_vars_is_global_name(ns) => {
                    Some(Family::NsCall(ns.clone(), name.clone()))
                }
                _ => None,
            },
            // The pre-pass runs on the post-desugar AST, where
            // desugar_classes has rewritten `new C(...)` into a
            // `__new_C(...)` factory call — the dominant shape.
            Expr::Ident(n) if n.starts_with("__new_") => Some(Family::New(n.clone())),
            _ => None,
        },
        _ => None,
    }
}

/// TS casts are static assertions — classify the value underneath.
pub(super) fn strip_as(ast: &Ast, mut e: ExprId) -> ExprId {
    while let Expr::As { expr, .. } = ast.get_expr(e) {
        e = *expr;
    }
    e
}
