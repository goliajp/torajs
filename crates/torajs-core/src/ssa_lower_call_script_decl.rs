//! RFC 20260912-global-var-cell knife 5 — lowering of the synthetic
//! calls a Script evaluated inside a function is made of
//! (`check_type_of_call_script_decl` is the checker half).
//!
//! The name operand is a string literal, so it lowers to the interned
//! Str the kernels borrow (the same shape `emit_script_global_var_bind`
//! hands `__torajs_global_var_bind`). The function value of
//! `__torajs_global_fn_declare` is boxed to Any under the
//! three-shape ownership rule the `__torajs_heritage_check` lowering
//! follows: a borrow-shape source takes a +1 before boxing, and the
//! boxed temp is released after the kernel returns (the kernel takes
//! its own stake). Every kernel may record a throw.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// `globalthis_object_decls::PREFLIGHT_*` — the kernel's `kind`.
const PREFLIGHT_LEX: i64 = 0;
const PREFLIGHT_FN: i64 = 1;
const PREFLIGHT_VAR: i64 = 2;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Ident(n) = ctx.ast.get_expr(callee) else {
        return None;
    };
    if !crate::check_type_of_call_script_decl::is_script_decl_synthetic(n, args.len()) {
        return None;
    }
    let name = ctx.lower_expr(args[0]);
    let cur_block = ctx.cur_block;
    match n.as_str() {
        "__torajs_script_preflight_lex"
        | "__torajs_script_preflight_fn"
        | "__torajs_script_preflight_var" => {
            let kind = match n.as_str() {
                "__torajs_script_preflight_lex" => PREFLIGHT_LEX,
                "__torajs_script_preflight_fn" => PREFLIGHT_FN,
                _ => PREFLIGHT_VAR,
            };
            let kernel = ctx.intrinsics.script_preflight;
            ctx.f.append_void(
                cur_block,
                InstKind::Call(kernel, vec![Operand::ConstI64(kind), name]),
            );
        }
        "__torajs_global_var_declare" => {
            let kernel = ctx.intrinsics.global_var_declare;
            ctx.f
                .append_void(cur_block, InstKind::Call(kernel, vec![name]));
        }
        _ => {
            let raw = ctx.lower_expr(args[1]);
            let (value, boxed) = if ctx.operand_ty(&raw) == Type::Any {
                (raw, None)
            } else {
                if matches!(
                    ctx.ast.get_expr(args[1]),
                    Expr::Ident(_) | Expr::Member { .. }
                ) && ctx.operand_ty(&raw).is_refcounted()
                {
                    ctx.emit_rc_inc(raw.clone());
                }
                let b = ctx.box_to_any_from_expr(args[1], raw);
                (b.clone(), Some(b))
            };
            let kernel = ctx.intrinsics.global_fn_declare;
            let cur_block = ctx.cur_block;
            ctx.f
                .append_void(cur_block, InstKind::Call(kernel, vec![name, value]));
            if let Some(b) = boxed {
                ctx.emit_drop_value(b, Type::Any);
            }
        }
    }
    ctx.emit_throw_check(None);
    Some(Operand::ConstPtrNull)
}
