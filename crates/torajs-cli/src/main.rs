#[cfg(test)]
mod accessor_override_check_tests;
mod ast_pipeline;
#[cfg(test)]
mod catch_infer_ir_tests;
#[cfg(test)]
mod closure_narrow_tests;
mod cmd_build;
mod cmd_build_dispatch_judge;
mod cmd_build_dispatch_judge_callee;
#[cfg(test)]
mod cmd_build_dispatch_judge_census;
#[cfg(test)]
mod cmd_build_dispatch_judge_census_parse;
mod cmd_build_dispatch_judge_coerce_defs;
mod cmd_build_dispatch_judge_coerce_hooks;
#[cfg(test)]
mod cmd_build_dispatch_judge_coerce_hooks_tests;
mod cmd_build_dispatch_judge_coerce_inert;
mod cmd_build_dispatch_judge_coerce_roles;
#[cfg(test)]
mod cmd_build_dispatch_judge_coerce_roles_tests;
mod cmd_build_dispatch_judge_ctor_reach;
#[cfg(test)]
mod cmd_build_dispatch_judge_ctor_reach_tests;
mod cmd_build_dispatch_judge_ctor_uses;
#[cfg(test)]
mod cmd_build_dispatch_judge_ctor_uses_tests;
mod cmd_build_dispatch_judge_match_symbol;
mod cmd_build_dispatch_judge_obj_world;
mod cmd_build_dispatch_judge_regexp_escape;
mod cmd_build_dispatch_judge_regexp_unseen;
#[cfg(test)]
mod cmd_build_dispatch_judge_regexp_unseen_tests;
mod cmd_build_dispatch_judge_regexp_world;
mod cmd_build_dispatch_judge_synth_error;
mod cmd_build_dispatch_stubs;
mod cmd_build_elide;
mod cmd_build_elide_class;
mod cmd_build_extern_relocs;
mod cmd_build_lower;
mod cmd_build_ssa_string_registries;
mod cmd_build_synthesize;
mod cmd_build_twin_rows;
#[cfg(test)]
mod cmd_build_twin_rows_tests;
mod cmd_build_value_escape;
mod cmd_build_value_escape_slots;
#[cfg(test)]
mod cmd_build_value_escape_tests;
mod cmd_cache;
mod cmd_debug;
mod cmd_fmt;
mod cmd_lint;
mod cmd_run;
#[cfg(test)]
mod fnexpr_this_escape_tests;
#[cfg(test)]
mod fnexpr_this_misrouted_tests;
mod lsp;
mod lsp_bench;
#[cfg(test)]
mod objlit_argv_accessor_tests;
#[cfg(test)]
mod pin_facts_lower_tests;
mod repl;
#[cfg(all(target_arch = "aarch64", target_vendor = "apple"))]
mod rtlinked;
mod runtime_image;
#[cfg(test)]
mod str_split_ir_tests;
mod usage;
mod util;
#[cfg(test)]
mod value_class_census_tests;
#[cfg(test)]
mod value_class_lower_tests;
#[cfg(test)]
mod widen_ret_tests;

use std::env;
use std::path::Path;
use std::process::ExitCode;

use torajs_core::{check, lexer, modules, parser, ssa, ssa_lower};

use cmd_cache::run_cache_subcmd;
use cmd_debug::run_debug;
use cmd_fmt::run_fmt;
use cmd_lint::run_lint;
use usage::print_usage;
use util::{base_dir_for, read_source};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Copy)]
enum Stage {
    Tokenize,
    Parse,
    Check,
    Ssa,
}

fn main() -> ExitCode {
    // Compact panic hook — strips backtrace + thread-name noise and
    // prints a single `not yet supported: <msg>` line so callers can
    // classify the failure cleanly. The bench harness and test262
    // runner both look at the first stderr line; the longer multi-
    // line default hook splits the diagnostic across the panic
    // location and the "note: run with RUST_BACKTRACE" footer.
    std::panic::set_hook(Box::new(|info| {
        let payload = info.payload();
        let msg = if let Some(s) = payload.downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "tr panicked".to_string()
        };
        eprintln!("not yet supported: {msg}");
    }));

    let args: Vec<String> = env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str);

    match cmd {
        Some("--version") | Some("-V") => {
            println!("tr {VERSION}");
            ExitCode::SUCCESS
        }
        None | Some("--help") | Some("-h") => {
            print_usage();
            ExitCode::SUCCESS
        }
        Some("tokenize") => run_pipeline(args.get(1), Stage::Tokenize),
        Some("parse") => run_pipeline(args.get(1), Stage::Parse),
        Some("check") => run_pipeline(args.get(1), Stage::Check),
        Some("ssa") => run_pipeline(args.get(1), Stage::Ssa),
        // `tr run` compiles unconditionally and execs a temp binary
        // (no memoization — the `~/.torajs/cache` run-cache described
        // here historically was never wired into this path; `tr cache`
        // subcommands manage the directory for a future revival).
        // `jit` is kept as a back-compat alias.
        // Pipeline: codegen → torajs-obj → torajs-link (in-house aarch64 toolchain,
        // ssa_inkwell/LLVM retired in #9 atomic swap).
        Some("run") | Some("jit") => cmd_run::run(&args[1..]),
        Some("build") => cmd_build::run(&args[1..]),
        Some("lsp") => match lsp::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("lsp error: {e}");
                ExitCode::from(1)
            }
        },
        Some("lsp-bench") => match env::current_exe() {
            Ok(p) => lsp_bench::run(&p),
            Err(e) => {
                eprintln!("lsp-bench: cannot locate self exe: {e}");
                ExitCode::from(1)
            }
        },
        Some("repl") => repl::run(),
        Some("debug") => run_debug(&args[1..]),
        Some("fmt") => run_fmt(&args[1..]),
        Some("lint") => run_lint(&args[1..]),
        Some("ssa-demo") => {
            ssa::demo_fib40().print();
            ExitCode::SUCCESS
        }
        Some("cache") => run_cache_subcmd(&args[1..]),
        Some(other) => {
            eprintln!("error: unknown command `{other}`");
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn run_pipeline(file_arg: Option<&String>, stage: Stage) -> ExitCode {
    let path = match file_arg {
        Some(p) => p,
        None => {
            eprintln!("error: missing file argument (use `-` for stdin)");
            return ExitCode::from(2);
        }
    };
    let src = match read_source(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    let base_dir = base_dir_for(path);
    let entry = (path != "-").then(|| Path::new(path));
    let requests = std::collections::HashSet::new();
    let sloppy_goal = util::sloppy_goal_for(path);
    pipeline(&src, &base_dir, entry, stage, sloppy_goal, &requests)
}

fn pipeline(
    src: &str,
    base_dir: &Path,
    entry: Option<&Path>,
    stage: Stage,
    sloppy_goal: bool,
    requests: &std::collections::HashSet<String>,
) -> ExitCode {
    // annexB §B.1.3 HTML-like comments are Script-only, and the goal is
    // knowable here — it comes from the input extension. The lexer has
    // had the Script-goal stream all along; nothing but the eval channel
    // was reaching it, because a whole program used to be a module by
    // assumption.
    let lex = if sloppy_goal {
        lexer::tokenize_script
    } else {
        lexer::tokenize
    };
    let tokens = match lex(src) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("lex error: {e}");
            return ExitCode::from(1);
        }
    };
    if matches!(stage, Stage::Tokenize) {
        for t in &tokens {
            println!("{:?} @ {}..{}", t.token, t.span.start, t.span.end);
        }
        return ExitCode::SUCCESS;
    }

    let mut ast = match parser::parse(src, &tokens) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("parse error: {e}");
            return ExitCode::from(1);
        }
    };
    // v0.3 #4 DWARF — retain source bytes so byte_to_line_col can
    // resolve Expr spans into DILocation values during codegen and
    // during runtime panic backtraces.
    ast.source = src.to_string();
    ast.warm_newline_cache();
    // RFC 20260810-sloppy-goal-arguments S1 — goal bit from the input
    // extension (bun mapping: `.cts` = CommonJS sloppy).
    ast.sloppy_script_goal = sloppy_goal;
    ast.generator_twin_requests = requests.clone();
    // Goal-triage gates that must beat the resolver's diagnostics —
    // see ast_pipeline.rs.
    if ast_pipeline::run_pre_resolve_gates(&mut ast).is_err() {
        return ExitCode::from(1);
    }
    // K.2 — resolve cross-file imports BEFORE the desugar pipeline so
    // imported decls go through the same downstream passes (class
    // desugar, arrow lift, etc.) as same-file decls.
    if let Err(e) = modules::resolve_imports(&mut ast, base_dir, entry) {
        eprintln!("import error: {e}");
        return ExitCode::from(1);
    }
    // Raw-AST early-error gates + the pre-chain passes — see
    // ast_pipeline.rs.
    if ast_pipeline::run_ast_prelude(&mut ast).is_err() {
        return ExitCode::from(1);
    }
    // Shared 31-pass desugar chain — see ast_pipeline.rs for the
    // per-pass ordering notes.
    if ast_pipeline::run_ast_desugar_pipeline(&mut ast).is_err() {
        return ExitCode::from(1);
    }
    if matches!(stage, Stage::Parse) {
        ast.print();
        return ExitCode::SUCCESS;
    }

    let artifacts = match check::check_with_arity(&ast) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("type error: {e}");
            return ExitCode::from(1);
        }
    };
    // the generator twins this check asked for: compile again with
    // them seeded, as `tr build` does (`cmd_build::lower_to_ssa`)
    let asked = &artifacts.5.mono_ast.generator_twin_requests;
    if !asked.is_subset(requests) {
        let mut seeded = requests.clone();
        seeded.extend(asked.iter().cloned());
        return pipeline(src, base_dir, entry, stage, sloppy_goal, &seeded);
    }
    if matches!(stage, Stage::Check) {
        return ExitCode::SUCCESS;
    }

    if matches!(stage, Stage::Ssa) {
        let m = ssa_lower::lower_with_arity(&ast, &artifacts);
        m.print();
        return ExitCode::SUCCESS;
    }
    ExitCode::SUCCESS
}
