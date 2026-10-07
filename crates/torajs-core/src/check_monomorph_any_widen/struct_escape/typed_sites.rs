//! The construction sites the lowering only ever calls through their
//! typed face, which therefore need no all-`any` twin (`closure.rs`).

use std::collections::HashSet;

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

/// The callback arguments the lowering calls through their typed face
/// only: a builtin array iteration on a typed array, and a string
/// `replace` whose callback the typed callback ABI serves. A receiver
/// or callee the checker typed `any` (an any receiver, a patched
/// prototype method) goes through the any lane instead.
pub(super) fn typed_callback_sites(c: &Checker, ast: &Ast) -> HashSet<ExprId> {
    let mut out = HashSet::new();
    for e in &ast.exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Member { obj, name } = ast.get_expr(*callee) else {
            continue;
        };
        if c.expr_types.get(callee) == Some(&Type::Any) {
            continue;
        }
        let recv = c.expr_types.get(obj);
        let site = match name.as_str() {
            "map" | "filter" | "reduce" | "reduceRight" | "forEach" | "find" | "findIndex"
            | "findLast" | "findLastIndex" | "some" | "every" => {
                matches!(recv, Some(Type::Array(el)) if **el != Type::Any).then_some(0)
            }
            "sort" => match (recv, args.first().map(|a| ast.get_expr(*a))) {
                (Some(Type::Array(el)), Some(Expr::Closure { fn_name, .. }))
                    if **el != Type::Any =>
                {
                    matches!(c.globals.get(fn_name), Some(Type::Function(ps, _))
                        if ps.iter().all(|p| p == &**el))
                    .then_some(0)
                }
                _ => None,
            },
            "replace" | "replaceAll" if recv == Some(&Type::String) && args.len() >= 2 => {
                replace_stays_typed(c, ast, args).then_some(1)
            }
            _ => None,
        };
        if let Some(i) = site
            && let Some(&cb) = args.get(i)
            && matches!(ast.get_expr(cb), Expr::Closure { .. })
        {
            out.insert(cb);
        }
    }
    out
}

/// `s.replace(re, cb)` whose callback the kernel lane calls through the
/// typed callback ABI, by the lowering's own predicate. Only a string
/// slot is mapped onto its lowered type; any other parameter makes
/// the site loose.
fn replace_stays_typed(c: &Checker, ast: &Ast, args: &[ExprId]) -> bool {
    use crate::ssa::Type as Ssa;
    use crate::ssa_lower_call_str_regex_emit::{callback_stays_typed, static_replace_caps};
    let Expr::Closure { fn_name, .. } = ast.get_expr(args[1]) else {
        return false;
    };
    let Some(Type::Function(params, ret)) = c.globals.get(fn_name) else {
        return false;
    };
    let lowered = |t: &Type| {
        if *t == Type::String {
            Ssa::Str
        } else {
            Ssa::Any
        }
    };
    let params: Vec<Ssa> = params.iter().map(lowered).collect();
    let caps = static_replace_caps(ast, args[0]);
    // a pattern that is no RegExp takes the string-pattern lane
    if caps.is_none() && c.expr_types.get(&args[0]) != Some(&Type::RegExp) {
        return false;
    }
    callback_stays_typed(&params, lowered(ret), caps)
}
