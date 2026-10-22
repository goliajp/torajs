//! What a BINARY operator's result gets named by the implicit
//! return-annotation sniff.
//!
//! Split out of [`super::implicit_generics_infer`] at rotation 621
//! under the 500-line file cap and the 200-line function cap.
//! Verbatim move — the arm reads its operands through the `recur`
//! closure, which arrives as two already-resolved `Option<String>`
//! thunks so the sibling needs no view of the expression arena.

use super::BinOp;
use super::implicit_generics_infer::{is_typevar_ann, unnameable_arith_operand};

/// `l` / `r` are the operands' own inferred annotations, resolved by
/// the caller (they are only consulted by the arms that read them).
pub(crate) fn binop_ann(
    op: &BinOp,
    l: &dyn Fn() -> Option<String>,
    r: &dyn Fn() -> Option<String>,
) -> Option<String> {
    match op {
        BinOp::Lt
        | BinOp::Gt
        | BinOp::Le
        | BinOp::Ge
        | BinOp::Eq
        | BinOp::Neq
        | BinOp::LooseEq
        | BinOp::LooseNeq => Some("boolean".into()),
        // §13.13 — `a && b` IS one of its two operands, never a
        // boolean: `a` when `a` is falsy, `b` otherwise. Claiming
        // `boolean` here put that annotation on the enclosing
        // function's return, and `function f(o) { return o.q && x }`
        // then answered `false` where node answers `undefined`.
        // Both operands agreeing is the only case with a name to
        // give; anything else declines and lets the checker's own
        // join (`check_type_of_binop::check_logical`) answer.
        BinOp::LAnd | BinOp::LOr => {
            let (l, r) = (l()?, r()?);
            (l == r).then_some(l)
        }
        // §6.1.6.2 — the BigInt operators answer a BigInt, not a
        // Number. The blanket `"number"` here stamped `: number`
        // on the enclosing function's return, and the specialized
        // body then handed a BigInt back through a Number-shaped
        // slot: `function f(a, b) { return a - b }` called with
        // `(7n, 2n)` printed the POINTER (`4344807632`), `/ % **`
        // failed in the register allocator, and the declared
        // spelling was refused outright ("expects Number, got
        // BigInt"). Same shape as the LAnd/LOr answer above.
        //
        // A mixed pair is a TypeError (§6.1.6.1.x) and has no
        // annotation to give, so it declines and lets the
        // checker's own rule answer — naming it `number` is how
        // `f(1n, 2)` came to answer 0 instead of throwing.
        BinOp::Sub
        | BinOp::Mul
        | BinOp::Div
        | BinOp::Mod
        | BinOp::Pow
        | BinOp::BitAnd
        | BinOp::BitOr
        | BinOp::BitXor
        | BinOp::Shl
        | BinOp::Shr
        | BinOp::UShr => match (l(), r()) {
            (Some(l), Some(r)) if l == "bigint" || r == "bigint" => (l == r).then_some(l),
            // An `any` or type-var operand CAN be a BigInt at run
            // time, and then the answer is a BigInt. `+` already
            // declines that pair (its arm reads its operands);
            // the rest claimed `number`, which is how
            // `function f(a, b) { return a - b }` called with
            // `(7n, 2n)` printed the pointer. Declining lets the
            // caller stamp `any`, which carries both.
            (Some(l), Some(r)) if unnameable_arith_operand(&l) || unnameable_arith_operand(&r) => {
                None
            }
            // an operand the sniff cannot name (`x.a` on `x: any`) may
            // be a BigInt as much as an `any` one; beside a number, a
            // string or a boolean it answers a Number or throws, since
            // a BigInt does not mix (§13.15.3)
            (Some(k), None) | (None, Some(k)) if never_bigint(&k) => Some("number".into()),
            (None, _) | (_, None) => None,
            _ => Some("number".into()),
        },
        // `+`: string-propagates / num+num=num / TypeVar+num=`any` (else Void).
        BinOp::Add => {
            let (l, r) = (l()?, r()?);
            match (l.as_str(), r.as_str()) {
                ("string", _) | (_, "string") => Some("string".into()),
                ("number", "number") => Some("number".into()),
                (a, "number") | ("number", a) if is_typevar_ann(a) => Some("any".into()),
                _ => None,
            }
        }
    }
}

/// A value whose numeric conversion is never a BigInt.
fn never_bigint(ann: &str) -> bool {
    matches!(ann, "number" | "string" | "boolean")
}
