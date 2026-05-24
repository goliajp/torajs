//! `tr build <in> -o <out> [--opt O0|O1|O2|O3] [--target native|wasm32-wasi]`
//!
//! Canonical AOT entry point. Pipeline: lex → parse → check →
//! ssa_lower → ssa_inkwell → cc. Bench harness's `torajs` runner
//! calls this. P3.7 retired the previous wasm-via-C `tr build`;
//! this is the only `tr build` shape now.

use std::process::ExitCode;

use torajs_core::{ast, check, lexer, modules, parser, ssa_inkwell, ssa_lower};

use crate::util::{base_dir_for, read_source};

pub(crate) fn run_build_llvm(args: &[String]) -> ExitCode {
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
    if !explicit_opt && let Ok(level) = std::env::var("TORAJS_OPT") {
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
