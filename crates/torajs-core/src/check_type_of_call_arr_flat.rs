//! `arr.flat(N)` early-route arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 212 — sixth sub-batch of check_type_of_call.rs
//! per-shape decomposition).
//!
//! `arr.flat(N)` deep flattens an Array<Array<…>> N layers
//! deep. N must be a literal number / `Infinity` / `undefined`
//! so the type checker can peel that many `Array<>` layers
//! from the receiver's element type. depth=0 is a shallow
//! clone (returns Array<T_0>); depth>0 peels per-iter,
//! stopping early if a layer is non-Array. Subset constraint:
//! literal depth only (no `flat(n)` with runtime n — would
//! need a depth-aware runtime helper).
//!
//! Special depth literals:
//! - S129-5: `Infinity` → peel up to 64 layers (matches V8/JSC
//!   fixture conventions; no realistic nesting reaches that
//!   depth).
//! - S220: `undefined` → behaves as `xs.flat()` default
//!   (depthNum = 1) per ES §23.1.3.10 step 1.
//!
//! Trailing args[1..] silent-drop per S289 (ES §23.1.3.10).
//!
//! Returns `Some(Ok(_))` on match, `Some(Err(_))` on arg
//! shape mismatch (negative depth / non-Array receiver / non-
//! literal depth), `None` when callee isn't `.flat` or args
//! is empty (`xs.flat()` 0-arg uses the regular method-table
//! arm).

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    if let Expr::Member {
        obj: recv,
        name: m_name,
    } = ast.get_expr(*callee)
        && m_name == "flat"
        && !args.is_empty()
    {
        // S289 — Array<T>.flat(depth, ...trailing) trailing-arg
        // ignore per ES §23.1.3.10. Spec reads only `depth`;
        // tora's runtime + SSA-emit also peek only args[0].
        // Eval-and-drop args[1..] for side effects below.
        for &aid in &args[1..] {
            if let Err(e) = checker.type_of(ast, aid) {
                return Some(Err(e));
            }
        }
        // S129-5 — accept `Infinity` as the depth literal
        // (ES §23.1.3.13 spec form for full-depth flatten).
        // Both check + ssa-lower peel up to 64 layers
        // (matches V8/JSC fixture conventions; no
        // realistic nesting reaches that depth).
        let depth_opt: Option<i64> = match ast.get_expr(args[0]) {
            Expr::Number(d) => Some(*d as i64),
            Expr::Ident(name) if name == "Infinity" => Some(64),
            // S220 — `xs.flat(undefined)` per ES §23.1.3.10
            // step 1: `If depth is undefined, depthNum = 1`.
            // bun matches: explicit-undefined behaves as the
            // 0-arg `xs.flat()` default.
            Expr::Ident(name) if name == "undefined" => Some(1),
            _ => None,
        };
        if let Some(depth) = depth_opt {
            if depth < 0 {
                return Some(Err("flat depth must be non-negative".into()));
            }
            let recv_ty = match checker.type_of(ast, *recv) {
                Ok(t) => t,
                Err(e) => return Some(Err(e)),
            };
            let Type::Array(_) = &recv_ty else {
                return Some(Err(format!(
                    "flat receiver must be Array<...>, got {recv_ty:?}"
                )));
            };
            let mut t = recv_ty.clone();
            for _ in 0..depth {
                if let Type::Array(elem) = t.clone()
                    && let Type::Array(inner_inner) = (*elem).clone()
                {
                    t = Type::Array(inner_inner);
                } else {
                    break;
                }
            }
            return Some(Ok(t));
        }
        // Non-literal depth (a variable / member read / call) —
        // §23.1.3.13 step 2 runs ToIntegerOrInfinity at RUNTIME
        // (NaN → 0, a Symbol/BigInt operand throws), so the peel
        // count is unknowable here: the receiver must still be an
        // Array, the operand types on its own, and the product is
        // Array<Any> (lowering mirror: the runtime-depth lane in
        // `ssa_lower_str_arr_join_flat` keys on the same
        // literal-shape test and rides the flat-depth kernel).
        let recv_ty = match checker.type_of(ast, *recv) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        let Type::Array(_) = &recv_ty else {
            return Some(Err(format!(
                "flat receiver must be Array<...>, got {recv_ty:?}"
            )));
        };
        if let Err(e) = checker.type_of(ast, args[0]) {
            return Some(Err(e));
        }
        return Some(Ok(Type::Array(Box::new(Type::Any))));
    }
    None
}
