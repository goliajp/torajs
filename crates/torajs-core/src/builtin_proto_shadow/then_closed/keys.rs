//! The computed keys of the `then` closure proof. A key clears only by
//! its form: a literal, literals the walk folds into one, a loop
//! counter ([`super::counters`]), or an operator whose answer is never
//! a string. Its type does not count: an `as`, an `any` argument or an
//! `any` initializer puts a string built at run time under a string
//! literal type or under `number` (ECMA-262 §13.15.2 evaluates the
//! key's value, whatever the program declared it to be).

use super::counters::Counters;
use super::forms::danger_key;
use super::{Opening, open};
use crate::ast::{Ast, BinOp, Expr, ExprId};

/// A computed member key that does not fold to a harmless spelled key.
pub(super) fn key_opening(ast: &Ast, index: ExprId, counters: &Counters) -> Option<Opening> {
    match fold_key(ast, index, counters) {
        Some(Key::Str(k)) if danger_key(&k) => open("computed key", k),
        Some(_) => None,
        None => open("computed key", ""),
    }
}

/// A call whose callee the walk must read with its arguments: the
/// `super` property kernels take their key as the second argument.
pub(super) fn call_opening(
    ast: &Ast,
    callee: ExprId,
    args: &[ExprId],
    counters: &Counters,
) -> Option<Opening> {
    match ast.get_expr(callee) {
        Expr::Ident(n) if n.starts_with("__torajs_super_prop_") => match args.get(1) {
            Some(&k) => key_opening(ast, k, counters).map(|_| Opening {
                why: "super key",
                at: n.clone(),
            }),
            None => open("super key", n.clone()),
        },
        _ => None,
    }
}

/// The value a key folds to: a string, or a value that is not one (a
/// number, a boolean, a bigint), whose property key is a numeral,
/// `true` / `false`, `NaN` or `Infinity`, none of them dangerous.
enum Key {
    Str(String),
    NotStr,
}

/// Fold a key by its form: `+` of two strings or of two non-strings,
/// a loop counter's read, and any other binary arithmetic, comparison or
/// bitwise operator and any unary operator, whatever their operands
/// (§13.5, §13.6-§13.12: their answer is a Number, a BigInt or a
/// Boolean; the operands are expressions the walk judges on their own).
/// A mix of a string and a number is left unfolded (the number's
/// spelling is not modelled), as are `&&` / `||` / `??`, whose value is
/// one of the operands.
fn fold_key(ast: &Ast, id: ExprId, counters: &Counters) -> Option<Key> {
    match ast.get_expr(id) {
        Expr::Number(_) | Expr::Bool(_) | Expr::BigInt { .. } => Some(Key::NotStr),
        Expr::Ident(_) if counters.contains(id) => Some(Key::NotStr),
        Expr::String(s) => s.as_wtf8().as_str().map(|k| Key::Str(k.to_string())),
        Expr::Unary { .. } | Expr::PostIncr { .. } => Some(Key::NotStr),
        Expr::BinOp {
            op: BinOp::LAnd | BinOp::LOr,
            ..
        } => None,
        Expr::BinOp {
            op: BinOp::Add,
            left,
            right,
        } => match (
            fold_key(ast, *left, counters)?,
            fold_key(ast, *right, counters)?,
        ) {
            (Key::Str(a), Key::Str(b)) => Some(Key::Str(a + &b)),
            (Key::NotStr, Key::NotStr) => Some(Key::NotStr),
            _ => None,
        },
        Expr::BinOp { .. } => Some(Key::NotStr),
        _ => None,
    }
}
