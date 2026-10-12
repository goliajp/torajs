//! The spelling of a binding initialized from a closure FIELD of a
//! top-level object literal (`const g = src.g`).

use std::collections::HashSet;

use crate::ast::{Ast, Expr, ExprId};

use super::init_ann::{objlit_literal_inlobj_ann, unique_toplevel_binding};

/// `recv.f` where `recv` names the single top-level `let` / `const`
/// whose layout is an `__inlobj(` (written, or the one its all-literal
/// initializer registers under) and `f` is a closure-repr field of it:
/// the field's own `__cls(` spelling. The read copies the field's env
/// cell, so the binding holds the same Closure value the field does
/// and takes the same typed slot. Without this a named fn reading
/// such a binding — or a closure inside one capturing it — answered
/// "unknown identifier" and threw a ReferenceError at run time.
///
/// An alias of such a binding (`const h = g`, through a chain of
/// unannotated top-level `let` / `const` names) holds the same value
/// and spells the same. A name reached twice is a cycle and answers
/// nothing, as in `closure_alias_fn_canon`.
///
/// The literal's layout is only the binding's type while nothing
/// moved it to `any`: an init in `degraded` (a `delete`, a
/// `defineProperty`, a write the literal does not spell —
/// `crate::dynobj_degrade`) or in `widened` (a cross-type write,
/// `crate::let_widen`) is read through the any lane, where the field
/// may hold anything; spelling it `__cls(` would store a boxed value
/// into a Closure slot. Each hop's own init is held to the same test.
///
/// A METHOD field (`__mth(`) is left out: reading it as a value
/// detaches the receiver it was spelled around. A variadic field is
/// left out as its siblings leave it out: its boxed-dual routing is a
/// fn-local table.
pub fn closure_field_read_ann(
    ast: &Ast,
    init: ExprId,
    degraded: &HashSet<ExprId>,
    widened: &HashSet<ExprId>,
) -> Option<String> {
    let moved_to_any = |e: &ExprId| degraded.contains(e) || widened.contains(e);
    let mut init = init;
    let mut seen: Vec<&str> = Vec::new();
    while let Expr::Ident(n) = ast.get_expr(init) {
        if seen.contains(&n.as_str()) {
            return None;
        }
        seen.push(n);
        let (next, None) = unique_toplevel_binding(ast, n)? else {
            return None;
        };
        if moved_to_any(&next) {
            return None;
        }
        init = next;
    }
    let Expr::Member { obj, name } = ast.get_expr(init) else {
        return None;
    };
    let Expr::Ident(recv) = ast.get_expr(*obj) else {
        return None;
    };
    let (recv_init, recv_ann) = unique_toplevel_binding(ast, recv)?;
    if moved_to_any(&recv_init) {
        return None;
    }
    let layout = match recv_ann {
        Some(a) => a.to_string(),
        None => objlit_literal_inlobj_ann(ast, recv_init)?,
    };
    let body = layout.strip_prefix("__inlobj(")?.strip_suffix(')')?;
    let field = crate::check_type_ann::split_top_pipe(body)
        .into_iter()
        .find_map(|f| f.strip_prefix(name.as_str())?.strip_prefix(':'))?;
    let is_closure = field.starts_with("__cls(") || field.starts_with("__nullable(__cls(");
    (is_closure && !field.contains("__rest(")).then(|| field.to_string())
}
