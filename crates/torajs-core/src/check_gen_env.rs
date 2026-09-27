//! RFC 20260912 — the checker's side of a generator's by-reference
//! captures: seed the iterator method's scope with the captured names
//! and check those methods after every construction site is typed.
//!
//! A closure body is typed at its construction site, with the
//! enclosing scopes in view, and `closure_captures[fn]` records what it
//! captured and at what type. A generator's iterator method is a
//! top-level class method typed on its own; the names it reads through
//! the wrapper's env are declared here from that record, mutable — a
//! write in the body is a write to the live binding.

use crate::ast::{Ast, Stmt};
use crate::check::{Checker, DiagPush, LocalInfo, Type};

/// Declare the by-reference captures of `fn_name` (a method in
/// `gen_env_methods`) in the current scope. Any other fn: nothing.
pub(crate) fn seed_captures(checker: &mut Checker, ast: &Ast, fn_name: &str) {
    let Some(info) = ast.gen_env_methods.get(fn_name) else {
        return;
    };
    let wrapper = info.wrapper_fn_name(ast);
    let Some(caps) = checker.closure_captures.get(&wrapper).cloned() else {
        checker.errors.push_err(format!(
            "internal: generator method `{fn_name}` checked before its wrapper `{wrapper}` \
             was constructed"
        ));
        return;
    };
    for (n, ty) in caps {
        if !info.names.contains(&n) {
            continue;
        }
        if let Err(e) = checker.declare(
            n,
            LocalInfo {
                ty,
                mutable: true,
                moved: false,
                borrowed: false,
                declared_class: None,
                builtin_mv: false,
            },
        ) {
            checker.errors.push_err(e);
        }
    }
}

/// Pass 2's statement walk: every top-level statement in order, except
/// lifted closure bodies (typed at their construction sites) and
/// generic bodies (typed per instantiation). Function bodies wait for
/// the rest: a body only runs once called, and by then every top-level
/// binding it reads has its type ([`register_late_homes`]). The
/// gen-env iterator methods wait further, for the construction site of
/// their wrapper (in a user fn, main, or a closure body typed at ITS
/// site), which records the capture types they seed from.
pub(crate) fn check_stmts(c: &mut Checker, ast: &Ast) {
    let mut deferred: Vec<&Stmt> = Vec::new();
    for stmt in &ast.stmts {
        if let Stmt::FnDecl { name, .. } = stmt {
            if c.closure_fn_names.contains(name) || c.generic_type_params.contains_key(name) {
                continue;
            }
            deferred.push(stmt);
            continue;
        }
        c.check_stmt(ast, stmt);
    }
    register_late_homes(c, ast);
    check_deferred(c, ast, deferred);
}

/// A top-level binding a function body reads needs a home outside the
/// program body. The pre-pass (`check_pipeline::pass_2`) gives one to
/// the init shapes it can type without checking them; a scalar init it
/// could not (`const n = xs.length`) is typed by now, and registers
/// here under that type. The lowerer asks [`late_home_ty`] in the same
/// fallback position.
fn register_late_homes(c: &mut Checker, ast: &Ast) {
    let refs = crate::ast_refs::toplevel_binding_refs(ast);
    for stmt in crate::ast::toplevel_stmts_flat(ast) {
        if let Stmt::LetDecl {
            name,
            init,
            type_ann: None,
            is_var: false,
            ..
        } = stmt
            && refs.named_fn_refs.contains(name)
            && !c.globals.contains_key(name)
            && let Some(ty) = c.expr_types.get(init).filter(|t| late_home_ty(t))
        {
            c.globals.insert(name.clone(), ty.clone());
        }
    }
}

/// The init types a late home takes: the scalars, whose global slot
/// needs no ownership story at the init store.
pub(crate) fn late_home_ty(ty: &Type) -> bool {
    matches!(ty, Type::Number | Type::Boolean)
}

/// Check the deferred gen-env methods once their wrappers' construction
/// sites are typed. A wrapper constructed inside another such method's
/// body is typed only when that method is — so this drains in rounds,
/// each round taking the methods whose wrapper is recorded; whatever
/// is left after a round with no progress is checked as is (its own
/// seeding reports the missing wrapper).
pub(crate) fn check_deferred(checker: &mut Checker, ast: &Ast, mut pending: Vec<&Stmt>) {
    while !pending.is_empty() {
        let (ready, rest): (Vec<&Stmt>, Vec<&Stmt>) = pending.into_iter().partition(|s| {
            let Stmt::FnDecl { name, .. } = s else {
                return true;
            };
            ast.gen_env_methods.get(name).is_none_or(|info| {
                checker
                    .closure_captures
                    .contains_key(&info.wrapper_fn_name(ast))
            })
        });
        let stuck = ready.is_empty();
        let batch = if stuck { rest.clone() } else { ready };
        for s in batch {
            checker.check_stmt(ast, s);
        }
        pending = if stuck { Vec::new() } else { rest };
    }
}
