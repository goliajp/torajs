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
    let tokens = lexer::tokenize(src)
        .map_err(|e| EvalOutcome::CompileError(format!("lex: {e}")))?;
    let mut a = parser::parse(&tokens)
        .map_err(|e| EvalOutcome::CompileError(format!("parse: {e}")))?;
    a.source = src.to_string();
    a.warm_newline_cache();
    ast::unwrap_exports(&mut a);
    ast::rename_user_main(&mut a);
    ast::desugar_generators(&mut a);
    ast::desugar_async(&mut a);
    ast::desugar_builtin_imports(&mut a);
    ast::desugar_builtin_new(&mut a);
    ast::desugar_classes(&mut a);
    ast::lift_arrow_fns(&mut a);
    ast::infer_anonymous_closure_params(&mut a);
    ast::synthesize_forwarders(&mut a);
    ast::desugar_uninit_let(&mut a);
    ast::desugar_arguments_object(&mut a);
    ast::rewrite_split_for_i_to_iter(&mut a);
    ast::escape_analyze_array_literals(&mut a);
    ast::desugar_implicit_generics(&mut a);
    ast::apply_default_args(&mut a);
    ast::apply_rest_args(&mut a);
    ast::compute_consuming_params(&mut a);
    let (gcs, expr_types) = check::check_with_types(&a)
        .map_err(|e| EvalOutcome::CompileError(format!("type: {e}")))?;
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
        assert!(matches!(out, EvalOutcome::Ok { exit_code: 0 }), "got {out:?}");
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
}
