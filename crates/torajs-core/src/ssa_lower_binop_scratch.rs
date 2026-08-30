//! Per-binop scratch state — the flags [`crate::ssa_lower_binop_with_ids`]
//! sets before dispatching to the inner impl and restores after, plus
//! the plain-NaN operand trio, whose lifetime is one operand rather
//! than one binop (unary `-` / `+` sets it too — same question, one
//! operand instead of two).
//!
//! Split out of the [`crate::ssa_lower_ctx_struct::LowerCtx`] field face
//! (RFC 20260806 blade 0). The seven live together because they share one
//! lifetime — *the binop currently being lowered* — which is categorically
//! shorter than everything around them in that struct: the module-level
//! tables threaded in at construction, and the per-let analysis sets primed
//! once per function. Grouping them by that lifetime is what makes the
//! set-then-restore discipline visible at the type level instead of spread
//! across seven sibling fields.

use crate::ast::ExprId;

/// The scratch flags for one in-flight binop lowering.
///
/// Every field is written by `lower_binop_with_ids` on the way in and
/// restored on the way out; nothing here survives a single binop.
#[derive(Default)]
pub(crate) struct BinopScratch {
    /// P1.5/P1.8 — which side (if any) is a frontend `Type::Undefined`
    /// source. The Eq/Neq Any-side packing reads these to pick
    /// `ANY_UNDEF=5` vs `ANY_NULL=0`.
    pub(crate) left_undef_id: Option<ExprId>,
    pub(crate) right_undef_id: Option<ExprId>,
    /// Chunk 612 companion — which side (if any) is a frontend `Type::Null`
    /// source (the `null` literal or a Null-typed binding). The Eq/Neq
    /// nullish folds combine these with the undef flags to answer
    /// undefined-vs-null statically for bindings, not just literals (a
    /// Null/Undefined-typed Load is not `ConstPtrNull`, so operand-shape
    /// checks alone miss it).
    pub(crate) left_null_id: Option<ExprId>,
    pub(crate) right_null_id: Option<ExprId>,
    /// RFC 20260708-typed-arr-oob-read chunk 2 — the operand is an F64 that
    /// may hold the undefined-NaN sentinel (`number[]` index read / alias);
    /// `=== undefined` compares the bits instead of the cross-type false
    /// fold.
    pub(crate) left_f64_undefable: bool,
    pub(crate) right_f64_undefable: bool,
    /// The one index-read expression whose out-of-range exit should
    /// answer a plain NaN instead of the F64 `undefined` sentinel.
    /// Set around lowering the operand of a numeric-only operator, so
    /// ToNumber(undefined) costs nothing — the exit already exists
    /// and only the constant in it changes. Matched by ExprId, so a
    /// read nested inside the index expression is untouched. See
    /// [`crate::ssa_lower_f64_sentinel_canon`].
    pub(crate) f64_oob_plain_for: Option<ExprId>,
    /// Set by [`crate::ssa_lower_binop`] when it lowered that operand
    /// with the index read's out-of-range exit already answering a
    /// plain NaN. The undefable flag above is then false for that
    /// side: the value cannot be the sentinel, so nothing downstream
    /// needs to undo it. Read and cleared by `lower_binop_with_ids`.
    pub(crate) left_lowered_plain: bool,
    pub(crate) right_lowered_plain: bool,
    /// Cluster #6 (rotation 442) — the operand is a nullable-arr
    /// source (an un-narrowed `match`/`exec` result; checker type
    /// Nullable(Array), SSA repr a plain Arr pointer with the
    /// in-band 0 sentinel). The string-concat coerce reads these to
    /// guard the sentinel and answer "null" per §13.15.3
    /// ToString(null) instead of handing NULL to `arr_join`.
    pub(crate) left_nullable_arr: bool,
    pub(crate) right_nullable_arr: bool,
    /// S9 square carve — set when both Mul operands are the same identifier
    /// (`x * x`): a value times itself can never be negative×zero, so -0 is
    /// unmintable and the int path keeps (mirrors the `width_of` square
    /// carve).
    pub(crate) mul_square: bool,
}
