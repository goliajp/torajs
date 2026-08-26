mod ast_pipeline;
mod cmd_build;
mod cmd_build_dispatch_judge;
mod cmd_build_dispatch_stubs;
mod cmd_build_elide;
mod cmd_build_extern_relocs;
mod cmd_build_ssa_string_registries;
mod cmd_build_synthesize;
mod cmd_cache;
mod cmd_debug;
mod cmd_fmt;
mod cmd_lint;
mod cmd_run;
mod lsp;
mod lsp_bench;
mod repl;
mod util;

use std::env;
use std::path::Path;
use std::process::ExitCode;

use torajs_core::{check, lexer, modules, parser, ssa, ssa_lower};

use cmd_cache::run_cache_subcmd;
use cmd_debug::run_debug;
use cmd_fmt::run_fmt;
use cmd_lint::run_lint;
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
        Some("run") | Some("jit") => cmd_run::run(args.get(1)),
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

fn print_usage() {
    println!("tr {VERSION}");
    println!();
    println!("USAGE:");
    println!("    tr <COMMAND> <file|->");
    println!();
    println!("COMMANDS:");
    println!("    run <file>           AOT-compile and execute");
    println!("    jit <file>           alias for `run` (back-compat)");
    println!("    tokenize <file>      print the token stream");
    println!("    parse <file>         print the parsed AST");
    println!("    check <file>         type-check, exit nonzero on error");
    println!("    ssa <file>           print the lowered SSA IR");
    println!("    build <in> -o <out>");
    println!("                         AOT-compile via in-house aarch64 toolchain → native binary");
    println!("    ssa-demo             print a hand-built SSA fib40 (P3.5 step 1 leftover)");
    println!("    lsp                  speak Language Server Protocol over stdio");
    println!("    lsp-bench            measure LSP latency on a synthetic 1K-line fixture");
    println!(
        "    repl                 launch interactive evaluator (history at ~/.torajs/repl_history)"
    );
    println!(
        "    debug <file>         compile with DWARF + drop into lldb (set breakpoints, step, inspect)"
    );
    println!(
        "    fmt <file> [--write] reformat source to tr's canonical style (stdout, or in-place with --write)"
    );
    println!(
        "    lint <file> [--deny] surface 5 lint warnings (unused-let, dead-code-after-return, unreachable-catch, shadowed-let, unused-import); --deny exits non-zero on any warning"
    );
    println!("    cache size           print ~/.torajs/cache size");
    println!(
        "    cache clean [--max-mb N]  LRU-prune ~/.torajs/cache to under N MB (default 2048)"
    );
    println!();
    println!("    --version, -V        print version");
    println!("    --help, -h           print this help");
    println!();
    println!("Use `-` as the file to read from stdin.");
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
    pipeline(&src, &base_dir, stage, util::sloppy_goal_for(path))
}

fn pipeline(src: &str, base_dir: &Path, stage: Stage, sloppy_goal: bool) -> ExitCode {
    let tokens = match lexer::tokenize(src) {
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
    // Goal-triage gates that must beat the resolver's diagnostics —
    // see ast_pipeline.rs.
    if ast_pipeline::run_pre_resolve_gates(&mut ast).is_err() {
        return ExitCode::from(1);
    }
    // K.2 — resolve cross-file imports BEFORE the desugar pipeline so
    // imported decls go through the same downstream passes (class
    // desugar, arrow lift, etc.) as same-file decls.
    if let Err(e) = modules::resolve_imports(&mut ast, base_dir) {
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
