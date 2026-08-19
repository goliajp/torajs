//! Program-level adapter-admission predicates — which classes'
//! factories are worth a boxed adapter at all. Split from the parent
//! when the rotation-451 Promise-heir arm pushed it past the
//! 500-line file rule (verbatim moves).

use crate::ast::Ast;

/// S-NEW 刀 2 — factory adapters exist only so
/// `__torajs_anyv_construct` can reach a class through a value. A
/// program that never constructs from a value gets none of them: one
/// extra function per class is a real cost to an artifact whose size
/// is a differentiator. r293 — `Reflect.construct` reaches the same
/// registry through its own kernel, so its call shape arms the
/// synthesis too.
pub(crate) fn program_constructs_from_value(ast: &Ast) -> bool {
    ast.exprs.iter().any(|e| match e {
        crate::ast::Expr::NewDynamic { .. } => true,
        // A `.construct(...)` call arms regardless of the receiver
        // shape: the direct `Reflect.construct` form is provable,
        // but an alias (`const R: any = Reflect; R.construct(C,
        // ...)`) reaches the same kernel through the ns-object
        // singleton's cell, and the AST cannot see through the
        // binding. The method NAME is the signal; a user object's
        // own `construct` method false-positives into one adapter
        // per class — the pay-for-use trade the species
        // `constructor`-write arm below already makes.
        crate::ast::Expr::Call { callee, .. } => matches!(
            ast.get_expr(*callee),
            crate::ast::Expr::Member { name, .. } if name == "construct"
        ),
        // A bare `Reflect.construct` member READ detaches the cell
        // (`const c = Reflect.construct; c(C, ...)`) — the call
        // through the detached value is invisible, so the read
        // itself arms.
        crate::ast::Expr::Member { obj, name } => {
            name == "construct"
                && matches!(ast.get_expr(*obj), crate::ast::Expr::Ident(ns) if ns == "Reflect")
        }
        // RFC 20260808-construct-channel B5 — a `constructor`
        // property write is the ArraySpeciesCreate consumption
        // signal (§9.4.2.3 step 5 reads it back and step 10
        // constructs through the value; an `extends Array` class
        // needs no explicit `@@species` write — the inherited
        // default getter answers the class itself). Without the
        // adapters the species construct dead-ends on a loud
        // entry-miss for a class the program clearly wired up.
        crate::ast::Expr::Assign { target, .. } => matches!(
            ast.get_expr(*target),
            crate::ast::Expr::Member { name, .. } if name == "constructor"
        ),
        _ => false,
    })
}

/// Every class whose ctor chain bottoms out at the Promise builtin —
/// the direct `extends Promise` classes plus their user-class
/// descendants (`class CP2 extends CP`). These are the classes the
/// runtime capability path (`NewPromiseCapability(C)`, rotation 451)
/// can construct through a first-class value, so their factories
/// always get a boxed adapter.
pub(super) fn promise_heir_classes(ast: &Ast) -> std::collections::HashSet<String> {
    let mut out: std::collections::HashSet<String> = ast
        .builtin_class_parents
        .iter()
        .filter(|(_, p)| p.as_str() == "Promise")
        .map(|(c, _)| c.clone())
        .collect();
    loop {
        let mut grew = false;
        for (c, p) in &ast.class_parents {
            if let Some(parent) = p
                && !out.contains(c)
                && out.contains(parent)
            {
                out.insert(c.clone());
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    out
}
