//! Module-namespace member direct-connect — §10.4.6 groundwork.
//!
//! `import * as ns from "./lib"` materializes the namespace as a
//! synthetic `const ns = { <export>: <local>, ... }` object literal
//! (`modules::resolve_helpers::materialize_pending_namespaces`), and
//! every `ns.<export>` read then went through that object. That made
//! the literal the only representation the language had for a module
//! namespace, and §10.4.6 wants an EXOTIC one: null prototype, not
//! extensible, non-configurable entries, a `[[Set]]` that always
//! fails. Giving the literal those attributes would tax every static
//! member read with a dynamic lookup, because the fast lane reads it
//! as an ordinary struct.
//!
//! Both wants hold at once because the two uses are disjoint. A
//! member read through a namespace alias names an export the resolver
//! ALREADY injected as a top-level declaration, so it can read that
//! declaration directly and never touch the object. This pass
//! performs that retarget; what stays pointing at the object is the
//! namespace used AS A VALUE, which is exactly the surface §10.4.6
//! describes — and the surface the next knife makes exotic.
//!
//! Soundness bar (the `ns_alias` / `globalthis_member` program-wide
//! gate shape, applied to BOTH ends of the retarget):
//! - the alias must be declared exactly once program-wide and never
//!   be a write target — a shadowed alias cannot tell which scope a
//!   member read resolves to;
//! - the LOCAL must be declared exactly once program-wide too. The
//!   injected `const a = 1` is one declaration; a `function f() {
//!   const a = 2; return ns.a }` elsewhere is a second, and
//!   retargeting into that scope would read 2. Any name with a second
//!   binding of any form keeps its member shape;
//! - mutation positions keep their shape: `ns.x = v`, `delete ns.x`
//!   and `ns.x++` are the §10.4.6 `[[Set]]` / `[[Delete]]` surfaces,
//!   and retargeting them would write the LOCAL instead of failing.
//!
//! Runs right after `desugar_ns_alias_members` and before anything
//! consumes member shapes.

use std::collections::{HashMap, HashSet};

use super::ns_alias::walk_stmts;
use super::{Ast, Expr, ExprId, Stmt};

pub fn desugar_module_ns_members(ast: &mut Ast) {
    if ast.namespace_bindings.is_empty() {
        return;
    }

    // Program-wide binding census. Unlike the `ns_alias` bar this
    // counts every binding FORM into one tally instead of splitting
    // "declarations" from "other forms": a local's own injected
    // `function add() {}` is its single declaration, so a form-based
    // blocklist would reject exactly the common case.
    let mut count: HashMap<String, u32> = HashMap::new();
    walk_stmts(&ast.stmts, &mut |s| count_bindings(s, &mut count));
    let mut written: HashSet<String> = HashSet::new();
    for e in &ast.exprs {
        match e {
            Expr::ArrowFn { params, body, .. } => {
                for p in params {
                    bump(&mut count, &p.name);
                }
                walk_stmts(body, &mut |s| count_bindings(s, &mut count));
            }
            Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => {
                if let Expr::Ident(n) = &ast.exprs[target.0 as usize] {
                    written.insert(n.clone());
                }
            }
            _ => {}
        }
    }

    let unambiguous = |n: &String| count.get(n).copied() == Some(1) && !written.contains(n);

    // alias → export spelling → local binding, keeping only the pairs
    // whose BOTH ends survive the bar.
    let mut retarget: HashMap<String, HashMap<String, String>> = HashMap::new();
    for (alias, fields) in &ast.namespace_bindings {
        if !unambiguous(alias) {
            continue;
        }
        let mut m: HashMap<String, String> = HashMap::new();
        for (export, local) in fields {
            if unambiguous(local) {
                m.insert(export.clone(), local.clone());
            }
        }
        if !m.is_empty() {
            retarget.insert(alias.clone(), m);
        }
    }
    if retarget.is_empty() {
        return;
    }

    let mut excluded: HashSet<ExprId> = HashSet::new();
    for e in &ast.exprs {
        match e {
            Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => {
                excluded.insert(*target);
            }
            Expr::Delete { expr } => {
                excluded.insert(*expr);
            }
            _ => {}
        }
    }

    // Each syntactic occurrence owns its ExprId, so replacing the
    // whole Member node never touches a value use of the alias (the
    // synthetic object literal's own field Idents are not Members).
    for i in 0..ast.exprs.len() {
        if excluded.contains(&ExprId(i as u32)) {
            continue;
        }
        let Expr::Member { obj, name } = &ast.exprs[i] else {
            continue;
        };
        let (obj, name) = (*obj, name.clone());
        let Expr::Ident(recv) = &ast.exprs[obj.0 as usize] else {
            continue;
        };
        let Some(local) = retarget.get(recv).and_then(|m| m.get(&name)) else {
            continue;
        };
        ast.exprs[i] = Expr::Ident(local.clone());
    }
}

fn bump(count: &mut HashMap<String, u32>, name: &str) {
    *count.entry(name.to_string()).or_insert(0) += 1;
}

/// One stmt's contribution to the binding census — every form that
/// claims a name in some scope.
fn count_bindings(s: &Stmt, count: &mut HashMap<String, u32>) {
    match s {
        Stmt::LetDecl { name, .. } | Stmt::UsingDecl { name, .. } => bump(count, name),
        Stmt::FnDecl { name, params, .. } => {
            bump(count, name);
            for p in params {
                bump(count, &p.name);
            }
        }
        Stmt::Try { catch_param, .. } => {
            if let Some(cp) = catch_param {
                bump(count, cp);
            }
        }
        Stmt::ForOf { var_name, .. } | Stmt::ForOfSplitIter { var_name, .. } => {
            bump(count, var_name)
        }
        Stmt::ClassDecl {
            name,
            ctor,
            methods,
            static_methods,
            ..
        } => {
            bump(count, name);
            if let Some(c) = ctor {
                for p in &c.params {
                    bump(count, &p.name);
                }
            }
            for m in methods.iter().chain(static_methods.iter()) {
                for p in &m.params {
                    bump(count, &p.name);
                }
            }
        }
        _ => {}
    }
}
