//! `Object.values(o)` / `Object.entries(o)` read every field of `o` as
//! a value, so a method field leaves the literal the same way `o.m`
//! does. Constructed through the nominal body, `new (Object.values(o)[0]
//! as any)()` wrote the fresh object at the literal's struct offsets
//! and answered `undefined`.
//!
//! Only a field that takes the literal as its receiver matters: a
//! function expression or method shorthand whose body reads `this`
//! (its lifted closure captures `__this`). An arrow takes the lexical
//! `this` and a `this`-free method never reads a receiver, so a literal
//! holding only those keeps its nominal type.

use std::collections::HashSet;

use super::{Expr, ExprId, Stmt, widen_inner};

pub(super) fn widen_values_read_objlits(
    stmts: &mut [Stmt],
    exprs: &[Expr],
    objlit_method_exprs: &HashSet<ExprId>,
) {
    let spread = whole_value_reads(exprs);
    if spread.is_empty() {
        return;
    }
    let admit = |name: &str, init: ExprId| {
        spread.contains(name)
            && matches!(&exprs[init.0 as usize], Expr::ObjectLit { fields }
                if fields.iter().any(|(_, fe)| receiver_method(exprs, objlit_method_exprs, *fe)))
    };
    widen_inner(stmts, &admit);
}

fn receiver_method(exprs: &[Expr], objlit_method_exprs: &HashSet<ExprId>, fe: ExprId) -> bool {
    objlit_method_exprs.contains(&fe)
        && matches!(&exprs[fe.0 as usize], Expr::Closure { captures, .. }
            if captures.iter().any(|c| c == "__this"))
}

/// Bindings handed whole to `Object.values` / `Object.entries`.
fn whole_value_reads(exprs: &[Expr]) -> HashSet<&str> {
    exprs
        .iter()
        .filter_map(|e| {
            let Expr::Call { callee, args } = e else {
                return None;
            };
            let Expr::Member { obj, name } = &exprs[callee.0 as usize] else {
                return None;
            };
            let object = matches!(&exprs[obj.0 as usize], Expr::Ident(o) if o == "Object");
            if !object || !matches!(name.as_str(), "values" | "entries") {
                return None;
            }
            match &exprs[args.first()?.0 as usize] {
                Expr::Ident(b) => Some(b.as_str()),
                _ => None,
            }
        })
        .collect()
}
