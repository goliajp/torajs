//! `Stmt::FnDecl { name, params, body, .. }` typecheck pulled out
//! of [`crate::check::Checker::check_stmt`]'s `Stmt::FnDecl` arm
//! as chunk-107 of the check_stmt decomp. 10th check_stmt sibling.
//!
//! Signature was already hoisted in pass 0; this walks the body:
//!
//! 1. Resolve signature via `globals[name]`. If missing (first-pass
//!    error), skip body to avoid cascading.
//! 2. Replace scope stack with fresh `[HashMap]` — top-level
//!    FnDecls see no outer locals (mirrors arrow-fn no-capture
//!    rule); globals still reach via lookup-fallback.
//! 3. Replace `expected_return` with the fn's return type.
//! 4. **M-OO.5 class context** — fn name pattern → enclosing class.
//!    `__cm_<C>__<m>` (instance method) and `__sm_<C>__<m>` (static
//!    method) both put the body inside class C; visibility checks
//!    compare against `current_class`. Free fns / `__new_<C>` /
//!    `__dispatch_<m>` / `__env_drop_<closure>` don't establish
//!    class scope (`__new_C` is factory, not user-written code).
//! 5. Declare each param with `declared_class` propagated for
//!    `__this` (from enclosing class) and for plain-class-name
//!    annotations (via `ast.class_parents`).
//! 6. `check_stmt` each body stmt.
//! 7. Restore `expected_return` / `scopes` / `current_class`.

use std::collections::HashMap;

use crate::ast::{Ast, Param, Stmt};
use crate::check::{Checker, DiagPush, LocalInfo, Type};

pub(crate) fn check(checker: &mut Checker, ast: &Ast, name: &str, params: &[Param], body: &[Stmt]) {
    let Some(Type::Function(param_tys, ret_ty)) = checker.globals.get(name).cloned() else {
        return;
    };
    let saved_scopes = std::mem::replace(&mut checker.scopes, vec![HashMap::new()]);
    let saved_return = checker.expected_return.replace(*ret_ty);
    checker.toplevel_captures.push(Default::default());
    let saved_class = checker.current_class.take();
    // A class name can itself contain `__`: a class EXPRESSION binds
    // under a synth name (`__ClassExpr_<id>`), so `__sm___ClassExpr_0
    // __bad` split at its FIRST `__` answered the empty string and the
    // body believed it was inside no class at all. Ask the known class
    // names instead, longest first so a class whose name is a prefix of
    // another cannot claim the body; the old split stays as the fall
    // back for a mangled name whose owner is not a declared class, but
    // only when it answers something.
    let new_class: Option<String> = name
        .strip_prefix("__cm_")
        .or_else(|| name.strip_prefix("__sm_"))
        .and_then(|rest| {
            checker
                .class_names
                .iter()
                .filter(|c| {
                    rest.strip_prefix(c.as_str())
                        .is_some_and(|r| r.starts_with("__"))
                })
                .max_by_key(|c| c.len())
                .cloned()
                .or_else(|| {
                    rest.split_once("__")
                        .map(|(c, _)| c.to_string())
                        .filter(|c| !c.is_empty())
                })
        });
    if new_class.is_some() {
        checker.current_class = new_class;
    }
    // RFC 20260816-headless-argv-face — the synthetic raw-argv
    // pointer at position 0 is absent from the caller-visible
    // signature (`pass_1_hoist_fn_signatures` drops it), so it must
    // not consume a `param_tys` slot here either: zipping it against
    // the first user type shifted every later param off by one and
    // the body's real names went unbound ("unknown identifier `a`").
    // It still needs a binding of its own — the synthesized
    // `__torajs_arguments_materialize` call reads it.
    let params = if params.first().is_some_and(|p| p.name == "__torajs_argv") {
        checker
            .declare(
                "__torajs_argv".to_string(),
                LocalInfo {
                    ty: Type::Any,
                    mutable: false,
                    moved: false,
                    borrowed: false,
                    declared_class: None,
                    builtin_mv: false,
                },
            )
            .ok();
        &params[1..]
    } else {
        params
    };
    for (p, ty) in params.iter().zip(param_tys.iter()) {
        let declared_class = if p.name == "__this" {
            checker.current_class.clone()
        } else {
            p.type_ann.as_ref().and_then(|s| {
                let base = crate::check_type_ann::strip_nullable(s);
                if ast.class_parents.contains_key(base) {
                    Some(base.to_string())
                } else {
                    None
                }
            })
        };
        // 刀 1 (RFC 20260815-fn-value-rest-spread) — the signature
        // spells a rest param as the `Rest(elem)` sentinel for CALL
        // SITES; the body's binding is the packed array (§10.2.1.3
        // IteratorBindingInitialization collects the tail into one).
        let binding_ty = match ty {
            Type::Rest(elem) => Type::Array(elem.clone()),
            other => other.clone(),
        };
        if let Err(e) = checker.declare(
            p.name.clone(),
            LocalInfo {
                ty: binding_ty,
                mutable: true,
                moved: false,
                borrowed: false,
                declared_class,
                builtin_mv: false,
            },
        ) {
            checker.errors.push_err(e);
        }
    }
    let saved_hoists = crate::check_hoist_closure_lets::enter(checker, ast, body);
    for s in body {
        checker.check_stmt(ast, s);
    }
    checker.hoisted_closure_lets = saved_hoists;
    checker.expected_return = saved_return;
    checker.scopes = saved_scopes;
    checker.toplevel_captures.pop();
    checker.current_class = saved_class;
}
