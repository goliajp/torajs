//! A relational or equality compare whose one operand is the square
//! `x * x` of one identifier. The Mul that squares keeps the integer
//! product, and the compare reads it with a guard on the root; see
//! [`square_cmp`].

use crate::ast::{BinOp as AstBinOp, Expr, ExprId};
use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// One binary operator's part in a square compare: the compare that
/// reads a square, or the square's own Mul.
pub(crate) struct SquareCmp {
    /// The operand that is the square (`true` for the left), on a compare.
    side: Option<bool>,
    /// On the Mul that squares for a compare: it keeps the int path.
    armed: bool,
}

impl SquareCmp {
    pub(crate) fn begin(
        ctx: &mut LowerCtx<'_>,
        eid: ExprId,
        op: AstBinOp,
        left: ExprId,
        right: ExprId,
    ) -> Self {
        let armed = matches!(op, AstBinOp::Mul) && ctx.binop.square_cmp_operand == Some(eid);
        if armed {
            ctx.binop.square_cmp_operand = None;
        }
        Self {
            side: square_cmp_side(ctx, op, left, right),
            armed,
        }
    }

    /// Lowers the left (`is_left`) or right operand `e` through `lower`.
    pub(crate) fn operand<'a>(
        &self,
        ctx: &mut LowerCtx<'a>,
        is_left: bool,
        e: ExprId,
        lower: impl FnOnce(&mut LowerCtx<'a>) -> Operand,
    ) -> Operand {
        if self.side == Some(is_left) {
            ctx.binop.square_cmp_operand = Some(e);
        }
        lower(ctx)
    }

    /// The operator over its lowered operands `(a, b)`.
    pub(crate) fn lower(
        self,
        ctx: &mut LowerCtx<'_>,
        op: AstBinOp,
        (a, b): (Operand, Operand),
        (left, right): (ExprId, ExprId),
    ) -> Operand {
        let square = self.side.and_then(|left_side| {
            let (r, x) = ctx.binop.square_root.take()?;
            let sq = if left_side { a } else { b };
            (sq == Operand::Value(r)).then_some((left_side, x))
        });
        let saved_armed = std::mem::replace(&mut ctx.binop.square_armed, self.armed);
        // P1.5/P1.8 — pass the operand ExprIds so the Eq/Neq Any-side
        // packing can pick ANY_UNDEF=5 vs ANY_NULL=0.
        let result = match square {
            Some((left_side, x)) => square_cmp(ctx, op, (a, b), (left, right), left_side, x),
            None => ctx.lower_binop_with_ids(op, a, b, Some(left), Some(right)),
        };
        ctx.binop.square_armed = saved_armed;
        if self.armed
            && let Operand::Value(r) = result
            && ctx.operand_ty(&result) == Type::I64
        {
            ctx.binop.square_root = Some((r, a));
        }
        result
    }
}

/// `floor(sqrt(i64::MAX))`: for `|x|` up to this, `x * x` does not wrap.
const SQUARE_ROOT_MAX: i64 = 3_037_000_499;

/// Which operand of the relational or equality compare `op` is a square
/// `x * x` of one identifier (`true` for the left), when the other is
/// not one too.
fn square_cmp_side(ctx: &LowerCtx<'_>, op: AstBinOp, left: ExprId, right: ExprId) -> Option<bool> {
    if !matches!(
        op,
        AstBinOp::Lt
            | AstBinOp::Gt
            | AstBinOp::Le
            | AstBinOp::Ge
            | AstBinOp::Eq
            | AstBinOp::Neq
            | AstBinOp::LooseEq
            | AstBinOp::LooseNeq
    ) {
        return None;
    }
    let is_square = |e: ExprId| {
        matches!(ctx.ast.get_expr(e), Expr::BinOp { op: AstBinOp::Mul, left: l, right: r }
            if matches!((ctx.ast.get_expr(*l), ctx.ast.get_expr(*r)),
                (Expr::Ident(a), Expr::Ident(b)) if a == b))
    };
    match (is_square(left), is_square(right)) {
        (true, false) => Some(true),
        (false, true) => Some(false),
        _ => None,
    }
}

/// A compare of the int square `x * x` (the left operand when
/// `left_side`). Number multiplication is f64: the product rounds past
/// 2^53 and never wraps. Against another integer, which an i64 Number
/// never takes past 2^53, the exact product compares as the rounded one
/// does (a square past 2^53 rounds to at least 2^53, and none rounds
/// onto 2^53 itself), so the integer compare stands while the product
/// does not wrap; past `SQUARE_ROOT_MAX` the square exceeds every i64
/// and the compare's answer is fixed. Against anything else the square
/// is the f64 product.
fn square_cmp(
    ctx: &mut LowerCtx<'_>,
    op: AstBinOp,
    (a, b): (Operand, Operand),
    (left, right): (ExprId, ExprId),
    left_side: bool,
    x: Operand,
) -> Operand {
    let other = if left_side { b } else { a };
    if ctx.operand_ty(&other) != Type::I64 {
        let xf = ctx
            .f
            .append_inst(ctx.cur_block, InstKind::SiToFp(x), Type::F64, None);
        let sq = ctx.bin(
            SsaBinOp::FMul,
            Operand::Value(xf),
            Operand::Value(xf),
            Type::F64,
        );
        let (a, b) = if left_side { (sq, b) } else { (a, sq) };
        return ctx.lower_binop_with_ids(op, a, b, Some(left), Some(right));
    }
    let c = ctx.lower_binop_with_ids(op, a, b, Some(left), Some(right));
    // the answer when the square exceeds every i64
    let huge = matches!(
        (op, left_side),
        (AstBinOp::Gt | AstBinOp::Ge, true)
            | (AstBinOp::Lt | AstBinOp::Le, false)
            | (AstBinOp::Neq | AstBinOp::LooseNeq, _)
    );
    // `|x| <= SQUARE_ROOT_MAX` as one unsigned compare; a wrapped sum
    // lands above the span
    let shifted = ctx.bin(
        SsaBinOp::Add,
        x,
        Operand::ConstI64(SQUARE_ROOT_MAX),
        Type::I64,
    );
    let span = Operand::ConstI64(2 * SQUARE_ROOT_MAX + 1);
    let (pred, join) = if huge {
        (IPred::Uge, SsaBinOp::Or)
    } else {
        (IPred::Ult, SsaBinOp::And)
    };
    let g = ctx.cmp(pred, shifted, span);
    ctx.bin(join, g, c, Type::Bool)
}
