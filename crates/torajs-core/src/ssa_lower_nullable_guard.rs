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

/// RFC 20260707-undefined-sentinel-repr chunk 1 — true when a
/// Str-typed expression may legally hold NULL (missed exec/match
/// capture slot per the 591 NULL-means-undefined convention):
/// an element load off a nullable-arr source (`m[1]`), or a
/// binding recorded in `ctx.nullable_str_lets` (let-init of that
/// shape, alias-propagated).
pub(crate) fn is_nullable_str_source(ctx: &LowerCtx<'_>, eid: ExprId) -> bool {
    match ctx.ast.get_expr(eid) {
        Expr::Ident(n) => ctx.nullable_str_lets.contains(n),
        // 660 residual — a `string[]` element slot may hold the Str
        // sentinel (an OOB string-index read pushed/stored into the
        // array), so any Str-array index read routes the same as a
        // nullable-arr element load: typeof takes the two-state
        // runtime branch, the eq fast path declines to the identity-
        // aware compare, `.length` guards. Over-broad for in-range
        // reads — one well-predicted runtime call, never wrong.
        Expr::Index { obj, .. } => {
            is_nullable_arr_source(ctx, *obj)
                || matches!(
                    ctx.expr_types.get(obj),
                    Some(crate::check::Type::Array(elem)) if **elem == crate::check::Type::String
                )
        }
        // RFC 20260707 residual chunk — `process.env.X` answers the
        // undefined sentinel on a missing var (chunk 644 producer),
        // so a `.length` load on it (direct or via a let alias)
        // must guard. Same for `sym.description` on the static
        // Symbol lane — `Symbol()` answers the sentinel (§20.4.3.2),
        // so typeof / eq / `.length` consumers take the identity-
        // aware branch.
        Expr::Member { obj, name } => {
            if name == "description"
                && matches!(ctx.expr_types.get(obj), Some(crate::check::Type::Symbol))
            {
                return true;
            }
            if let Expr::Member { obj: inner, name } = ctx.ast.get_expr(*obj) {
                name == "env"
                    && matches!(ctx.ast.get_expr(*inner), Expr::Ident(n) if n == "process")
            } else {
                false
            }
        }
        // `x!` / `x as T` are type-side identities (`!` encodes as
        // `As { ty_ann: "__nonnull__" }`) — the runtime value flows
        // through unchanged, so nullability does too.
        Expr::As { expr, .. } => is_nullable_str_source(ctx, *expr),
        // rotation 184 — `d.toJSON()` on a Date answers Str-slot
        // NULL (JS null) for an invalid date (§21.4.4.37 steps 2-3),
        // so typeof / eq / `.length` consumers take the identity-
        // aware branch. RFC 20260722-find-miss chunk A — a
        // `string[].find/findLast` miss answers the undefined
        // sentinel, so the same consumers route identity-aware.
        Expr::Call { callee, .. } => match ctx.ast.get_expr(*callee) {
            Expr::Member { obj, name } if name == "toJSON" => {
                matches!(ctx.expr_types.get(obj), Some(crate::check::Type::Date))
            }
            Expr::Member { obj, name } if matches!(name.as_str(), "find" | "findLast") => {
                matches!(
                    ctx.expr_types.get(obj),
                    Some(crate::check::Type::Array(elem)) if **elem == crate::check::Type::String
                )
            }
            _ => false,
        },
        _ => false,
    }
}

/// RFC 20260708-typed-arr-oob-read chunk 2 — the F64 `undefined`
/// sentinel: a quiet NaN whose payload no arithmetic produces (the
/// hardware default qNaN is 0x7FF8_0000_0000_0000; the low bits
/// echo VALUE_UNDEFINED=0x0A as a grep anchor). Consumers gate
/// STATICALLY via [`is_undef_f64_source`] and only then re-check
/// the bits at runtime — AArch64 (FPCR.DN=0) propagates operand
/// NaN payloads through arithmetic, so a bits-only global check
/// would misread `a[oob] + 1` (a plain NaN under JS semantics) as
/// `undefined`. The static gate keeps arithmetic results out.
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
        // branch), so its result may hold the sentinel too.
        Expr::Call { callee, .. } => {
            matches!(
                ctx.ast.get_expr(*callee),
                Expr::Member { obj, name } if name == "at" && matches!(
                    ctx.expr_types.get(obj),
                    Some(crate::check::Type::Array(elem)) if **elem == crate::check::Type::Number
                )
            )
        }
        Expr::As { expr, .. } => is_undef_f64_source(ctx, *expr),
        _ => false,
    }
}

/// RFC 20260707 residual chunk — true when a Substr-typed
/// expression may hold the Substr-shaped undefined sentinel
/// (string INDEX read, OOB → sentinel): `s[i]` on a string-typed
/// receiver, or a binding recorded in `ctx.undefable_substr_lets`
/// (let-init of that shape, alias-propagated). Over-broad for
/// in-range reads — one runtime call instead of the inline byte
/// walk, never wrong.
pub(crate) fn is_undefable_substr_source(ctx: &LowerCtx<'_>, eid: ExprId) -> bool {
    match ctx.ast.get_expr(eid) {
        Expr::Ident(n) => ctx.undefable_substr_lets.contains(n),
        Expr::Index { obj, .. } => {
            matches!(ctx.expr_types.get(obj), Some(crate::check::Type::String))
        }
        Expr::As { expr, .. } => is_undefable_substr_source(ctx, *expr),
        _ => false,
    }
}

/// Emit the Str null guard when `obj` is a nullable-str source or
/// an undefable-substr source (string index read — its slot may
/// hold the Substr-shaped sentinel); no-op otherwise. `str_val`
/// must be the already-lowered receiver. Same shape as
/// [`emit_nullable_arr_guard`]: `str_null_check` arms a catchable
/// TypeError on any nullish repr, the throw-check right after
/// diverts before the inline `.length` load dereferences.
pub(crate) fn emit_nullable_str_guard(ctx: &mut LowerCtx<'_>, obj: ExprId, str_val: &Operand) {
    if !is_nullable_str_source(ctx, obj) && !is_undefable_substr_source(ctx, obj) {
        return;
    }
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(ctx.intrinsics.str_null_check, vec![str_val.clone()]),
    );
    ctx.emit_throw_check(None);
}
