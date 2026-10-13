//! `var` hoisting pass — chunk 355, extracted from ast.rs.
//!
//! Pub entry `desugar_var_hoist` runs the P2.1 var-hoist pass per
//! ES spec §14.3.2.1 VariableStatement. Two walker helpers here
//! (`hoist_vars_in_block`, `collect_and_rewrite_var`) and the
//! nested-statement descent in the sibling `var_hoist_recurse`
//! implement the fn-body descent.

use super::var_hoist_recurse::hoist_recurse_stmt;
use super::{Expr, ExprId, Stmt};

mod destr_params;

/// RFC 20260912-global-var-cell — how a script's top-level `var` name
/// reached the global object, which decides the property's
/// attributes (§9.1.1.4.17 vs §9.1.1.4.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptGlobalVar {
    /// A `var` the program wrote: `{writable, enumerable}`, NOT
    /// configurable (`delete this.v` answers false).
    Declared,
    /// A `var` `synthesize_sloppy_implicit_globals` invented for an
    /// assignment to an undeclared name: the property a [[Set]] would
    /// have created, so configurable too.
    Implicit,
    /// The hidden `any` slot holding a top-level `function`
    /// declaration's value (`script_global_fns`): the property is the
    /// function's — `{writable, enumerable}`, not configurable
    /// (§9.1.1.4.18 with D = false) — and its name is the slot's
    /// without [`super::script_global_fns::SCRIPT_GLOBAL_FN_SLOT_PREFIX`].
    Function,
    /// A `var` declared only by eval text (`(0, eval)("var x")`, a
    /// sloppy direct eval at the top level): §19.2.1.3 CreateGlobalVar-
    /// Binding with D = true — `{writable, enumerable, configurable}` —
    /// bound where the eval ran (the property does not exist before).
    EvalVar,
    /// A `var` declared only by a Script evaluated from the program
    /// (`$262.evalScript("var x")`): the written-`var` attributes
    /// (§16.1.7 step 17, not configurable), bound where the Script ran.
    ScriptVar,
    /// The hidden slot of a `function` declared only by eval text:
    /// §9.1.1.4.18 CreateGlobalFunctionBinding with D = true —
    /// configurable — bound where the eval ran.
    EvalFn,
    /// The hidden slot of a `function` declared only by a Script
    /// evaluated from the program: the function attributes (not
    /// configurable), bound where the Script ran.
    ScriptFn,
}

/// P2.1 — `var` hoisting pass per ES spec §14.3.2.1
/// VariableStatement. Walks every fn body + the top-level script
/// (taken as the implicit `main` body for hoisting purposes), finds
/// every `Stmt::LetDecl { is_var: true, .. }` (including deep inside
/// `if` / `while` / `for` / `block` / `switch` / `try` bodies), and:
///
///   1. Emits a synthetic `let <name>: <type_ann> = Uninit` at the top
///      of the enclosing fn-body / script. Pre-init reads return
///      undefined per spec (P1.3's Expr::Uninit → Type::Undefined
///      substrate makes this work).
///   2. Replaces the original site:
///      - If the original had an init: convert to `<name> = <init>`
///        assignment (Stmt::Expr wrapping Expr::Assign).
///      - If no init (`var x;`): remove the stmt entirely.
///   3. Same name declared multiple times in the same fn scope:
///      ONE hoisted decl + N assignments. Later decls' type anns
///      override earlier ones (last-write-wins matches JS shape).
///
/// `let` / `const` (is_var=false) are block-scoped per spec and not
/// touched by this pass.
pub fn desugar_var_hoist(ast: &mut super::Ast) {
    // Take stmts out so we can pass &mut ast.exprs and a separate
    // &mut Vec<Stmt> to the recursive helpers without aliasing.
    let mut top = std::mem::take(&mut ast.stmts);
    // RFC 20260912-global-var-cell — under the script goal every
    // top-level `var` is also an own property of the global object
    // (§16.1.7), so the names are recorded before the hoist rewrites
    // the sites away. The GOAL, not strictness: a strict Script still
    // instantiates its `var`s on the global object.
    if ast.sloppy_script_goal {
        let mut counts: std::collections::HashMap<String, Vec<ExprId>> =
            std::collections::HashMap::new();
        count_var_names(&top, &mut counts);
        // Before the hoist rewrites sites (and before the eval-site
        // markers below add self-assignments): which names does the
        // program write at all? `ast.stmts` is taken, so the walk is
        // over `top`; no seed functions — the top-level list already
        // holds every lifted body.
        let assigned =
            crate::ssa_lower_closure_captures::collect_assigned_names_scoped(ast, top.iter(), &[]);
        // `function f() {} var f;` — the `var` re-declares the function's
        // binding (§16.1.7 step 8: a var name that is also a function
        // name is the function's), so the name is the FUNCTION's and
        // takes no data global: giving it one put a `GlobalRef("f")`
        // next to the function symbol `f` and the program died of
        // SIGBUS (rotation 636 close sweep: 43 dynamic-import /
        // redeclaration-global / RegExp cases). The function's own
        // property on the object is the RFC's knife 3.
        let fn_names = super::script_global_fns::toplevel_fn_decl_names(&top);
        // A name tr minted (`__dstr_<n>` for a destructuring head, a
        // for-of temp) is no program binding: it neither reaches the
        // object nor takes the global home the object needs.
        // A `var f = v;` over `function f() {}` no longer reaches here:
        // `nested_fns_top` (rotation 640) gave that declaration a
        // mangled name and a head `var` of its own, so `f` is an
        // ordinary written `var` by now. What is left under a function
        // name is the init-less re-declaration, which rides the
        // parameter route below.
        for (name, sites) in counts {
            if super::is_synthesized_dunder(ast, &name) {
                continue;
            }
            if fn_names.contains(&name) {
                continue;
            }
            // A name every site of which came out of eval / Script text
            // takes that text's flavour and binds at its site; a name
            // the program wrote anywhere is a written `var`.
            let flavor = super::var_hoist_eval_sites::eval_flavor(&sites, &ast.eval_var_sites)
                .unwrap_or(ScriptGlobalVar::Declared);
            if !assigned.contains(&name)
                && sites
                    .iter()
                    .all(|e| matches!(ast.exprs[e.0 as usize], Expr::Uninit))
            {
                ast.script_global_writerless.insert(name.clone());
            }
            ast.script_global_var_names.entry(name).or_insert(flavor);
        }
        super::var_hoist_eval_sites::mark_eval_var_sites(&mut top, ast);
        // Top-level script: hoist over `top` itself. The function names
        // ride as the pre-existing bindings a parameter list does: a
        // `var f;` over `function f() {}` mints no prelude slot (it
        // shadowed the function — `typeof f` answered `undefined` and
        // `f()` was "not a function", rotation 637).
        hoist_vars_in_block(&mut top, &mut ast.exprs, &fn_names);
    } else {
        hoist_vars_in_block(&mut top, &mut ast.exprs, &[]);
    }
    // Each top-level FnDecl: hoist over its body. The parameter names
    // ride along — §10.2.11 step 28 gives a `var` that re-declares a
    // parameter the parameter's own binding (initialized from it),
    // so `function f(a = 0) { var a; }` declares nothing new.
    // A destructuring parameter's names ride along too (`destr_params`).
    for stmt in top.iter_mut() {
        if let Stmt::FnDecl { body, params, .. } = stmt {
            let mut names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
            names.extend(destr_params::names(ast, body));
            hoist_vars_in_block(body, &mut ast.exprs, &names);
            destr_params::make_written_mutable(ast, body);
        }
    }
    ast.stmts = top;
}

/// Walks `body` recursively, collects every `is_var: true` LetDecl's
/// (name, type_ann), and rewrites the original site to either an
/// assignment (if the decl had an init) or a no-op. Returned hoisted
/// decls are inserted at the top of `body`.
fn hoist_vars_in_block(body: &mut Vec<Stmt>, exprs: &mut Vec<Expr>, params: &[String]) {
    use std::collections::BTreeMap;
    // Cluster #11 (rotation 442) — pre-scan the whole fn scope for
    // names `var`-declared more than once. The typed escape hatch is
    // sound only for a single-declaration name: §14.3.2 shares ONE
    // binding across every same-name `var`, so a second declarator
    // with a different shape (`var o = {a:1}; var o = {b:"x"}`) must
    // land in a slot both shapes fit — the Any prelude — not in the
    // first shape's pinned `let`.
    let mut counts: std::collections::HashMap<String, Vec<ExprId>> =
        std::collections::HashMap::new();
    count_var_names(body, &mut counts);
    let multi: std::collections::HashSet<String> = counts
        .into_iter()
        .filter(|(_, sites)| sites.len() > 1)
        .map(|(n, _)| n)
        .collect();
    // A parameter name is already the fn's binding: the `var` site
    // becomes an assignment onto it (or nothing, for a bare `var a;`),
    // and no prelude slot is minted — a second declaration of the
    // name in the same scope is what the checker refuses.
    let mut hoisted: BTreeMap<String, Option<String>> =
        params.iter().map(|p| (p.clone(), None)).collect();
    collect_and_rewrite_var(body, &mut hoisted, exprs, false, &multi);
    for p in params {
        hoisted.remove(p);
    }
    if hoisted.is_empty() {
        return;
    }
    let mut prelude: Vec<Stmt> = Vec::with_capacity(hoisted.len());
    for (name, _user_ann) in hoisted {
        let init = ExprId(exprs.len() as u32);
        exprs.push(Expr::Uninit);
        // Hoisted vars are ALWAYS Type::Any-typed slots regardless
        // of user annotation. Two reasons:
        //   1. The pre-init read returns undefined per spec, which
        //      doesn't fit Type::Number / Array<Number> / etc. Any
        //      fits because Undefined is a valid Any-tag.
        //   2. var's runtime type can change across reassignments
        //      (`var x = 1; x = "hello"` is legal JS); Any covers
        //      both. The user's annotation is treated as a hint for
        //      the eventual value, not a slot constraint.
        // User-typed slots that need precise types should use `let`
        // (block-scoped) which keeps its annotation.
        prelude.push(Stmt::LetDecl {
            mutable: true,
            name,
            type_ann: Some("any".to_string()),
            init,
            is_var: false, // already hoisted; downstream sees a regular let
        });
    }
    // after a "use strict" prologue, which has to stay the body's head
    let at = super::sloppy_this_prologue::directive_prologue_len(body, exprs);
    body.splice(at..at, prelude);
}

/// Count every `var`-declared name in this stmt list, recursing the
/// same nested positions the rewrite walks (FnDecl bodies excluded —
/// their own hoist pass counts separately). Feeds the multi-decl
/// escape-hatch gate in [`hoist_vars_in_block`].
pub(super) fn count_var_names(
    stmts: &[Stmt],
    counts: &mut std::collections::HashMap<String, Vec<ExprId>>,
) {
    for s in stmts {
        count_var_names_stmt(s, counts);
    }
}

fn count_var_names_stmt(s: &Stmt, counts: &mut std::collections::HashMap<String, Vec<ExprId>>) {
    match s {
        Stmt::LetDecl {
            name,
            init,
            is_var: true,
            ..
        } => counts.entry(name.clone()).or_default().push(*init),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            count_var_names_stmt(then_branch, counts);
            if let Some(eb) = else_branch.as_deref() {
                count_var_names_stmt(eb, counts);
            }
        }
        Stmt::Block(b) | Stmt::Multi(b) => count_var_names(b, counts),
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
            count_var_names_stmt(body, counts)
        }
        Stmt::For { init, body, .. } => {
            if let Some(init_box) = init.as_deref() {
                count_var_names_stmt(init_box, counts);
            }
            count_var_names_stmt(body, counts);
        }
        Stmt::ForOfSplitIter { body, .. } | Stmt::ForOf { body, .. } => {
            count_var_names_stmt(body, counts)
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            count_var_names(body, counts);
            count_var_names(catch_body, counts);
            if let Some(fb) = finally_body {
                count_var_names(fb, counts);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for case in cases {
                count_var_names(&case.body, counts);
            }
            if let Some(dflt) = default {
                count_var_names(dflt, counts);
            }
        }
        _ => {}
    }
}

pub(super) fn collect_and_rewrite_var(
    stmts: &mut Vec<Stmt>,
    hoisted: &mut std::collections::BTreeMap<String, Option<String>>,
    exprs: &mut Vec<Expr>,
    nested: bool,
    multi: &std::collections::HashSet<String>,
) {
    let mut new_stmts: Vec<Stmt> = Vec::with_capacity(stmts.len());
    // Rotation 205 — names the escape hatch already declared as a
    // `let` in THIS stmt list. ES §14.3.2 allows same-scope `var`
    // re-declaration (one shared binding); a second escape-hatch
    // conversion would emit a second `let` and trip the checker's
    // redeclaration reject, so later same-name `var`s convert to
    // plain assignments instead (matching the hoisted path's
    // BTreeMap dedup). Scoped to this list only: the converted `let`
    // is block-scoped, so a name declared in a NESTED block is not
    // assignable from here (the escape hatch's pre-existing
    // block-scope compromise).
    let mut escaped: std::collections::HashSet<String> = std::collections::HashSet::new();
    let drained = std::mem::take(stmts);
    for s in drained {
        match s {
            Stmt::LetDecl {
                mutable,
                name,
                type_ann,
                init,
                is_var: true,
            } => {
                // P2.1 escape hatches — three cases where we don't
                // hoist (treat as a regular block-scoped `let`):
                //   1. User wrote an explicit type annotation
                //      (`var arr: number[] = [...]`). The typed slot
                //      can't carry the spec's pre-init undefined
                //      (Type::Undefined doesn't fit Type::Array<Number>).
                //   2. Init is a function/arrow expression. Hoisting
                //      to Type::Any loses the FnSig and makes the
                //      var uncallable (substrate: call-on-Any is P3).
                //      `var f = function() {}; f()` pre-P2.1 worked
                //      via let-aliasing; this keeps that path live.
                //   3. (future) Init produces other types that lose
                //      method dispatch when boxed (Array, Obj, etc.)
                //      — covered by case 2's spirit; add cases as
                //      they surface.
                let init_keeps_type = match &exprs[init.0 as usize] {
                    Expr::ArrowFn { .. } | Expr::Closure { .. } => true,
                    // After lift_arrow_fns, capturing-less function
                    // expressions become `Expr::Ident("__closure_N")`
                    // pointing at the lifted FnDecl. That ident
                    // resolves to a FnSig at the SSA layer, NOT to
                    // a regular let-typed value, so hoisting to Any
                    // would lose the call-site dispatch.
                    Expr::Ident(n) if n.starts_with("__closure_") => true,
                    // P5 — `var xs = [literal]` is the dominant
                    // test262 / plain-JS pattern. Hoisting to
                    // Type::Any made `xs.length` return "undefined"
                    // (Member-on-Any with no length dispatch) — a
                    // silent-wrong bug that gated every test262
                    // case using `var arr = [...]` from passing.
                    // Keep the typed Array<T> slot; pre-init read
                    // returns the slot default (null) which is rare
                    // enough that test coverage is happy with the
                    // typed path. See `var-hoist-001` for the
                    // pre-init undefined case (uses `var x = 1`,
                    // Number, not Array).
                    Expr::Array(_) => true,
                    // Same fix for object literals — `var obj =
                    // {a: 1}; obj.a` returned undefined under the
                    // Any-promotion path because Member-on-Any
                    // routes through dynobj_get which doesn't yet
                    // resolve typed-struct fields. Keep the typed
                    // Struct slot so direct field reads work.
                    Expr::ObjectLit { .. } => true,
                    _ => false,
                };
                // Rotation 205 — a re-declared name (either form
                // already declared it: an escape-hatch `let` in this
                // list, or a hoisted prelude slot in this fn)
                // converts to an assignment onto the existing
                // binding — ES's one-shared-binding semantics.
                let redeclared = escaped.contains(&name) || hoisted.contains_key(&name);
                // Rotation 264 — the escape hatch is sound ONLY at
                // the fn-body / script top level, where block scope
                // and fn scope coincide. In a NESTED list (try /
                // block / switch case) an escaped `let` is invisible
                // to the rest of the fn — `try { var x = [] } x`
                // answered "undeclared" (the compromise the escape
                // notes admitted to). Member/call-on-Any has matured
                // since P5 (length / index / methods / field reads /
                // invocation all dispatch), so a nested var always
                // hoists to the Any prelude slot per §14.3.2.1.
                // Cluster #11 (rotation 442) — a multiply-declared
                // name never escapes: the second declarator's shape
                // may differ, and §14.3.2's one shared binding must
                // fit both. An explicit annotation used to keep its
                // escape; with a `var` of the name in a NESTED block —
                // which always hoists to the prelude (rotation 264) —
                // that made two declarations of one name, and the
                // checker rejected a legal program (`var x: any; { var
                // x = 3 }`, rotation 640). The annotation now rides the
                // prelude slot like any other site's (last one wins).
                if (type_ann.is_some() || init_keeps_type)
                    && !multi.contains(&name)
                    && !redeclared
                    && !nested
                {
                    escaped.insert(name.clone());
                    new_stmts.push(Stmt::LetDecl {
                        mutable,
                        name,
                        type_ann,
                        init,
                        is_var: false,
                    });
                    continue;
                }
                if !redeclared {
                    hoisted.insert(name.clone(), None);
                }
                if !matches!(exprs[init.0 as usize], Expr::Uninit) {
                    let target_id = ExprId(exprs.len() as u32);
                    exprs.push(Expr::Ident(name));
                    let assign_id = ExprId(exprs.len() as u32);
                    exprs.push(Expr::Assign {
                        target: target_id,
                        value: init,
                    });
                    new_stmts.push(Stmt::Expr(assign_id));
                }
            }
            mut other => {
                hoist_recurse_stmt(&mut other, hoisted, exprs, multi);
                new_stmts.push(other);
            }
        }
    }
    *stmts = new_stmts;
}

#[cfg(test)]
mod tests {
    use super::super::Ast;
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn hoisted_ast(src: &str) -> Ast {
        let tokens = tokenize(src).expect("lex");
        let mut ast = parse(src, &tokens).expect("parse");
        desugar_var_hoist(&mut ast);
        ast
    }

    fn count_letdecls(ast: &Ast, name: &str) -> usize {
        ast.stmts
            .iter()
            .filter(|s| matches!(s, Stmt::LetDecl { name: n, .. } if n == name))
            .count()
    }

    #[test]
    fn same_scope_var_redeclaration_dedups_to_one_let() {
        // rotation 205 — ES §14.3.2 one-shared-binding: the second
        // escape-hatch var converts to an assignment, not a second let
        let ast = hoisted_ast("var v = { a: 1 };\nvar v = { a: 2 };\n");
        assert_eq!(count_letdecls(&ast, "v"), 1);
    }

    #[test]
    fn mixed_hoist_then_escape_shape_dedups() {
        // first var hoists to the any-slot prelude; the second
        // (escape-shape) must assign onto it, not redeclare
        let ast = hoisted_ast("var w = 5;\nvar w = { a: 3 };\n");
        assert_eq!(count_letdecls(&ast, "w"), 1);
    }

    #[test]
    fn redeclaration_without_init_is_dropped() {
        let ast = hoisted_ast("var m = { x: 7 };\nvar m;\n");
        assert_eq!(count_letdecls(&ast, "m"), 1);
    }
}
