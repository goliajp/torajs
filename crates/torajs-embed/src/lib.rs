//! V3-14 — torajs embed API.
//!
//! Two consumer surfaces:
//!   1. **Rust** — `eval_source(src: &str) -> EvalOutcome` for
//!      programs that already depend on this crate via Cargo.
//!   2. **C ABI** — `extern "C" fn tora_eval(src, len) -> i32`
//!      exposed via the `staticlib` / `cdylib` build outputs so
//!      third-party C / Go / Swift hosts can link `libtorajs_embed`
//!      and call `tora_eval` directly.
//!
//! Implementation: runs tora's full compile pipeline (lex → parse
//! → check → ssa_lower → ssa_inkwell), writes the resulting native
//! binary to a temp file, and execs it as a subprocess. The exit
//! code propagates back as the eval result.
//!
//! Subprocess is the V3-14 MVP — it gives the C ABI surface and
//! lets third-party hosts run tora source today. In-process JIT
//! lands in V3-16 (Function ctor / `eval()`) which needs the
//! current process's address space to share state with the eval'd
//! source.

use std::ffi::CStr;
use std::ops::Deref;
use std::os::raw::{c_char, c_int};
use std::path::PathBuf;

use torajs_core::{ast, check, lexer, parser, ssa_inkwell, ssa_lower};

/// Outcome of a single `eval_source` call.
#[derive(Debug, Clone)]
pub enum EvalOutcome {
    /// Source compiled and ran. `exit_code` is the subprocess exit
    /// code (0 = clean, nonzero = runtime error / panic / explicit
    /// `process.exit(N)` from the source).
    Ok { exit_code: i32 },
    /// Lex / parse / typecheck / lower / link failed before the
    /// program could run. The string is the first diagnostic.
    CompileError(String),
    /// Filesystem or subprocess plumbing error (couldn't write
    /// temp file, couldn't spawn the binary, etc).
    HostError(String),
}

impl EvalOutcome {
    /// Return the subprocess exit code, mapping compile errors to
    /// 1 and host errors to 2 — matches `tr`'s own CLI exit
    /// conventions and gives the C ABI a single-int return.
    pub fn as_exit_code(&self) -> i32 {
        match self {
            EvalOutcome::Ok { exit_code } => *exit_code,
            EvalOutcome::CompileError(_) => 1,
            EvalOutcome::HostError(_) => 2,
        }
    }
}

/// Compile and run `src` in a fresh subprocess. Returns the
/// outcome — see [`EvalOutcome`].
pub fn eval_source(src: &str) -> EvalOutcome {
    let bin = match compile_to_temp(src) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let result = std::process::Command::new(&bin).status();
    let _ = std::fs::remove_file(&bin);
    let dsym = bin.with_extension("dSYM");
    if dsym.is_dir() {
        let _ = std::fs::remove_dir_all(&dsym);
    }
    match result {
        Ok(status) => EvalOutcome::Ok {
            exit_code: status.code().unwrap_or(-1),
        },
        Err(e) => EvalOutcome::HostError(format!("spawning compiled binary: {e}")),
    }
}

fn compile_to_temp(src: &str) -> Result<PathBuf, EvalOutcome> {
    let tokens =
        lexer::tokenize(src).map_err(|e| EvalOutcome::CompileError(format!("lex: {e}")))?;
    let mut a =
        parser::parse(&tokens).map_err(|e| EvalOutcome::CompileError(format!("parse: {e}")))?;
    a.source = src.to_string();
    a.warm_newline_cache();
    ast::unwrap_exports(&mut a);
    ast::rename_user_main(&mut a);
    ast::desugar_generators(&mut a);
    ast::desugar_async(&mut a);
    ast::desugar_builtin_imports(&mut a);
    ast::desugar_builtin_new(&mut a);
    ast::inject_builtin_classes(&mut a);
    ast::desugar_classes(&mut a);
    ast::synthesize_class_globals(&mut a);
    ast::tag_struct_field_closure_types(&mut a);
    ast::lift_arrow_fns(&mut a);
    ast::infer_anonymous_closure_params(&mut a);
    ast::synthesize_forwarders(&mut a);
    ast::synthesize_fn_to_closure_forwarders(&mut a);
    ast::desugar_function_prototype_methods(&mut a);
    // P2.1 — order matters: uninit_let inlines `let x; x = e` into
    // `let x = e` for early type binding. var_hoist creates synthetic
    // `let x = Uninit` that should NOT be inlined (var semantics
    // require x to be undefined at every read before its assignment,
    // not at the declaration site). Run uninit_let FIRST so it only
    // sees user-written `let x;` (which IS legal to inline), then
    // var_hoist inserts hoisted `let x = Uninit` that uninit_let
    // never gets a chance to touch.
    ast::desugar_uninit_let(&mut a);
    ast::desugar_var_hoist(&mut a);
    // P3.4 — lift nested function declarations to top-level
    // (Annex B §B.3.3 web-compat hoist). Runs after var-hoist so
    // it sees the post-hoist body shape.
    ast::desugar_nested_fns(&mut a);
    ast::desugar_array_isarray_value(&mut a);
    ast::desugar_arguments_object(&mut a);
    ast::rewrite_split_for_i_to_iter(&mut a);
    ast::escape_analyze_array_literals(&mut a);
    ast::desugar_implicit_generics(&mut a);
    ast::apply_default_args(&mut a);
    ast::apply_rest_args(&mut a);
    ast::compute_consuming_params(&mut a);
    let (gcs, expr_types) =
        check::check_with_types(&a).map_err(|e| EvalOutcome::CompileError(format!("type: {e}")))?;
    let module = ssa_lower::lower_with_types(&a, &gcs, &expr_types);
    let out = std::env::temp_dir().join(format!(
        "torajs-embed-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    ssa_inkwell::compile(&module, &out, "O3", None, Some(&a))
        .map_err(|e| EvalOutcome::CompileError(format!("compile: {e:?}")))?;
    Ok(out)
}

fn rand_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{n:x}")
}

// ============================================================
// V3-16 m2 — Function ctor substrate: compile a tora source
// snippet to a position-independent shared library, dlopen it,
// expose the named entry as a typed Rust fn pointer.
// ============================================================

/// Owns a dlopen'd library whose lifetime ties to the
/// [`Library`] handle. Drop the [`LoadedFunction`] to dlclose.
///
/// The `T` type parameter is the fn-pointer signature the caller
/// expects (`unsafe extern "C" fn(...) -> ...`). Wrong T = UB on
/// invocation — the compiler can't sanity-check JIT'd ABI shapes.
pub struct LoadedFunction<T: Copy> {
    /// dlopen handle. Held to keep the library mapped for the
    /// lifetime of the LoadedFunction.
    _lib: libloading::Library,
    /// Cast'd fn pointer.
    f: T,
    /// Path to the .dylib on disk; cleaned up on Drop.
    path: PathBuf,
}

impl<T: Copy> LoadedFunction<T> {
    /// Pointer to the JIT'd entry. Cast / call at your own risk —
    /// see the `T` type-parameter caveat above.
    pub fn ptr(&self) -> T {
        self.f
    }
}

impl<T: Copy> Deref for LoadedFunction<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.f
    }
}

impl<T: Copy> Drop for LoadedFunction<T> {
    fn drop(&mut self) {
        // The libloading::Library Drop closes the dlopen handle;
        // we just clean the on-disk file to keep /tmp tidy.
        let _ = std::fs::remove_file(&self.path);
        let dsym = self.path.with_extension("dSYM");
        if dsym.is_dir() {
            let _ = std::fs::remove_dir_all(&dsym);
        }
    }
}

/// Compile `src` to a position-independent shared library, dlopen
/// it, and resolve `entry_symbol` to a typed Rust fn pointer.
/// Returns a [`LoadedFunction`] that owns the dlopen handle —
/// dropping it dlcloses the library and unlinks the on-disk file.
///
/// `T` must be a `Copy` fn-pointer type whose ABI matches the
/// tora fn at `entry_symbol`. Tora maps `number` → `i64`,
/// `boolean` → `i32` zero/one, `string` → `*const u8`-shaped
/// pointer to the tora Str struct. See the test module for the
/// canonical `extern "C" fn(i64, i64) -> i64` example.
///
/// # Safety
/// The returned fn pointer is `unsafe extern "C"` — invoking it
/// with the wrong `T` type or after the [`LoadedFunction`] has
/// dropped is UB. The compiled body shares the host process's
/// runtime symbols (str_alloc, obj_drop, etc) via macOS's
/// `-undefined dynamic_lookup` linker flag, so a body that uses
/// strings / arrays / heap WILL allocate on the host's heap.
pub unsafe fn compile_function<T: Copy>(
    src: &str,
    entry_symbol: &str,
) -> Result<LoadedFunction<T>, String> {
    let dylib = compile_to_dylib(src)?;
    let lib = unsafe { libloading::Library::new(&dylib) }
        .map_err(|e| format!("dlopen({}): {e}", dylib.display()))?;
    let symbol: libloading::Symbol<T> = unsafe { lib.get(entry_symbol.as_bytes()) }
        .map_err(|e| format!("dlsym({entry_symbol}): {e}"))?;
    let f = *symbol;
    drop(symbol);
    Ok(LoadedFunction {
        _lib: lib,
        f,
        path: dylib,
    })
}

fn compile_to_dylib(src: &str) -> Result<PathBuf, String> {
    let tokens = lexer::tokenize(src).map_err(|e| format!("lex: {e}"))?;
    let mut a = parser::parse(&tokens).map_err(|e| format!("parse: {e}"))?;
    a.source = src.to_string();
    a.warm_newline_cache();
    ast::unwrap_exports(&mut a);
    ast::rename_user_main(&mut a);
    ast::desugar_generators(&mut a);
    ast::desugar_async(&mut a);
    ast::desugar_builtin_imports(&mut a);
    ast::desugar_builtin_new(&mut a);
    ast::inject_builtin_classes(&mut a);
    ast::desugar_classes(&mut a);
    ast::synthesize_class_globals(&mut a);
    ast::tag_struct_field_closure_types(&mut a);
    ast::lift_arrow_fns(&mut a);
    ast::infer_anonymous_closure_params(&mut a);
    ast::synthesize_forwarders(&mut a);
    ast::synthesize_fn_to_closure_forwarders(&mut a);
    ast::desugar_function_prototype_methods(&mut a);
    // P2.1 — order matters: uninit_let inlines `let x; x = e` into
    // `let x = e` for early type binding. var_hoist creates synthetic
    // `let x = Uninit` that should NOT be inlined (var semantics
    // require x to be undefined at every read before its assignment,
    // not at the declaration site). Run uninit_let FIRST so it only
    // sees user-written `let x;` (which IS legal to inline), then
    // var_hoist inserts hoisted `let x = Uninit` that uninit_let
    // never gets a chance to touch.
    ast::desugar_uninit_let(&mut a);
    ast::desugar_var_hoist(&mut a);
    // P3.4 — lift nested function declarations to top-level
    // (Annex B §B.3.3 web-compat hoist). Runs after var-hoist so
    // it sees the post-hoist body shape.
    ast::desugar_nested_fns(&mut a);
    ast::desugar_array_isarray_value(&mut a);
    ast::desugar_arguments_object(&mut a);
    ast::rewrite_split_for_i_to_iter(&mut a);
    ast::escape_analyze_array_literals(&mut a);
    ast::desugar_implicit_generics(&mut a);
    ast::apply_default_args(&mut a);
    ast::apply_rest_args(&mut a);
    ast::compute_consuming_params(&mut a);
    let (gcs, exprs) = check::check_with_types(&a).map_err(|e| format!("type: {e}"))?;
    let m = ssa_lower::lower_with_types(&a, &gcs, &exprs);
    let out = std::env::temp_dir().join(format!(
        "torajs-fn-{}-{}.dylib",
        std::process::id(),
        rand_suffix()
    ));
    ssa_inkwell::compile_for_kind(
        &m,
        &out,
        "O3",
        None,
        Some(&a),
        ssa_inkwell::CompileTarget::Native,
        ssa_inkwell::OutputKind::SharedLib,
    )
    .map_err(|e| format!("compile: {e:?}"))?;
    Ok(out)
}

// ============================================================
// C ABI
// ============================================================

/// `tora_eval(src, len) -> i32` — eval `len` bytes of UTF-8
/// source pointed to by `src`. Returns the exit code (0 on
/// clean run; 1 on compile error; 2 on host error).
///
/// # Safety
/// `src` must point to at least `len` bytes of valid UTF-8. The
/// pointer is read but not retained past the call return.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tora_eval(src: *const c_char, len: usize) -> c_int {
    if src.is_null() {
        return 2;
    }
    let bytes = unsafe { std::slice::from_raw_parts(src as *const u8, len) };
    let s = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return 2,
    };
    eval_source(s).as_exit_code() as c_int
}

/// `tora_eval_cstr(src) -> i32` — convenience wrapper that
/// reads a NUL-terminated C string. Cheaper for callers that
/// already have a `const char *`.
///
/// # Safety
/// `src` must be a valid NUL-terminated C string with valid
/// UTF-8 contents.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tora_eval_cstr(src: *const c_char) -> c_int {
    if src.is_null() {
        return 2;
    }
    let cs = unsafe { CStr::from_ptr(src) };
    let s = match cs.to_str() {
        Ok(s) => s,
        Err(_) => return 2,
    };
    eval_source(s).as_exit_code() as c_int
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eval_hello() {
        let out = eval_source("console.log(1 + 2)");
        assert!(
            matches!(out, EvalOutcome::Ok { exit_code: 0 }),
            "got {out:?}"
        );
    }

    #[test]
    fn eval_compile_error() {
        let out = eval_source("let x: number = 'hi'");
        assert!(matches!(out, EvalOutcome::CompileError(_)), "got {out:?}");
    }

    #[test]
    fn c_abi_smoke() {
        let s = b"console.log(42)\0";
        let rc = unsafe { tora_eval_cstr(s.as_ptr() as *const c_char) };
        assert_eq!(rc, 0);
    }

    /// V3-16 milestone 1 — verify the new SharedLib output kind
    /// produces a valid dylib that can be dlopen'd + called from
    /// the host process. This is the substrate the in-process
    /// Function ctor / eval path uses end-to-end: compile a fn
    /// into a .dylib, dlopen it, look up the symbol, invoke
    /// through a fn pointer, get the result back.
    #[test]
    fn shared_lib_emit_and_call() {
        let src = r#"
function torajs_add(a: number, b: number): number {
  return a + b;
}
"#;
        let tokens = lexer::tokenize(src).unwrap();
        let mut a = parser::parse(&tokens).unwrap();
        a.source = src.to_string();
        a.warm_newline_cache();
        ast::unwrap_exports(&mut a);
        ast::rename_user_main(&mut a);
        ast::desugar_generators(&mut a);
        ast::desugar_async(&mut a);
        ast::desugar_builtin_imports(&mut a);
        ast::desugar_builtin_new(&mut a);
        ast::inject_builtin_classes(&mut a);
        ast::desugar_classes(&mut a);
        ast::synthesize_class_globals(&mut a);
        ast::tag_struct_field_closure_types(&mut a);
        ast::lift_arrow_fns(&mut a);
        ast::infer_anonymous_closure_params(&mut a);
        ast::synthesize_forwarders(&mut a);
        ast::synthesize_fn_to_closure_forwarders(&mut a);
        ast::desugar_function_prototype_methods(&mut a);
        // P2.1 — order matters: uninit_let inlines `let x; x = e` into
        // `let x = e` for early type binding. var_hoist creates synthetic
        // `let x = Uninit` that should NOT be inlined (var semantics
        // require x to be undefined at every read before its assignment,
        // not at the declaration site). Run uninit_let FIRST so it only
        // sees user-written `let x;` (which IS legal to inline), then
        // var_hoist inserts hoisted `let x = Uninit` that uninit_let
        // never gets a chance to touch.
        ast::desugar_uninit_let(&mut a);
        ast::desugar_var_hoist(&mut a);
        ast::desugar_array_isarray_value(&mut a);
        ast::desugar_arguments_object(&mut a);
        ast::rewrite_split_for_i_to_iter(&mut a);
        ast::escape_analyze_array_literals(&mut a);
        ast::desugar_implicit_generics(&mut a);
        ast::apply_default_args(&mut a);
        ast::apply_rest_args(&mut a);
        ast::compute_consuming_params(&mut a);
        let (gcs, exprs) = torajs_core::check::check_with_types(&a).unwrap();
        let m = ssa_lower::lower_with_types(&a, &gcs, &exprs);
        let out = std::env::temp_dir().join(format!("torajs-embed-test-{}.dylib", rand_suffix()));
        ssa_inkwell::compile_for_kind(
            &m,
            &out,
            "O3",
            None,
            Some(&a),
            ssa_inkwell::CompileTarget::Native,
            ssa_inkwell::OutputKind::SharedLib,
        )
        .expect("compile_for_kind SharedLib");
        let meta = std::fs::metadata(&out).expect("dylib stat");
        assert!(meta.len() > 0, "dylib is empty");

        // dlopen + look up the symbol + call it.
        unsafe {
            let lib = libloading::Library::new(&out).expect("Library::new on emitted dylib");
            let add: libloading::Symbol<unsafe extern "C" fn(i64, i64) -> i64> =
                lib.get(b"torajs_add").expect("symbol torajs_add");
            assert_eq!(add(2, 3), 5);
            assert_eq!(add(40, 2), 42);
            assert_eq!(add(-1, 1), 0);
            drop(lib);
        }
        let _ = std::fs::remove_file(&out);
    }

    /// V3-16 m2 — public Rust API surface for compile + dlopen +
    /// resolve-symbol. The substrate that the in-tora `Function`
    /// constructor / `eval(src)` will lower to once the syntactic
    /// surface lands.
    #[test]
    fn compile_function_arith() {
        let src = r#"
function torajs_mul(a: number, b: number): number {
  return a * b;
}
"#;
        let f =
            unsafe { compile_function::<unsafe extern "C" fn(i64, i64) -> i64>(src, "torajs_mul") }
                .expect("compile_function");
        unsafe {
            assert_eq!(f.ptr()(6, 7), 42);
            assert_eq!(f.ptr()(-3, 5), -15);
            assert_eq!(f.ptr()(0, 999), 0);
        }
    }

    /// Two LoadedFunction handles in flight at the same time —
    /// each owns its own dlopen handle, drops independently.
    #[test]
    fn compile_function_two_handles() {
        let src1 = r#"
function f1(x: number): number { return x + 1; }
"#;
        let src2 = r#"
function f2(x: number): number { return x * 10; }
"#;
        let h1 =
            unsafe { compile_function::<unsafe extern "C" fn(i64) -> i64>(src1, "f1") }.unwrap();
        let h2 =
            unsafe { compile_function::<unsafe extern "C" fn(i64) -> i64>(src2, "f2") }.unwrap();
        unsafe {
            assert_eq!(h1.ptr()(5), 6);
            assert_eq!(h2.ptr()(5), 50);
            // Cross-call: handles are independent.
            assert_eq!(h1.ptr()(h2.ptr()(3) as i64), 31);
        }
    }
}
