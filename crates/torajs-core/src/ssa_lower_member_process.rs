//! `process.{platform|argv|env}` + `Bun.argv` + `process.env.NAME`
//! Member-access cluster pulled out of
//! [`crate::ssa_lower::lower_expr_inner`]'s `Expr::Member` god-arm as
//! chunk-57 of the decomp. First non-`Expr::Call` god-arm chunk —
//! this sibling owns four atomic Member-shape lowerings, each a
//! self-contained intrinsic Call or constant operand.
//!
//! - **v0.3 #3** — `process.platform` → `__torajs_process_platform()`
//!   `Type::Str` (other `process.*` are calls and route through
//!   `resolve_callee`).
//! - **v0.3 #3.c** — `process.argv` / `Bun.argv` →
//!   `__torajs_process_argv()` building an `Array<Str>` (element type
//!   interned through the same `arr_layouts` path other Array
//!   builders use).
//! - **v0.3 #3** — `process.env` namespace marker → `ConstPtrNull`
//!   (zero-cost; the actual env lookup fires when this is the
//!   receiver of a Member access, see next bullet).
//! - **v0.3 #3** — `process.env.<NAME>` → `__torajs_process_getenv`
//!   with the property name interned as a `Str` literal. The
//!   `process.env` marker (ConstPtrNull) above is the receiver and
//!   gets discarded — only the name is forwarded.
//!
//! Returns `Some(op)` on hit; `None` on miss. Sibling tries each
//! arm in source order and falls through the chain.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LowerCtx, intern_arr_layout};

pub(crate) fn try_lower(ctx: &mut LowerCtx<'_>, obj: ExprId, name: &str) -> Option<Operand> {
    let obj_expr = ctx.ast.get_expr(obj);
    if let Expr::Ident(n) = obj_expr {
        let n = n.as_str();
        if n == "process" && name == "platform" {
            let cur_block = ctx.cur_block;
            let v = ctx.f.append_inst(
                cur_block,
                InstKind::Call(ctx.intrinsics.process_platform, Vec::new()),
                Type::Str,
                None,
            );
            return Some(Operand::Value(v));
        }
        if (n == "process" || n == "Bun") && name == "argv" {
            let arr_id = intern_arr_layout(ctx.arr_layouts, Type::Str);
            let cur_block = ctx.cur_block;
            let v = ctx.f.append_inst(
                cur_block,
                InstKind::Call(ctx.intrinsics.process_argv, Vec::new()),
                Type::Arr(arr_id),
                None,
            );
            return Some(Operand::Value(v));
        }
        if n == "process" && name == "env" {
            return Some(Operand::ConstPtrNull);
        }
    }
    if let Expr::Member {
        obj: inner_obj,
        name: inner_name,
    } = obj_expr
        && inner_name == "env"
        && let Expr::Ident(n) = ctx.ast.get_expr(*inner_obj)
        && n == "process"
    {
        let key_str = ctx.intern_string_literal(name);
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.process_getenv, vec![Operand::Value(key_str)]),
            Type::Str,
            None,
        );
        return Some(Operand::Value(v));
    }
    None
}
