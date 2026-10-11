//! The goal-triage gates `tr run` and `tr build` run before the
//! module resolver, split from `ast_pipeline` under the file-size
//! limit.

use torajs_core::ast;

/// The passes that run BEFORE the chain below, plus the two raw-AST
/// early-error gates that bracket them.
///
/// These stayed copy-pasted in `tr run` and `tr build` when the
/// 31-pass chain got a home, for one reason: the gates return
/// `ExitCode` from one caller and `Err(ExitCode)` from the other. So
/// this reports the failure as `Err(())` — having already printed the
/// diagnostics, which the two spelled identically — and each caller
/// wraps it in its own shape. The pass list itself, the thing a new
/// pass would have to be added to twice, is now written once.
///
/// (The REPL and LSP keep their own reduced pipelines.)
/// Goal-triage gates that must fire BEFORE `modules::resolve_imports`:
/// a strict-goal SyntaxError must precede any resolver diagnostics —
/// `import('./missing.js', yield)` expects the parse-phase reject, not
/// "import path not found" (the same ordering the parse-time yield
/// gate used to guarantee before the goal bit moved post-parse).
/// The delete triage has no resolver-facing face and stays in the
/// prelude with the other raw-AST gates.
pub(crate) fn run_pre_resolve_gates(ast: &mut ast::Ast) -> Result<(), ()> {
    // `yield`-as-identifier goal triage (§12.7.2) — the parser
    // admitted the sites, strict raises the SyntaxError here, sloppy
    // keeps the identifiers as parsed.
    if let Some(msg) = ast::triage_yield_idents(ast) {
        eprintln!("parse error: {msg}");
        return Err(());
    }
    // `await`-as-identifier goal triage (§13.1.1) — the yield gate's
    // sibling, judged on the goal axis alone: the word is reserved in
    // module code and an ordinary name in a Script, strict or not.
    if let Some(msg) = ast::triage_await_idents(ast) {
        eprintln!("parse error: {msg}");
        return Err(());
    }
    // §12.7.2 strict-only future reserved words, goal half — same
    // reason it sits with the yield gate rather than in the prelude.
    if let Some(msg) = ast::triage_strict_reserved_idents(ast) {
        eprintln!("parse error: {msg}");
        return Err(());
    }
    // annexB §B.1.1 / §B.1.2 legacy octal, goal half — the lexer gave
    // every site its sloppy value, strict raises the SyntaxError here.
    if let Some(msg) = ast::triage_legacy_octal(ast) {
        eprintln!("parse error: {msg}");
        return Err(());
    }
    // annexB §B.3.2 / §B.3.4 function declarations, goal half — the
    // parser refused the sites it could already see were strict; a
    // module makes the rest of them SyntaxErrors too.
    if let Some(msg) = ast::triage_annexb_fn_decls(ast) {
        eprintln!("parse error: {msg}");
        return Err(());
    }
    // annexB §B.3.5 for-in initializers, goal half — the parser
    // refused the heads it could already see were strict; a module
    // makes the rest of them SyntaxErrors too.
    if let Some(msg) = ast::triage_annexb_forin_init(ast) {
        eprintln!("parse error: {msg}");
        return Err(());
    }
    // §15.1.2 duplicate parameters, goal half — the parser refused
    // the lists it could already see were strict; a module makes
    // every remaining one strict code too.
    if let Some(msg) = ast::triage_duplicate_params(ast) {
        eprintln!("parse error: {msg}");
        return Err(());
    }
    // §16.2.3 duplicate ExportedNames — same reason it sits here: the
    // duplicate-export cases point their `from` clause at the file
    // itself, so "import path not found" / "import error" would
    // otherwise answer before the SyntaxError does.
    if let Some(msg) = ast::triage_duplicate_exports(ast) {
        eprintln!("parse error: {msg}");
        return Err(());
    }
    // §16.2.1.1 — a module's top level is one lexical scope: `var f;
    // function f() {}` is an early SyntaxError there (a Script's
    // var-scopes the function). Asked before the resolver splices
    // other modules' declarations in beside the entry's own, and
    // only under the module goal.
    if !ast.sloppy_script_goal
        && let Some(msg) = torajs_core::ast_early_redecl::module_top_level_redecl(&ast.stmts)
    {
        eprintln!("parse error: {msg}");
        return Err(());
    }
    // §16.1.1 / §16.2.1.1 — `new.target` in global code (arrows
    // included) is an early error; a function's own body binds its
    // [[NewTarget]] and stops the walk.
    if torajs_core::parser::global_code_contains_new_target(ast) {
        eprintln!(
            "parse error: `new.target` is only valid inside a function body, not in global code (ES §16.1.1)"
        );
        return Err(());
    }
    Ok(())
}
