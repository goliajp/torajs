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

/// Emit the null guard when `obj` is a nullable-arr source or an
/// undefable-heap source (RFC 20260722 chunk B — an `Arr[]`
/// find/findLast miss answers the generic undefined cell, and the
/// kernel throws on it too); no-op otherwise. `arr_val` must be the
/// already-lowered receiver.
pub(crate) fn emit_nullable_arr_guard(ctx: &mut LowerCtx<'_>, obj: ExprId, arr_val: &Operand) {
    if !is_nullable_arr_source(ctx, obj) && !is_undefable_heap_source(ctx, obj) {
        return;
    }
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(ctx.intrinsics.arr_null_check, vec![arr_val.clone()]),
    );
    ctx.emit_throw_check(None);
}

/// RFC 20260725-fallthrough-return knives 1-2 — true when `eid` calls
/// a function whose body can run off its end, which answers
/// `undefined` there (ES §10.2.1.4 step 11) and so hands back that
/// return width's sentinel. Every "may this hold the sentinel"
/// predicate below consults it: the answer is a property of the
/// callee, not of the width it arrives in.
pub(crate) fn callee_falls_through(ctx: &LowerCtx<'_>, eid: ExprId) -> bool {
    let Expr::Call { callee, .. } = ctx.ast.get_expr(eid) else {
        return false;
    };
    matches!(
        ctx.ast.get_expr(*callee),
        Expr::Ident(f) if ctx.num_f64_slots.returns_undef_on_fallthrough(f)
    )
}

/// `await p` parses to `p.value`, so this answers the inner type of the
/// promise being awaited (`None` for any other expression).
///
/// A promise's value slot carries whatever settled it, and an async body
/// that runs off its end settles with that width's undefined sentinel
/// (ES §10.2.1.4 step 11) — as does any sentinel-bearing value handed to
/// `Promise.resolve`. Every "may this hold the sentinel" predicate
/// consults it for its own width. Over-broad for the promises that
/// always settle with a real value: one predictable compare, never
/// wrong.
pub(crate) fn awaited_promise_inner<'c>(
    ctx: &'c LowerCtx<'_>,
    eid: ExprId,
) -> Option<&'c crate::check::Type> {
    let Expr::Member { obj, name } = ctx.ast.get_expr(eid) else {
        return None;
    };
    if name != "value" {
        return None;
    }
    match ctx.expr_types.get(obj) {
        Some(crate::check::Type::Promise(inner)) => Some(inner),
        _ => None,
    }
}

/// RFC 20260707-undefined-sentinel-repr chunk 1 — true when a
/// Str-typed expression may legally hold NULL (missed exec/match
/// capture slot per the 591 NULL-means-undefined convention):
/// an element load off a nullable-arr source (`m[1]`), or a
/// binding recorded in `ctx.nullable_str_lets` (let-init of that
/// shape, alias-propagated).
pub(crate) fn is_nullable_str_source(ctx: &LowerCtx<'_>, eid: ExprId) -> bool {
    // A read the CHECKER already typed `Nullable(String)` is one by
    // definition, whatever its syntax — the probes below each infer
    // the same fact from one shape (a let's init, a param's
    // annotation, an element load), and a class field read matched
    // none of them, so `typeof k.p` on `p?: string` answered
    // "string" for an undefined. Asking the type covers every
    // position at once, including the ones no probe was written for.
    if matches!(
        ctx.expr_types.get(&eid),
        Some(crate::check::Type::Nullable(inner)) if **inner == crate::check::Type::String
    ) {
        return true;
    }
    if callee_falls_through(ctx, eid) {
        return true;
    }
    if matches!(
        awaited_promise_inner(ctx, eid),
        Some(crate::check::Type::String)
    ) {
        return true;
    }
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
            // §25.5.2 — `JSON.stringify` answers undefined for a value
            // with no JSON representation (undefined itself, a
            // callable, a symbol), and the value lane already returns
            // it. Its static type stays `String` on purpose: that is
            // what TS's lib.d.ts declares, and a stricter one would
            // refuse `JSON.stringify(x).length`, which bun runs. So the
            // undefined-ness is carried HERE — the consumers that
            // cannot be answered from the static type alone (typeof,
            // eq, `.length`) take their identity-aware branch, and
            // `typeof JSON.stringify(undefined)` stops folding to
            // "string".
            Expr::Member { obj, name } if name == "stringify" => {
                matches!(ctx.ast.get_expr(*obj), Expr::Ident(n) if n == "JSON")
            }
            // rotation 216 — `pop` / `shift` on an empty array answer
            // undefined too (§23.1.3.20 step 4.a / §23.1.3.25 step 3.a),
            // and the Str slot spells that as the same immortal cell.
            Expr::Member { obj, name }
                if matches!(name.as_str(), "find" | "findLast" | "pop" | "shift") =>
            {
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

/// RFC 20260722-find-miss-undefined-sentinel chunk B — true when a
/// refcounted-pointer expression may hold the generic immortal
/// undefined cell: the checker's `Nullable` typing (the C2b
/// optional-field producer), a read past the end or a `find` miss on
/// an array of such elements, a field of such a type (the class
/// factory seeds it with the cell), or a binding recorded in
/// `ctx.undefable_heap_lets` (let-init of those shapes,
/// alias-propagated). Over-broad for hit-path reads — one
/// well-predicted cmp, never wrong.
/// 403-03 — does `callee` name a FnDecl whose declared fn-typed
/// return got the `effective_ret_ty` any-binding upgrade? Such a fn
/// can hand back the undefined sentinel (`coerce_to_ret`'s
/// Any→Closure arm), so its call result is an undefable-heap source.
/// Same predicate pair the upgrade itself uses — the two sites must
/// answer alike or the guard misses exactly the returns that need it.
fn ret_upgraded_from_any_binding(ctx: &LowerCtx<'_>, callee: ExprId) -> bool {
    let Expr::Ident(n) = ctx.ast.get_expr(callee) else {
        return false;
    };
    ctx.ast.stmts.iter().any(|s| {
        matches!(s,
            crate::ast::Stmt::FnDecl { name, params, return_type, body, .. }
            if name == n
                && return_type.as_deref().is_some_and(crate::ast::is_fn_like_ann)
                && crate::ssa_lower_body_returns_closure::body_returns_any_binding(
                    ctx.ast, params, body))
    })
}

pub(crate) fn is_undefable_heap_source(ctx: &LowerCtx<'_>, eid: ExprId) -> bool {
    if callee_falls_through(ctx, eid) {
        return true;
    }
    if awaited_promise_inner(ctx, eid).is_some_and(spells_undef_with_generic_cell) {
        return true;
    }
    if matches!(
        ctx.expr_types.get(&eid),
        Some(crate::check::Type::Nullable(_))
    ) {
        return true;
    }
    match ctx.ast.get_expr(eid) {
        Expr::Ident(n) => ctx.undefable_heap_lets.contains(n),
        // Reading past the end answers the same immortal cell a miss
        // does — the mirror of `is_undef_f64_source`'s Index arm, for
        // the element families that have a cell to answer with.
        Expr::Index { obj, .. } | Expr::OptIndex { obj, .. } => heap_elem_array(ctx, *obj),
        Expr::Call { callee, .. } => {
            matches!(
                ctx.ast.get_expr(*callee),
                // `pop` / `shift` on an empty array answer undefined the
                // same way a miss does, and `at` is here because it takes
                // the same out-of-range exit under another spelling. A
                // pointer-shaped element slot spells it with the generic
                // immortal cell.
                Expr::Member { obj, name } if matches!(name.as_str(), "find" | "findLast" | "pop" | "shift" | "at")
                    && heap_elem_array(ctx, *obj)
            )
            // 403-03 — a call whose callee's fn-typed return was
            // upgraded from an `any` binding (`effective_ret_ty`):
            // coerce_to_ret answers the sentinel for a non-callable
            // box, so calling the RESULT must arm the guard.
            || ret_upgraded_from_any_binding(ctx, *callee)
        }
        // A field read. The class factory seeds a field of one of these
        // types with that same immortal cell (`default_init_for_type`),
        // and it stays there until something writes the field — so a
        // field is undefable for exactly the reason an optional one is,
        // and the two were answering differently: `c.d === undefined`
        // agreed the field held `undefined` while `typeof c.d` read the
        // slot's static type and said "object". `expr_types` carries the
        // declared field type here, which is the question being asked.
        Expr::Member { .. } | Expr::OptChain { .. } => ctx
            .expr_types
            .get(&eid)
            .is_some_and(spells_undef_with_generic_cell),
        Expr::As { expr, .. } => is_undefable_heap_source(ctx, *expr),
        _ => false,
    }
}

/// The type that carries a read of a `T` element when that read has
/// an exit answering `undefined` — an out-of-range index, `at`, a
/// `find` miss, `pop` / `shift` on an empty array.
///
/// Every type answers in its own slot, because each has a bit
/// pattern to spare that no live value uses: the F64 sentinel NaN,
/// the Str / Substr oddballs, the generic cell for pointers. `Bool`
/// is the one that does not — a bool is exactly two states — so its
/// read comes back as a tagged value instead. Only the value handed
/// back changes; the array's own slots stay bool-shaped, so nothing
/// is paid by an array that is never read past its end.
///
/// Promoting rather than inventing a third bool state is what keeps
/// this right for consumers nobody has written yet: an in-band third
/// state has to be remembered at every consumer forever, and the two
/// conversions that mask a bool to its low bit would erase it into
/// `false` on the way through.
pub(crate) fn undefable_read_ty(elem_ty: crate::ssa::Type) -> crate::ssa::Type {
    if elem_ty == crate::ssa::Type::Bool {
        crate::ssa::Type::Any
    } else {
        elem_ty
    }
}

/// True when `obj` is an array whose elements spell `undefined` with
/// the generic immortal cell.
fn heap_elem_array(ctx: &LowerCtx<'_>, obj: ExprId) -> bool {
    matches!(
        ctx.expr_types.get(&obj),
        Some(crate::check::Type::Array(elem)) if spells_undef_with_generic_cell(elem)
    )
}

/// The checker-side reading of [`crate::ssa::Type::
/// spells_undef_with_generic_cell`] — true when a value of this type
/// lands in a slot that can hold the generic immortal cell.
///
/// The two are mirrors and must move together: a checker type that
/// starts lowering to a refcounted pointer belongs on the `true` side
/// here the same day. Both are exhaustive so that adding a type is a
/// build error rather than a silent fall into the wrong half — the
/// `container_key_lookup` mirror taught that lesson by answering the
/// wrong class for a name it had never heard of.
///
/// One-way slack is fine and deliberate: saying `true` for something
/// that turns out to lower to `FnSig` only buys a well-predicted
/// identity compare, because the emitting sites gate on the SSA type
/// as well. Saying `false` for something that does hold the cell is
/// the direction that goes silently wrong, so `Nullable` recurses
/// rather than guessing.
pub(crate) fn spells_undef_with_generic_cell(t: &crate::check::Type) -> bool {
    use crate::check::Type as T;
    match t {
        T::Struct(_)
        | T::ClassRef(_)
        | T::Array(_)
        | T::Function(..)
        | T::BigInt
        | T::Date
        | T::RegExp
        | T::Symbol
        | T::Promise(_)
        | T::Map
        | T::Set
        | T::MapIter
        | T::ArrIter
        | T::WeakRef
        | T::WeakMap
        | T::WeakSet => true,
        // `T | null` for a pointer-shaped T is that same slot with
        // NULL in band, so the answer is T's.
        T::Nullable(inner) => spells_undef_with_generic_cell(inner),
        // Str and Substr carry their own family oddballs; Any carries
        // the answer in its tag; the rest are scalars, absent values,
        // or names that are gone by the time anything is lowered
        // (`TypeVar` is substituted, `Rest` is an annotation marker,
        // `Object` is a global stand-in).
        T::String
        | T::Number
        | T::Boolean
        | T::Void
        | T::Any
        | T::Null
        | T::Undefined
        | T::Object(_)
        | T::TypeVar(_)
        | T::Rest(_) => false,
    }
}

/// Emit the heap nullish guard when `obj` is an undefable-heap
/// source; no-op otherwise. `obj_val` must be the already-lowered
/// receiver. Same shape as [`emit_nullable_str_guard`]:
/// `heap_nullish_check` arms a catchable TypeError on NULL or the
/// generic undefined cell, the throw-check right after diverts
/// before the member read dereferences.
pub(crate) fn emit_undefable_heap_guard(ctx: &mut LowerCtx<'_>, obj: ExprId, obj_val: &Operand) {
    if !is_undefable_heap_source(ctx, obj) {
        return;
    }
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(ctx.intrinsics.heap_nullish_check, vec![obj_val.clone()]),
    );
    ctx.emit_throw_check(None);
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
