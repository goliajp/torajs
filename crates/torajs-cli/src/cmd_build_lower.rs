//! The front half of `tr build` and `tr run`: lex, parse, desugar and
//! check the entry file, then lower it to SSA.

use std::process::ExitCode;

use crate::ast_pipeline;
use crate::util::{base_dir_for, read_source, sloppy_goal_for};
use torajs_core::ssa::Module;
use torajs_core::{check, lexer, modules, parser, ssa_lower};
use torajs_link::phase_timer::PhaseTimer;

pub(crate) fn lower_to_ssa(input: &str) -> Result<Module, ExitCode> {
    let mut t = PhaseTimer::start(1);
    let src = read_source(input).map_err(|e| {
        eprintln!("error: {e}");
        ExitCode::from(2)
    })?;
    // annexB §B.1.3 — Script-only, and the goal is knowable here (same
    // extension mapping the AST bit is stamped from, a few lines down).
    let sloppy_goal = sloppy_goal_for(input);
    let lex = if sloppy_goal {
        lexer::tokenize_script
    } else {
        lexer::tokenize
    };
    let tokens = lex(&src).map_err(|e| {
        eprintln!("lex error: {e}");
        ExitCode::from(1)
    })?;
    t.tick("lex");
    // A direct call handing a generator an `any` argument is known only
    // once checked, and the generator's all-`any` twin is cut by the
    // desugar: such a program compiles again with the twins it asked
    // for. The set only grows and is bounded by the generators.
    let mut requests = std::collections::HashSet::new();
    let (ast, artifacts, warnings) = loop {
        let (ast, artifacts, warnings) =
            check_program(input, &src, &tokens, sloppy_goal, &requests, &mut t)?;
        let asked = &artifacts.5.mono_ast.generator_twin_requests;
        if asked.is_subset(&requests) {
            break (ast, artifacts, warnings);
        }
        requests.extend(asked.iter().cloned());
    };
    // RFC 20260730-undeclared-ident — non-fatal diagnostics: printed,
    // program still compiles and runs (spec semantics take over at
    // runtime). The `warning:` prefix stays outside the test262
    // verdict classifier's compile-reject set.
    for w in &warnings {
        eprintln!("warning: {w}");
    }

    // ssa_lower panics on unsupported AST shapes; surface them as the
    // exit-3 "not yet supported" contract the bench harness keys on.
    // The workspace release profile is `panic = "abort"`, so the
    // catch_unwind below never sees the payload there — the hook is
    // the only place the message can surface, and exiting from it
    // (instead of falling through to abort) preserves both the
    // message and the exit code. Pre-fix the hook was empty: every
    // lower reject was a silent SIGABRT with zero output, and the
    // harness misclassified skips as real failures.
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|info| {
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "ssa_lower panicked".to_string()
        };
        eprintln!("not yet supported: {msg}");
        std::process::exit(3);
    }));
    let lower_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ssa_lower::lower_with_arity(&ast, &artifacts)
    }));
    std::panic::set_hook(prev_hook);
    t.tick("ssa_lower");
    // Unreachable under panic=abort (the hook exits first) and a pure
    // backstop under panic=unwind — the hook already printed.
    lower_result.map_err(|_| ExitCode::from(3))
}

/// Parse, desugar and check `src`, the desugar seeded with the
/// generator twins a previous check asked for.
fn check_program(
    input: &str,
    src: &str,
    tokens: &[lexer::Spanned],
    sloppy_goal: bool,
    requests: &std::collections::HashSet<String>,
    t: &mut PhaseTimer,
) -> Result<(torajs_core::ast::Ast, check::CheckArtifacts, Vec<String>), ExitCode> {
    let mut ast = parser::parse_program(src, tokens, sloppy_goal).map_err(|e| {
        eprintln!("parse error: {e}");
        ExitCode::from(1)
    })?;
    ast.source = src.to_string();
    ast.warm_newline_cache();
    // RFC 20260810-sloppy-goal-arguments S1 — goal bit from the input
    // extension (bun mapping: `.cts` = CommonJS sloppy).
    ast.sloppy_script_goal = sloppy_goal;
    ast.generator_twin_requests = requests.clone();
    // Goal-triage gates that must beat the resolver's diagnostics —
    // see ast_pipeline.rs.
    ast_pipeline::run_pre_resolve_gates(&mut ast).map_err(|()| ExitCode::from(1))?;
    t.tick("parse");
    let base_dir = base_dir_for(input);
    let entry = (input != "-").then(|| std::path::Path::new(input));
    modules::resolve_imports(&mut ast, &base_dir, entry).map_err(|e| {
        eprintln!("import error: {e}");
        ExitCode::from(1)
    })?;
    t.tick("resolve_imports");

    // Raw-AST early-error gates + the pre-chain passes — see
    // ast_pipeline.rs.
    ast_pipeline::run_ast_prelude(&mut ast).map_err(|()| ExitCode::from(1))?;
    // Shared 31-pass desugar chain — see ast_pipeline.rs for the
    // per-pass ordering notes.
    ast_pipeline::run_ast_desugar_pipeline(&mut ast).map_err(|()| ExitCode::from(1))?;
    t.tick("desugar");

    let (artifacts, warnings) = check::check_with_arity_warn(&ast).map_err(|e| {
        eprintln!("type error: {e}");
        ExitCode::from(1)
    })?;
    t.tick("check");
    Ok((ast, artifacts, warnings))
}
