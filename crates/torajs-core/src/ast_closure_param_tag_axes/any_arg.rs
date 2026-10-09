//! Any-argument axis — a call through a fn-typed parameter that hands
//! an `any` binding to an object-typed position (`cb(a)` inside
//! `function hof(cb: (p: P) => string, a: any)`).
//!
//! The checker admits that call only through the value's boxed entry,
//! whose adapter hands an argument the typed body cannot read to the
//! all-`any` twin (`check_type_of_call::any_struct_slot`); a bare
//! pointer has no boxed entry, so the parameter takes the closure
//! repr. `any` is read off the syntax — a parameter or local of the
//! function annotated `any`, or an `as any` — so an `any` the checker
//! infers (`const v = JSON.parse(s)`) misses the axis and the call
//! stays refused. A primitive position never reaches the twin.

use super::{is_fn_typed_seg, param_callee_calls};
use crate::ast::{Ast, Expr, ExprId, Stmt};
use std::collections::{HashMap, HashSet};

/// Seed marks for the fn-typed parameters called with an `any`
/// argument for an object position.
pub(crate) fn any_obj_arg_seeds(
    ast: &Ast,
    fn_params: &HashMap<String, Vec<(usize, String)>>,
) -> HashSet<(String, usize)> {
    let mut seeds = HashSet::new();
    for (d, call, segs) in param_callee_calls(ast) {
        let Expr::Call { callee, args } = ast.get_expr(call) else {
            continue;
        };
        let (Expr::Ident(f), Some(own)) = (ast.get_expr(*callee), fn_params.get(&d)) else {
            continue;
        };
        let Some((i, _)) = own.iter().find(|(_, p)| p == f) else {
            continue;
        };
        let anys = any_bindings(ast, &d);
        let any_arg = |a: ExprId| match ast.get_expr(a) {
            Expr::Ident(n) => anys.contains(n),
            Expr::As { ty_ann, .. } => ty_ann.trim() == "any",
            _ => false,
        };
        if segs
            .iter()
            .zip(args)
            .any(|(s, &a)| object_seg(s) && any_arg(a))
        {
            seeds.insert((d, *i));
        }
    }
    seeds
}

/// The parameters and body-level locals of `fname` annotated `any`.
fn any_bindings(ast: &Ast, fname: &str) -> HashSet<String> {
    let is_any = |a: &Option<String>| a.as_deref().map(str::trim) == Some("any");
    let mut out = HashSet::new();
    let mut stack: Vec<&Stmt> = ast.stmts.iter().collect();
    while let Some(s) = stack.pop() {
        if let Stmt::FnDecl {
            name, params, body, ..
        } = s
            && name == fname
        {
            out.extend(
                params
                    .iter()
                    .filter(|p| is_any(&p.type_ann))
                    .map(|p| p.name.clone()),
            );
            let mut inner: Vec<&Stmt> = body.iter().collect();
            while let Some(st) = inner.pop() {
                match st {
                    Stmt::LetDecl { name, type_ann, .. } if is_any(type_ann) => {
                        out.insert(name.clone());
                    }
                    Stmt::FnDecl { .. } => continue,
                    _ => {}
                }
                crate::ast_closure_param_tag::push_child_stmts(st, &mut inner);
            }
            return out;
        }
        crate::ast_closure_param_tag::push_child_stmts(s, &mut stack);
    }
    out
}

/// May the parameter annotation `seg` lower to an object cell? Only
/// the primitive, array and function spellings are ruled out; an alias
/// of a primitive over-marks, which costs that parameter its direct
/// dispatch.
fn object_seg(seg: &str) -> bool {
    let s = seg.trim();
    let s = s
        .strip_prefix("__nullable(")
        .and_then(|r| r.strip_suffix(')'))
        .unwrap_or(s);
    !(matches!(
        s,
        "number" | "string" | "boolean" | "bigint" | "symbol" | "any" | "unknown" | "void"
    ) || s.ends_with("[]")
        || s.starts_with('"')
        || is_fn_typed_seg(s))
}
