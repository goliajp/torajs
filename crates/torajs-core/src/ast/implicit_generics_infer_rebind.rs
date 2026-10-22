//! Return-type sniff vs. reassigned bindings.
//!
//! The sniff types a `let` from its initializer, but a later
//! `name = value` may store a value of another type: `let t = 0;
//! t = t + a` (`a: any`) holds `"0x"` for a string `a`, so `return t`
//! is not a number. A binding keeps its sniffed type only when every
//! assign in the body sniffs to that same type (read with the binding
//! itself at that type, so `i = i + 1` keeps `number`); otherwise the
//! sniff forgets it and a return of it falls to `any`. A mutable
//! binding a closure in the body captures may be reassigned in the
//! closure's lifted body, which this walk cannot see, so the sniff
//! forgets it too.

use std::collections::{HashMap, HashSet};

use super::desugar_with::walk::{expr_node_children, stmt_children_ref, stmt_exprs};
use super::implicit_generics_infer::{AstExprsView, binds_to_params, infer_expr_ann_with};
use super::{Expr, ExprId, Stmt};

/// Drop from `binds` every name some assign in `body` gives a value
/// that does not sniff to its registered type, until none is left
/// (dropping `t` can unsettle `u = t`).
pub(super) fn drop_rebound(
    exprs: AstExprsView,
    body: &[Stmt],
    binds: &mut HashMap<String, String>,
    fn_sigs: &HashMap<String, String>,
) {
    let (assigned, captured_mutable) = assigned_values(exprs, body);
    for name in captured_mutable {
        binds.remove(&name);
    }
    drop_held(exprs, body, binds, fn_sigs);
    loop {
        let params = binds_to_params(binds);
        let unsettled: Vec<String> = assigned
            .iter()
            .filter(|(name, values)| {
                binds.get(*name).is_some_and(|ann| {
                    values.iter().any(|v| {
                        infer_expr_ann_with(exprs, *v, &params, binds, fn_sigs).as_ref()
                            != Some(ann)
                    })
                })
            })
            .map(|(name, _)| name.clone())
            .collect();
        if unsettled.is_empty() {
            return;
        }
        for name in unsettled {
            binds.remove(&name);
        }
    }
}

/// Drop every `number` / `string` / `boolean` binding of `body` whose
/// initializer does not sniff to that same type: an initializer that
/// reads an `any` value (`x`, `await Promise.resolve(x)`, `x as any`)
/// leaves the binding holding that value as is
/// (`check_stmt_let_decl::any_scalar_slot`), so only an initializer
/// shown to be of the annotated type lets the annotation say what a
/// return of it answers.
fn drop_held(
    exprs: AstExprsView,
    body: &[Stmt],
    binds: &mut HashMap<String, String>,
    fn_sigs: &HashMap<String, String>,
) {
    let params = binds_to_params(binds);
    let mut held = Vec::new();
    let mut stmts: Vec<&Stmt> = body.iter().collect();
    while let Some(s) = stmts.pop() {
        if let Stmt::LetDecl {
            name,
            type_ann: Some(ann),
            init,
            ..
        } = s
            && matches!(ann.as_str(), "number" | "string" | "boolean")
            && infer_expr_ann_with(exprs, *init, &params, binds, fn_sigs).as_ref() != Some(ann)
        {
            held.push(name.clone());
        }
        stmts.extend(stmt_children_ref(s));
    }
    for name in held {
        binds.remove(&name);
    }
}

/// Every `name = value` rhs in `body`, by name, and the mutable
/// `let`s of `body` some closure made in it captures. A nested
/// function is its own scope and is not entered.
fn assigned_values(
    exprs: AstExprsView,
    body: &[Stmt],
) -> (HashMap<String, Vec<ExprId>>, HashSet<String>) {
    let mut out: HashMap<String, Vec<ExprId>> = HashMap::new();
    let mut mutable: HashSet<&str> = HashSet::new();
    let mut captured: HashSet<&str> = HashSet::new();
    let mut stmts: Vec<&Stmt> = body.iter().collect();
    let mut roots: Vec<ExprId> = Vec::new();
    while let Some(s) = stmts.pop() {
        if let Stmt::LetDecl {
            name,
            mutable: true,
            ..
        } = s
        {
            mutable.insert(name);
        }
        roots.extend(stmt_exprs(s));
        stmts.extend(stmt_children_ref(s));
    }
    while let Some(e) = roots.pop() {
        let node = &exprs[e.0 as usize];
        if let Expr::Assign { target, value } = node
            && let Expr::Ident(name) = &exprs[target.0 as usize]
        {
            out.entry(name.clone()).or_default().push(*value);
        }
        if let Expr::Closure { captures, .. } = node {
            captured.extend(captures.iter().map(String::as_str));
        }
        roots.extend(expr_node_children(node));
    }
    let captured_mutable = mutable
        .intersection(&captured)
        .map(|s| s.to_string())
        .collect();
    (out, captured_mutable)
}
