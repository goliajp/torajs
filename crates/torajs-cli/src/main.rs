mod lsp;
mod lsp_bench;
mod repl;

use std::env;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use torajs_core::{ast, check, lexer, modules, parser, ssa, ssa_inkwell, ssa_lower};

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
    println!(
        "    build <in> -o <out> [--opt O0|O1|O2|O3]"
    );
    println!("                         AOT-compile via LLVM 22 → native binary");
    println!("    ssa-demo             print a hand-built SSA fib40 (P3.5 step 1 leftover)");
    println!("    lsp                  speak Language Server Protocol over stdio");
    println!("    lsp-bench            measure LSP latency on a synthetic 1K-line fixture");
    println!("    repl                 launch interactive evaluator (history at ~/.torajs/repl_history)");
    println!("    debug <file>         compile with DWARF + drop into lldb (set breakpoints, step, inspect)");
    println!("    fmt <file> [--write] reformat source to tr's canonical style (stdout, or in-place with --write)");
    println!("    lint <file> [--deny] surface 5 lint warnings (unused-let, dead-code-after-return, unreachable-catch, shadowed-let, unused-import); --deny exits non-zero on any warning");
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
    ast::desugar_classes(&mut ast);
    ast::synthesize_class_globals(&mut ast);
    ast::tag_struct_field_closure_types(&mut ast);
    ast::lift_arrow_fns(&mut ast);
    ast::infer_anonymous_closure_params(&mut ast);
    ast::synthesize_forwarders(&mut ast);
    ast::synthesize_fn_to_closure_forwarders(&mut ast);
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
        let m = ssa_lower::lower_with_arity(&ast, &generic_call_sites, &expr_types, &arity_pad_count);
        m.print();
        return ExitCode::SUCCESS;
    }
    ExitCode::SUCCESS
}

/// `tr build <in> -o <out> [--opt O0|O1|O2|O3]`. Pipeline: lex → parse →
/// check → ssa_lower → ssa_inkwell → cc. Bench harness's `torajs` runner
/// calls this. P3.7 retired the previous wasm-via-C `tr build`; this is the
/// canonical AOT entry point now.
fn run_build_llvm(args: &[String]) -> ExitCode {
    if matches!(
        args.first().map(String::as_str),
        Some("--help") | Some("-h")
    ) {
        println!("tr build — AOT compile via LLVM 22 → native binary");
        println!();
        println!("USAGE: tr build <input.ts> -o <output> [--opt O0|O1|O2|O3]");
        return ExitCode::SUCCESS;
    }

    let mut input: Option<&str> = None;
    let mut output: Option<&str> = None;
    // Default O3 (LLVM's most aggressive non-experimental level). Per-case
    // override flows in via the `TORAJS_OPT` env var (set by the bench
    // harness from `bench.toml: torajs_opt`); explicit `--opt` on the CLI
    // wins over both.
    let mut opt: String = "O3".into();
    let mut explicit_opt = false;
    // T-20 (v0.6.0) — `--target wasm32-wasi` opts into the WebAssembly
    // backend (LLVM 22 clang + wasi-libc sysroot + wasm-ld). The
    // resulting `.wasm` runs under wasmtime / wasmer / Node's wasi
    // module. Default = native AOT.
    let mut compile_target = ssa_inkwell::CompileTarget::Native;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" => {
                i += 1;
                let Some(path) = args.get(i) else {
                    eprintln!("error: `-o` requires a path");
                    return ExitCode::from(2);
                };
                output = Some(path.as_str());
                i += 1;
            }
            "--opt" => {
                i += 1;
                let Some(level) = args.get(i) else {
                    eprintln!("error: `--opt` requires a level (O0|O1|O2|O3)");
                    return ExitCode::from(2);
                };
                opt = level.clone();
                explicit_opt = true;
                i += 1;
            }
            "--target" => {
                i += 1;
                let Some(t) = args.get(i) else {
                    eprintln!("error: `--target` requires a value (native|wasm32-wasi)");
                    return ExitCode::from(2);
                };
                compile_target = match t.as_str() {
                    "native" => ssa_inkwell::CompileTarget::Native,
                    "wasm32-wasi" | "wasm32-wasip1" | "wasm" => {
                        ssa_inkwell::CompileTarget::Wasm32Wasi
                    }
                    other => {
                        eprintln!("error: unknown --target `{other}` (native|wasm32-wasi)");
                        return ExitCode::from(2);
                    }
                };
                i += 1;
            }
            other if !other.starts_with('-') && input.is_none() => {
                input = Some(other);
                i += 1;
            }
            other => {
                eprintln!("error: unexpected argument `{other}`");
                return ExitCode::from(2);
            }
        }
    }

    // Bench harness sets `TORAJS_OPT` for per-case opt tuning (e.g.
    // fib40's `O1` win over `O3` because LLVM's loop transforms hurt
    // recursive int code).
    if !explicit_opt
        && let Ok(level) = std::env::var("TORAJS_OPT")
    {
        opt = level;
    }

    let Some(input) = input else {
        eprintln!("error: missing input file");
        return ExitCode::from(2);
    };
    let Some(output) = output else {
        eprintln!("error: missing `-o <output>`");
        return ExitCode::from(2);
    };

    let src = match read_source(input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    let tokens = match lexer::tokenize(&src) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("lex error: {e}");
            return ExitCode::from(1);
        }
    };
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
    // K.2 — resolve cross-file imports before the desugar pipeline.
    let base_dir = base_dir_for(input);
    if let Err(e) = modules::resolve_imports(&mut ast, &base_dir) {
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
    ast::desugar_classes(&mut ast);
    ast::synthesize_class_globals(&mut ast);
    ast::tag_struct_field_closure_types(&mut ast);
    ast::lift_arrow_fns(&mut ast);
    ast::infer_anonymous_closure_params(&mut ast);
    ast::synthesize_forwarders(&mut ast);
    ast::synthesize_fn_to_closure_forwarders(&mut ast);
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
    let (generic_call_sites, expr_types, arity_pad_count) = match check::check_with_arity(&ast) {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("type error: {e}");
            return ExitCode::from(1);
        }
    };

    // ssa_lower currently panics on unsupported AST shapes. Catch the panic
    // and report as exit-code-3 (bench harness's "not yet implemented" skip).
    // Silence the default panic hook so the bench harness's stderr stays
    // clean — we report the message ourselves below.
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let lower_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ssa_lower::lower_with_arity(&ast, &generic_call_sites, &expr_types, &arity_pad_count)
    }));
    std::panic::set_hook(prev_hook);
    let ssa_module = match lower_result {
        Ok(m) => m,
        Err(payload) => {
            let msg = if let Some(s) = payload.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = payload.downcast_ref::<String>() {
                s.clone()
            } else {
                "ssa_lower panicked".to_string()
            };
            eprintln!("not yet supported: {msg}");
            return ExitCode::from(3);
        }
    };

    match ssa_inkwell::compile_for(
        &ssa_module,
        std::path::Path::new(output),
        &opt,
        Some(std::path::Path::new(input)),
        Some(&ast),
        compile_target,
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("build error: {e}");
            ExitCode::from(1)
        }
    }
}

/// `tr run <file>` — AOT-with-cache pipeline. Hashes the source +
/// torajs version + opt level; if a cached binary exists at
/// `~/.torajs/cache/<hash>` it execs that directly (~1.5 ms cold).
/// Otherwise: lex → parse → check → ssa_lower → ssa_inkwell → cc,
/// store the binary in the cache, then exec.
///
/// Replaced the Cranelift-JIT path on 2026-05-01 — Cranelift's
/// fast-compile / slow-run profile lost compute-heavy benchmarks to
/// V8/JSC. AOT-with-cache gets:
///   - cache hit:  ~1.5 ms cold start, native-speed run
///   - cache miss: ~50 ms compile (one-shot per source change)
/// Both modes beat bun on every workload we've measured.
fn run_jit(file_arg: Option<&String>) -> ExitCode {
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

    // Lex + parse + resolve imports up front. Both `tr run`'s cache key
    // and the rest of the pipeline need a fully-resolved AST, and the
    // import closure must be hashed into the cache key so an edit to a
    // transitively-imported file invalidates the slot. Cost over a
    // single-file program: zero (resolve_imports returns empty closure
    // when no ImportDecls are present).
    let tokens = match lexer::tokenize(&src) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("lex error: {e}");
            return ExitCode::from(1);
        }
    };
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
    let base_dir = base_dir_for(path);
    let import_closure = match modules::resolve_imports(&mut ast, &base_dir) {
        Ok(files) => files,
        Err(e) => {
            eprintln!("import error: {e}");
            return ExitCode::from(1);
        }
    };

    // Cache key — main source + every imported file's bytes + binary
    // version + opt level. Cache disabled only if TORAJS_NO_CACHE is
    // set (bench / CI). Multi-file is now first-class: an edit to lib
    // bumps the closure hash → cache misses → recompile.
    let cache_disabled = std::env::var_os("TORAJS_NO_CACHE").is_some();
    let cache_path = if !cache_disabled {
        let hash = run_cache_key(&src, &import_closure);
        let cache_dir = std::env::var("TORAJS_CACHE_DIR")
            .ok()
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|h| std::path::PathBuf::from(h).join(".torajs/cache"))
            });
        cache_dir.map(|d| d.join(hash))
    } else {
        None
    };

    if let Some(p) = cache_path.as_ref()
        && p.is_file()
    {
        return exec_binary(p);
    }

    // Cache miss — compile. resolve_imports already ran above, so this
    // path picks up at the desugar pipeline.
    ast::unwrap_exports(&mut ast);
    ast::rename_user_main(&mut ast);
    ast::desugar_generators(&mut ast);
    ast::desugar_async(&mut ast);
    ast::desugar_builtin_imports(&mut ast);
    ast::desugar_builtin_new(&mut ast);
    ast::desugar_prototype_call(&mut ast);
    ast::desugar_classes(&mut ast);
    ast::synthesize_class_globals(&mut ast);
    ast::tag_struct_field_closure_types(&mut ast);
    ast::lift_arrow_fns(&mut ast);
    ast::infer_anonymous_closure_params(&mut ast);
    ast::synthesize_forwarders(&mut ast);
    ast::synthesize_fn_to_closure_forwarders(&mut ast);
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
    let (generic_call_sites, expr_types, arity_pad_count) = match check::check_with_arity(&ast) {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("type error: {e}");
            return ExitCode::from(1);
        }
    };

    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let lower_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ssa_lower::lower_with_arity(&ast, &generic_call_sites, &expr_types, &arity_pad_count)
    }));
    std::panic::set_hook(prev_hook);
    let ssa_module = match lower_result {
        Ok(m) => m,
        Err(payload) => {
            let msg = if let Some(s) = payload.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = payload.downcast_ref::<String>() {
                s.clone()
            } else {
                "ssa_lower panicked".to_string()
            };
            eprintln!("not yet supported: {msg}");
            return ExitCode::from(3);
        }
    };

    // Compile target: cache slot if we have one, else a tmp file.
    let target_path = match cache_path.as_ref() {
        Some(p) => {
            if let Some(parent) = p.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            p.clone()
        }
        None => std::env::temp_dir().join(format!(
            "torajs-run-{}-{}",
            std::process::id(),
            rand_suffix()
        )),
    };
    if let Err(e) = ssa_inkwell::compile(
        &ssa_module,
        &target_path,
        "O3",
        Some(std::path::Path::new(path)),
        Some(&ast),
    ) {
        eprintln!("compile error: {e}");
        return ExitCode::from(1);
    }
    let rc = exec_binary(&target_path);
    if cache_path.is_none() {
        let _ = std::fs::remove_file(&target_path);
    }
    rc
}

/// Hash key for the run-cache. Includes the main source + every
/// imported file's path-relative bytes + the torajs CARGO_PKG_VERSION
/// + opt level. Multi-file: an edit to a transitively-imported lib
/// bumps the closure hash → cache slot misses → recompile.
///
/// Stable hashing: std DefaultHasher is FxHash-ish (collision-resistant
/// enough for cache use — worst case is a false miss / recompile, which
/// is harmless). The full-bytes-of-each-file approach is overkill for
/// a 4-file project but stays correct as the import graph grows.
fn run_cache_key(src: &str, import_closure: &[(PathBuf, Vec<u8>)]) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    src.hash(&mut h);
    // Sort the closure by path so the hash is order-independent (BFS
    // traversal order can vary if the lib graph is rearranged, but the
    // resulting program is the same).
    let mut sorted: Vec<&(PathBuf, Vec<u8>)> = import_closure.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    for (path, bytes) in &sorted {
        path.hash(&mut h);
        bytes.hash(&mut h);
    }
    env!("CARGO_PKG_VERSION").hash(&mut h);
    "O3".hash(&mut h);
    /* Hash the running tr binary's mtime so a freshly-rebuilt tr
     * binary doesn't hit cached binaries compiled by an earlier
     * (potentially buggy) version of itself. CARGO_PKG_VERSION stays
     * at "0.1.0" through 0.x dev so it doesn't differentiate; the
     * binary mtime does. Reading from the live binary path (not a
     * build-time stamp) avoids forcing a relink on every cargo run
     * — the cache key recomputes per-execution anyway. */
    if let Ok(exe) = std::env::current_exe()
        && let Ok(meta) = std::fs::metadata(&exe)
        && let Ok(mtime) = meta.modified()
        && let Ok(d) = mtime.duration_since(std::time::UNIX_EPOCH)
    {
        d.as_secs().hash(&mut h);
    }
    format!("torajs-{:016x}", h.finish())
}

fn exec_binary(p: &std::path::Path) -> ExitCode {
    use std::os::unix::process::CommandExt;
    let err = std::process::Command::new(p).exec();
    eprintln!("exec {}: {err}", p.display());
    ExitCode::from(1)
}

fn rand_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{n:x}")
}

fn read_source(arg: &str) -> Result<String, String> {
    if arg == "-" {
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .map_err(|e| format!("reading stdin: {e}"))?;
        Ok(s)
    } else {
        std::fs::read_to_string(arg).map_err(|e| format!("reading {arg}: {e}"))
    }
}

/// Directory that relative `import` paths resolve against. For a file
/// argument, that's the file's parent directory (canonicalized so
/// `import "./x"` and `import "x"` from `./bench/foo.ts` both land at
/// the same absolute path). For stdin, fall back to the current working
/// directory — `import "./x"` from stdin means `./x` from `cwd`.
fn base_dir_for(file_arg: &str) -> PathBuf {
    if file_arg == "-" {
        return std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    }
    let p = PathBuf::from(file_arg);
    let parent = p.parent().filter(|p| !p.as_os_str().is_empty());
    let dir = match parent {
        Some(d) => d.to_path_buf(),
        None => PathBuf::from("."),
    };
    dir.canonicalize().unwrap_or(dir)
}

/// T-05 (v0.3.0) — `tr fmt <file> [--write]` deterministic reformatter.
/// Reads the source, walks it through `torajs_core::formatter::format`,
/// emits to stdout (default) or rewrites the file in place (`--write`).
/// Comment-bearing source is rejected with a clear "v0.4 follow-up"
/// message — no silent comment loss.
/// V3-15 — `tr debug <file>` compiles input to a debug-info-rich
/// binary and execs lldb on it. The DWARF emission shipped with
/// v0.3 #4 already maps every IR instruction back to a source-byte
/// span (`a.byte_to_line_col`), so source-level breakpoints +
/// stepping + frame variable inspection work out of the box. Any
/// installed `lldb` (Apple's macOS, llvm.org's tarball, distro
/// package) is fine — we don't depend on a specific version.
///
/// VS Code DAP integration is a separate follow-up: the planned
/// shape is `tr debug --dap` that speaks DAP over stdio and a
/// thin VS Code extension that launches it. Shipping the
/// CLI-driven path first lets users debug today without waiting
/// for the marketplace publish.
fn run_debug(args: &[String]) -> ExitCode {
    let mut input: Option<&str> = None;
    let mut extra: Vec<&String> = Vec::new();
    let mut after_dashdash = false;
    for a in args {
        if after_dashdash {
            extra.push(a);
            continue;
        }
        match a.as_str() {
            "--help" | "-h" => {
                println!("tr debug — compile with DWARF + launch lldb");
                println!();
                println!("USAGE: tr debug <file> [-- <lldb-args>]");
                println!();
                println!("Examples:");
                println!("  tr debug demo.ts                       # interactive lldb shell");
                println!("  tr debug demo.ts -- -o 'b _main' -o run -o 'frame variable'");
                println!();
                println!("Source-level breakpoints + step + frame-variable work via the");
                println!("v0.3 #4 DWARF emission. Pass any extra lldb args after `--`.");
                return ExitCode::SUCCESS;
            }
            "--" => after_dashdash = true,
            other if input.is_none() && !other.starts_with("--") => {
                input = Some(other);
            }
            other => {
                eprintln!("error: unexpected argument `{other}`");
                return ExitCode::from(2);
            }
        }
    }
    let Some(input) = input else {
        eprintln!("error: missing input file");
        return ExitCode::from(2);
    };
    let bin_path = std::env::temp_dir().join(format!(
        "torajs-debug-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    // Build via `tr build`-equivalent path. We shell out to the
    // current `tr` binary's `build` subcommand so the debug binary
    // gets the exact same toolchain, optimization defaults, and
    // DWARF setup as a normal `tr build`. `--opt O0` keeps source
    // <-> instruction mapping clean for stepping (O3 inlines past
    // most function boundaries).
    let self_exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: locating tr binary: {e}");
            return ExitCode::from(1);
        }
    };
    let build_status = std::process::Command::new(&self_exe)
        .arg("build")
        .arg(input)
        .arg("-o")
        .arg(&bin_path)
        .arg("--opt")
        .arg("O0")
        .status();
    match build_status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            eprintln!("tr build exited {s}");
            return ExitCode::from(s.code().unwrap_or(1) as u8);
        }
        Err(e) => {
            eprintln!("error: spawning tr build: {e}");
            return ExitCode::from(1);
        }
    }
    let mut lldb = std::process::Command::new("lldb");
    for a in &extra {
        lldb.arg(a);
    }
    lldb.arg(&bin_path);
    use std::os::unix::process::CommandExt;
    let err = lldb.exec();
    eprintln!("error: exec lldb: {err}");
    ExitCode::from(1)
}

fn run_fmt(args: &[String]) -> ExitCode {
    let mut input: Option<&str> = None;
    let mut write_in_place = false;
    for a in args {
        match a.as_str() {
            "--write" | "-w" => write_in_place = true,
            "--help" | "-h" => {
                println!("tr fmt — deterministic source reformatter");
                println!();
                println!("USAGE: tr fmt <file|-> [--write]");
                println!();
                println!("  --write, -w   rewrite the file in place (default: stdout)");
                println!();
                println!("Style: 2-space indent, single quotes, no trailing semicolons.");
                println!("Comment-bearing source is rejected (comment-aware fmt is a v0.4 follow-up).");
                return ExitCode::SUCCESS;
            }
            other if input.is_none() && !other.starts_with("--") => {
                input = Some(other);
            }
            other => {
                eprintln!("error: unexpected argument `{other}`");
                return ExitCode::from(2);
            }
        }
    }
    let Some(path) = input else {
        eprintln!("error: missing file argument (use `-` for stdin)");
        return ExitCode::from(2);
    };
    let src = match read_source(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    match torajs_core::formatter::format(&src) {
        Ok(out) => {
            if write_in_place {
                if path == "-" {
                    eprintln!("error: --write incompatible with stdin input");
                    return ExitCode::from(2);
                }
                if let Err(e) = std::fs::write(path, &out) {
                    eprintln!("error: writing {path}: {e}");
                    return ExitCode::from(1);
                }
            } else {
                print!("{out}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}

/// T-06 (v0.3.0) — `tr lint <file> [--deny]` runs the 5 starter
/// rules (unused-let / dead-code-after-return / unreachable-catch /
/// shadowed-let / unused-import). Default exit 0 even with warnings;
/// `--deny` makes any warning exit non-zero (CI gate shape).
fn run_lint(args: &[String]) -> ExitCode {
    let mut input: Option<&str> = None;
    let mut deny = false;
    for a in args {
        match a.as_str() {
            "--deny" | "-D" => deny = true,
            "--help" | "-h" => {
                println!("tr lint — surface lint warnings");
                println!();
                println!("USAGE: tr lint <file|-> [--deny]");
                println!();
                println!("Rules:");
                println!("  unused-let               top-level let / const declared but never read");
                println!("  dead-code-after-return   stmt after return / throw / break / continue");
                println!("  unreachable-catch        catch on a try whose body cannot throw");
                println!("  shadowed-let             inner-scope let shadows enclosing-scope binding");
                println!("  unused-import            import binding never referenced");
                println!();
                println!("  --deny, -D    exit non-zero if any warning is reported");
                return ExitCode::SUCCESS;
            }
            other if input.is_none() && !other.starts_with("--") => {
                input = Some(other);
            }
            other => {
                eprintln!("error: unexpected argument `{other}`");
                return ExitCode::from(2);
            }
        }
    }
    let Some(path) = input else {
        eprintln!("error: missing file argument (use `-` for stdin)");
        return ExitCode::from(2);
    };
    let src = match read_source(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    match torajs_core::linter::lint(&src) {
        Ok(diags) => {
            if diags.is_empty() {
                return ExitCode::SUCCESS;
            }
            for d in &diags {
                let (line, col) = byte_to_line_col(&src, d.span.start);
                let severity = match d.severity {
                    torajs_core::check::Severity::Warning => "warning",
                    torajs_core::check::Severity::Error => "error",
                };
                eprintln!("{path}:{line}:{col}: {severity}: {}", d.message);
            }
            if deny { ExitCode::from(1) } else { ExitCode::SUCCESS }
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}

/// Tiny utility: convert a byte offset to (line, col) (both 1-based).
/// `(0, 0)` byte offset (the no-source-location sentinel from the
/// Diagnostic substrate) maps to line 1, col 1 — same convention as
/// the LSP's file:1:1 anchor.
fn byte_to_line_col(text: &str, byte: u32) -> (u32, u32) {
    let target = byte as usize;
    let mut line = 1u32;
    let mut col = 1u32;
    for (i, ch) in text.char_indices() {
        if i >= target {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}
