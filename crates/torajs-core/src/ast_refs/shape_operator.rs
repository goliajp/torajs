//! The operator half of the slot-shape inference — split from
//! `shape.rs` (rotation 542) when the BigInt legs pushed
//! `infer_slot_shape` to 199 of its 200 allowed lines.
//!
//! The seam is the question each half answers. `shape.rs` asks what
//! shape a VALUE has: a literal, an alias of another top-level
//! binding, the return annotation of a named fn. This file asks what
//! shape an OPERATOR produces — sometimes from the operator alone
//! (`<` is a boolean whatever it is handed), sometimes only once the
//! operands are known well enough to pick a leg (`&` is an integer
//! between Numbers and a BigInt between BigInts).
//!
//! Recursion runs back through the parent's `infer_slot_shape`, so
//! the depth cap and the "uncertain answers None" contract are the
//! same ones documented on `GlobalSlotShape`.

use super::shape::{GlobalSlotShape, infer_slot_shape};
use crate::ast::{Ast, BinOp, Expr, ExprId, UnaryOp};

pub(super) fn infer_operator_shape(ast: &Ast, init: ExprId, depth: u32) -> Option<GlobalSlotShape> {
    match ast.get_expr(init) {
        Expr::Unary {
            op: UnaryOp::Neg,
            expr,
        } => infer_slot_shape(ast, *expr, depth + 1),
        // A concatenation of two strings is a string, and string has
        // no width question — so the slot's runtime type is as
        // certain here as it is for a literal. Registered as 468-01:
        // `const src = "a b" + "!"` stayed a main-fn local, so
        // `function f() { return src.split(" ").length }` answered
        // "unknown identifier" and threw at runtime, while the same
        // program with the halves already joined compiled.
        //
        // One side is enough: §13.15.3 concatenates whenever EITHER
        // primitive is a String, so `"n" + count` is as certainly a
        // string as `"a" + "b"`. (A Symbol on the other side throws
        // in ToString, and a binding that never gets a value has no
        // slot to be wrong about.) `&&` / `||` / `??` yield an
        // operand rather than a fresh string, and are not additions.
        Expr::BinOp {
            op: BinOp::Add,
            left,
            right,
        } if infer_slot_shape(ast, *left, depth + 1) == Some(GlobalSlotShape::Str)
            || infer_slot_shape(ast, *right, depth + 1) == Some(GlobalSlotShape::Str) =>
        {
            Some(GlobalSlotShape::Str)
        }
        // The comparisons and `!` answer a boolean whatever they are
        // handed (§13.10, §13.11, §13.5.7), so no operand needs to be
        // known at all.
        Expr::BinOp { op, .. }
            if matches!(
                op,
                BinOp::Lt
                    | BinOp::Gt
                    | BinOp::Le
                    | BinOp::Ge
                    | BinOp::Eq
                    | BinOp::Neq
                    | BinOp::LooseEq
                    | BinOp::LooseNeq
            ) =>
        {
            Some(GlobalSlotShape::Bool)
        }
        Expr::Unary {
            op: UnaryOp::Not, ..
        } => Some(GlobalSlotShape::Bool),
        // `>>>` has no BigInt leg at all (§13.9.3.1 throws a
        // TypeError on either operand), so ToUint32 runs on whatever
        // it is handed and the answer is an integer.
        Expr::BinOp {
            op: BinOp::UShr, ..
        } => Some(GlobalSlotShape::I64),
        // The remaining bitwise operators and shifts dispatch on the
        // operand TYPE (§13.12 / §13.9 both go through
        // ApplyStringOrNumericBinaryOperator): two BigInts stay in
        // the BigInt world, two Numbers run ToInt32 / ToUint32, and a
        // mixed pair throws. Rotation 542 — the arm used to answer
        // I64 unconditionally, which was a SILENT wrong for the
        // BigInt leg: `const b = 6n & 3n` plus a named-fn read
        // promoted a BigInt CELL into an i64 slot and every read
        // printed the pointer as a decimal (`4372201680` where bun
        // says `2n`), while the same program without the named fn
        // was correct.
        Expr::BinOp { op, left, right }
            if matches!(
                op,
                BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr
            ) && is_known_bigint(ast, *left, depth)
                && is_known_bigint(ast, *right, depth) =>
        {
            Some(GlobalSlotShape::BigInt)
        }
        // One known non-BigInt side is enough for the integer answer:
        // the other side either ToInt32s alongside it or makes the
        // whole operator throw, and a binding that never gets a value
        // has no slot to be wrong about. Two UNKNOWN sides could be
        // a BigInt pair, so they decline — the same certainty bar the
        // arithmetic arm below already holds itself to. (This arm
        // used to be unconditional; it never saw a String operand
        // either way, since the checker refuses `"3" & 1` outright.)
        Expr::BinOp { op, left, right }
            if matches!(
                op,
                BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr
            ) && (is_known_non_bigint(ast, *left, depth)
                || is_known_non_bigint(ast, *right, depth)) =>
        {
            Some(GlobalSlotShape::I64)
        }
        // Two BigInts stay in the BigInt world for every arithmetic
        // operator (`+` included — the string arm above has already
        // declined, and BigInt + BigInt is addition, not
        // concatenation). Div / Mod / Pow can throw a RangeError,
        // which is the harmless direction: a binding that never gets
        // a value has no slot to be wrong about.
        Expr::BinOp { op, left, right }
            if matches!(
                op,
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod | BinOp::Pow
            ) && is_known_bigint(ast, *left, depth)
                && is_known_bigint(ast, *right, depth) =>
        {
            Some(GlobalSlotShape::BigInt)
        }
        // The remaining arithmetic answers a number: `-` `*` `/` `%`
        // `**` coerce both sides with ToNumber, and `+` does too once
        // neither side is a string — which the arm above has already
        // established by declining. Both operands must have a known
        // shape, which is what keeps BigInt out: `is_known_non_bigint`
        // excludes it by name, so `1n * 2n` is answered by the arm
        // above rather than claiming a number slot for a BigInt cell.
        //
        // WIDTH is not decided here. The arm answers I64 and the
        // lowerer corrects it to F64 when `num_width` marked this
        // global's slot fractional — the same correction the written
        // `: number` lane rides, and top-level lets are keyed as
        // globals in that analysis whether or not they promote.
        Expr::BinOp { op, left, right }
            if matches!(
                op,
                BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod | BinOp::Pow
            ) && is_known_non_bigint(ast, *left, depth)
                && is_known_non_bigint(ast, *right, depth) =>
        {
            Some(GlobalSlotShape::I64)
        }
        Expr::BinOp {
            op: BinOp::Add,
            left,
            right,
        } if is_known_number_ish(ast, *left, depth) && is_known_number_ish(ast, *right, depth) => {
            Some(GlobalSlotShape::I64)
        }
        _ => None,
    }
}

/// A shape that is certainly not a BigInt cell — every arithmetic
/// operator ToNumbers such an operand rather than staying in the
/// BigInt world.
fn is_known_non_bigint(ast: &Ast, e: ExprId, depth: u32) -> bool {
    matches!(
        infer_slot_shape(ast, e, depth + 1),
        Some(
            GlobalSlotShape::I64
                | GlobalSlotShape::F64
                | GlobalSlotShape::Bool
                | GlobalSlotShape::Str
        )
    )
}

/// A shape that is certainly a BigInt cell — the operand side of the
/// arms that stay in the BigInt world rather than coercing out of it.
fn is_known_bigint(ast: &Ast, e: ExprId, depth: u32) -> bool {
    infer_slot_shape(ast, e, depth + 1) == Some(GlobalSlotShape::BigInt)
}

/// The same, minus `Str` — `+` concatenates rather than adds when
/// either side is one, which the string arm answers first.
fn is_known_number_ish(ast: &Ast, e: ExprId, depth: u32) -> bool {
    matches!(
        infer_slot_shape(ast, e, depth + 1),
        Some(GlobalSlotShape::I64 | GlobalSlotShape::F64 | GlobalSlotShape::Bool)
    )
}
