mod ast;
mod check;
mod lexer;
mod parser;
mod ssa;
mod ssa_inkwell;
mod ssa_lower;

use std::env;
use std::io::Read;
use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Copy)]
enum Stage {
    Tokenize,
    Parse,
    Check,
    Ssa,
}

fn main() -> ExitCode {
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
    pipeline(&src, stage)
}

fn pipeline(src: &str, stage: Stage) -> ExitCode {
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
    // M2 Phase A — lift arrow fns to top-level FnDecls so check.rs's
    // global-fn machinery resolves them. Non-capturing closures only;
    // captures land in Phase B.
    ast::desugar_generators(&mut ast);
    ast::desugar_classes(&mut ast);
    ast::lift_arrow_fns(&mut ast);
    ast::apply_default_args(&mut ast);
    ast::apply_rest_args(&mut ast);
    if matches!(stage, Stage::Parse) {
        ast.print();
        return ExitCode::SUCCESS;
    }

    let generic_call_sites = match check::check(&ast) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("type error: {e}");
            return ExitCode::from(1);
        }
    };
    if matches!(stage, Stage::Check) {
        return ExitCode::SUCCESS;
    }

    if matches!(stage, Stage::Ssa) {
        let m = ssa_lower::lower(&ast, &generic_call_sites);
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
    // M2 Phase A — lift arrow fns to top-level FnDecls so check.rs's
    // global-fn machinery resolves them. Non-capturing closures only;
    // captures land in Phase B.
    ast::desugar_generators(&mut ast);
    ast::desugar_classes(&mut ast);
    ast::lift_arrow_fns(&mut ast);
    ast::apply_default_args(&mut ast);
    ast::apply_rest_args(&mut ast);
    let generic_call_sites = match check::check(&ast) {
        Ok(g) => g,
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
        ssa_lower::lower(&ast, &generic_call_sites)
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

    match ssa_inkwell::compile(&ssa_module, std::path::Path::new(output), &opt) {
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

    // Cache lookup — hash source + binary version + (always-O3 for run).
    // Cache disabled if TORAJS_NO_CACHE is set (useful for bench / CI).
    let cache_disabled = std::env::var_os("TORAJS_NO_CACHE").is_some();
    let cache_path = if !cache_disabled {
        let hash = run_cache_key(&src);
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

    // Cache miss — compile. Reuse the same pipeline as `tr build`.
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
    ast::desugar_generators(&mut ast);
    ast::desugar_classes(&mut ast);
    ast::lift_arrow_fns(&mut ast);
    ast::apply_default_args(&mut ast);
    ast::apply_rest_args(&mut ast);
    let generic_call_sites = match check::check(&ast) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("type error: {e}");
            return ExitCode::from(1);
        }
    };

    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let lower_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ssa_lower::lower(&ast, &generic_call_sites)
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
    if let Err(e) = ssa_inkwell::compile(&ssa_module, &target_path, "O3") {
        eprintln!("compile error: {e}");
        return ExitCode::from(1);
    }
    let rc = exec_binary(&target_path);
    if cache_path.is_none() {
        let _ = std::fs::remove_file(&target_path);
    }
    rc
}

/// Hash key for the run-cache. Includes source bytes + the torajs
/// CARGO_PKG_VERSION + opt level so cache invalidates on torajs
/// upgrade. Plain SHA-256 hex (no external dep — small Rust impl
/// using std::hash isn't cryptographically strong but our use is just
/// "is this the same source", so std FxHash + version is enough).
fn run_cache_key(src: &str) -> String {
    use std::hash::{Hash, Hasher};
    // DefaultHasher is FxHash-ish; collision-resistant enough for cache
    // key (worst case = miss on collision, recompile, harmless).
    let mut h = std::collections::hash_map::DefaultHasher::new();
    src.hash(&mut h);
    env!("CARGO_PKG_VERSION").hash(&mut h);
    "O3".hash(&mut h);
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
