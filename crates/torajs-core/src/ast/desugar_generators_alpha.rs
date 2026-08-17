//! J.2.b — two `let`s of the same name in different scopes of one
//! generator body.
//!
//! Every `let` in a generator body lifts to a field of the `__Gen_*`
//! class, and fields live in one flat namespace, so `try { } catch (e)
//! { const ee = e; }` written twice mapped both `ee`s onto `this.ee`.
//! The desugar refused to guess and aborted: "duplicate `let ee`
//! declarations across scopes — rename one". That is a shape an
//! ordinary program reaches without trying (the test262 async harness
//! port does it twice in one function), and under RFC 20260805 D1 —
//! where an `async function` becomes a generator — every async body
//! inherits the restriction.
//!
//! The second declaration is renamed instead. A `let` is visible from
//! its own declaration to the end of the enclosing statement list, so
//! renaming across exactly that range renames exactly its own
//! references: a read before an inner redeclaration is a
//! temporal-dead-zone error and one after it resolves to the inner
//! binding, which is why a statement list that redeclares the name
//! can hold no reference to the outer one at all.
//!
//! `rebinds_name` finds such an inner redeclaration and declines,
//! leaving the original abort in place — loud, not silently wrong.
//! Its reach is the statement structure; an arrow is left to the
//! rename walk's own `nested_fns_idents::arrow_rebinds`, which is the
//! same depth `desugar_generators_rewrite` relies on when it descends
//! into arrow bodies to answer the identical question.

use super::{Ast, Stmt};
use std::collections::HashMap;

/// Rename the `let` declared at `scope[0]` if a lifted field already
/// claims its name, so both survive as distinct fields.
///
/// `scope` is the declaration's own visibility range: the enclosing
/// statement list from the declaration onward. `taken` is what the
/// lift has claimed so far in this generator body.
///
/// A `Stmt::Multi` at `scope[0]` counts as a declaration in this same
/// range, not a nested scope of its own — it is the wrapper the
/// yield-into expansion puts around a `[yield e; let v = this.__sent]`
/// pair, and the `let` inside it belongs to the surrounding list.
pub(super) fn resolve_duplicate_let(ast: &mut Ast, scope: &mut [Stmt], taken: &[(String, String)]) {
    let Some(name) = scope.first().and_then(declared_name) else {
        return;
    };
    if !taken.iter().any(|(n, _)| *n == name) || rebinds_name(&scope[1..], &name) {
        return;
    }
    let fresh = mint(&name, taken);
    rewrite_range(ast, scope, &name, &fresh);
    rename_declaration(&mut scope[0], &name, &fresh);
}

/// The `for (let i = 0; ..)` counterpart. The binding's range is the
/// whole `for` statement — init, test, step and body — rather than a
/// slice of the enclosing list, so it is renamed as one. Two
/// sequential counting loops in one generator body is the shape that
/// reaches this.
pub(super) fn resolve_duplicate_for_let(ast: &mut Ast, s: &mut Stmt, taken: &[(String, String)]) {
    let Stmt::For {
        init: Some(i),
        body,
        ..
    } = s
    else {
        return;
    };
    let Some(name) = declared_name(i) else {
        return;
    };
    if !taken.iter().any(|(n, _)| *n == name) || rebinds_name(std::slice::from_ref(body), &name) {
        return;
    }
    let fresh = mint(&name, taken);
    rewrite_range(ast, std::slice::from_mut(s), &name, &fresh);
    if let Stmt::For { init: Some(i), .. } = s {
        rename_declaration(i, &name, &fresh);
    }
}

fn rewrite_range(ast: &mut Ast, range: &mut [Stmt], from: &str, to: &str) {
    let mut map: HashMap<String, String> = HashMap::new();
    map.insert(from.to_string(), to.to_string());
    super::nested_fns_idents::rewrite_idents_in_body(ast, range, &map, true);
}

/// The name a statement declares into its enclosing list, if any.
fn declared_name(s: &Stmt) -> Option<String> {
    match s {
        Stmt::LetDecl { name, .. } => Some(name.clone()),
        Stmt::Multi(inner) => inner.iter().find_map(declared_name),
        _ => None,
    }
}

/// Point a declaration at its new name. The rename walk rewrote every
/// *reference*; a declaration carries its name in a field the walk
/// does not touch.
fn rename_declaration(s: &mut Stmt, from: &str, to: &str) {
    match s {
        Stmt::LetDecl { name, .. } if name == from => *name = to.to_string(),
        Stmt::Multi(inner) => {
            for st in inner {
                rename_declaration(st, from, to);
            }
        }
        _ => {}
    }
}

/// A field name no lifted local has claimed.
fn mint(name: &str, taken: &[(String, String)]) -> String {
    let mut k = 2;
    loop {
        let candidate = format!("{name}__j2b{k}");
        if !taken.iter().any(|(n, _)| *n == candidate) {
            return candidate;
        }
        k += 1;
    }
}

/// True when some scope inside `stmts` binds `name` again.
fn rebinds_name(stmts: &[Stmt], name: &str) -> bool {
    stmts.iter().any(|s| rebinds_in_stmt(s, name))
}

pub(crate) fn rebinds_in_stmt(s: &Stmt, name: &str) -> bool {
    match s {
        Stmt::LetDecl { name: n, .. } => n == name,
        Stmt::FnDecl {
            name: n,
            params,
            body,
            ..
        } => n == name || params.iter().any(|p| p.name == name) || rebinds_name(body, name),
        Stmt::ForOf {
            var_name,
            i_ident,
            body,
            ..
        } => var_name == name || i_ident == name || rebinds_in_stmt(body, name),
        Stmt::ForOfSplitIter { var_name, body, .. } => {
            var_name == name || rebinds_in_stmt(body, name)
        }
        Stmt::YieldInto { var, .. } => var == name,
        Stmt::Try {
            body,
            catch_param,
            catch_body,
            finally_body,
            ..
        } => {
            catch_param.as_deref() == Some(name)
                || rebinds_name(body, name)
                || rebinds_name(catch_body, name)
                || finally_body
                    .as_deref()
                    .is_some_and(|fb| rebinds_name(fb, name))
        }
        Stmt::For { init, body, .. } => {
            init.as_deref().is_some_and(|i| rebinds_in_stmt(i, name)) || rebinds_in_stmt(body, name)
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            rebinds_in_stmt(then_branch, name)
                || else_branch
                    .as_deref()
                    .is_some_and(|eb| rebinds_in_stmt(eb, name))
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => rebinds_in_stmt(body, name),
        Stmt::Labeled { body, .. } => rebinds_in_stmt(body, name),
        Stmt::Block(inner) | Stmt::Multi(inner) => rebinds_name(inner, name),
        Stmt::Switch { cases, default, .. } => {
            cases.iter().any(|c| rebinds_name(&c.body, name))
                || default.as_deref().is_some_and(|d| rebinds_name(d, name))
        }
        // Knife-D preface: a class rebinds its own name, and its
        // ctor / method / static-block scopes rebind through params
        // and body decls like any fn. This arm was `_ => false`, so
        // both callers were blind to class shadows — the deconflict
        // census would blind-rewrite a reference a method's local
        // `let` actually owns. Fields don't bind names into any
        // statement scope; a heritage is an expression (never a
        // binder).
        Stmt::ClassDecl {
            name: n,
            ctor,
            methods,
            static_methods,
            static_init,
            ..
        } => {
            n == name
                || ctor.as_ref().is_some_and(|c| {
                    c.params.iter().any(|p| p.name == name) || rebinds_name(&c.body, name)
                })
                || methods
                    .iter()
                    .chain(static_methods.iter())
                    .any(|m| m.params.iter().any(|p| p.name == name) || rebinds_name(&m.body, name))
                || static_init.iter().any(|si| match si {
                    super::StaticInit::Block(v) => rebinds_name(v, name),
                    super::StaticInit::Field(_) => false,
                })
        }
        _ => false,
    }
}
