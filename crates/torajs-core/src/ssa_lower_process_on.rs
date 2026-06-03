//! P10.5-A4 — `process.on(eventName, cb)` lowering.
//!
//! Currently restricted to the `'unhandledRejection'` event — the
//! handler hook for [`promise::unhandled`]'s pending-list sweep.
//! `'rejectionHandled'` (its symmetric "previously-reported
//! rejection now observed" event) is unreachable under torajs'
//! sync MVP because every same-tick `attach_then` runs before the
//! sweep, and the sweep happens once at `main` exit; landing the
//! event API for completeness is tracked as L3b follow-up.
//!
//! ## Lower path
//!
//! - [`declare`] registers two extern intrinsics during module
//!   setup:
//!     * `__torajs_process_on_unhandled_rejection_register_simple
//!       (fn_ptr)` — bare `(reason: any) => void` named-fn cb.
//!     * `__torajs_process_on_unhandled_rejection_register_closure
//!       (env_ptr)` — capturing lambda cb, env layout matches the
//!       Closure ABI used by `promise_then_closure` /
//!       `microtask_enqueue_closure` (fn_ptr at env+8, captures
//!       starting at env+24, drop_fn at env+16).
//! - [`try_lower`] returns `Some(Operand::ConstI64(0))` (the
//!   modelled `void` result) when the Call's callee is the
//!   `process.on` member; selects simple vs closure by `cb`'s
//!   static SSA type (`Type::FnSig` vs `Type::Closure`) — same
//!   discriminator `queueMicrotask` / `promise_then` already use.
//!
//! Lives outside `ssa_lower.rs` so the file-size known-debt #1
//! god-file doesn't grow on this substrate step.

use std::collections::HashMap;

use crate::ast::{Expr, ExprId};
use crate::ssa::{FuncId, InstKind, Module, Operand, Type};
use crate::ssa_lower::{LowerCtx, declare_intrinsic};

const REG_SIMPLE_SYM: &str = "__torajs_process_on_unhandled_rejection_register_simple";
const REG_CLOSURE_SYM: &str = "__torajs_process_on_unhandled_rejection_register_closure";

/// Declare the two `process.on('unhandledRejection', cb)` runtime
/// registers at module setup. Both take a single pointer-shaped
/// argument (raw fn ptr or closure env) and return void.
pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) {
    declare_intrinsic(module, fn_table, REG_SIMPLE_SYM, &[Type::Ptr], Type::Void);
    declare_intrinsic(module, fn_table, REG_CLOSURE_SYM, &[Type::Ptr], Type::Void);
}

/// Try to lower `process.on(eventName, cb)`. Returns `None` for
/// any other callee shape so the caller falls through to the
/// regular Call dispatch. Panics on shape errors (wrong arity,
/// non-literal event name, unsupported event, unexpected cb type)
/// because they are programmer errors — typecheck (`check::
/// process_on`) is the user-facing diagnostic surface.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee_id: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member { obj, name: m_name } = ctx.ast.get_expr(callee_id) else {
        return None;
    };
    if m_name != "on" {
        return None;
    }
    let Expr::Ident(n) = ctx.ast.get_expr(*obj) else {
        return None;
    };
    if n != "process" {
        return None;
    }
    if args.len() != 2 {
        panic!(
            "ssa-lower: process.on expects 2 args (event, cb), got {}",
            args.len()
        );
    }
    let event_ok = matches!(
        ctx.ast.get_expr(args[0]),
        Expr::String(s) if s == "unhandledRejection"
    );
    if !event_ok {
        panic!(
            "ssa-lower: process.on event must be the literal \
             \"unhandledRejection\" in v0.5 MVP (P10.5-A4)"
        );
    }
    let cb_op = ctx.lower_expr(args[1]);
    let cb_ty = ctx.operand_ty(&cb_op);
    let sym = match cb_ty {
        Type::Closure(_) => REG_CLOSURE_SYM,
        Type::FnSig(_) => REG_SIMPLE_SYM,
        _ => panic!("ssa-lower: process.on cb expected Closure or FnSig, got {cb_ty:?}"),
    };
    let fid = *ctx
        .fn_table
        .get(sym)
        .expect("process.on register intrinsic must be declared");
    ctx.f
        .append_void(ctx.cur_block, InstKind::Call(fid, vec![cb_op]));
    Some(Operand::ConstI64(0))
}
