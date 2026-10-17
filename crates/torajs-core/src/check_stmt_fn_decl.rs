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

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Param, Stmt};
use crate::check::{Checker, DiagPush, LocalInfo, Type};

pub(crate) fn check(checker: &mut Checker, ast: &Ast, name: &str, params: &[Param], body: &[Stmt]) {
    let Some(Type::Function(param_tys, ret_ty)) = checker.globals.get(name).cloned() else {
        return;
    };
    // TS2534: a `: never` function whose end point is reachable.
    // A tsc type error with no runtime semantics (§7.2 first tier);
    // only a provably reachable end rejects, so a body ending in a
    // loop tsc can see never exits stays accepted.
    if *ret_ty == Type::Never && crate::ast::list_completion(body) == crate::ast::Completion::Falls
    {
        checker.errors.push_err(format!(
            "function `{name}` returns `never` but its end point is reachable"
        ));
    }
    let saved_scopes = std::mem::replace(&mut checker.scopes, vec![HashMap::new()]);
    let saved_return = checker.expected_return.replace(*ret_ty);
    // a top-level function's body holds every write to its locals;
    // a nested one's may not
    let saved_root = std::mem::replace(
        &mut checker.narrows.root,
        checker.ret_owner.is_none().then(|| name.to_string()),
    );
    let saved_owner = checker.ret_owner.replace(name.to_string());
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
        .or_else(|| name.strip_prefix("__cmany_"))
        .or_else(|| name.strip_prefix("__smany_"))
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
    // A ctor on the argv lane carries the slot at position 2, after
    // its head (rotation 616) — for a ctor it is stripped by name; a
    // method's slot (position 1) stays, as the signature hoist keeps
    // it (see `check_pipeline`).
    let stripped: Vec<Param>;
    // A one-slot this-first body on the argv lane (a static twin or a
    // promoted declaration, rotation 627) carries it at position 1
    // and is called directly, so it strips like a ctor's; a `__cm_`
    // method's slot at the same position stays.
    let params: &[Param] = if argv_slot_hidden(name, params) {
        checker
            .declare(
                "__torajs_argv".to_string(),
                LocalInfo {
                    mutable: false,
                    ..LocalInfo::plain(Type::Any)
                },
            )
            .ok();
        stripped = params
            .iter()
            .filter(|p| p.name != "__torajs_argv")
            .cloned()
            .collect();
        &stripped
    } else {
        params
    };
    let boxed = boxed_params(checker, ast, name);
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
                declared_class,
                fn_slot: boxed
                    .contains(&p.name)
                    .then(|| crate::check::FnSlot::Param(name.to_string())),
                ann: p.type_ann.clone(),
                decl: crate::check::FnSlot::param(name, &binding_ty),
                ..LocalInfo::plain(binding_ty)
            },
        ) {
            checker.errors.push_err(e);
        }
    }
    crate::check_gen_env::seed_captures(checker, ast, name);
    let saved_hoists = crate::check_hoist_closure_lets::enter(checker, ast, body);
    for s in body {
        checker.check_stmt(ast, s);
    }
    checker.hoisted_closure_lets = saved_hoists;
    checker.expected_return = saved_return;
    checker.ret_owner = saved_owner;
    checker.narrows.root = saved_root;
    checker.scopes = saved_scopes;
    checker.toplevel_captures.pop();
    checker.current_class = saved_class;
}

/// The parameters of `name` whose direct calls ride the boxed entry: the
/// census the AST carries, and the verdicts this check has reached so far
/// (they reach the AST only once every specialization has been checked,
/// while the lowerer reads them for each), for `name` and for the
/// generic it specializes.
fn boxed_params(checker: &Checker, ast: &Ast, name: &str) -> HashSet<String> {
    let generic = crate::ast::boxed_params::spec_generic(name);
    let mut out = ast.boxed_params_of(name).unwrap_or_default();
    out.extend(
        checker
            .face_mismatch_params
            .iter()
            .filter(|(f, _)| f == name || Some(f.as_str()) == generic)
            .map(|(_, p)| p.clone()),
    );
    out
}

/// Whether the caller-visible signature of `name` leaves out the
/// `__torajs_argv` slot in `params` (its user params, past any env
/// head): at position 0 (head-less), anywhere on a constructor (after
/// `__this` / `__new_target`), or at position 1 off the `__cm_`
/// methods (a static twin or a promoted declaration). A `$$` clone
/// answers as its template does.
pub(crate) fn argv_slot_hidden(name: &str, params: &[Param]) -> bool {
    let name = name.split_once("$$").map_or(name, |(t, _)| t);
    params.first().is_some_and(|p| p.name == "__torajs_argv")
        || (name.ends_with("__ctor") && params.iter().any(|p| p.name == "__torajs_argv"))
        || (!name.starts_with("__cm_") && params.get(1).is_some_and(|p| p.name == "__torajs_argv"))
}

#[cfg(test)]
mod tests {
    fn check_src(src: &str) -> Result<(), String> {
        let tokens = crate::lexer::tokenize(src).map_err(|e| format!("lex: {e}"))?;
        let mut ast = crate::parser::parse(src, &tokens).map_err(|e| format!("parse: {e}"))?;
        crate::ast::lift_arrow_fns(&mut ast);
        crate::check::check(&ast).map(|_| ())
    }

    #[test]
    fn never_with_reachable_end_is_rejected() {
        let err = check_src("function f(): never { console.log(1); }\n").unwrap_err();
        assert!(err.contains("end point is reachable"), "{err}");
    }

    #[test]
    fn never_that_throws_or_spins_is_accepted() {
        let src = "function f(): never { throw 1; }\n\
                   function g(): never { while (true) {} }\n";
        assert!(check_src(src).is_ok(), "{:?}", check_src(src));
    }
}
