//! Compile-time evaluation of enum member initializers.
//!
//! A member whose initializer is a constant enum expression gets its
//! value at compile time: it is folded into reads and, when numeric,
//! gets its reverse mapping as a literal key. Constant expressions are
//! literals, members of this enum, constant members of another enum
//! (`A.X`, `A["X"]`), `const` bindings without a type annotation whose
//! initializer is constant, and unary, binary and template operators
//! over them. Anything else is computed at runtime.

use super::super::*;
use crate::ast::{PropKey, UnaryOp, number_prop_key};
use std::ops::Range;
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

/// What the names in an initializer can refer to.
pub(super) struct EvalScope<'m> {
    /// The members declared so far, when evaluating inside an enum.
    pub(super) members: &'m [(PropKey, EnumValue)],
    pub(super) enum_name: &'m str,
    /// Token index the names are resolved at.
    pub(super) at: usize,
    /// The declaration being parsed, which binds nothing yet.
    pub(super) skip: Range<usize>,
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

fn const_of(v: Option<&EnumValue>) -> Option<EnumConst> {
    match v? {
        EnumValue::Const(c) => Some(c.clone()),
        EnumValue::Computed(_) => None,
    }
}

impl Parser<'_> {
    /// The spec-shaped compile-time evaluation of an initializer —
    /// `None` when it needs the runtime.
    pub(super) fn enum_const_eval(&self, eid: ExprId, sc: &EvalScope) -> Option<EnumConst> {
        use EnumConst::{Num, Str};
        match self.ast.get_expr(eid) {
            Expr::Number(n) => Some(Num(*n)),
            Expr::String(s) => Some(Str(s.clone())),
            Expr::Ident(x) => match member_const(sc.members, x) {
                Some(v) => const_of(Some(v)),
                None => self.const_binding_value(x, sc),
            },
            Expr::Member { obj, name } => self.enum_member_value(*obj, name, sc),
            Expr::Index { obj, index } => match self.ast.get_expr(*index) {
                Expr::String(s) => self.enum_member_value(*obj, s.as_str()?, sc),
                _ => None,
            },
            Expr::Unary { op, expr } => match (op, self.enum_const_eval(*expr, sc)?) {
                (UnaryOp::Neg, Num(n)) => Some(Num(-n)),
                (UnaryOp::Plus, Num(n)) => Some(Num(n)),
                (UnaryOp::BitNot, Num(n)) => Some(Num(f64::from(!to_int32(n)))),
                _ => None,
            },
            Expr::BinOp { op, left, right } => {
                let l = self.enum_const_eval(*left, sc)?;
                let r = self.enum_const_eval(*right, sc)?;
                binop(*op, l, r)
            }
            // A template substitution is wrapped in a synthesized
            // `String(…)` call; over a constant it is a constant.
            Expr::Call { callee, args }
                if args.len() == 1 && self.ast.template_str_calls.contains(callee) =>
            {
                match self.enum_const_eval(args[0], sc)? {
                    Num(n) => Some(Str(Wtf8Buf::from(number_prop_key(n).lossy().as_ref()))),
                    s => Some(s),
                }
            }
            _ => None,
        }
    }

    /// `E.M` / `E["M"]`: a member of the enum being declared, or of
    /// another enum in scope.
    fn enum_member_value(&self, obj: ExprId, member: &str, sc: &EvalScope) -> Option<EnumConst> {
        let Expr::Ident(o) = self.ast.get_expr(obj) else {
            return None;
        };
        if o == sc.enum_name {
            return const_of(member_const(sc.members, member));
        }
        let i = self.visible_enum(o, sc.at, &sc.skip)?;
        const_of(member_const(&self.enums.states[i].members, member))
    }

    /// A `const` in scope whose initializer is constant, evaluated
    /// where it is declared.
    fn const_binding_value(&self, name: &str, sc: &EvalScope) -> Option<EnumConst> {
        let i = self.visible_const(name, sc.at, &sc.skip)?;
        let (init, pos) = self.const_binding(i);
        let at_decl = EvalScope {
            members: &[],
            enum_name: "",
            at: pos,
            skip: 0..0,
        };
        self.enum_const_eval(init, &at_decl)
    }
}

fn binop(op: BinOp, l: EnumConst, r: EnumConst) -> Option<EnumConst> {
    use EnumConst::{Num, Str};
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
