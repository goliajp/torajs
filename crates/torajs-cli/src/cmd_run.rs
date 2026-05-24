//! `tr run <file>` — AOT-with-cache pipeline.
//!
//! Hashes the source + torajs version + opt level; if a cached
//! binary exists at `~/.torajs/cache/<hash>` it execs that
//! directly (~1.5 ms cold). Otherwise: lex → parse → check →
//! ssa_lower → ssa_inkwell → cc, store the binary in the cache,
//! then exec.
//!
//! Replaced the Cranelift-JIT path on 2026-05-01 — Cranelift's
//! fast-compile / slow-run profile lost compute-heavy benchmarks
//! to V8/JSC. AOT-with-cache gets:
//!
//! - cache hit:  ~1.5 ms cold start, native-speed run
//! - cache miss: ~50 ms compile (one-shot per source change)
//!
//! Both modes beat bun on every workload we've measured.

use std::process::ExitCode;

use torajs_core::{ast, check, lexer, modules, parser, ssa_inkwell, ssa_lower};

use crate::cache_keys::{fixture_o_cache_key, run_cache_key};
use crate::util::{base_dir_for, exec_binary, rand_suffix, read_source};

pub(crate) fn run_jit(file_arg: Option<&String>) -> ExitCode {
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
                std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".torajs/cache"))
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
    // (No inline prune here — measured to add ~10 ms per cache miss
    // ×685 fixtures = 7 s of overhead during a conformance gate, and
    // creates parallel-worker races when multiple cache-miss paths
    // walk the same dir simultaneously. Prune is now a separate
    // `tr cache clean` subcommand that the conformance runner kicks
    // once at gate start.)
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
    // B-1 phase 2 — per-fixture .o cache. Cache key hashes inputs that
    // affect the LLVM .o output (source + imports + opt + compiler-rev)
    // but NOT the staticlib content (different from binary cache key —
    // .o is link-stage-input, staticlibs are link-stage-output deps).
    // Substrate ship invalidates binary cache (mtime), keeps .o cache
    // warm → relink is fast.
    let fixture_o_cache_path: Option<std::path::PathBuf> = if !cache_disabled {
        let key = fixture_o_cache_key(&src, &import_closure);
        let cache_dir = std::env::var("TORAJS_CACHE_DIR")
            .ok()
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".torajs/cache"))
            });
        cache_dir.map(|d| d.join(format!("fixture-{key}.o")))
    } else {
        None
    };
    if let Err(e) = ssa_inkwell::compile_for_kind_with_cache(
        &ssa_module,
        &target_path,
        "O3",
        Some(std::path::Path::new(path)),
        Some(&ast),
        ssa_inkwell::CompileTarget::Native,
        ssa_inkwell::OutputKind::Executable,
        fixture_o_cache_path.as_deref(),
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
