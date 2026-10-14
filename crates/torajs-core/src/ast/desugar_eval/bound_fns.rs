//! §19.2.1.3 EvalDeclarationInstantiation for a function a global
//! eval's text declares at its top level: its global binding is
//! created when the eval runs — a configurable property of the global
//! object, over the function's hidden slot (`script_global_fns`) —
//! and not before. Until then the name is unbound: a read or a call is
//! a ReferenceError and `typeof` answers `"undefined"`; a property the
//! program put there first is what the name reads; and after the text
//! has run the binding is still that property, which the program may
//! overwrite.
//!
//! So every bare occurrence of the name that resolves to the global
//! binding names the hidden slot instead: the reads sync it from the
//! property (`ssa_lower_ident_site_bound`), a call calls the value it
//! holds, and the declaration itself keeps its name, which only the
//! function's own relay (the slot's value) calls. Occurrences a local
//! declaration of the name shadows stay as written, and so does
//! `new f()`, whose callee is a class name to the passes that follow.
//!
//! Only a name whose slot is minted gets here
//! (`var_hoist_eval_sites::eval_bound_fn_slot`): a sloppy script, a
//! function only one text declares, spliced into the program's top
//! level, with no block function or `var` of its name, not a class's
//! parent, and no `with` anywhere, whose object a name may resolve
//! on instead.

use std::collections::HashSet;

use super::super::desugar_with::walk::stmt_children_ref;
use super::super::globalthis_member_scope::take_census;
use super::super::var_hoist_eval_sites::eval_bound_fn_slot;
use super::super::{Ast, Expr, ExprId, Stmt};

pub(super) fn route_global_reads(ast: &mut Ast) {
    if ast.eval_fn_names.is_empty() || !ast.sloppy_script_goal || ast.has_with_stmt {
        return;
    }
    let top: HashSet<&str> = ast
        .stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    let mut vars = std::collections::HashMap::new();
    super::super::var_hoist::count_var_names(&ast.stmts, &mut vars);
    let mut nested = HashSet::new();
    for s in &ast.stmts {
        if !matches!(s, Stmt::FnDecl { .. }) {
            block_fns(s, &mut nested);
        }
    }
    let names: HashSet<String> = ast
        .eval_fn_names
        .keys()
        .filter(|n| {
            top.contains(n.as_str())
                && !nested.contains(n.as_str())
                && !vars.contains_key(n.as_str())
                && eval_bound_fn_slot(ast, n).is_some()
                && !ast
                    .class_parents
                    .iter()
                    .any(|(c, p)| c == *n || p.as_deref() == Some(n.as_str()))
        })
        .cloned()
        .collect();
    if names.is_empty() {
        return;
    }
    // The declarations are the global object's own properties, so a
    // name that reaches one of them reaches the global binding.
    let census = take_census(ast, &names, &names);
    let shadowed: HashSet<ExprId> = census.user_refs.values().flatten().copied().collect();
    let mut sites: Vec<ExprId> = census.reached_refs.difference(&shadowed).copied().collect();
    sites.sort_unstable_by_key(|e| e.0);
    for eid in sites {
        if let Expr::Ident(n) = &ast.exprs[eid.0 as usize] {
            let slot = eval_bound_fn_slot(ast, n).expect("a routed name has a slot");
            ast.exprs[eid.0 as usize] = Expr::Ident(slot);
        }
    }
}

/// The functions declared in the program's top-level blocks. Annex B
/// writes such a function into the global binding of its name, which
/// takes the top-level declaration of that name off the
/// direct-function lane, and its slot with it.
fn block_fns<'s>(s: &'s Stmt, out: &mut HashSet<&'s str>) {
    for c in stmt_children_ref(s) {
        if let Stmt::FnDecl { name, .. } = c {
            out.insert(name.as_str());
        } else {
            block_fns(c, out);
        }
    }
}
