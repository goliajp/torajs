//! Top-level statement lowering for `LowerCtx<'a>` extracted from
//! `ssa_lower.rs` chunk 400 — Path A.3-batch21.
//!
//! Single method: `lower_top_stmt(s)` — specialized entry for the
//! synthesized `main` function's top-level statements. Provides a
//! type-directed `console.log/error/warn` fast path that emits
//! direct `print_*` intrinsic calls instead of walking through the
//! generic `Expr::Call` dispatch. Handles:
//!
//! - single-arg `console.<m>(x)` with type-dispatched print target
//!   (Null/Undefined → literal label, Substr → materialized owned
//!   Str, BigInt → bigint_to_string + "n" suffix, Str/I64/other →
//!   direct print);
//! - single-arg borrow vs. owned drop semantics (Ident / Member /
//!   Index expressions don't own the heap so no drop);
//! - multi-arg `console.log` per-arg inspect dispatch via sibling
//!   `ssa_lower_console_log_multiarg`;
//! - multi-arg `console.error/warn` Str-coerce + str_concat joiner
//!   path;
//! - fallback to the general `lower_stmt` path when the statement
//!   isn't a `console.*` call.

use crate::ast::{Expr, ExprId, Stmt};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    /// Top-level statement lowering inside the synthesized `main` function.
    /// `console.log(<expr>)` dispatches on the lowered operand's type:
    ///   - Type::Str → `call print_str(<ptr>)`
    ///   - Type::I64 / others → `call print_i64(<value>)`
    /// Same dispatch handles literal strings (`Expr::String`) and string
    /// bindings — the literal path interns through `lower_expr`'s general
    /// `Expr::String` arm and gets the same Type::Str operand.
    pub(crate) fn lower_top_stmt(&mut self, s: &Stmt) {
        if let Stmt::Expr(eid) = s
            && let Expr::Call { callee, args } = self.ast.get_expr(*eid)
            && let Some(method) = self.console_method_member(*callee)
            && args.len() == 1
        {
            // V3-18 Phase D + S139 — `console.log(null)` / `console.
            // log(undefined)` print 'null' / 'undefined' (per Node
            // util.inspect), not '0' as the generic Type::Ptr path
            // would. Both lower to ConstPtrNull at the runtime layer,
            // so we use the frontend type (expr_types) as the source
            // of truth. This covers the literal forms (Expr::Null,
            // Expr::Ident("undefined")) AND derived expressions like
            // `null && 'x'` (S138) whose result type is statically
            // Null/Undefined.
            let arg_check_ty = self.expr_types.get(&args[0]).cloned();
            let prim_label = match arg_check_ty {
                Some(crate::check::Type::Null) => Some("null"),
                Some(crate::check::Type::Undefined) => Some("undefined"),
                _ => None,
            };
            if let Some(label) = prim_label {
                // Side-effects: lower the arg first (in case it's a
                // Call), discard its value; then emit the literal
                // label via the str print path.
                let _ = self.lower_expr(args[0]);
                let lit = self.intern_string_literal(label);
                let target = self.console_print_target(Type::Str);
                self.emit_console_print(method, target, Operand::Value(lit));
                return;
            }
            let is_borrow = matches!(
                self.ast.get_expr(args[0]),
                Expr::Ident(_) | Expr::Member { .. } | Expr::Index { .. }
            );
            let arg = self.lower_expr(args[0]);
            let arg_ty = self.operand_ty(&arg);
            // Substr: materialize to owned Str (always-drop), then print as Str.
            if arg_ty == Type::Substr {
                let owned = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.substr_to_owned, vec![arg]),
                    Type::Str,
                    None,
                );
                let target = self.console_print_target(Type::Str);
                self.emit_console_print(method, target, Operand::Value(owned));
                self.emit_drop_value(Operand::Value(owned), Type::Str);
                if !is_borrow {
                    self.emit_drop_value(arg, Type::Substr);
                }
                return;
            }
            /* T-25 — BigInt prints via bigint_to_string + str_concat
             * with `"n"` (matches node/bun console.log formatting,
             * which appends the `n` suffix even though `toString()`
             * itself doesn't). Rotation 326 — routed through
             * `coerce_to_str`, which owns the sentinel gate: an OOB /
             * miss read answers the immortal generic undefined cell,
             * and this inlined copy read its limbs (SIGBUS on
             * `console.log(bs[5])`) while the coercer's copy got the
             * guard — the two lanes were the same logic drifted
             * apart. The coercer answers a fresh-owned Str (static
             * "undefined" on the sentinel arm; drop no-ops). */
            if arg_ty == Type::BigInt {
                let owned = self.coerce_to_str(arg.clone(), Type::BigInt);
                let target = self.console_print_target(Type::Str);
                self.emit_console_print(method, target, owned.clone());
                self.emit_drop_value(owned, Type::Str);
                if !is_borrow {
                    self.emit_drop_value(arg, Type::BigInt);
                }
                return;
            }
            let is_str = arg_ty == Type::Str;
            // RFC 20260708-typed-arr-oob-read chunk 2 — a possibly-
            // sentinel F64 arg branches to the Str printer with the
            // immortal "undefined" cell (mirror of the in-expr
            // console lane's gate).
            if arg_ty == Type::F64
                && crate::ssa_lower_undef_f64_source::is_undef_f64_source(self, args[0])
            {
                crate::ssa_lower_call_console::lower_print_f64_or_undef(self, method, arg);
                return;
            }
            let target = self.console_print_target(arg_ty);
            // RFC 20260710 C2b — an Obj/Arr/Closure arg may hold the
            // generic undefined cell (Nullable slot); branch to the
            // "undefined" label print (shared helper — mirror of the
            // in-expr console lane).
            let sentinel_join = crate::ssa_lower_call_console::open_console_sentinel_branch(
                self, method, &arg, arg_ty,
            );
            // RFC 20260704 L3b #5 — typed Arr with no dedicated typed
            // printer routes through the tag-aware print_any; this
            // direct path never crosses the boxing boundary, so mark
            // the elem-kind chain here (same wiring as
            // `lower_single_arg` — unmarked, the walker reads raw i64
            // slots as NaN-box cell pointers and crashes).
            if target == self.intrinsics.print_any && matches!(arg_ty, Type::Arr(_)) {
                self.emit_arr_mark_kind(&arg);
            }
            self.emit_console_print(method, target, arg.clone());
            crate::ssa_lower_call_console::close_console_sentinel_branch(self, sentinel_join);
            if !is_borrow {
                if is_str {
                    self.emit_drop_value(arg, Type::Str);
                } else if arg_ty.is_refcounted() && self.expr_owned_shape(args[0]) {
                    // Rotation 542 — same release the in-fn lane
                    // takes; without it a top-level
                    // `console.log(new Date())` stranded its temp.
                    // The two lanes had drifted on the drop the way
                    // they had on the BigInt print target.
                    self.emit_drop_value(arg, arg_ty);
                }
            }
            return;
        }
        // Multi-arg `console.log` per-arg inspect dispatch lives in
        // the sibling module so the `lower_stmt` (try-body) caller
        // can share it.
        if crate::ssa_lower_console_log_multiarg::try_lower(self, s) {
            return;
        }
        // Multi-arg `console.error` / `console.warn` — pre-existing
        // Str-coerce + str_concat joiner path. typed Arr / Obj /
        // Map / Set still panic here (only the `log` variant is
        // upgraded to per-arg inspect dispatch above); the stderr
        // arms see less coverage in conformance and the panic
        // surface is unchanged from the baseline.
        if let Stmt::Expr(eid) = s
            && let Expr::Call { callee, args } = self.ast.get_expr(*eid)
            && let Some(method) = self.console_method_member(*callee)
            && args.len() > 1
        {
            let arg_ids: Vec<ExprId> = args.clone();
            let space_str = self.intern_string_literal(" ");
            let mut acc: Option<Operand> = None;
            for (i, &aid) in arg_ids.iter().enumerate() {
                let arg = self.lower_expr(aid);
                let arg_ty = self.operand_ty(&arg);
                let s_op = self.coerce_to_str(arg, arg_ty);
                if i > 0 {
                    let prev = acc.unwrap();
                    let with_sep = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.str_concat,
                            vec![prev, Operand::Value(space_str)],
                        ),
                        Type::Str,
                        None,
                    );
                    let combined = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.str_concat,
                            vec![Operand::Value(with_sep), s_op],
                        ),
                        Type::Str,
                        None,
                    );
                    acc = Some(Operand::Value(combined));
                } else {
                    acc = Some(s_op);
                }
            }
            let target = self.console_print_target(Type::Str);
            let final_str = acc.unwrap();
            self.emit_console_print(method, target, final_str.clone());
            self.emit_drop_value(final_str, Type::Str);
            return;
        }
        self.lower_stmt(s);
    }
}
