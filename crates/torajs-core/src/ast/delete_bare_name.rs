//! `delete <bare name>` goal triage — ES §13.5.1 (rotation 372).
//!
//! The parser can no longer judge this form: §13.5.1.1 makes it a
//! SyntaxError under the STRICT goal only, and the goal bit
//! (`ast.sloppy_script_goal`, keyed on the `.cts` extension per the
//! bun mapping) is stamped after parsing. So the parser now emits a
//! plain `Delete { Ident }` node and this gate — run in the prelude
//! right after the eval inline and the `with` desugar — resolves it
//! per goal:
//!
//! - **strict** (every `.ts` module): the same SyntaxError text the
//!   parser used to raise.
//! - **sloppy** (`.cts`): §13.5.1.2 evaluates the reference —
//!   statically decidable here. A name declared anywhere in the
//!   program is a var/function/lexical binding, and §9.1.1.1.7
//!   DeleteBinding answers **false** for them (bindings minted by
//!   declarations are non-configurable). The non-configurable
//!   global value properties (`undefined` / `NaN` / `Infinity` /
//!   `globalThis`, §19.1) answer **false** too. Everything else —
//!   an unresolvable name (§13.5.1.2 step 3.a) or a configurable
//!   global builtin — answers **true**. RESIDUE (recorded): a
//!   configurable builtin answers true but is NOT actually removed
//!   from the global; a program that deletes `Math` and then reads
//!   it observes the difference. The same residue covers a sloppy
//!   implicit global (`x = 1; delete x` answers true, and a later
//!   read sees `undefined` rather than the ReferenceError a real
//!   removal leaves) — this pass used to keep such a name out of the
//!   implicit-globals sibling to preserve that read, which cost the
//!   ASSIGNMENT its binding and killed the program at compile time
//!   before either answer was reached.
//!
//! Both goals leave zero `Delete { Ident }` nodes behind, so no
//! downstream pass ever meets the shape.
//!
//! # Why it runs where it does (rotation 392)
//!
//! It folds a site to a constant from what the program DECLARES, so
//! it has to run after every pass that can change either the set of
//! sites or what a site means. It used to run first, before the eval
//! inline and the `with` desugar, and both were wrong:
//!
//! - a `delete` inlined out of an eval was minted after this gate had
//!   already run, so it survived to the checker and died there on
//!   "delete target must be a property reference";
//! - inside a `with` body §14.11 resolves the reference through the
//!   object, so the site is a PROPERTY reference — `with (o) { delete
//!   x }` must remove `o.x`. Folding it first answered `true` and
//!   removed nothing, silently.
//!
//! Nothing between the parse and here declares or removes a binding
//! this gate reads, and the desugars that follow do not mint a bare
//! `delete`, so the raw-AST property it wants still holds.

use super::{Ast, Expr, Stmt};

/// Resolve every `delete <bare name>` site per the compile goal.
/// `Some(msg)` = strict-goal SyntaxError (the caller reports it as
/// a parse error and stops).
pub fn triage_delete_bare_names(ast: &mut Ast) -> Option<String> {
    let sites: Vec<(usize, String)> = ast
        .exprs
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            if let Expr::Delete { expr } = e
                && let Expr::Ident(n) = ast.get_expr(*expr)
            {
                Some((i, n.clone()))
            } else {
                None
            }
        })
        .collect();
    if sites.is_empty() {
        return None;
    }
    if !ast.sloppy_script_goal {
        return Some(format!(
            "`delete` on a bare name is a SyntaxError in strict code (modules are strict) — `delete {}`",
            sites[0].1
        ));
    }
    let mut declared: std::collections::HashSet<String> = std::collections::HashSet::new();
    collect_declared_names(&ast.stmts, &mut declared);
    for (i, n) in sites {
        ast.exprs[i] = Expr::Bool(sloppy_delete_answer(&n, &declared));
    }
    None
}

/// Every name §9.1.1.1.7 DeleteBinding would find declared: params,
/// var/let/for-of/catch bindings, plus FnDecl / ClassDecl names.
pub(crate) fn collect_declared_names(stmts: &[Stmt], out: &mut std::collections::HashSet<String>) {
    crate::ast_collect_bindings::collect_local_binding_names(stmts, out);
    collect_decl_names(stmts, out);
}

/// §13.5.1.2 evaluated statically — what `delete <name>` answers in
/// sloppy code. Declared bindings and the non-configurable global
/// value properties (§19.1) answer false; an unresolvable name or a
/// configurable global builtin answers true. Shared with the
/// `Function(...)` body desugar, which resolves the same sites for a
/// sloppy body inlined into a strict program.
pub(crate) fn sloppy_delete_answer(n: &str, declared: &std::collections::HashSet<String>) -> bool {
    !(declared.contains(n) || matches!(n, "undefined" | "NaN" | "Infinity" | "globalThis"))
}

/// FnDecl / ClassDecl NAMES (the shared binding collector gathers
/// params and let/var/for-of/catch bindings but not the declaring
/// names themselves).
///
/// The two halves must agree on WHERE a declaration can be. This one
/// used to descend through `Block` / `Multi` only, while
/// `collect_local_binding_names` walked every statement container
/// there is — so a declaration under an `if`, a loop, a `switch` case
/// or a `try` was invisible to the pair. What that cost was not the
/// `delete` answer it was written for but the implicit-globals
/// sibling, which shares this set: `if (c) { function f(){ f = 1 } }`
/// left `f` undeclared, so the assignment in the body read as a write
/// to an unresolvable name and a global `var f` was synthesized out of
/// nothing. That invented binding then made §B.3.3.1 step 1.a.ii
/// answer "an enclosing scope already owns this name", and the block
/// function never got its var binding at all. `delete` gains the same
/// correction on the way: a §B.3.3 var binding is not configurable, so
/// the name answers false, as it already did with the braces at the
/// top of a scope.
///
/// Recorded boundary: the set stays scope-BLIND — it answers "the
/// program declares this name somewhere", not "this name is bound
/// here". A `class` in a block binds only that block, so
/// `{ class C {} } delete C` answers false where the spec says true.
/// That approximation is not new (the `Block` arm has always
/// collected such a name) and the containers only extend its reach;
/// narrowing it wants a scope-aware set, which is its own knife.
fn collect_decl_names(stmts: &[Stmt], out: &mut std::collections::HashSet<String>) {
    for s in stmts {
        collect_decl_names_stmt(s, out);
    }
}

/// One statement: its own declaring name, and every list it owns.
fn collect_decl_names_stmt(s: &Stmt, out: &mut std::collections::HashSet<String>) {
    match s {
        Stmt::FnDecl { name, body, .. } => {
            out.insert(name.clone());
            collect_decl_names(body, out);
        }
        Stmt::ClassDecl { name, .. } => {
            out.insert(name.clone());
        }
        Stmt::Block(inner) | Stmt::Multi(inner) => collect_decl_names(inner, out),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_decl_names_stmt(then_branch, out);
            if let Some(eb) = else_branch.as_deref() {
                collect_decl_names_stmt(eb, out);
            }
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::Labeled { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::ForOfSplitIter { body, .. } => collect_decl_names_stmt(body, out),
        Stmt::For { init, body, .. } => {
            if let Some(init) = init.as_deref() {
                collect_decl_names_stmt(init, out);
            }
            collect_decl_names_stmt(body, out);
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                collect_decl_names(&c.body, out);
            }
            if let Some(d) = default {
                collect_decl_names(d, out);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            collect_decl_names(body, out);
            collect_decl_names(catch_body, out);
            if let Some(fb) = finally_body {
                collect_decl_names(fb, out);
            }
        }
        _ => {}
    }
}
