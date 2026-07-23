//! RFC 20260724-regex-literal-syntax-error — chunks 1 (stmt-position
//! `Stmt::LetDecl` init arm, full nested walk) + 2 (expression-
//! position IIFE wrapper for `f(/bad/)` / `return /bad/` / element
//! init / etc.).
//!
//! Rewrites malformed regex literals into a `throw new
//! SyntaxError(...)` control-flow effect, so that sources like
//!
//!     try { const r = /[a-\d]/u; } catch (e) { … }        // chunk 1
//!     try { f(/[a-\d]/u); } catch (e) { … }               // chunk 2
//!
//! stop behaving as silent-wrong (previous: matcher returned wrong
//! result at runtime) and instead match bun's `SyntaxError` throw
//! semantics.
//!
//! Chunk 1 (stmt LetDecl) — walks every `Stmt::LetDecl` in the
//! program — top level, block-nested, fn-body-nested, `try` /
//! `catch` / `finally` body, loop body, labeled, `Switch` cases,
//! `Multi`, `FnDecl` body — for each `LetDecl { init: Expr::Regex
//! (pat, flags) }` whose regex is malformed per ES §22.2.3.1,
//! records `ast.regex_parse_errors[init_id] = <msg>` and REPLACES
//! the whole statement with `Stmt::Multi([original LetDecl,
//! Stmt::Throw(new SyntaxError(msg))])`. LetDecl-first order lets
//! ssa-lower emit an SSA slot for the binding; the Throw fires
//! before any reference reads it. Re-entry into the wrapper Multi
//! is gated so the walker doesn't rewrite the same LetDecl
//! recursively.
//!
//! Chunk 2 (expression-position) — walks the `ast.exprs` arena, and
//! for each `Expr::Regex(pat, flags)` NOT already handled by chunk 1
//! (side-table dedup) that is malformed, REPLACES the arena slot
//! in place with an IIFE
//!
//!     (() : any => { throw new SyntaxError(msg) })()
//!
//! The IIFE has zero params, `return_type = Some("any")` so
//! consumers expecting any target type accept it, and a single
//! `Stmt::Throw(new SyntaxError(msg))` body. Because the arrow's
//! ExprId is fresh but the Call replaces the original Regex ExprId
//! in place, every parent statement / expression that referenced the
//! Regex now sees the IIFE Call transparently. The freshly emitted
//! `ArrowFn` still flows through the downstream `lift_arrow_fns` pass
//! (this pass runs before it in the pipeline order).

use crate::ast::{Ast, Expr, ExprId, Stmt};

pub fn run(ast: &mut Ast) {
    // Chunk 1 — stmt-position walker.
    let mut stmts = std::mem::take(&mut ast.stmts);
    walk_and_rewrite(&mut stmts, ast);
    ast.stmts = stmts;
    // Chunk 2 — expression-position IIFE wrapper.
    wrap_expression_position(ast);
}

fn wrap_expression_position(ast: &mut Ast) {
    let n = ast.exprs.len();
    for i in 0..n {
        let expr_id = ExprId(i as u32);
        // Skip regexes already handled by chunk 1 (their arena slot
        // is still `Expr::Regex`, but they now live inside a Multi
        // whose first stmt is the original LetDecl — the Throw is
        // already emitted at stmt level, no IIFE needed).
        if ast.regex_parse_errors.contains_key(&expr_id) {
            continue;
        }
        let (pattern, flags) = match &ast.exprs[i] {
            Expr::Regex { pattern, flags } => (pattern.clone(), flags.clone()),
            _ => continue,
        };
        let Some(msg) = try_regex_parse_error(&pattern, &flags) else {
            continue;
        };
        ast.regex_parse_errors.insert(expr_id, msg.clone());
        let msg_id = ast.add_expr(Expr::String(msg));
        let new_id = ast.add_expr(Expr::New {
            class_name: "SyntaxError".into(),
            args: vec![msg_id],
            type_args: Vec::new(),
        });
        let arrow_id = ast.add_expr(Expr::ArrowFn {
            params: Vec::new(),
            return_type: Some("any".into()),
            body: vec![Stmt::Throw(new_id)],
        });
        // Replace the Regex arena slot in place — parents keep the
        // same ExprId and now transparently see the IIFE Call.
        ast.exprs[i] = Expr::Call {
            callee: arrow_id,
            args: Vec::new(),
        };
    }
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
    // Keep the original LetDecl FIRST so ssa-lower emits a real SSA
    // slot for `r`; downstream statements referencing `r` (checker-
    // clean, unreachable at runtime) then have a def to bind to. The
    // Throw follows immediately, terminating control flow before any
    // reference actually runs — try/catch still catches the
    // SyntaxError, and top-level uncaught yields exit 1 stderr
    // `error: uncaught SyntaxError: …` (matches bun's exit 1 shape).
    let orig = std::mem::replace(stmt, Stmt::Block(Vec::new()));
    *stmt = Stmt::Multi(vec![orig, Stmt::Throw(new_id)]);
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
