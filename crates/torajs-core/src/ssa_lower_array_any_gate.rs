//! Which array literals lower through the `FLAG_ARR_ANY` lane
//! (`lower_array_any_literal`) instead of the typed 8-byte-slot one.
//!
//! Every gate answers the same question — does some element have no
//! typed slot repr — from a different direction, so any one of them
//! sends the whole literal to the any lane. They are asked before a
//! single element is lowered: the typed lane allocates its block off
//! the first element's lowered type and stores as it goes, so a
//! literal that learns it needed the any lane halfway through has
//! already stored box bits into a typed slot.

use crate::ast::{BinOp, Expr, ExprId};
use crate::ssa::Type;
use crate::ssa_lower::LowerCtx;

/// True when the literal `eid` with `element_ids` needs the any lane.
pub(crate) fn takes_any_lane(
    ctx: &LowerCtx<'_>,
    element_ids: &[ExprId],
    eid: ExprId,
    has_spread: bool,
) -> bool {
    // §13.2.4 — an elision slot needs the Arr<Any> lane: the hole
    // marking (shadow entry + exotic flag) only exists there, and a
    // typed lane would store ConstPtrNull into a numeric slot.
    if element_ids
        .iter()
        .any(|eid| matches!(ctx.ast.get_expr(*eid), Expr::Elision))
    {
        return true;
    }
    if has_spread {
        return false;
    }
    shape_needs_any(ctx, element_ids, eid) || elems_need_any(ctx, element_ids)
}

/// Gates on the literal as a whole: its element kinds, where its type
/// came from, and where it flows.
fn shape_needs_any(ctx: &LowerCtx<'_>, element_ids: &[ExprId], eid: ExprId) -> bool {
    if ctx.array_literal_is_heterogeneous(element_ids) {
        return true;
    }
    // Chunk 702 — checker contextual typing: a literal whose Array<Any>
    // type came from a let-decl annotation (side-set populated by
    // check_stmt_let_decl::apply_contextual_array_ann, propagated
    // through nested literals) mints the FLAG_ARR_ANY flavor even when
    // its elements are kind-uniform, so kind-change mutators behave
    // like bun instead of hitting the typed-alias any-view protocol.
    // Nested literals recurse here per-element and hit the same gate.
    // The gate keys off the side-set, NOT off expr_types: T-10.c
    // infer-widened Array<Any> shapes (`["a", undefined]`) share the
    // recorded type but are deliberately taken by the typed lane
    // below (Str undefined sentinel slots). Mono-specialized clones
    // carry fresh ExprIds and fall back to the pre-702 typed mint.
    if ctx.contextual_any.contains(&eid) {
        return true;
    }
    // Holes X+Y (rotation 231) — the kind probe collapses every
    // struct-ish element into kind 10 and every array element into
    // kind 2, so `[{r: 2}, {p: 5}]` and `[[1], [undefined, 2]]`
    // both took the typed lane and the anchor's layout was forced
    // onto every sibling (loud no-field reject when the shapes
    // differ; silent garbage when only the reprs differ — an
    // undefined-valued field read back 0, a mixed inner array read
    // back 5e-323). When the checker already widened the literal to
    // Array<Any> AND the recorded heap-element types disagree,
    // there is no shared typed slot repr — route through
    // FLAG_ARR_ANY. The recorded-Any gate keeps the width-subtyped
    // struct family (`[{r: 2}, {r: 3, s: 4}]`, prefix-compatible
    // offsets, checker keeps it typed) on the typed lane.
    if matches!(
        ctx.expr_types.get(&eid),
        Some(crate::check::Type::Array(inner)) if matches!(**inner, crate::check::Type::Any)
    ) && ctx.heap_elem_types_disagree(element_ids)
    {
        return true;
    }
    // W-ESC (RFC 20260706-typed-arr-any-escape) — a literal whose
    // Anon alias class flows into the `any` world lowers as Arr<Any>
    // directly: return-position / arg-position literals never pass
    // an annotation-consuming widen site, so the escape re-intern
    // must fire here. (Escaped spread literals stay typed — the any
    // side then hits the mark_kind loud fallback, never silent.)
    if ctx
        .num_f64_slots
        .slot_escapes_any(&crate::num_width::SlotKey::Anon(eid.0))
    {
        return true;
    }
    false
}

/// Gates on single elements whose lowered value has no typed slot.
fn elems_need_any(ctx: &LowerCtx<'_>, element_ids: &[ExprId]) -> bool {
    // Chunk 739 — an element whose checker type is Any (`[x]` with
    // `x: any`) has no typed 8-byte slot repr: the heterogeneity
    // gate classifies Any as None, so an all-Any (or Any-anchored)
    // literal fell through to the typed lane, which stored the
    // NaN-box bits into an 8-byte slot while every Arr<Any> reader
    // decodes 16-byte tagged slots — `[x][0]` answered undefined
    // for ANY x. Route through the FLAG_ARR_ANY literal lane.
    // Undefined-typed elements deliberately stay out (T-10.c infer-
    // widened `["a", undefined]` keeps the typed Str sentinel lane).
    // A scalar `T | null` element reaches the same shape by a
    // different road: its checker type stays `Nullable(Number)` /
    // `Nullable(Boolean)`, but it MATERIALIZES as Any, because a
    // scalar slot has no spare bit pattern to spell `null` with
    // (`ssa_lower_parse_type` — the RFC 20260710 C4 box tax). The
    // gate above only reads the checker type, so these fell into the
    // typed lane and reproduced chunk 739's failure exactly: box bits
    // stored into an 8-byte slot, every element reading back
    // undefined.
    //
    // A pointer-shaped `T | null` was left on the typed lane on the
    // grounds that its in-band null sentinel is a real pointer value.
    // It is — the slot can hold it. What the typed lane cannot do is
    // TELL anyone: `let q: O | null = null; console.log([q])`
    // answered `[ [unknown-any-tag] ]` where bun says `[ null ]`, and
    // the annotated spelling `const a: (O | null)[] = [null, {x: 1}]`
    // died at runtime with "array element does not match the
    // annotated element type". The any lane has the encoding for
    // exactly this: `anyv_box_from_pair` maps a zero heap payload to
    // ANY_NULL, which is the whole reason the slot READER
    // (`box_heap_slot_or_undef`) already goes through it.
    if element_ids.iter().any(|id| {
        matches!(ctx.expr_types.get(id), Some(crate::check::Type::Any))
            || matches!(
                ctx.expr_types.get(id),
                Some(crate::check::Type::Nullable(_))
            )
    }) {
        return true;
    }
    // 刀 10 G5b (RFC 20260721-array-proto-cluster) — a builtin-
    // namespace Object-typed element (`[Number]` / `[Object, Array]`)
    // has no typed 8-byte slot repr either; the FLAG_ARR_ANY pack
    // lane reifies the ctor ident through the interned cell
    // (lower_ident's try_builtin_ctor_ident answers Type::Any), so
    // `[Number].lastIndexOf(Number)` compares the same identity the
    // bound `const a: any = [Number]` shape already does. The typed
    // lane stored the box bits behind a kind-less block — element
    // reads answered undefined (chunk-739's shape keyed off
    // Type::Object instead of Type::Any).
    if element_ids
        .iter()
        .any(|id| matches!(ctx.expr_types.get(id), Some(crate::check::Type::Object(_))))
    {
        return true;
    }
    // Chunk 807 — undefined / null elements have no typed 8-byte
    // slot repr outside the Str sentinel lane: `[undefined]` /
    // `[null]` stored ConstPtrNull behind a kind-less typed block
    // (printed `[unknown-any-tag]`), and a scalar anchor
    // (`[undefined, 1]`) mixed raw ints with null slots (SIGSEGV in
    // the print walk). Route through the FLAG_ARR_ANY lane, whose
    // pack arm tags ANY_UNDEF / ANY_NULL. A String-typed sibling
    // keeps the whole literal on the typed lane — T-10.c's
    // infer-widened `["a", undefined]` stores the Str undefined
    // sentinel per slot and stays byte-correct.
    if element_ids.iter().any(|id| {
        matches!(
            ctx.expr_types.get(id),
            Some(crate::check::Type::Undefined | crate::check::Type::Null)
        )
    }) && !element_ids
        .iter()
        .any(|id| matches!(ctx.expr_types.get(id), Some(crate::check::Type::String)))
    {
        return true;
    }
    // An element the checker types `string` can still lower to Any:
    // `x + x` over a `string | undefined` the lowering's sentinel
    // analysis knows about (a callback parameter over a `string[]`
    // that may hold the undefined sentinel) goes through the any
    // lane's kernel, because `undefined + undefined` is NaN, and a
    // binding initialized from it holds the box. The typed lane then
    // stored the box into a Str slot — a TypeError at the `string[]`
    // boundary for `[x + x]`, a segfault reading `["s", x + x]`.
    if element_ids.iter().any(|id| lowers_to_unseen_any(ctx, *id)) {
        return true;
    }
    false
}

/// An element whose checker type is not Any but whose lowered value
/// is: a binding the lowering holds as Any, or `l + r` that the binop
/// lowering sends to the any lane for a nullable string side
/// ([`nullable_str_add_rides_any`]), asked from the operands' checker
/// types and the same sentinel predicate the lowering reads.
fn lowers_to_unseen_any(ctx: &LowerCtx<'_>, mut eid: ExprId) -> bool {
    // a comma expression lowers to its right operand's value
    while let Expr::Sequence { right, .. } = ctx.ast.get_expr(eid) {
        eid = *right;
    }
    // A class method read as a value types as a Function but lowers
    // to the any cell the prototype hands out (S2.34); its container
    // is an `Arr<Any>` or the typed lane stores box bits into an
    // 8-byte slot every `Arr<Any>` reader decodes as 16.
    if crate::ssa_lower_member::is_class_method_value_read(ctx, eid) {
        return true;
    }
    let (left, right) = match ctx.ast.get_expr(eid) {
        Expr::Ident(n) => {
            return ctx.locals.get(n).is_some_and(|l| l.ty == Type::Any)
                || ctx.globals.get(n) == Some(&Type::Any);
        }
        Expr::BinOp {
            op: BinOp::Add,
            left,
            right,
        } => (*left, *right),
        _ => return false,
    };
    // The nullable flag is keyed partly by name (a field or binding
    // some write elsewhere handed the Str sentinel), so it can be set
    // on a number / boolean read. Such a side lowers to a scalar, and
    // the binop lowering boxes only the string sides of the pair: it
    // stays on the numeric lane, and so must the literal holding it.
    let side = |id: ExprId| {
        let ty = ctx.expr_types.get(&id);
        let scalar = matches!(
            ty,
            Some(
                crate::check::Type::Number
                    | crate::check::Type::Boolean
                    | crate::check::Type::BigInt
            )
        );
        (
            !scalar && crate::ssa_lower_nullable_guard::is_nullable_str_source(ctx, id),
            matches!(ty, Some(crate::check::Type::String)),
        )
    };
    nullable_str_add_rides_any(side(left), side(right))
}

/// §13.15.3 with a `string | undefined` on one side and anything but
/// a plain string on the other: the union's faces answer different
/// operators, so the any lane's kernel decides at run time. Each
/// side is `(nullable string, string-typed)`; a plain string on the
/// other side keeps the concat lane (both faces concatenate), a
/// second nullable string does not (`undefined + undefined` is NaN).
pub(crate) fn nullable_str_add_rides_any(l: (bool, bool), r: (bool, bool)) -> bool {
    let plain_str = |(nullable, stringish): (bool, bool)| stringish && !nullable;
    (l.0 && !plain_str(r)) || (r.0 && !plain_str(l))
}
