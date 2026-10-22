//! The handler half of the receiver reading: whether a handler takes
//! its value on a typed lane, and the source annotation read lazily.

use super::super::{Ast, Stmt};

/// Does the function `name` take its first user parameter on a typed
/// lane: an annotation written on it, other than `any`, `unknown`, a
/// union or a nullable (which take every value on the any lane)?
pub(super) fn typed_lane_handler(ast: &Ast, name: &str) -> bool {
    let Some(params) = ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl {
            name: n, params, ..
        } if n == name => Some(params),
        _ => None,
    }) else {
        return false;
    };
    let user = params.iter().find(|p| p.name != "__env");
    user.and_then(|p| p.type_ann.as_deref()).is_some_and(|a| {
        !matches!(a, "any" | "unknown") && !a.contains('|') && !a.starts_with("__nullable(")
    })
}

/// The source's value annotation, read at most once.
pub(super) enum Lazy<F> {
    Ready(Option<String>),
    Later(F),
}

impl<F: FnOnce() -> Option<String>> Lazy<F> {
    pub(super) fn get(self) -> Option<String> {
        match self {
            Lazy::Ready(s) => s,
            Lazy::Later(f) => f(),
        }
    }
}
