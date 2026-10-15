//! A name declared by `let` / `const` in sibling blocks, one of whose
//! bindings holds an unclaimed function expression.
//!
//! [`super::held::scoped_twice`] keeps such a name out of the body's
//! tracked bindings: a use cannot be told apart by name alone. But each
//! of those bindings is visible only in the statement list that
//! declares it (§14.2.2: a Block's lexical declarations bind in its own
//! environment), so the binding that holds the function expression is
//! judged there, in its own list, as if it were the body's only
//! binding of the name. `{ const g = f; g() } { const g = 1 }` in a
//! class method is refused at `g()`; it used to run `f` with the
//! method's receiver. A nested list that declares the name again
//! (a block, a `for` head, a `for … of` binding, a `catch` parameter)
//! binds another `g` there, so the judgment skips it.

use std::collections::{HashMap, HashSet};

use super::{Held, Outer, Skip};
use crate::ast::fnexpr_this_default::unclaimed_fnexpr;
use crate::ast::fnexpr_this_names::peel_as;
use crate::ast::stmt_nested_lists::for_each_nested_list;
use crate::ast::{Ast, Expr, ExprId, Stmt};

/// The name, what its binding holds, the statements it is visible in,
/// and the nested statements where another binding of the name hides
/// it.
pub(super) type Region<'a> = (&'a str, Held, &'a [Stmt], Skip);

struct Cx<'a, 'h> {
    ast: &'a Ast,
    scoped: &'h HashSet<&'a str>,
    held: &'h HashMap<&'a str, Held>,
    own: Option<Outer>,
}

pub(super) fn regions<'a>(
    ast: &'a Ast,
    body: &'a [Stmt],
    scoped: &HashSet<&'a str>,
    held: &HashMap<&'a str, Held>,
    own: Option<Outer>,
) -> Vec<Region<'a>> {
    let mut out = Vec::new();
    if !scoped.is_empty() {
        let cx = Cx {
            ast,
            scoped,
            held,
            own,
        };
        walk(&cx, body, body, &mut out);
    }
    out
}

fn walk<'a>(cx: &Cx<'a, '_>, list: &'a [Stmt], scope: &'a [Stmt], out: &mut Vec<Region<'a>>) {
    for s in list {
        match s {
            Stmt::FnDecl { .. } => {}
            Stmt::LetDecl { name, init, .. } => {
                if let Some(k) = cx.holds(name, *init) {
                    out.push((name, k, scope, shadowed(scope, name)));
                }
            }
            // generated statements in the surrounding scope
            Stmt::Multi(v) => walk(cx, v, scope, out),
            // `for (const g = f; …) body` — the head binding reaches
            // the body
            Stmt::For {
                init: Some(head), ..
            } => {
                if let Stmt::LetDecl { name, init, .. } = &**head
                    && let Some(k) = cx.holds(name, *init)
                {
                    let region = std::slice::from_ref(s);
                    out.push((name, k, region, shadowed(region, name)));
                }
                for_each_nested_list(s, &mut |inner| walk(cx, inner, inner, out));
            }
            other => for_each_nested_list(other, &mut |inner| walk(cx, inner, inner, out)),
        }
    }
}

impl Cx<'_, '_> {
    /// A `let` / `const` of a scoped name initialized with an unclaimed
    /// function expression or a binding known to hold one.
    fn holds(&self, name: &str, init: ExprId) -> Option<Held> {
        if !self.scoped.contains(name) {
            return None;
        }
        let v = peel_as(&self.ast.exprs, init);
        match self.ast.get_expr(v) {
            Expr::Ident(src) => self.held.get(src.as_str()).copied(),
            _ => self
                .own
                .filter(|_| unclaimed_fnexpr(self.ast, v).is_some())
                .map(|k| (k, true)),
        }
    }
}

/// The statements under `region` (its own list excluded) that sit in
/// the scope of another binding of `name`.
fn shadowed(region: &[Stmt], name: &str) -> Skip {
    let mut out = Skip::new();
    for s in region {
        hide_under(s, name, &mut out);
    }
    out
}

fn hide_under(s: &Stmt, name: &str, out: &mut Skip) {
    match s {
        Stmt::FnDecl { .. } => {}
        Stmt::For {
            init: Some(head), ..
        } if matches!(&**head, Stmt::LetDecl { name: n, .. } if n == name) => {
            out.insert(s as *const Stmt);
        }
        Stmt::ForOf { var_name, .. } if var_name == name => {
            out.insert(s as *const Stmt);
        }
        Stmt::Try {
            body,
            catch_param,
            catch_body,
            finally_body,
            ..
        } => {
            hide_list(body, name, out);
            if catch_param.as_deref() == Some(name) {
                out.extend(catch_body.iter().map(|c| c as *const Stmt));
            } else {
                hide_list(catch_body, name, out);
            }
            if let Some(f) = finally_body {
                hide_list(f, name, out);
            }
        }
        // generated statements of the surrounding scope
        Stmt::Multi(v) => v.iter().for_each(|c| hide_under(c, name, out)),
        other => for_each_nested_list(other, &mut |inner| hide_list(inner, name, out)),
    }
}

fn hide_list(list: &[Stmt], name: &str, out: &mut Skip) {
    if declares(list, name) {
        out.extend(list.iter().map(|c| c as *const Stmt));
    } else {
        list.iter().for_each(|c| hide_under(c, name, out));
    }
}

/// `list` declares `name` lexically at its own level.
fn declares(list: &[Stmt], name: &str) -> bool {
    list.iter().any(|s| match s {
        Stmt::LetDecl { name: n, .. } => n == name,
        Stmt::Multi(v) => declares(v, name),
        _ => false,
    })
}
