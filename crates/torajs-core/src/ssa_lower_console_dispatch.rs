//! Console-call dispatch helpers for `LowerCtx<'a>` extracted from
//! `ssa_lower.rs` chunk 374.
//!
//! `console.{log,error,warn,info,debug}(...)` recognizer +
//! print-intrinsic picker: `console_method_member` returns the method
//! name as a static string (or None for non-console calls);
//! `console_print_target` maps `arg_ty` to the appropriate runtime
//! print helper (Str / Substr / F64 / Bool / Any / Symbol / typed
//! Arr walkers / Map / Set / FnSig / typed heap receivers / catch-all
//! int); `emit_console_print` is the one gate every console print
//! call goes through — it brackets the stderr methods with the io
//! current-sink switch (RFC 20260812-console-sink). Siblings and
//! `ssa_lower.rs` reach them through the impl block on the shared
//! `crate::ssa_lower::LowerCtx` type.

use crate::ast::{Expr, ExprId};
use crate::ssa::{FuncId, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    /// `console.{log,error,warn}` recognizer returning the method name as
    /// a static string (or None). Used to dispatch the appropriate
    /// print intrinsic in lower_top_stmt + the in-expr console-call arm.
    pub(crate) fn console_method_member(&self, eid: ExprId) -> Option<&'static str> {
        if let Expr::Member { obj, name } = self.ast.get_expr(eid)
            && let Expr::Ident(ns) = self.ast.get_expr(*obj)
            && ns == "console"
        {
            return match name.as_str() {
                "log" => Some("log"),
                "error" => Some("error"),
                "warn" => Some("warn"),
                // S328 — WHATWG console §1.1.{2,4} — info / debug
                // alias log (stdout) in bun/node behavior. Print
                // routing in `console_print_target` keeps both on
                // the non-stderr branch.
                "info" => Some("info"),
                "debug" => Some("debug"),
                _ => None,
            };
        }
        None
    }

    /// Pick the right print intrinsic for `console.<method>(<arg>)`.
    ///
    /// RFC 20260812-console-sink knife 2 — the stderr methods are no
    /// longer a mapping concern: every method uses the one printer
    /// table below, and [`Self::emit_console_print`] brackets the
    /// call with the io current-sink switch for `error` / `warn`.
    /// (Pre-knife-2, only Str/F64/Bool/I64 had `_err` twins and every
    /// other type silently printed to stdout — content right, stream
    /// wrong.)
    pub(crate) fn console_print_target(&self, arg_ty: Type) -> FuncId {
        match arg_ty {
            Type::Str => self.intrinsics.str_print,
            // V3-18 m1.h.34 — Substr layout differs from Str
            // (parent+offset+len vs inline data). Dedicated
            // substr_print walks parent + offset; pre-fix Substr
            // fell through to the catch-all print_i64 which printed
            // the pointer-as-integer (or nothing for empty), so any
            // `console.log("a-b".split("-")[0])` etc diverged from
            // bun.
            Type::Substr => self.intrinsics.substr_print,
            Type::F64 => self.intrinsics.print_f64,
            Type::Bool => self.intrinsics.print_bool,
            // T-10.d.i — Type::Any operand routes through the
            // tag-aware `__torajs_print_any` runtime helper.
            Type::Any => self.intrinsics.print_any,
            // T-13.a — Type::Symbol prints `Symbol(<desc>)` via the
            // dedicated runtime helper.
            Type::Symbol => self.intrinsics.symbol_print,
            // Rotation 542 — Type::BigInt joins the tag-aware
            // `print_anyv` family, whose `Tag::BigInt` arm emits
            // `<decimal>n` (the bun form) via
            // `__torajs_bigint_print_inline`. Without an arm here a
            // BigInt fell into the `print_i64` catch-all below and
            // printed the raw cell pointer as a decimal — the exact
            // failure the Substr and Commit-4 arms above were added
            // for, and SILENT: `function f() { console.log(2n) }`
            // printed `4312514640` while the same statement at top
            // level printed `2n`, because `lower_top_stmt` carries
            // its own BigInt arm and this shared target table did
            // not. Every neighbouring lane already had it — the
            // multi-arg joiner coerces through `coerce_to_str`, a
            // boxed `any` prints through this same tag walker, and
            // `bigint[]` reaches it through the Arr arm's `_`.
            Type::BigInt => self.intrinsics.print_any,
            // V3-18 m1.h.12 — `console.log(arr)` array pretty-print.
            // Per element type: I64 / F64 / Bool / Str / Substr; any
            // other elem type (Any / Arr<...> / Obj / Map / Set /
            // Closure / etc) routes through the tag-aware
            // __torajs_print_anyv (Commit 4 wired its Tag::Arr +
            // Tag::DynObj walkers; Commits 5-8 wire the remaining
            // typed Tag walkers). Closes W-O-1 (`const a:any[]=[]`),
            // W-O-3-nested (`console.log(Object.entries(o))`).
            Type::Arr(arr_id) => {
                let elem_ty = self.arr_layouts[arr_id.0 as usize];
                match elem_ty {
                    Type::I64 => self.intrinsics.arr_print_i64,
                    Type::F64 => self.intrinsics.arr_print_f64,
                    Type::Bool => self.intrinsics.arr_print_bool,
                    // V3-18 m1.h.28 — Substr layout differs from Str
                    // (parent + offset + len vs inline data); pick the
                    // matching helper. Pre-fix arr_print_str read
                    // parent-pointer bytes as data and printed garbage.
                    Type::Str => self.intrinsics.arr_print_str,
                    Type::Substr => self.intrinsics.arr_print_substr,
                    // Nested-print substrate trunk Commit 4.
                    _ => self.intrinsics.print_any,
                }
            }
            // Nested-print substrate trunk Commit 4 — typed heap
            // receivers (Type::Obj / Promise / Date / RegExp /
            // Closure / WeakRef / WeakMap / WeakSet / MapIter /
            // ArrIter) route through __torajs_print_anyv, which
            // reads HeapHeader::type_tag and dispatches to the
            // matching typed walker. Pre-Commit 4 these all fell
            // through to print_i64 below, which emitted the raw
            // heap pointer as a decimal.
            // Commit 7 — Map / Set route through dedicated wrappers
            // because runtime Tag::Map=15 covers BOTH Map and Set
            // heap blocks (no separate Tag::Set). Going through
            // print_any would print Sets as `Map(...)`.
            Type::Map => self.intrinsics.map_print_outer,
            Type::Set => self.intrinsics.set_print_outer,
            // Fn-name registry Phase 1 narrow — Type::FnSig is a
            // raw code-section pointer (not a heap object) so it
            // can't go through print_any's NaN-box tag-walker
            // (top16 of __TEXT vaddr is usually nonzero, so
            // `is_cell` returns false → `[unknown-any-tag]`
            // fallthrough). The dedicated outer wrapper emits
            // `[Function]\n` directly; Phase 2 swaps the body for
            // the rodata table binary-search.
            Type::FnSig(_) => self.intrinsics.fn_print_outer,
            Type::Obj(_)
            | Type::Promise
            | Type::Date
            | Type::RegExp
            | Type::Closure(_)
            | Type::WeakRef
            | Type::WeakMap
            | Type::WeakSet
            | Type::MapIter
            | Type::ArrIter => self.intrinsics.print_any,
            _ => self.intrinsics.print_i64,
        }
    }

    /// Emit one `console.<method>` print call, bracketed with the io
    /// current-sink switch when the method targets stderr (`error` /
    /// `warn`) — RFC 20260812-console-sink knife 2. The bracket must
    /// wrap only the print call, never the argument lowering: a user
    /// `toString` running during coercion may itself `console.log`,
    /// and that output belongs on stdout. Every console print call
    /// site goes through here so a new lowering branch cannot
    /// silently print to the wrong stream. Both switch intrinsics
    /// drain the buffer they leave, so `2>&1` keeps caller order;
    /// they never throw (fn_meta no-throw set).
    pub(crate) fn emit_console_print(&mut self, method: &str, target: FuncId, arg: Operand) {
        let to_stderr = matches!(method, "error" | "warn");
        if to_stderr {
            let cb = self.cur_block;
            self.f.append_void(
                cb,
                InstKind::Call(self.intrinsics.io_sink_to_stderr, vec![]),
            );
        }
        let cb = self.cur_block;
        self.f.append_void(cb, InstKind::Call(target, vec![arg]));
        if to_stderr {
            let cb = self.cur_block;
            self.f.append_void(
                cb,
                InstKind::Call(self.intrinsics.io_sink_to_stdout, vec![]),
            );
        }
    }
}
