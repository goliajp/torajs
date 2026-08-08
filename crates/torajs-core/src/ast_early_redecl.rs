//! Block / switch-CaseBlock redeclaration early errors — parse-phase
//! static semantics per ES §14.2.1 (Block: LexicallyDeclaredNames
//! must have no duplicates and no overlap with VarDeclaredNames) and
//! §14.12.1 (CaseBlock: same, across ALL clauses — they share one
//! scope).
//!
//! Runs on the RAW post-parse AST, before any desugar pass: the
//! checker's declare-time duplicate detection cannot see these
//! conflicts because desugar moves one side away first (generator /
//! async rewrites hoist the FnDecl, `desugar_var_hoist` lifts the
//! `var` to the fn prelude), leaving a single unconflicted binding.
//!
//! Pipeline-consumer shape mirrors `ast_desugar_regex_syntax_error`:
//! the pass only fills `ast.redecl_parse_errors`; the CLI drivers
//! print each entry with the `parse error:` prefix (the shape the
//! test262 verdict judge recognises as a compile-time reject) and
//! exit 1.
//!
//! Duplicate matrix (bun-verified 2026-08-01): every same-name pair
//! in one block rejects EXCEPT plain `function` × plain `function`
//! (Annex B.3.3.4 sloppy allowance — bun accepts it, and the test262
//! strict-mode counterpart is flagged onlyStrict) and `var` × `var`
//! (legal everywhere). `var` entries also collect from NESTED
//! statements down to (not through) function boundaries — a `{ var
//! f; }` inner block conflicts with an outer-block lexical `f`.
//!
//! Recorded boundary: closure/function-expression bodies reached
//! only through the expr arena are not walked (the generated-matrix
//! corpus declares its conflicts in statement position); top-level
//! script and fn-body lists keep the checker's existing coverage.

use crate::ast::{Ast, StaticInit, Stmt};
use std::collections::HashMap;

#[derive(PartialEq, Clone, Copy)]
enum DeclKind {
    Lexical,
    PlainFn,
}

pub fn run(ast: &mut Ast) {
    let mut errors: Vec<String> = Vec::new();
    for s in &ast.stmts {
        walk_stmt(s, ast, &mut errors);
    }
    ast.redecl_parse_errors = errors;
}

/// Recurse looking for `Block` / `Switch` statement lists to check.
fn walk_stmt(s: &Stmt, ast: &Ast, errors: &mut Vec<String>) {
    match s {
        Stmt::Block(list) => {
            check_list(list.iter(), ast, errors);
            for c in list {
                walk_stmt(c, ast, errors);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            // §14.12.1 — every clause body shares ONE block scope.
            let all = cases
                .iter()
                .flat_map(|c| c.body.iter())
                .chain(default.iter().flatten());
            check_list(all, ast, errors);
            for c in cases {
                for s in &c.body {
                    walk_stmt(s, ast, errors);
                }
            }
            for s in default.iter().flatten() {
                walk_stmt(s, ast, errors);
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            walk_stmt(then_branch, ast, errors);
            if let Some(e) = else_branch {
                walk_stmt(e, ast, errors);
            }
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::ForOfSplitIter { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::Labeled { body, .. } => walk_stmt(body, ast, errors),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                walk_stmt(i, ast, errors);
            }
            walk_stmt(body, ast, errors);
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            // try/catch/finally bodies are each their own block scope.
            check_list(body.iter(), ast, errors);
            check_list(catch_body.iter(), ast, errors);
            for s in body.iter().chain(catch_body.iter()) {
                walk_stmt(s, ast, errors);
            }
            if let Some(f) = finally_body {
                check_list(f.iter(), ast, errors);
                for s in f {
                    walk_stmt(s, ast, errors);
                }
            }
        }
        Stmt::Multi(list) => {
            for c in list {
                walk_stmt(c, ast, errors);
            }
        }
        Stmt::FnDecl { body, .. } => {
            for c in body {
                walk_stmt(c, ast, errors);
            }
        }
        Stmt::ClassDecl {
            ctor,
            methods,
            static_methods,
            static_init,
            ..
        } => {
            if let Some(c) = ctor {
                for s in &c.body {
                    walk_stmt(s, ast, errors);
                }
            }
            for m in methods.iter().chain(static_methods.iter()) {
                for s in &m.body {
                    walk_stmt(s, ast, errors);
                }
            }
            for si in static_init {
                if let StaticInit::Block(list) = si {
                    for s in list {
                        walk_stmt(s, ast, errors);
                    }
                }
            }
        }
        Stmt::ExportDecl { inner, .. } => {
            if let Some(i) = inner {
                walk_stmt(i, ast, errors);
            }
        }
        _ => {}
    }
}

/// One block-scope statement list: reject duplicate lexical names
/// (except plain-fn pairs) and lexical ∩ var overlap.
fn check_list<'a>(list: impl Iterator<Item = &'a Stmt>, ast: &Ast, errors: &mut Vec<String>) {
    let mut lexical: HashMap<&str, DeclKind> = HashMap::new();
    let mut vars: Vec<&str> = Vec::new();
    // `Multi` shares the SURROUNDING scope (parse-time destructuring
    // desugar) — flatten it so its decls count as direct members.
    let mut flat: Vec<&Stmt> = Vec::new();
    for s in list {
        flatten_multi(s, &mut flat);
    }
    for s in flat {
        match s {
            Stmt::LetDecl { name, is_var, .. } => {
                if *is_var {
                    vars.push(name);
                } else {
                    insert_lexical(name, DeclKind::Lexical, &mut lexical, errors);
                }
            }
            // `using` is a lexical (const-natured) binding — it
            // participates in §14.2.1 duplicate / lexical∩var
            // conflicts exactly like `const` (RFC 20260809 B1).
            Stmt::UsingDecl { name, .. } => {
                insert_lexical(name, DeclKind::Lexical, &mut lexical, errors);
            }
            Stmt::FnDecl {
                name, is_generator, ..
            } => {
                let kind = if *is_generator || ast.async_fns.contains(name) {
                    DeclKind::Lexical
                } else {
                    DeclKind::PlainFn
                };
                insert_lexical(name, kind, &mut lexical, errors);
            }
            Stmt::ClassDecl { name, .. } => {
                insert_lexical(name, DeclKind::Lexical, &mut lexical, errors);
            }
            other => collect_nested_vars(other, &mut vars),
        }
    }
    for v in vars {
        if lexical.contains_key(v) {
            errors.push(format!("\"{v}\" has already been declared"));
        }
    }
}

fn flatten_multi<'a>(s: &'a Stmt, out: &mut Vec<&'a Stmt>) {
    if let Stmt::Multi(list) = s {
        for c in list {
            flatten_multi(c, out);
        }
    } else {
        out.push(s);
    }
}

fn insert_lexical<'a>(
    name: &'a str,
    kind: DeclKind,
    lexical: &mut HashMap<&'a str, DeclKind>,
    errors: &mut Vec<String>,
) {
    if let Some(prev) = lexical.get(name) {
        if !(*prev == DeclKind::PlainFn && kind == DeclKind::PlainFn) {
            errors.push(format!("\"{name}\" has already been declared"));
        }
        return;
    }
    lexical.insert(name, kind);
}

/// VarDeclaredNames — `var` bindings from nested statements count
/// toward the enclosing block, stopping at function boundaries.
fn collect_nested_vars<'a>(s: &'a Stmt, out: &mut Vec<&'a str>) {
    match s {
        Stmt::LetDecl { name, is_var, .. } if *is_var => out.push(name),
        Stmt::Block(list) | Stmt::Multi(list) => {
            for c in list {
                collect_nested_vars(c, out);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                for s in &c.body {
                    collect_nested_vars(s, out);
                }
            }
            for s in default.iter().flatten() {
                collect_nested_vars(s, out);
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_nested_vars(then_branch, out);
            if let Some(e) = else_branch {
                collect_nested_vars(e, out);
            }
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::ForOfSplitIter { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::Labeled { body, .. } => collect_nested_vars(body, out),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                collect_nested_vars(i, out);
            }
            collect_nested_vars(body, out);
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for s in body.iter().chain(catch_body.iter()) {
                collect_nested_vars(s, out);
            }
            for s in finally_body.iter().flatten() {
                collect_nested_vars(s, out);
            }
        }
        _ => {}
    }
}
