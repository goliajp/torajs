//! §10.4.6.8 → §9.1.1.1.6 — a module namespace field over a `let` /
//! `const` the entry declares AFTER the namespace object exists
//! (`import * as ns from "./self"` at the top, `export let x` further
//! down) reads the binding through the object before its declaration
//! has run: a ReferenceError, the binding's temporal dead zone. The
//! object's field is a cell over the binding's slot
//! (`try_emit_ns_binding_field`), and a slot cannot tell that dead
//! zone from a binding initialized to `undefined` or `0`, so each such
//! binding gets a runtime initialization flag: `let __nsinit_<x>:
//! boolean = false` declared in front of the first namespace literal
//! naming it, `__nsinit_<x> = true` set right after the declaration
//! (both inside a transparent `Multi`, so no index of the list moves),
//! and the flag's slot handed to the cell when the field is defined.
//! The static `ns.x` read is already the binding's own read
//! (`read_through_namespace`) and takes the closure dead-zone flag;
//! this flag serves the reflective faces — a dynamic `ns[k]`, `Object.
//! keys`, a descriptor, for-in — which reach the binding only through
//! the cell.
//!
//! Runs after `flag_closure_tdz`, over the same flat view of the
//! top-level list (`Multi` is transparent), and only for an entry that
//! imports its own namespace: any other namespace object is built
//! after the module it names has evaluated.

use std::collections::HashMap;

use super::closure_tdz_flag::{at, at_mut, flat_paths, wrap};
use super::{Ast, Expr, Stmt};

/// The flag's spelling: this prefix plus the binding's own name. The
/// lowering finds a field's flag by this spelling among the promoted
/// globals, so no table has to carry the pairing.
pub const NS_INIT_FLAG_PREFIX: &str = "__nsinit_";

pub fn flag_ns_tdz(ast: &mut Ast) {
    if ast.namespace_bindings.is_empty() {
        return;
    }
    let mut paths: Vec<Vec<usize>> = Vec::new();
    flat_paths(&ast.stmts, &mut Vec::new(), &mut paths);
    // `let` / `const` only: a `var` is initialized when its scope is
    // entered and has no dead zone
    let mut decls: HashMap<String, usize> = HashMap::new();
    for (j, p) in paths.iter().enumerate() {
        if let Stmt::LetDecl {
            name,
            is_var: false,
            ..
        } = at(&ast.stmts, p)
        {
            decls.insert(name.clone(), j);
        }
    }
    // (literal index, binding) for every field a namespace literal
    // names ahead of the binding's declaration
    let mut wanted: Vec<(usize, String)> = Vec::new();
    for (i, p) in paths.iter().enumerate() {
        let Stmt::LetDecl { name, init, .. } = at(&ast.stmts, p) else {
            continue;
        };
        if !ast.namespace_bindings.contains_key(name) {
            continue;
        }
        let Expr::ObjectLit { fields } = ast.get_expr(*init) else {
            continue;
        };
        for (_, v) in fields {
            if let Expr::Ident(n) = ast.get_expr(*v)
                && ast.ns_binding_fields.contains(v)
                && decls.get(n).is_some_and(|&j| j > i)
                && !wanted.iter().any(|(_, w)| w == n)
            {
                wanted.push((i, n.clone()));
            }
        }
    }
    for (_, local) in &wanted {
        let flag = format!("{NS_INIT_FLAG_PREFIX}{local}");
        let target = ast.add_expr(Expr::Ident(flag));
        let value = ast.add_expr(Expr::Bool(true));
        let set = Stmt::Expr(ast.add_expr(Expr::Assign { target, value }));
        wrap(at_mut(&mut ast.stmts, &paths[decls[local]]), |s| {
            vec![s, set]
        });
    }
    for (i, local) in wanted {
        let init = ast.add_expr(Expr::Bool(false));
        let decl = Stmt::LetDecl {
            mutable: true,
            name: format!("{NS_INIT_FLAG_PREFIX}{local}"),
            type_ann: Some("boolean".into()),
            init,
            is_var: false,
        };
        wrap(at_mut(&mut ast.stmts, &paths[i]), |s| vec![decl, s]);
    }
}
