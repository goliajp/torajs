//! Whether an F64-typed expression may be carrying the `undefined`
//! sentinel, and the bit pattern that is it.
//!
//! Split out of [`crate::ssa_lower_nullable_guard`], which is the
//! receiver-guard module its own doc describes; this is the F64 half
//! of the "may this hold a sentinel" family that accreted alongside
//! it. The Str / Substr and pointer halves still live there, as do
//! the two shape helpers both halves consult
//! ([`callee_falls_through`], [`awaited_promise_inner`]).

use crate::ast::{Expr, ExprId};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_nullable_guard::{awaited_promise_inner, callee_falls_through};

/// RFC 20260708-typed-arr-oob-read chunk 2 — the F64 `undefined`
/// sentinel: a quiet NaN carrying a payload that nothing but a
/// genuine `undefined` wears (the hardware default qNaN is
/// 0x7FF8_0000_0000_0000; the low bits echo VALUE_UNDEFINED=0x0A as
/// a grep anchor).
///
/// "Nothing else wears it" is a fact about the machine, not a hope:
/// the program entry selects FPCR.DN, so every FP operation that
/// returns a NaN returns the default one whatever payload its
/// operands carried (`torajs-cli::cmd_build_synthesize::FPCR_DN_BIT`).
/// Without that mode `a[oob] + 1` came back bit-for-bit the sentinel
/// and read as `undefined` where the spec says `NaN`.
///
/// [`is_undef_f64_source`] gates the runtime bits compare. Which
/// way it may err is not symmetric, and the mode changed only one
/// side of that: arming where no sentinel can arrive costs one
/// well-predicted compare and is always safe, while *declining* a
/// read that can be holding one answers `NaN` where the program
/// should see `undefined`. So the predicate is allowed to be
/// generous and is not allowed to be clever.
pub(crate) const F64_UNDEF_SENTINEL_BITS: u64 = 0x7FFC_0000_0000_000A;

/// RFC 20260708-typed-arr-oob-read chunk 2 — true when an
/// F64-typed expression may hold the undefined-NaN sentinel (a
/// `number[]` index read whose OOB exit produced it), or a binding
/// recorded in `ctx.undefable_f64_lets` (let-init of that shape).
/// Over-broad for in-range reads — one predictable bits compare,
/// never wrong.
pub(crate) fn is_undef_f64_source(ctx: &LowerCtx<'_>, eid: ExprId) -> bool {
    match ctx.ast.get_expr(eid) {
        Expr::Ident(n) => ctx.undefable_f64_lets.contains(n),
        Expr::Index { obj, .. } => {
            matches!(
                ctx.expr_types.get(obj),
                Some(crate::check::Type::Array(elem)) if **elem == crate::check::Type::Number
            )
        }
        // `xs.at(i)` on a number[] rides the same OOB exit as `xs[i]`
        // (RFC 20260721 G1 routes at() through the checked index
        // branch), so its result may hold the sentinel too. RFC
        // 20260722 chunk D — a `find`/`findLast` miss answers the
        // sentinel the same way.
        _ if callee_falls_through(ctx, eid) => true,
        _ if matches!(
            awaited_promise_inner(ctx, eid),
            Some(crate::check::Type::Number)
        ) =>
        {
            true
        }
        Expr::Call { callee, .. } => {
            matches!(
                ctx.ast.get_expr(*callee),
                Expr::Member { obj, name } if matches!(name.as_str(), "at" | "find" | "findLast" | "pop" | "shift")
                    && matches!(
                        ctx.expr_types.get(obj),
                        Some(crate::check::Type::Array(elem)) if **elem == crate::check::Type::Number
                    )
            )
        }
        // A field read. Unlike the pointer-shaped families a `number`
        // field has no type-level tell — it is seeded with 0 — so the
        // question is whether some write put a sentinel-shaped value
        // in a field of this name (`crate::undef_f64_fields`). Over-
        // broad across two structs sharing a field name, which costs
        // one bits compare; blanket-true for every `number` field
        // would instead arm the ToNumber step on arithmetic that can
        // never see a sentinel.
        Expr::Member { name, .. } => ctx.undefable_f64_fields.contains(name),
        // The value-transparent wrappers — the same set the 11-A1
        // escape visitor names in `collect_value_flow_idents`. What
        // comes out is what went in, so the question passes straight
        // through, and only the cast was being asked. `true ? xs[9] :
        // 0` printed NaN, `typeof` of it answered "number", and
        // `(0, xs[9]) === undefined` answered false — all five shapes
        // silently, because a sentinel that is not recognised is just
        // a NaN.
        Expr::Ternary {
            then_branch,
            else_branch,
            ..
        } => is_undef_f64_source(ctx, *then_branch) || is_undef_f64_source(ctx, *else_branch),
        // Only the right arm: `a ?? b` yields `a` exactly when `a` is
        // neither null nor undefined, and a sentinel-carrying `a` IS
        // undefined, so it can never be what comes out.
        Expr::Nullish { rhs, .. } => is_undef_f64_source(ctx, *rhs),
        Expr::Sequence { right, .. } => is_undef_f64_source(ctx, *right),
        Expr::Assign { value, .. } => is_undef_f64_source(ctx, *value),
        Expr::As { expr, .. } => is_undef_f64_source(ctx, *expr),
        _ => false,
    }
}
