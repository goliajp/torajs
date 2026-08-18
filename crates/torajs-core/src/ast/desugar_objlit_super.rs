//! SuperProperty in OBJECT-LITERAL methods — §10.2.4 gives a method
//! shorthand a [[HomeObject]] (the literal itself), so `super.x`
//! inside it reads off GetPrototypeOf(home) (§13.3.7 GetSuperBase,
//! re-evaluated per access — `Object.setPrototypeOf(obj, …)` after
//! the definition changes what `super` sees, and the t262 family
//! asserts exactly that).
//!
//! The parser already encodes these sites off the `__superbase__` /
//! `__supercall__<m>` markers (primary_new_super; SuperProperty is
//! legal in a method body per §15.4.1). The class desugar consumes
//! the markers inside class bodies; until this pass, an object
//! literal's markers survived to the checker as unknown idents. This
//! pass runs right after `desugar_classes`, claims the shape it can
//! prove, and leaves everything else loud:
//!
//!   admitted — READS in a declaration whose init IS the literal
//!   (`let/var/const obj = { m() { … super.x … } }`). The home binding
//!   pre-declares mutable (the declared name may be reassigned later;
//!   the HomeObject never moves), and reads rewrite the marker in
//!   place to `Object.getPrototypeOf(__home_N)` — minted fresh per
//!   site, since GetSuperBase re-reads the prototype on every access.
//!
//!   left loud (marker → unknown ident, same posture as the class
//!   pass): `super.m(args)` calls — §13.3.6 wants the CURRENT `this`
//!   as receiver, and a `this` node minted at this stage reaches the
//!   checker unclaimed, while a home-object receiver would be
//!   silently wrong for a detached method; writes (`super.x = v`
//!   needs §9.1.9 receiver-is-this PutValue); `super.x++`;
//!   `super[k](...)`; a literal in any position other than a
//!   declaration init; and a method whose body nests ANOTHER
//!   literal-with-method — the inner method's markers belong to the
//!   inner home, and rewriting them against the outer one would be
//!   the silent wrong this table exists to prevent.
//!
//!   shared boundary with the class pass (recorded silent): a GETTER
//!   on the prototype reads with the prototype as receiver, not
//!   `this` (§13.3.7 wants `this`); a plain data read is
//!   receiver-indifferent, which is what the admitted set is.

use super::super_collect_prop::{SuperPropSite, SuperPropSites, collect_superprop_in_stmt};
use super::{Ast, Expr, ExprId, Stmt};

pub fn desugar_objlit_super(ast: &mut Ast) {
    let mut counter: u32 = 0;
    let mut stmts = std::mem::take(&mut ast.stmts);
    process_list(ast, &mut stmts, &mut counter);
    ast.stmts = stmts;
}

fn process_list(ast: &mut Ast, stmts: &mut [Stmt], counter: &mut u32) {
    for s in stmts.iter_mut() {
        process_stmt(ast, s, counter);
    }
}

fn process_stmt(ast: &mut Ast, s: &mut Stmt, counter: &mut u32) {
    if let Stmt::LetDecl { init, name, .. } = s
        && matches!(ast.get_expr(*init), Expr::ObjectLit { .. })
        && let Some(home) = claim_literal(ast, *init, counter)
    {
        // The methods capture `__home_N` BEFORE the literal finishes
        // evaluating (the closure mints inside the init), so the home
        // binding pre-declares mutable-undefined and is assigned back
        // right after the declaration — the capture is a box, so the
        // method bodies see the assignment (measured: the plain
        // `let y; const o = { m() { return y; } }; y = o;` spelling
        // answers `o`).
        let declared = name.clone();
        let undef = ast.add_expr(Expr::Ident("undefined".to_string()));
        let home_let = Stmt::LetDecl {
            mutable: true,
            name: home.clone(),
            type_ann: Some("any".to_string()),
            init: undef,
            is_var: false,
        };
        let home_ref = ast.add_expr(Expr::Ident(home));
        let name_ref = ast.add_expr(Expr::Ident(declared));
        let assign = ast.add_expr(Expr::Assign {
            target: home_ref,
            value: name_ref,
        });
        let decl = std::mem::replace(s, Stmt::Multi(Vec::new()));
        *s = Stmt::Multi(vec![home_let, decl, Stmt::Expr(assign)]);
        return;
    }
    match s {
        Stmt::Block(list) | Stmt::Multi(list) => process_list(ast, list, counter),
        Stmt::FnDecl { body, .. } => process_list(ast, body, counter),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            process_stmt(ast, then_branch, counter);
            if let Some(eb) = else_branch {
                process_stmt(ast, eb, counter);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
            process_stmt(ast, body, counter)
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                process_stmt(ast, i, counter);
            }
            process_stmt(ast, body, counter);
        }
        Stmt::ForOf { body, .. } | Stmt::ForOfSplitIter { body, .. } => {
            process_stmt(ast, body, counter)
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases.iter_mut() {
                process_list(ast, &mut c.body, counter);
            }
            if let Some(db) = default {
                process_list(ast, db, counter);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            process_list(ast, body, counter);
            process_list(ast, catch_body, counter);
            if let Some(fb) = finally_body {
                process_list(ast, fb, counter);
            }
        }
        Stmt::ExportDecl { inner, .. } => {
            if let Some(inner) = inner {
                process_stmt(ast, inner, counter);
            }
        }
        _ => {}
    }
}

/// Rewrite the literal's claimable super sites against a fresh home
/// binding. `Some(name)` = at least one site was rewritten and the
/// caller must hoist the literal into that binding; `None` = nothing
/// claimed (no marker, or the nested-literal bail), the declaration
/// stays as written.
fn claim_literal(ast: &mut Ast, objlit: ExprId, counter: &mut u32) -> Option<String> {
    let Expr::ObjectLit { fields } = ast.get_expr(objlit) else {
        return None;
    };
    let method_values: Vec<ExprId> = fields
        .iter()
        .map(|(_, v)| *v)
        .filter(|v| ast.objlit_method_exprs.contains(v))
        .collect();
    if method_values.is_empty() {
        return None;
    }
    let mut prop = SuperPropSites::default();
    let mut supercalls: Vec<ExprId> = Vec::new();
    let mut nested_home = false;
    for &mv in &method_values {
        let Expr::ArrowFn { body, .. } = ast.get_expr(mv) else {
            continue;
        };
        for st in body {
            collect_superprop_in_stmt(ast, st, &mut prop);
            scan_extra(ast, st, &mut supercalls, &mut nested_home);
        }
    }
    // A nested literal-with-method inside a method body: its markers
    // belong to ITS home. The read-only collector cannot tell them
    // apart, so the whole outer claim bails (loud beats wrong-home).
    if nested_home {
        return None;
    }
    // `super.m(args)` stays loud in this knife: §13.3.6 wants the
    // CURRENT `this` as receiver, and a `this` node minted at this
    // stage reaches the checker unclaimed (the objlit-method `this`
    // machinery types the nodes the PARSER wrote, not ones a desugar
    // adds). A home-object receiver would be silently wrong the
    // moment the method is extracted and called detached.
    let _ = &supercalls;
    let claimable = prop.sites.iter().any(|s| {
        matches!(
            s,
            SuperPropSite::Read { .. } | SuperPropSite::IndexRead { .. }
        )
    });
    if !claimable {
        return None;
    }
    let home = format!("__home_{}", *counter);
    *counter += 1;
    for site in &prop.sites {
        match site {
            // The marker Ident rewrites IN PLACE to the base
            // expression; the surrounding Member / Index keeps its
            // node, so every reference to the site stays valid.
            SuperPropSite::Read { member, .. } => {
                let Expr::Member { obj, .. } = ast.get_expr(*member) else {
                    continue;
                };
                let marker = *obj;
                let base = super_base_expr(ast, &home);
                ast.exprs[marker.0 as usize] = base;
            }
            SuperPropSite::IndexRead { index_expr } => {
                let Expr::Index { obj, .. } = ast.get_expr(*index_expr) else {
                    continue;
                };
                let marker = *obj;
                let base = super_base_expr(ast, &home);
                ast.exprs[marker.0 as usize] = base;
            }
            // Writes keep their marker — §9.1.9 wants receiver-is-this
            // PutValue semantics this rewrite cannot spell — and fail
            // loud at the checker like they did before this pass.
            SuperPropSite::AssignName { .. } | SuperPropSite::AssignIndex { .. } => {}
        }
    }
    Some(home)
}

/// `Object.getPrototypeOf(__home_N)` — minted fresh per site so each
/// access re-reads the prototype (§13.3.7 GetSuperBase is not cached).
fn super_base_expr(ast: &mut Ast, home: &str) -> Expr {
    let obj = ast.add_expr(Expr::Ident("Object".to_string()));
    let gpo = ast.add_expr(Expr::Member {
        obj,
        name: "getPrototypeOf".to_string(),
    });
    let home_ref = ast.add_expr(Expr::Ident(home.to_string()));
    Expr::Call {
        callee: gpo,
        args: vec![home_ref],
    }
}

/// The two shapes `collect_superprop_in_stmt` does not surface: the
/// `__supercall__<m>` call marker (claimed here) and a nested
/// literal-with-method (the bail signal). Same walk skeleton as the
/// unclaimed-`this` gate: an expression arm the child list lacks
/// costs an under-claim, never a wrong rewrite.
fn scan_extra(ast: &Ast, s: &Stmt, supercalls: &mut Vec<ExprId>, nested_home: &mut bool) {
    for root in super::desugar_with::walk::stmt_exprs(s) {
        scan_extra_expr(ast, root, supercalls, nested_home);
    }
    for child in super::desugar_with::walk::stmt_children_ref(s) {
        scan_extra(ast, child, supercalls, nested_home);
    }
}

fn scan_extra_expr(ast: &Ast, eid: ExprId, supercalls: &mut Vec<ExprId>, nested_home: &mut bool) {
    match ast.get_expr(eid) {
        Expr::Call { callee, .. }
            if matches!(
                ast.get_expr(*callee),
                Expr::Ident(n) if n.starts_with("__supercall__")
            ) =>
        {
            supercalls.push(eid);
        }
        Expr::ObjectLit { fields }
            if fields
                .iter()
                .any(|(_, v)| ast.objlit_method_exprs.contains(v)) =>
        {
            *nested_home = true;
        }
        // An arrow inherits the enclosing method's home (§8.3.4) —
        // `collect_superprop_in_stmt` descends into arrow bodies, so
        // this walk must see the same sites. `expr_children`
        // deliberately stops at arrow bodies; descend by hand.
        Expr::ArrowFn { body, .. } => {
            for s in body {
                scan_extra(ast, s, supercalls, nested_home);
            }
            return;
        }
        _ => {}
    }
    for c in super::desugar_with::walk::expr_children(ast, eid) {
        scan_extra_expr(ast, c, supercalls, nested_home);
    }
}
