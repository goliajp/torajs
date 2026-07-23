//! RFC 20260724-regex-literal-syntax-error — chunk 1
//! (`Stmt::LetDecl` init arm, full nested walk).
//!
//! Rewrites malformed regex literals bound to a `let` into a
//! `throw new SyntaxError(...)` statement, so that a source like
//!
//!     try { const r = /[a-\d]/u; } catch (e) { … }
//!
//! stops behaving as silent-wrong (previous: `r.test("a")` returned
//! `true`) and instead matches bun's `SyntaxError` throw at reach
//! time. The pass walks every `Stmt::LetDecl` in the program — top
//! level, block-nested, fn-body-nested, `try`/`catch`/`finally` body,
//! loop body, labeled, `Multi`, `Switch` cases, `FnDecl` body — for
//! each `LetDecl { init: Expr::Regex(pat, flags) }` whose regex
//! pattern or flags string is malformed per ES §22.2.3.1, records
//! `ast.regex_parse_errors[init_id] = <msg>` and REWRITES the whole
//! statement into `Stmt::Throw(new SyntaxError(msg))`.
//!
//! Non-let-init positions (`f(/bad/)`, `return /bad/`, `arr.push(
//! /bad/)`, …) are unaffected in chunk 1 — chunk 2 will add
//! expression-position IIFE wrapping.

use crate::ast::{Ast, Expr, Stmt};

pub fn run(ast: &mut Ast) {
    // Collect malformed sites first (avoid borrow issues while
    // recursing into stmts). Each entry is (stmt-path, init_id, msg).
    // We use in-place index-walk + replace loop instead of building
    // paths — see `walk_and_rewrite`.
    let mut stmts = std::mem::take(&mut ast.stmts);
    walk_and_rewrite(&mut stmts, ast);
    ast.stmts = stmts;
    // FnDecl bodies live inside Stmt::FnDecl; the walker above
    // covers them the same way.
}

fn walk_and_rewrite(stmts: &mut Vec<Stmt>, ast: &mut Ast) {
    for stmt in stmts.iter_mut() {
        if maybe_rewrite_letdecl(stmt, ast) {
            // Rewrite wrapped the original LetDecl inside a fresh
            // Multi; recursing would revisit the same LetDecl and
            // rewrite it again ad infinitum. The rewritten Multi
            // has no other nested statements worth walking.
            continue;
        }
        walk_children(stmt, ast);
    }
}

fn walk_children(stmt: &mut Stmt, ast: &mut Ast) {
    match stmt {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            walk_boxed(then_branch, ast);
            if let Some(e) = else_branch {
                walk_boxed(e, ast);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            walk_boxed(body, ast);
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases.iter_mut() {
                walk_and_rewrite(&mut c.body, ast);
            }
            if let Some(d) = default {
                walk_and_rewrite(d, ast);
            }
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                walk_boxed(i, ast);
            }
            walk_boxed(body, ast);
        }
        Stmt::ForOfSplitIter { body, .. } | Stmt::ForOf { body, .. } => {
            walk_boxed(body, ast);
        }
        Stmt::Labeled { body, .. } => walk_boxed(body, ast),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            walk_and_rewrite(body, ast);
            walk_and_rewrite(catch_body, ast);
            if let Some(f) = finally_body {
                walk_and_rewrite(f, ast);
            }
        }
        Stmt::Block(body) | Stmt::Multi(body) => walk_and_rewrite(body, ast),
        Stmt::FnDecl { body, .. } => walk_and_rewrite(body, ast),
        _ => {}
    }
}

fn walk_boxed(stmt: &mut Box<Stmt>, ast: &mut Ast) {
    if maybe_rewrite_letdecl(stmt, ast) {
        return;
    }
    walk_children(stmt, ast);
}

/// Returns `true` iff the statement was a malformed-regex
/// `Stmt::LetDecl` and got rewritten. Caller must not recurse into
/// the rewrite output — the wrapper Multi contains the original
/// LetDecl (kept for its type-checker binding effect) which would
/// otherwise be revisited by the walker and rewritten again.
fn maybe_rewrite_letdecl(stmt: &mut Stmt, ast: &mut Ast) -> bool {
    let init_id = match stmt {
        Stmt::LetDecl { init, .. } => *init,
        _ => return false,
    };
    let (pattern, flags) = match &ast.exprs[init_id.0 as usize] {
        Expr::Regex { pattern, flags } => (pattern.clone(), flags.clone()),
        _ => return false,
    };
    let Some(msg) = try_regex_parse_error(&pattern, &flags) else {
        return false;
    };
    ast.regex_parse_errors.insert(init_id, msg.clone());
    let msg_id = ast.add_expr(Expr::String(msg));
    let new_id = ast.add_expr(Expr::New {
        class_name: "SyntaxError".into(),
        args: vec![msg_id],
        type_args: Vec::new(),
    });
    // Preserve the original LetDecl inside a `Multi` so the checker
    // still sees a `let r = /pat/flags` def in the surrounding scope
    // — subsequent references (`r.test(...)`) then type-check cleanly
    // and the runtime Throw fires before any of them can execute.
    // Multi shares its parent scope (see `Stmt::Multi` docstring), so
    // the binding actually reaches the sibling statements.
    let orig = std::mem::replace(stmt, Stmt::Block(Vec::new()));
    *stmt = Stmt::Multi(vec![Stmt::Throw(new_id), orig]);
    true
}

/// Returns `Some(msg)` when `/pattern/flags` is malformed per ES
/// §22.2.3.1 (RegExpInitialize); `None` when well-formed.
fn try_regex_parse_error(pattern: &str, flags: &str) -> Option<String> {
    let f = match torajs_regex::flags::parse_flags(flags.as_bytes()) {
        Some(v) => v,
        None => {
            return Some(format!(
                "Invalid regular expression: /{}/{}: Invalid flags supplied to RegExp constructor",
                pattern, flags
            ));
        }
    };
    use torajs_regex::parser::{RE_FLAG_U, RE_FLAG_V};
    if (f & RE_FLAG_U) != 0 && (f & RE_FLAG_V) != 0 {
        return Some(format!(
            "Invalid regular expression: /{}/{}: Invalid flags supplied to RegExp constructor",
            pattern, flags
        ));
    }
    let mut p = torajs_regex::parser::Parser::new(pattern.as_bytes(), f);
    if p.parse().is_none() {
        return Some(format!(
            "Invalid regular expression: /{}/{}",
            pattern, flags
        ));
    }
    None
}
