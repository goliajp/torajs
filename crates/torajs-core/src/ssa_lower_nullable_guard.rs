//! RC-4 F1a — nullable-arr receiver guard emission.
//!
//! `re.exec(s)` / `s.match(re)` answer null on miss (checker types
//! them `Nullable<Array<Str>>`); the un-narrowed decay consumption
//! (`m.length`, `m[i]`) reaches the inline Load paths, so a miss
//! SIGSEGVd. The two consumption arms call
//! [`emit_nullable_arr_guard`] in front of their load: when the
//! receiver is a nullable-arr source, it emits
//! `__torajs_arr_null_check(arr)` (arms a catchable TypeError on
//! NULL — torajs-arr/null_guard.rs) + `emit_throw_check(None)`.
//!
//! Receiver shapes recognized as nullable-arr sources:
//! - an Ident recorded in `ctx.nullable_arr_lets` (let-init
//!   exec/match shape, filled by the LetDecl arm in declaration
//!   order — a shadowing same-named non-exec binding also guards:
//!   over-broad is one predictable cmp, never wrong, since NULL in
//!   a plain arr slot is the same TypeError);
//! - a direct `<recv>.exec(...)` / `<recv>.match(...)` call chain
//!   (`re.exec(s).length`).
//!
//! V3-18 narrowed reads (`if (m !== null) { m[0] }`) still guard —
//! the lowering has no narrow context; one well-predicted non-null
//! cmp on a cold-ish path (exec-result consumption is not an
//! inner-loop bench shape).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand};
use crate::ssa_lower::LowerCtx;

/// True when `obj`'s value may legally be null per the checker's
/// Nullable<Array> typing (exec/match result).
pub(crate) fn is_nullable_arr_source(ctx: &LowerCtx<'_>, obj: ExprId) -> bool {
    match ctx.ast.get_expr(obj) {
        Expr::Ident(n) => ctx.nullable_arr_lets.contains(n),
        Expr::Call { callee, .. } => {
            if let Expr::Member { name, .. } = ctx.ast.get_expr(*callee) {
                matches!(name.as_str(), "exec" | "match")
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Emit the null guard when `obj` is a nullable-arr source; no-op
/// otherwise. `arr_val` must be the already-lowered receiver.
pub(crate) fn emit_nullable_arr_guard(ctx: &mut LowerCtx<'_>, obj: ExprId, arr_val: &Operand) {
    if !is_nullable_arr_source(ctx, obj) {
        return;
    }
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(ctx.intrinsics.arr_null_check, vec![arr_val.clone()]),
    );
    ctx.emit_throw_check(None);
}
