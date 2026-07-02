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
                let target = self.console_print_target(method, Type::Str);
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(target, vec![Operand::Value(lit)]),
                );
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
                let target = self.console_print_target(method, Type::Str);
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(target, vec![Operand::Value(owned)]),
                );
                self.emit_drop_value(Operand::Value(owned), Type::Str);
                if !is_borrow {
                    self.emit_drop_value(arg, Type::Substr);
                }
                return;
            }
            /* T-25 — BigInt prints via bigint_to_string + str_concat
             * with `"n"` (matches node/bun console.log formatting,
             * which appends the `n` suffix even though `toString()`
             * itself doesn't). The two intermediate Strs are
             * fresh-owned: drop both after print. The BigInt input
             * drops if the source binding wasn't a borrow target. */
            if arg_ty == Type::BigInt {
                let body = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.bigint_to_string, vec![arg]),
                    Type::Str,
                    None,
                );
                let n_lit = self.intern_string_literal("n");
                let formatted = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.str_concat,
                        vec![Operand::Value(body), Operand::Value(n_lit)],
                    ),
                    Type::Str,
                    None,
                );
                let target = self.console_print_target(method, Type::Str);
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(target, vec![Operand::Value(formatted)]),
                );
                self.emit_drop_value(Operand::Value(formatted), Type::Str);
                self.emit_drop_value(Operand::Value(body), Type::Str);
                if !is_borrow {
                    self.emit_drop_value(arg, Type::BigInt);
                }
                return;
            }
            let is_str = arg_ty == Type::Str;
            let target = self.console_print_target(method, arg_ty);
            self.f
                .append_void(self.cur_block, InstKind::Call(target, vec![arg]));
            if is_str && !is_borrow {
                self.emit_drop_value(arg, Type::Str);
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
            let target = self.console_print_target(method, Type::Str);
            let final_str = acc.unwrap();
            self.f
                .append_void(self.cur_block, InstKind::Call(target, vec![final_str]));
            self.emit_drop_value(final_str, Type::Str);
            return;
        }
        self.lower_stmt(s);
    }
}
