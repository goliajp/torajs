//! A class method read as a VALUE at a top-level container literal —
//! the predicate the K.3 globals pre-pass spells such an element or
//! field `any` by.

use crate::ast::{Ast, Expr, ExprId, Stmt};

use super::init_ann::{new_class_ann, unique_toplevel_binding};

/// `recv.m` reads a class METHOD as a value: `recv` names the single
/// top-level `let` / `const` holding a `new C()` (the `__new_C`
/// factory spelling) and `m` is a method of `C` or of an ancestor —
/// the class desugar has minted its `__cm_C__m` mono body (or the
/// generator spelling `__cm_gen_C__m`) as a top-level FnDecl by the
/// time the globals pre-pass runs. The value is the prototype's
/// reified cell, an Any whatever the checker calls the read (the
/// lowering mirror is `ssa_lower_member::is_class_method_value_read`),
/// so a container literal holding one spells that slot `any`. A
/// fn-typed spelling laid the field out as a code pointer, and a
/// named fn's call through it reached the cell's receiver-less
/// entry: `const o = { f: d.n }; function g() { return o.f(4) }`
/// threw "class method called without a receiver" where bun runs
/// the method with `o` as `this`; `const m = [d.n]` refused every
/// spelling and the named fn's read died at "unknown identifier".
pub fn class_method_value_read(ast: &Ast, e: ExprId) -> bool {
    let Expr::Member { obj, name } = ast.get_expr(e) else {
        return false;
    };
    let Expr::Ident(recv) = ast.get_expr(*obj) else {
        return false;
    };
    let Some((init, _)) = unique_toplevel_binding(ast, recv) else {
        return false;
    };
    let Some(mut cname) = new_class_ann(ast, init) else {
        return false;
    };
    let stmts = crate::ast::toplevel_stmts_flat(ast);
    loop {
        let mono = format!("__cm_{cname}__{name}");
        let generator = format!("__cm_gen_{cname}__{name}");
        if stmts
            .iter()
            .any(|s| matches!(s, Stmt::FnDecl { name: n, .. } if *n == mono || *n == generator))
        {
            return true;
        }
        match ast.class_parents.get(&cname) {
            Some(Some(p)) => cname = p.clone(),
            _ => return false,
        }
    }
}
