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
use crate::lexer::Span;
use std::collections::HashMap;

#[derive(PartialEq, Clone, Copy)]
enum DeclKind {
    Lexical,
    PlainFn,
}

pub fn run(ast: &mut Ast) {
    let mut errors: Vec<String> = Vec::new();
    for s in &ast.stmts {
        walk_stmt(s, ast, Code::Program, &mut errors);
    }
    ast.redecl_parse_errors = errors;
}

/// Whose statements a walk is checking.
#[derive(Clone, Copy)]
enum Code {
    /// The program's own text. A function body's top level stays with
    /// the checker's declare-time detection (module doc).
    Program,
    /// Eval code (§19.2.1.1). Its early errors are a SyntaxError thrown
    /// when the eval runs (step 12), so nothing downstream may meet the
    /// conflict first: the checker would refuse the whole program for
    /// it. Function bodies are therefore checked here too, and the
    /// strictness the Annex B allowances hang on arrives as a flag —
    /// the text's spans are foreign to the program.
    Eval { strict: bool },
}

/// The redeclaration early errors of one eval code text: its ScriptBody
/// (§16.1.1), every Block / CaseBlock / loop head in it, and every
/// function or class body it declares in statement position. `strict`
/// is the text's own strictness, inherited or from its prologue.
pub fn eval_code_redecl(stmts: &[Stmt], ast: &Ast, strict: bool) -> Option<String> {
    let mut errors: Vec<String> = Vec::new();
    walk_body(stmts, ast, Code::Eval { strict }, &mut errors);
    errors.into_iter().next()
}

/// A function / script / static-block body: under `Code::Eval` its top
/// level is checked (§15.2.1, §15.7.1, §16.1.1), then its statements
/// are walked.
fn walk_body(body: &[Stmt], ast: &Ast, code: Code, errors: &mut Vec<String>) {
    if let Code::Eval { .. } = code {
        check_top_level(body, ast, errors);
    }
    for s in body {
        walk_stmt(s, ast, code, errors);
    }
}

/// Recurse looking for `Block` / `Switch` statement lists to check.
fn walk_stmt(s: &Stmt, ast: &Ast, code: Code, errors: &mut Vec<String>) {
    match s {
        Stmt::Block(list) => {
            check_list(list.iter(), &block_kind(ast, code), errors);
            for c in list {
                walk_stmt(c, ast, code, errors);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            // §14.12.1 — every clause body shares ONE block scope.
            let all = cases
                .iter()
                .flat_map(|c| c.body.iter())
                .chain(default.iter().flatten());
            check_list(all, &case_kind(ast, code), errors);
            for c in cases {
                for s in &c.body {
                    walk_stmt(s, ast, code, errors);
                }
            }
            for s in default.iter().flatten() {
                walk_stmt(s, ast, code, errors);
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            walk_stmt(then_branch, ast, code, errors);
            if let Some(e) = else_branch {
                walk_stmt(e, ast, code, errors);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
            walk_stmt(body, ast, code, errors)
        }
        // §14.7.5.1 / §14.7.4.1 — a loop head's lexical BoundNames may
        // not recur in the body's VarDeclaredNames (`for (let x of xs)
        // { var x }`). A `let` / `const` head keeps the user's name as
        // `var_name`; a `var` or bare head, and a destructuring one,
        // carry a minted `__` name and bind elsewhere (a pattern's
        // names are the body block's own lexicals, which the block
        // check above already holds against its nested `var`s).
        Stmt::ForOfSplitIter { var_name, body, .. } | Stmt::ForOf { var_name, body, .. } => {
            if !var_name.starts_with("__") {
                check_loop_head(std::iter::once(var_name.as_str()), body, errors);
            }
            walk_stmt(body, ast, code, errors)
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                let mut flat: Vec<&Stmt> = Vec::new();
                flatten_multi(i, &mut flat);
                let names = flat.iter().filter_map(|s| match s {
                    Stmt::LetDecl {
                        name,
                        is_var: false,
                        ..
                    } if !name.starts_with("__") => Some(name.as_str()),
                    _ => None,
                });
                check_loop_head(names, body, errors);
                walk_stmt(i, ast, code, errors);
            }
            walk_stmt(body, ast, code, errors);
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            // try/catch/finally bodies are each their own block scope.
            check_list(body.iter(), &block_kind(ast, code), errors);
            check_list(catch_body.iter(), &block_kind(ast, code), errors);
            for s in body.iter().chain(catch_body.iter()) {
                walk_stmt(s, ast, code, errors);
            }
            if let Some(f) = finally_body {
                check_list(f.iter(), &block_kind(ast, code), errors);
                for s in f {
                    walk_stmt(s, ast, code, errors);
                }
            }
        }
        Stmt::Multi(list) => {
            for c in list {
                walk_stmt(c, ast, code, errors);
            }
        }
        Stmt::FnDecl { body, .. } => {
            let code = match code {
                Code::Eval { strict } => Code::Eval {
                    strict: strict
                        || crate::ast::sloppy_this_prologue::has_use_strict_directive(
                            body, &ast.exprs,
                        ),
                },
                Code::Program => Code::Program,
            };
            walk_body(body, ast, code, errors);
        }
        Stmt::ClassDecl {
            ctor,
            methods,
            static_methods,
            static_init,
            ..
        } => {
            // §11.2.2 — all parts of a class are strict mode code.
            let code = match code {
                Code::Eval { .. } => Code::Eval { strict: true },
                Code::Program => Code::Program,
            };
            if let Some(c) = ctor {
                walk_body(&c.body, ast, code, errors);
            }
            for m in methods.iter().chain(static_methods.iter()) {
                walk_body(&m.body, ast, code, errors);
            }
            for si in static_init {
                if let StaticInit::Block(list) = si {
                    walk_body(list, ast, code, errors);
                }
            }
        }
        Stmt::ExportDecl { inner: Some(i), .. } => {
            walk_stmt(i, ast, code, errors);
        }
        _ => {}
    }
}

/// §16.2.1.1 — a Module's top-level statement list is one lexical
/// scope with no Annex B allowance: every `function` declaration is a
/// LexicallyDeclaredName there, so one beside a `var` of the same name
/// (`var f; function f() {}`), or beside a second `function f`, is an
/// early SyntaxError. A Script's top level is the other way round
/// (§16.1.1 var-scopes plain functions; duplicates resolve last-wins
/// in `dedupe_fn_decls`), so this is asked only of MODULE code: the
/// entry under the module goal, and every requested module — a
/// static one fails the link, a dynamic-import candidate rejects its
/// promise (`modules::dyn_import`). `export`-wrapped declarations are
/// members of the same list.
pub fn module_top_level_redecl(stmts: &[Stmt]) -> Option<String> {
    let mut errors: Vec<String> = Vec::new();
    let members = stmts.iter().map(|s| match s {
        Stmt::ExportDecl {
            inner: Some(inner), ..
        } => &**inner,
        other => other,
    });
    check_list(members, &|_, _, _| DeclKind::Lexical, &mut errors);
    errors.into_iter().next()
}

/// One block-scope statement list: reject duplicate lexical names
/// (except plain-fn pairs) and lexical ∩ var overlap. `fn_kind`
/// classifies a `function` declaration by name and generator-ness —
/// a block's plain function is the Annex B `PlainFn`, a module's top
/// level has no such thing.
fn check_list<'a>(
    list: impl Iterator<Item = &'a Stmt>,
    fn_kind: &dyn Fn(&str, bool, Span) -> DeclKind,
    errors: &mut Vec<String>,
) {
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
                name,
                is_generator,
                span,
                ..
            } => {
                insert_lexical(
                    name,
                    fn_kind(name, *is_generator, *span),
                    &mut lexical,
                    errors,
                );
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

/// A body's top level: LexicallyDeclaredNames without duplicates and
/// disjoint from VarDeclaredNames, where every top-level function
/// declaration is a var name, so only a lexical name can collide. The
/// `let`s a destructuring parameter desugars to are parameters, which
/// a `var` or a function may share a name with.
fn check_top_level(list: &[Stmt], ast: &Ast, errors: &mut Vec<String>) {
    let mut lexical: HashMap<&str, DeclKind> = HashMap::new();
    let mut vars: Vec<&str> = Vec::new();
    let mut flat: Vec<&Stmt> = Vec::new();
    for s in list {
        flatten_multi(s, &mut flat);
    }
    for s in flat {
        match s {
            Stmt::LetDecl { init, .. } if ast.param_destr_inits.contains(init) => {}
            Stmt::LetDecl {
                name,
                is_var: false,
                ..
            }
            | Stmt::UsingDecl { name, .. }
            | Stmt::ClassDecl { name, .. } => {
                insert_lexical(name, DeclKind::Lexical, &mut lexical, errors)
            }
            Stmt::FnDecl { name, .. } => vars.push(name),
            other => collect_nested_vars(other, &mut vars),
        }
    }
    for v in vars {
        if lexical.contains_key(v) {
            errors.push(format!("\"{v}\" has already been declared"));
        }
    }
}

/// The Block classifier for `code`: strict eval code has no Annex B
/// §B.3.3.4 allowance, so its plain functions are lexical.
fn block_kind(ast: &Ast, code: Code) -> impl Fn(&str, bool, Span) -> DeclKind + '_ {
    move |name, is_generator, span| match code {
        Code::Eval { strict: true } => DeclKind::Lexical,
        Code::Eval { strict: false } | Code::Program => {
            block_fn_kind(ast)(name, is_generator, span)
        }
    }
}

/// The CaseBlock classifier for `code` (§B.3.2.4 is sloppy-only too;
/// eval code carries its strictness as the flag, not as spans).
fn case_kind(ast: &Ast, code: Code) -> impl Fn(&str, bool, Span) -> DeclKind + '_ {
    move |name, is_generator, span| match code {
        Code::Eval { .. } => block_kind(ast, code)(name, is_generator, span),
        Code::Program => case_block_fn_kind(ast)(name, is_generator, span),
    }
}

/// A block's `function` is the Annex B `PlainFn` unless it is a
/// generator or async function (those are lexical everywhere).
fn block_fn_kind(ast: &Ast) -> impl Fn(&str, bool, Span) -> DeclKind + '_ {
    move |name, is_generator, _| {
        if is_generator || ast.async_fns.contains(name) {
            DeclKind::Lexical
        } else {
            DeclKind::PlainFn
        }
    }
}

/// A CaseBlock's `function` is the Annex B `PlainFn` only in sloppy
/// code: §B.3.2.4's allowance for duplicates bound only by function
/// declarations is for source text that is not strict mode code, so a
/// strict CaseBlock declaring `function f` twice is §14.12.1's error.
fn case_block_fn_kind(ast: &Ast) -> impl Fn(&str, bool, Span) -> DeclKind + '_ {
    move |name, is_generator, span| {
        if crate::ast::annexb_fn_var::annexb_applies_at(ast, name, span) {
            block_fn_kind(ast)(name, is_generator, span)
        } else {
            DeclKind::Lexical
        }
    }
}

/// A loop head's lexical names against the body's VarDeclaredNames.
fn check_loop_head<'a>(
    names: impl Iterator<Item = &'a str>,
    body: &Stmt,
    errors: &mut Vec<String>,
) {
    let mut vars: Vec<&str> = Vec::new();
    collect_nested_vars(body, &mut vars);
    for n in names {
        if vars.contains(&n) {
            errors.push(format!("\"{n}\" has already been declared"));
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
