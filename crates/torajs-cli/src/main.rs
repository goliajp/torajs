mod cache_keys;
mod cmd_build;
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

use torajs_core::{ast, check, lexer, modules, parser, ssa, ssa_lower};

use cmd_build::run_build_llvm;
use cmd_cache::run_cache_subcmd;
use cmd_debug::run_debug;
use cmd_fmt::run_fmt;
use cmd_lint::run_lint;
use cmd_run::run_jit;
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
        // `tr run` is AOT-with-cache (replaced Cranelift JIT 2026-05-01):
        // hash source → `~/.torajs/cache/<hash>` → exec, or compile + cache + exec.
        // `jit` is kept as a back-compat alias.
        Some("run") | Some("jit") => run_jit(args.get(1)),
        Some("build") => run_build_llvm(&args[1..]),
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
    println!("    run <file>           AOT-compile via LLVM (cached at ~/.torajs/cache), execute");
    println!("    jit <file>           alias for `run` (back-compat)");
    println!("    tokenize <file>      print the token stream");
    println!("    parse <file>         print the parsed AST");
    println!("    check <file>         type-check, exit nonzero on error");
    println!("    ssa <file>           print the lowered SSA IR");
    println!("    build <in> -o <out> [--opt O0|O1|O2|O3]");
    println!("                         AOT-compile via LLVM 22 → native binary");
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
    pipeline(&src, &base_dir, stage)
}

fn pipeline(src: &str, base_dir: &Path, stage: Stage) -> ExitCode {
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

    let mut ast = match parser::parse(&tokens) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("parse error: {e}");
            return ExitCode::from(1);
        }
    };
    // v0.3 #4 DWARF — retain source bytes so byte_to_line_col can
    // resolve Expr spans into DILocation values during ssa_inkwell
    // emission and during runtime panic backtraces.
    ast.source = src.to_string();
    ast.warm_newline_cache();
    // K.2 — resolve cross-file imports BEFORE the desugar pipeline so
    // imported decls go through the same downstream passes (class
    // desugar, arrow lift, etc.) as same-file decls.
    if let Err(e) = modules::resolve_imports(&mut ast, base_dir) {
        eprintln!("import error: {e}");
        return ExitCode::from(1);
    }
    // M2 Phase A — lift arrow fns to top-level FnDecls so check.rs's
    // global-fn machinery resolves them. Non-capturing closures only;
    // captures land in Phase B.
    ast::unwrap_exports(&mut ast);
    ast::rename_user_main(&mut ast);
    ast::desugar_generators(&mut ast);
    ast::desugar_async(&mut ast);
    ast::desugar_builtin_imports(&mut ast);
    ast::desugar_builtin_new(&mut ast);
    ast::desugar_prototype_call(&mut ast);
    ast::inject_builtin_classes(&mut ast);
    ast::desugar_classes(&mut ast);
    ast::synthesize_class_globals(&mut ast);
    ast::tag_struct_field_closure_types(&mut ast);
    ast::lift_arrow_fns(&mut ast);
    ast::infer_anonymous_closure_params(&mut ast);
    ast::synthesize_forwarders(&mut ast);
    ast::synthesize_fn_to_closure_forwarders(&mut ast);
    ast::desugar_function_prototype_methods(&mut ast);
    // P2.1 — see embed/lib.rs for ordering rationale.
    ast::desugar_uninit_let(&mut ast);
    ast::desugar_var_hoist(&mut ast);
    ast::desugar_nested_fns(&mut ast);
    ast::desugar_variadic_push(&mut ast);
    ast::desugar_array_isarray_value(&mut ast);
    ast::desugar_arguments_object(&mut ast);
    ast::rewrite_split_for_i_to_iter(&mut ast);
    ast::escape_analyze_array_literals(&mut ast);
    ast::desugar_implicit_generics(&mut ast);
    ast::apply_default_args(&mut ast);
    ast::apply_rest_args(&mut ast);
    ast::compute_consuming_params(&mut ast);
    if matches!(stage, Stage::Parse) {
        ast.print();
        return ExitCode::SUCCESS;
    }

    let (generic_call_sites, expr_types, arity_pad_count) = match check::check_with_arity(&ast) {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("type error: {e}");
            return ExitCode::from(1);
        }
    };
    if matches!(stage, Stage::Check) {
        return ExitCode::SUCCESS;
    }

    if matches!(stage, Stage::Ssa) {
        let m =
            ssa_lower::lower_with_arity(&ast, &generic_call_sites, &expr_types, &arity_pad_count);
        m.print();
        return ExitCode::SUCCESS;
    }
    ExitCode::SUCCESS
}
