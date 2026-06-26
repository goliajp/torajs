//! RegExp instance method dispatch (`re.test(s)` / `re.exec(s)` /
//! `re.toString()`) pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as
//! chunk-35 of the `Expr::Call` god-arm decomp (chunks 1-34 = ... +
//! Date instance method dispatch).
//!
//! v0.2 #1 — `re.method(args)` for the RegExp stdlib slice. Receiver
//! type-gated on `Type::RegExp`; methods route to the matching
//! `__torajs_regex_*` runtime intrinsic. Args are borrow-shaped
//! (the runtime helpers don't take ownership — the caller's drop
//! walk handles it).
//!
//! - `test(s)` — bool result (`__torajs_regex_test`); S266 trailing
//!   args silent-drop per ES §22.2.6.16.
//! - `exec(s)` — `Array<string>` result (`__torajs_regex_exec`,
//!   interned `Type::Arr(elem=Str)`); S266 trailing args silent-drop
//!   per ES §22.2.6.2.
//! - `toString()` — `/source/flags` per ES §22.2.6.13; one
//!   runtime alloc avoids the SSA-level concat overhead (`.source` /
//!   `.flags` each would otherwise allocate an intermediate Str +
//!   need explicit drops). S266 trailing args silent-drop.
//!
//! Returns `Some(op)` on hit; `None` on miss (callee not a
//! Member-call, method name not in {test, exec, toString}, or
//! receiver isn't `Type::RegExp`) so the caller falls through to the
//! `<Str>.{replace|split|match|matchAll}` regex-receiver arm and
//! beyond.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LowerCtx, intern_arr_layout};

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member { obj, name } = ctx.ast.get_expr(callee) else {
        return None;
    };
    if !matches!(name.as_str(), "test" | "exec" | "toString") {
        return None;
    }
    let recv_id = *obj;
    let method = name.clone();
    let recv_op = ctx.lower_expr(recv_id);
    let recv_ty = ctx.operand_ty(&recv_op);
    if recv_ty != Type::RegExp {
        return None;
    }

    let cur_block = ctx.cur_block;
    match method.as_str() {
        "toString" => {
            // S266 — trailing args silent-drop per ES §22.2.6.16.
            for a in args.iter() {
                let _ = ctx.lower_expr(*a);
            }
            let v = ctx.f.append_inst(
                cur_block,
                InstKind::Call(ctx.intrinsics.regex_to_string, vec![recv_op]),
                Type::Str,
                None,
            );
            Some(Operand::Value(v))
        }
        "test" => {
            // S266 — trailing args silent-drop per ES §22.2.6.16.
            debug_assert!(!args.is_empty());
            let s = ctx.lower_expr(args[0]);
            for a in args.iter().skip(1) {
                let _ = ctx.lower_expr(*a);
            }
            let v = ctx.f.append_inst(
                cur_block,
                InstKind::Call(ctx.intrinsics.regex_test, vec![recv_op, s]),
                Type::Bool,
                None,
            );
            Some(Operand::Value(v))
        }
        "exec" => {
            // S266 — trailing args silent-drop per ES §22.2.6.2.
            debug_assert!(!args.is_empty());
            let s = ctx.lower_expr(args[0]);
            for a in args.iter().skip(1) {
                let _ = ctx.lower_expr(*a);
            }
            let arr_id = intern_arr_layout(ctx.arr_layouts, Type::Str);
            let v = ctx.f.append_inst(
                cur_block,
                InstKind::Call(ctx.intrinsics.regex_exec, vec![recv_op, s]),
                Type::Arr(arr_id),
                None,
            );
            Some(Operand::Value(v))
        }
        _ => unreachable!("regex method `{method}` not yet wired"),
    }
}
