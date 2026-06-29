//! `Math.{pow,atan2,imul}` short-arity + 2-arg undef widen
//! arms extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 228 — twenty-first sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! Both arms share the same 3-method namespace
//! (`pow / atan2 / imul`) but cover disjoint arity slices;
//! they sit on either side of the `Math.{min,max}` variadic
//! arm in the main cascade (B can never match pow/atan2/imul
//! because `m` is disjoint, so abstracting both into one
//! sibling preserves cascade order).
//!
//! - **S205 short-arity (0/1-arg)** — declared
//!   `vec![Number, Number]` sig rejects shorter forms at the
//!   generic arity gate. Spec §21.3.2.{19,5,26}: `imul`
//!   `ToUint32` (undefined → 0 → 0); `pow / atan2` `ToNumber`
//!   (undefined → NaN → NaN). Args (if any) must be
//!   `Number`.
//! - **S228 2-arg undef widen** — propagate NaN / fold to 0
//!   when either arg is statically `Undefined`. Both args
//!   must be `Number | Undefined`; the all-Number-no-undef
//!   path falls through to the general `Type::Function`
//!   table.
//!
//! Returns `Some(Ok(Number))` on match; `Some(Err(_))` on
//! non-Number arg; `None` when no arm matches.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    let Expr::Member { obj, name: m } = ast.get_expr(*callee) else {
        return None;
    };
    let Expr::Ident(ns) = ast.get_expr(*obj) else {
        return None;
    };
    if ns != "Math" || !matches!(m.as_str(), "pow" | "atan2" | "imul") {
        return None;
    }
    if args.len() < 2 {
        for &aid in args {
            let aty = match checker.type_of(ast, aid) {
                Ok(t) => t,
                Err(e) => return Some(Err(e)),
            };
            if aty != Type::Number {
                return Some(Err(format!("Math.{m} args must be number, got {aty:?}")));
            }
        }
        return Some(Ok(Type::Number));
    }
    if args.len() == 2 {
        let arg0_ty = match checker.type_of(ast, args[0]) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        let arg1_ty = match checker.type_of(ast, args[1]) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        let any_undef = matches!(arg0_ty, Type::Undefined) || matches!(arg1_ty, Type::Undefined);
        if any_undef {
            for (i, aty) in [&arg0_ty, &arg1_ty].iter().enumerate() {
                if !matches!(**aty, Type::Number | Type::Undefined) {
                    return Some(Err(format!("Math.{m} arg {i} must be number, got {aty:?}")));
                }
            }
            return Some(Ok(Type::Number));
        }
    }
    None
}
