//! The functions whose return a check round may retype `any`.

use std::collections::{HashMap, HashSet};

use super::fn_slots::{Guarded, SlotFlow, fn_slot_values};
use crate::ast::{Ast, Expr, ExprId, Stmt};

/// The functions whose return a round may retype `any`: a declaration
/// or a lifted closure declared to return a `number`, `string` or
/// `boolean`, or with no annotation, and only ever called by name or
/// handed to a fn-typed slot ([`called_by_name_only`]). A generator or
/// an async function answers an object of its own and keeps its face.
pub(super) fn retypable_returns(
    ast: &Ast,
    allow: &dyn Fn(&Guarded) -> bool,
) -> (HashSet<String>, SlotFlow) {
    let mut out = HashSet::new();
    crate::ast::walk_stmts(&ast.stmts, &mut |s| {
        if let Stmt::FnDecl {
            name,
            return_type,
            is_generator: false,
            ..
        } = s
            && (!name.starts_with("__") || name.starts_with("__closure_"))
            && !ast.async_fns.contains(name)
            && matches!(
                return_type.as_deref(),
                None | Some("number" | "string" | "boolean")
            )
        {
            out.insert(name.clone());
        }
    });
    let flow = fn_slot_values(ast, allow);
    let by_name = called_by_name_only(ast, &flow.values);
    out.retain(|f| by_name.contains(f));
    (out, flow)
}

/// The names the functions `fns` are read by: their own, and the
/// binding with no annotation a closure among them initializes.
pub(super) fn roots(ast: &Ast, fns: &HashSet<String>) -> Vec<String> {
    let mut out: Vec<String> = fns.iter().cloned().collect();
    crate::ast::walk_stmts(&ast.stmts, &mut |s| {
        if let Stmt::LetDecl { name, init, .. } = s
            && let Expr::Closure { fn_name, .. } = ast.get_expr(*init)
            && fns.contains(fn_name)
        {
            out.push(name.clone());
        }
    });
    out
}

/// The functions only ever called by name, or handed to a fn-typed
/// slot whose calls follow the face of what it holds: the initializer
/// of a binding annotated with a function type (`const h: (x: number)
/// => number = q`, the let lane's face census) or the argument of a
/// declared function's fn-typed parameter (`run(q)`, the parameter
/// census), whose calls then answer what `q` returns. A function that
/// flows on as a value anywhere else may land in a slot typed with its
/// declared return (an array, a field, a builtin's callback), whose
/// callers read that return; retyping it would take the function out
/// of the slot. A closure that initializes a binding with no annotation
/// is called by name when every read of that binding is a call.
fn called_by_name_only(ast: &Ast, slot_values: &HashSet<ExprId>) -> HashSet<String> {
    let mut callees: HashSet<ExprId> = slot_values.clone();
    for e in &ast.exprs {
        if let Expr::Call { callee, .. } = e {
            callees.insert(*callee);
        }
    }
    // closure → the bindings it initializes
    let mut bound: HashMap<ExprId, String> = HashMap::new();
    crate::ast::walk_stmts(&ast.stmts, &mut |s| {
        if let Stmt::LetDecl {
            name,
            type_ann: None,
            init,
            ..
        } = s
            && let Expr::Closure { .. } = ast.get_expr(*init)
        {
            bound.insert(*init, name.clone());
        }
    });
    let (mut called, mut values) = (HashSet::new(), HashSet::new());
    let mut names: HashMap<&str, Vec<&str>> = HashMap::new();
    for (i, e) in ast.exprs.iter().enumerate() {
        let eid = ExprId(i as u32);
        match e {
            Expr::Ident(n) if callees.contains(&eid) => {
                called.insert(n.as_str());
            }
            Expr::Ident(n) => {
                values.insert(n.as_str());
            }
            Expr::Closure { fn_name, .. } => match bound.get(&eid) {
                Some(b) => names.entry(fn_name).or_default().push(b),
                None => {
                    values.insert(fn_name.as_str());
                }
            },
            _ => {}
        }
    }
    let mut out = HashSet::new();
    for f in called.iter().chain(names.keys()) {
        let mut all = names.get(f).cloned().unwrap_or_default();
        all.push(*f);
        if all.iter().all(|n| !values.contains(n)) && all.iter().any(|n| called.contains(n)) {
            out.insert(f.to_string());
        }
    }
    out
}
