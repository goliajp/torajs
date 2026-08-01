mod cmd_build;
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

use torajs_core::{ast, ast_closure_param_tag, check, lexer, modules, parser, ssa, ssa_lower};

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
    // K.2 — resolve cross-file imports BEFORE the desugar pipeline so
    // imported decls go through the same downstream passes (class
    // desugar, arrow lift, etc.) as same-file decls.
    if let Err(e) = modules::resolve_imports(&mut ast, base_dir) {
        eprintln!("import error: {e}");
        return ExitCode::from(1);
    }
    // Block/CaseBlock redeclaration early errors — must see the RAW
    // AST before generator / async / var-hoist desugars move one
    // side of a conflict away.
    ast::early_redecl_errors(&mut ast);
    if !ast.redecl_parse_errors.is_empty() {
        for msg in &ast.redecl_parse_errors {
            eprintln!("parse error: {msg}");
        }
        return ExitCode::from(1);
    }
    // M2 Phase A — lift arrow fns to top-level FnDecls so check.rs's
    // global-fn machinery resolves them. Non-capturing closures only;
    // captures land in Phase B.
    ast::unwrap_exports(&mut ast);
    ast::rename_user_main(&mut ast);
    ast::hoist_gen_fn_exprs(&mut ast);
    ast::desugar_generators(&mut ast);
    ast::desugar_async(&mut ast);
    ast::desugar_builtin_imports(&mut ast);
    ast::desugar_builtin_new(&mut ast);
    ast::desugar_regex_syntax_error(&mut ast);
    ast::desugar_promise_try(&mut ast);
    if !ast.regex_parse_errors.is_empty() {
        for msg in ast.regex_parse_errors.values() {
            eprintln!("parse error: regex literal {msg}");
        }
        return ExitCode::from(1);
    }
    ast::desugar_prototype_call(&mut ast);
    ast::inject_builtin_classes(&mut ast);
    ast::desugar_classes(&mut ast);
    // §8.6.2 default-param TDZ — after desugar_classes (methods are
    // flat `__cm_` FnDecls, the `__new_*` error factories exist),
    // before materialize_expr_defaults (which would otherwise move a
    // bare self/later-param read into the body where it reads the
    // undefined-bound parameter instead of throwing).
    ast::desugar_dflt_param_tdz(&mut ast);
    ast::materialize_expr_defaults(&mut ast);
    // Must follow desugar_classes: that pass is what turns `this`
    // into the name `__this`, and this one is what gives plain
    // functions somewhere to bind it (RFC 20260726 blade 1).
    ast::bind_this_param(&mut ast);
    ast::rewrite_toplevel_this(&mut ast);
    // Reads the receiver parameter the pass above may have added,
    // so it has to follow it (RFC 20260726 blade 2).
    ast::synthesize_fn_constructors(&mut ast);
    // Every factory that will exist exists by now, so a `new <name>()`
    // still holding a name nobody claimed is constructing a value
    // (S-NEW 刀 4). Must follow both factory-synthesizing passes.
    ast::route_non_class_new(&mut ast);
    ast::fill_optional_fields(&mut ast);
    ast::synthesize_class_globals(&mut ast);
    ast::tag_struct_field_closure_types(&mut ast);
    // A nested `function` that reads an outer local wants the closure
    // lane, not the top-level lift below — see ast/nested_fns_capture.
    // Must precede `lift_arrow_fns`: the rewrite it emits is a function
    // expression, and that is the pass which gives one its env.
    ast::desugar_capturing_nested_fns(&mut ast);
    ast::lift_arrow_fns(&mut ast);
    ast::infer_anonymous_closure_params(&mut ast);
    ast_closure_param_tag::tag_closure_arg_params(&mut ast);
    ast::synthesize_forwarders(&mut ast);
    // Nested-fn lift runs BEFORE the fn-to-closure collector (RFC
    // 20260717-namedfn-canonical-cell O3): a body-local fn name is
    // shadow-rejected by the collector, so lifting first hands it
    // the mangled top-level `__nested___top_*` decls and their
    // rewritten references — nested fn VALUE reads then join the
    // forwarder canonical-cell lane like any top-level fn (pre-fix
    // they lowered to raw FnSig addresses and face === bare-name
    // diverged).
    ast::desugar_nested_fns(&mut ast);
    ast::synthesize_fn_to_closure_forwarders(&mut ast);
    ast::desugar_function_prototype_methods(&mut ast);
    // P2.1 — see embed/lib.rs for ordering rationale.
    ast::desugar_uninit_let(&mut ast);
    ast::desugar_var_hoist(&mut ast);
    ast::desugar_variadic_push(&mut ast);
    ast::desugar_arguments_object(&mut ast);
    ast::rewrite_split_for_i_to_iter(&mut ast);
    ast::escape_analyze_array_literals(&mut ast);
    ast::desugar_implicit_generics(&mut ast);
    ast::apply_default_args(&mut ast);
    ast::apply_rest_args(&mut ast);
    ast::apply_spread_args(&mut ast);
    ast::fold_fromentries(&mut ast);
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
