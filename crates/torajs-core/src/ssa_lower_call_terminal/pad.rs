//! The `undefined` boxes a call pads its missing trailing `any`
//! arguments with.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn pad_trailing_undef(ctx: &mut LowerCtx<'_>, eid: ExprId, argv: &mut Vec<Operand>) {
    // T-28 — pad trailing missing Type::Any params with ANY_UNDEF
    // Any-box operands. check.rs's arity_pad_count recorded the
    // missing count for this Call ExprId iff all the trailing
    // missing params were Type::Any (the typed-tier strict-arity
    // check rejected mixed-typed missing). Shared by the indirect
    // arms (closure_local / fn_indirect) — an indirect CallIndirect
    // whose argv is shorter than its signature reads garbage
    // registers in the callee (RC-4 arguments-object SIGSEGV).
    let pad_n = match ctx.arity_pad_count.get(&eid).copied() {
        Some(n) if n > 0 => n,
        _ => return,
    };
    pad_undef_n(ctx, pad_n, argv);
}

/// Emit `n` ANY_UNDEF boxes onto `argv` — the count-known half of
/// [`pad_trailing_undef`], for arms that resolve the pad count from
/// `arity_pad_count` themselves before dropping the call ExprId
/// (struct-method dispatch resolves it in `try_lower`; its emit
/// helpers are shared with fixed-arity accessor callers that pad 0).
pub(crate) fn pad_undef_n(ctx: &mut LowerCtx<'_>, n: usize, argv: &mut Vec<Operand>) {
    for _ in 0..n {
        let cur_block = ctx.cur_block;
        let undef_box = ctx.f.append_inst(
            cur_block,
            InstKind::BoxAny(5, Operand::ConstI64(0)),
            Type::Any,
            None,
        );
        argv.push(Operand::Value(undef_box));
    }
}
