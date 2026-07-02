//! Callee resolution helper for `LowerCtx<'a>` extracted from
//! `ssa_lower.rs` chunk 392 — Path A.3-batch13.
//!
//! Single method:
//!
//! - `resolve_callee(eid)` — ExprId → FuncId resolution for direct
//!   function calls. Handles two receiver shapes:
//!   - `Expr::Ident(name)` → look up in `self.fn_table`
//!   - `Expr::Member { obj, name }` → dispatch on the receiver identity:
//!     - `Math.<method>` → `intrinsics.math_*` (36 methods)
//!     - `Date.<static>` → `intrinsics.date_*` (now / parse / UTC)
//!     - `fs.<method>` → `intrinsics.fs_*` (7 methods)
//!     - `process.<method>` → `intrinsics.process_*` (exit / cwd)
//!     - `process.{stdout,stderr}.write` → `intrinsics.process_*_write`
//!     - `Bun.<method>` → aliased intrinsics (write → fs_write_file_sync)
//!
//! Fn-typed locals are handled BEFORE this in `lower_expr`'s Call arm
//! (CallIndirect path). `console.log(...)` is handled by the top-level
//! shortcut in `lower_top_stmt`, so it never reaches here as a regular
//! Call. Unknown member shapes panic with a diagnostic message.
//!
//! Method body is byte-for-byte preserved from the source; the sibling
//! reaches LowerCtx fields via `impl<'a> super::LowerCtx<'a>`, so call
//! sites need zero edits.

use crate::ast::{Expr, ExprId};
use crate::ssa::FuncId;
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    pub(crate) fn resolve_callee(&self, eid: ExprId) -> FuncId {
        match self.ast.get_expr(eid) {
            Expr::Ident(name) => {
                // Resolve direct fn calls: callee Ident matches a global
                // FnDecl. Fn-typed locals are handled BEFORE this in
                // `lower_expr`'s Call arm (CallIndirect path).
                match self.fn_table.get(name) {
                    Some(f) => *f,
                    None => panic!("ssa-lower: unknown function `{name}`"),
                }
            }
            // Member call — currently only `Math.<method>` resolves here.
            // `console.log(...)` is handled by the top-level shortcut in
            // `lower_top_stmt`, so it never reaches here as a regular Call.
            Expr::Member { obj, name } => {
                let is_math = matches!(self.ast.get_expr(*obj), Expr::Ident(n) if n == "Math");
                if is_math {
                    return match name.as_str() {
                        "sqrt" => self.intrinsics.math_sqrt,
                        "abs" => self.intrinsics.math_abs,
                        "floor" => self.intrinsics.math_floor,
                        "ceil" => self.intrinsics.math_ceil,
                        "log" => self.intrinsics.math_log,
                        "exp" => self.intrinsics.math_exp,
                        "pow" => self.intrinsics.math_pow,
                        "min" => self.intrinsics.math_min,
                        "max" => self.intrinsics.math_max,
                        "sign" => self.intrinsics.math_sign,
                        "round" => self.intrinsics.math_round,
                        "trunc" => self.intrinsics.math_trunc,
                        "sin" => self.intrinsics.math_sin,
                        "cos" => self.intrinsics.math_cos,
                        "tan" => self.intrinsics.math_tan,
                        "asin" => self.intrinsics.math_asin,
                        "acos" => self.intrinsics.math_acos,
                        "atan" => self.intrinsics.math_atan,
                        "atan2" => self.intrinsics.math_atan2,
                        "log2" => self.intrinsics.math_log2,
                        "log10" => self.intrinsics.math_log10,
                        "cbrt" => self.intrinsics.math_cbrt,
                        "sinh" => self.intrinsics.math_sinh,
                        "cosh" => self.intrinsics.math_cosh,
                        "tanh" => self.intrinsics.math_tanh,
                        "asinh" => self.intrinsics.math_asinh,
                        "acosh" => self.intrinsics.math_acosh,
                        "atanh" => self.intrinsics.math_atanh,
                        "expm1" => self.intrinsics.math_expm1,
                        "log1p" => self.intrinsics.math_log1p,
                        "imul" => self.intrinsics.math_imul,
                        "clz32" => self.intrinsics.math_clz32,
                        "fround" => self.intrinsics.math_fround,
                        "f16round" => self.intrinsics.math_f16round,
                        "sumPrecise" => self.intrinsics.math_sum_precise,
                        "random" => self.intrinsics.math_random,
                        other => {
                            panic!("ssa-lower: unknown Math method `{other}`")
                        }
                    };
                }
                /* v0.2 #2 — Date.<static>. */
                let is_date = matches!(self.ast.get_expr(*obj), Expr::Ident(n) if n == "Date");
                if is_date {
                    return match name.as_str() {
                        "now" => self.intrinsics.date_now_static,
                        "parse" => self.intrinsics.date_parse_iso,
                        "UTC" => self.intrinsics.date_utc_components,
                        other => panic!("ssa-lower: unknown Date static method `{other}`"),
                    };
                }
                /* v0.3 #1 — fs.<method>. */
                let is_fs = matches!(self.ast.get_expr(*obj), Expr::Ident(n) if n == "fs");
                if is_fs {
                    return match name.as_str() {
                        "readFileSync" => self.intrinsics.fs_read_file_sync,
                        "writeFileSync" => self.intrinsics.fs_write_file_sync,
                        "existsSync" => self.intrinsics.fs_exists_sync,
                        "appendFileSync" => self.intrinsics.fs_append_file_sync,
                        "unlinkSync" => self.intrinsics.fs_unlink_sync,
                        "mkdirSync" => self.intrinsics.fs_mkdir_sync,
                        "readdirSync" => self.intrinsics.fs_readdir_sync,
                        other => panic!("ssa-lower: unknown fs method `{other}`"),
                    };
                }
                /* v0.3 #3 — process.<method>. */
                let is_process =
                    matches!(self.ast.get_expr(*obj), Expr::Ident(n) if n == "process");
                if is_process {
                    return match name.as_str() {
                        "exit" => self.intrinsics.process_exit,
                        "cwd" => self.intrinsics.process_cwd,
                        other => panic!("ssa-lower: unknown process method `{other}`"),
                    };
                }
                /* T-03 (v0.3.0) — process.{stdout, stderr}.write(s)
                 * and process.stdin.read(). The receiver here is a
                 * Member, not an Ident, so dispatch on the inner
                 * Member shape. */
                if let Expr::Member {
                    obj: inner_obj,
                    name: inner_name,
                } = self.ast.get_expr(*obj).clone()
                    && matches!(self.ast.get_expr(inner_obj), Expr::Ident(n) if n == "process")
                {
                    return match (inner_name.as_str(), name.as_str()) {
                        ("stdout", "write") => self.intrinsics.process_stdout_write,
                        ("stderr", "write") => self.intrinsics.process_stderr_write,
                        other => panic!(
                            "ssa-lower: unsupported process.{}.{} call",
                            other.0, other.1
                        ),
                    };
                }
                /* v0.3 #2 — Bun.<method>. Aliases to existing intrinsics. */
                let is_bun = matches!(self.ast.get_expr(*obj), Expr::Ident(n) if n == "Bun");
                if is_bun {
                    return match name.as_str() {
                        "write" => self.intrinsics.fs_write_file_sync,
                        other => panic!("ssa-lower: unknown Bun method `{other}`"),
                    };
                }
                panic!("ssa-lower: unsupported member call shape: {name}")
            }
            other => panic!("ssa-lower: unsupported callee form: {other:?}"),
        }
    }
}
