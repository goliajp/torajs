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
    /// Cluster #6 (rotation 442) — the operand is a nullable-arr
    /// source (an un-narrowed `match`/`exec` result; checker type
    /// Nullable(Array), SSA repr a plain Arr pointer with the
    /// in-band 0 sentinel). The string-concat coerce reads these to
    /// guard the sentinel and answer "null" per §13.15.3
    /// ToString(null) instead of handing NULL to `arr_join`.
    pub(crate) left_nullable_ptr: bool,
    pub(crate) right_nullable_ptr: bool,
    /// The operand is a nullable-STRING source (checker type
    /// `string | undefined`, SSA repr a Str with the undefined
    /// sentinel in band). `+` reads these: the union's two faces
    /// answer different operators (`"a" + undefined` concatenates,
    /// `undefined + 1` is NaN), so the pair belongs to the any lane
    /// whenever the other side is not a String.
    pub(crate) left_nullable_str: bool,
    pub(crate) right_nullable_str: bool,
    /// S9 square carve — set when both Mul operands are the same identifier
    /// (`x * x`) and the square is an operand of an integer compare
    /// (`square_cmp`): a value times itself can never be negative×zero, so
    /// -0 is unmintable and the int path keeps.
    pub(crate) mul_square: bool,
    /// The square `x * x` (its ExprId) a compare being lowered takes as
    /// an operand; see `ssa_lower_binop::square`.
    pub(crate) square_cmp_operand: Option<ExprId>,
    /// Set while that square's own Mul dispatches: only it may keep the
    /// int path.
    pub(crate) square_armed: bool,
    /// The int-path square just lowered for the compare: its product and
    /// the root it squared.
    pub(crate) square_root: Option<(crate::ssa::ValueId, crate::ssa::Operand)>,
    /// Set while an integer `+ - *` whose result may leave the exact
    /// window dispatches (`WidthTable::arith_in_window`): it computes
    /// in f64.
    pub(crate) arith_unbounded: bool,
    /// The member read on the left of a `??` being lowered, and, once
    /// the read has answered the nullish test itself, the block its
    /// nullish arms left open (`ssa_lower_nullish_any::split`).
    pub(crate) nullish_split: Option<(ExprId, Option<crate::ssa::BlockId>)>,
    /// The field read keying a number dictionary's read, and, once the
    /// read has answered borrowed, the slot of the flag that says it
    /// took a stake after all (`ssa_lower_index::dict::ask_key_borrow`).
    pub(crate) key_borrow: Option<(ExprId, Option<crate::ssa::ValueId>)>,
    /// The one field that outlives its binop: a string `+` on the
    /// right of an assignment hands its throw check to the assignment
    /// (`ssa_lower_binop::str_check`).
    pub(crate) str_check: crate::ssa_lower_binop::str_check::StrConcatCheck,
}
