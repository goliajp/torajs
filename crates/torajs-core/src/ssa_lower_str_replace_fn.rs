//! `<Str>.replace(pattern, cb)` / `replaceAll(pattern, cb)` — the
//! functional-replaceValue lane for STRING patterns (test262-bug-corpus
//! RC-4 replace A1_T4). Fires ahead of the generic str_str argv tower
//! in [`crate::ssa_lower_str_str_dispatch`]: that tower is
//! `(Str, Str, Str)`-shaped, and before this lane a closure
//! replacement was passed raw into `__torajs_str_replace`'s third Str
//! param — the runtime deref'd the closure block as a Str and
//! SIGSEGV'd.
//!
//! The runtime helper (`torajs-str/transform/replace_fn.rs`) invokes
//! the callback through the closure's boxed entry (+32), so any
//! callback param shape works (untyped params are Type::Any at the
//! ABI; extra declared params read the undefined argv pad). The
//! callback may throw — the call site propagates via
//! `emit_throw_check`.
//!
//! Gate is the CHECKER type of args[1] (`Type::Function`), not the
//! lowered operand type — deciding after `lower_expr` would double-
//! lower the arg on the fall-through path (double side effects +
//! a leaked minted env).

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Try to lower `<Str>.{replace|replaceAll}(pattern, cb)`. Returns
/// `Some(result)` when args[1] is checker-typed as a function;
/// `None` falls through to the generic Str dispatch tower.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    args: &[ExprId],
    recv_op: Operand,
) -> Option<Operand> {
    if !matches!(method, "replace" | "replaceAll") || args.len() < 2 {
        return None;
    }
    if !matches!(
        ctx.expr_types.get(&args[1]),
        Some(crate::check::Type::Function(..))
    ) {
        return None;
    }
    let pat = ctx.lower_expr(args[0]);
    if ctx.operand_ty(&pat) != Type::Str {
        // Non-Str pattern (null literal / Any) keeps its existing
        // path for now — the ToString(searchValue) fold is a
        // separate face.
        return None;
    }
    let repl = ctx.lower_expr(args[1]);
    let repl_ty = ctx.operand_ty(&repl);
    if !matches!(repl_ty, Type::Closure(_)) {
        // A named global fn reaches here as Type::FnSig — it has no
        // env block and no boxed entry, so the runtime protocol
        // can't invoke it. Loud reject over silent-wrong.
        panic!(
            "ssa-lower: s.{method}(pat, fn) — fn replacement must be a closure \
             (arrow / function expression), got {repl_ty:?}"
        );
    }
    // ES §22.1.3.{18,19} silently ignore args past (pattern,
    // replacement) — lower trailing args for side effects (S321
    // mirror from the regex lane).
    for &a in args.iter().skip(2) {
        let _ = ctx.lower_expr(a);
    }
    let fid = if method == "replace" {
        ctx.intrinsics.str_replace_fn
    } else {
        ctx.intrinsics.str_replace_all_fn
    };
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(fid, vec![recv_op, pat, repl]),
        Type::Str,
        None,
    );
    // The callback can throw — propagate the pending throw.
    ctx.emit_throw_check(None);
    // Release an inline closure literal's minted env after the
    // runtime call consumed it (RFC 20260705 chunk 552 mirror).
    ctx.release_owned_temp(args[1], &repl);
    Some(Operand::Value(v))
}
