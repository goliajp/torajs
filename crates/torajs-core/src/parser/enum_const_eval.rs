//! Compile-time evaluation of enum member initializers.
//!
//! A member whose initializer is a constant enum expression (literals,
//! earlier members, arithmetic and string concatenation over them) gets
//! its value at compile time: it is folded into reads and, when numeric,
//! gets its reverse mapping as a literal key. Anything else is computed
//! at runtime.

use super::*;
use crate::ast::{PropKey, UnaryOp, number_prop_key};
use torajs_wtf8::Wtf8Buf;

#[derive(Clone)]
pub(super) enum EnumConst {
    Num(f64),
    Str(Wtf8Buf),
}

#[derive(Clone)]
pub(super) enum EnumValue {
    Const(EnumConst),
    /// The hoisted `const __enum_E_M` holding the initializer's value.
    Computed(String),
}

/// Canonical array index per §6.1.7 — these keys sort first in
/// property order, so the object literal lists them first.
pub(super) fn is_array_index(n: f64) -> bool {
    n.fract() == 0.0 && (0.0..4294967295.0).contains(&n)
}

pub(super) fn to_int32(n: f64) -> i32 {
    if !n.is_finite() {
        return 0;
    }
    n.trunc().rem_euclid(4294967296.0) as u32 as i32
}

pub(super) fn member_const<'m>(
    members: &'m [(PropKey, EnumValue)],
    name: &str,
) -> Option<&'m EnumValue> {
    members
        .iter()
        .rev()
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v)
}

/// The spec-shaped compile-time evaluation of a member initializer —
/// `None` when it needs the runtime (a call, a reference to a binding
/// that is not a member of this enum, a non-literal string shape).
pub(super) fn const_eval(
    ast: &Ast,
    eid: ExprId,
    members: &[(PropKey, EnumValue)],
    enum_name: &str,
) -> Option<EnumConst> {
    use EnumConst::{Num, Str};
    match ast.get_expr(eid) {
        Expr::Number(n) => Some(Num(*n)),
        Expr::String(s) => Some(Str(s.clone())),
        Expr::Ident(x) => match member_const(members, x)? {
            EnumValue::Const(c) => Some(c.clone()),
            EnumValue::Computed(_) => None,
        },
        Expr::Member { obj, name } if matches!(ast.get_expr(*obj), Expr::Ident(o) if o == enum_name) => {
            match member_const(members, name)? {
                EnumValue::Const(c) => Some(c.clone()),
                EnumValue::Computed(_) => None,
            }
        }
        // bun folds a string literal's `.length` too, so a member after
        // one keeps its auto-incremented value on both sides.
        Expr::Member { obj, name } if name == "length" => match ast.get_expr(*obj) {
            Expr::String(s) => Some(Num(s.code_unit_len() as f64)),
            _ => None,
        },
        Expr::Unary { op, expr } => match (op, const_eval(ast, *expr, members, enum_name)?) {
            (UnaryOp::Neg, Num(n)) => Some(Num(-n)),
            (UnaryOp::Plus, Num(n)) => Some(Num(n)),
            (UnaryOp::BitNot, Num(n)) => Some(Num(f64::from(!to_int32(n)))),
            _ => None,
        },
        Expr::BinOp { op, left, right } => {
            let l = const_eval(ast, *left, members, enum_name)?;
            let r = const_eval(ast, *right, members, enum_name)?;
            match (op, l, r) {
                (BinOp::Add, Str(mut a), Str(b)) => {
                    a.push_wtf8(&b);
                    Some(Str(a))
                }
                (BinOp::Add, Str(mut a), Num(b)) => {
                    a.push_str(number_prop_key(b).lossy().as_ref());
                    Some(Str(a))
                }
                (BinOp::Add, Num(a), Str(b)) => {
                    let mut s = Wtf8Buf::from(number_prop_key(a).lossy().as_ref());
                    s.push_wtf8(&b);
                    Some(Str(s))
                }
                (op, Num(a), Num(b)) => {
                    let ia = to_int32(a);
                    let ib = to_int32(b);
                    let sh = (ib & 31) as u32;
                    Some(Num(match op {
                        BinOp::Add => a + b,
                        BinOp::Sub => a - b,
                        BinOp::Mul => a * b,
                        BinOp::Div => a / b,
                        BinOp::Mod => a % b,
                        // §6.1.6.1.3: unlike powf, `1 ** NaN` and
                        // `(-1) ** Infinity` are NaN
                        BinOp::Pow if b.is_nan() || (a.abs() == 1.0 && b.is_infinite()) => f64::NAN,
                        BinOp::Pow => a.powf(b),
                        BinOp::BitAnd => f64::from(ia & ib),
                        BinOp::BitOr => f64::from(ia | ib),
                        BinOp::BitXor => f64::from(ia ^ ib),
                        BinOp::Shl => f64::from(ia.wrapping_shl(sh)),
                        BinOp::Shr => f64::from(ia >> sh),
                        BinOp::UShr => f64::from((ia as u32) >> sh),
                        _ => return None,
                    }))
                }
                _ => None,
            }
        }
        // A template substitution is wrapped in a synthesized
        // `String(…)` call; over a constant it is a constant.
        Expr::Call { callee, args }
            if args.len() == 1 && ast.template_str_calls.contains(callee) =>
        {
            match const_eval(ast, args[0], members, enum_name)? {
                Num(n) => Some(Str(Wtf8Buf::from(number_prop_key(n).lossy().as_ref()))),
                s => Some(s),
            }
        }
        _ => None,
    }
}
