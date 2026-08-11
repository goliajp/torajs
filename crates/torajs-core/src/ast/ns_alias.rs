//! Namespace-alias member rewrite — a member access through a
//! TOP-LEVEL immutable alias of a namespace singleton (`const m =
//! Math; m.max(1, 2)`) rewrites the receiver to the namespace name,
//! so every existing syntactic dispatch lane (`Math.<method>` /
//! `JSON.<method>` / `console.<method>` / `Reflect.<method>`) matches
//! as if the source had spelled the namespace directly.
//!
//! Closes the ns-alias member-CALL boundary recorded by RFC
//! 20260812-console-sink knife 4: the alias as a VALUE already worked
//! (the interned singleton boxes — `m === Math` holds and keeps
//! holding, the LetDecl and every non-member use stay untouched), but
//! a member call through the typed alias fell through to the
//! `resolve_callee` "unsupported member call shape" panic.
//!
//! Narrow surface (B-4): exactly the four interned namespace-object
//! singletons. Constructor namespaces (`const O = Object`) are a
//! different family (first-class ctor cells with instances) and stay
//! a recorded boundary.
//!
//! Soundness bar (the `fnexpr_this` / `globalthis_member`
//! program-wide gate shape):
//! - the alias decl must be a TOP-LEVEL `const` whose init is the
//!   bare namespace Ident — module scope makes every use site legal,
//!   so the rewrite can never legalize an undeclared cross-fn read;
//! - the name must be declared exactly ONCE program-wide and never
//!   appear as any other binding form (fn name / param / catch /
//!   for-of var / class name) — a shadowed name cannot tell which
//!   scope a member read resolves to;
//! - the name must never be an assignment / post-incr target (a
//!   `const` guarantees this for legal programs; the check keeps the
//!   pass honest on not-yet-rejected ones).
//!
//! Runs right after `desugar_globalthis_members` (its rewrite can
//! mint the `const m = Math` shape from `const m = globalThis.Math`)
//! and before anything consumes member shapes.

use std::collections::{HashMap, HashSet};

use super::{Ast, Expr, Stmt};

/// The interned namespace-object singletons (RFC 20260801 Math /
/// JSON / Reflect + RFC 20260812 console).
const NS_SINGLETONS: [&str; 4] = ["Math", "JSON", "Reflect", "console"];

pub fn desugar_ns_alias_members(ast: &mut Ast) {
    // Top-level alias candidates: `const <name> = <ns>;`.
    let mut alias: HashMap<String, String> = HashMap::new();
    for s in &ast.stmts {
        let Stmt::LetDecl {
            mutable: false,
            name,
            init,
            ..
        } = s
        else {
            continue;
        };
        if let Expr::Ident(ns) = &ast.exprs[init.0 as usize]
            && NS_SINGLETONS.contains(&ns.as_str())
            && name != ns
        {
            alias.insert(name.clone(), ns.clone());
        }
    }
    if alias.is_empty() {
        return;
    }

    // Program-wide bar: drop a candidate on a second declaration of
    // its name, on any other binding form claiming it, or on a write
    // targeting it. Walks every stmt body (arrow-fn bodies included —
    // they still live in the expr arena at this point).
    let mut decl_count: HashMap<String, u32> = HashMap::new();
    let mut blocked: HashSet<String> = HashSet::new();
    walk_stmts(&ast.stmts, &mut |s| {
        collect_bindings(s, &mut decl_count, &mut blocked)
    });
    for e in &ast.exprs {
        match e {
            Expr::ArrowFn { params, body, .. } => {
                for p in params {
                    blocked.insert(p.name.clone());
                }
                walk_stmts(body, &mut |s| {
                    collect_bindings(s, &mut decl_count, &mut blocked)
                });
            }
            Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => {
                if let Expr::Ident(n) = &ast.exprs[target.0 as usize] {
                    blocked.insert(n.clone());
                }
            }
            _ => {}
        }
    }
    alias.retain(|name, _| decl_count.get(name).copied() == Some(1) && !blocked.contains(name));
    if alias.is_empty() {
        return;
    }

    // Rewrite: the OBJ ident of every member shape through an alias.
    // Each syntactic occurrence owns its ExprId, so retargeting the
    // ident never touches a value use (`m === Math` keeps reading the
    // binding).
    for i in 0..ast.exprs.len() {
        let Expr::Member { obj, .. } = &ast.exprs[i] else {
            continue;
        };
        let obj = *obj;
        let Expr::Ident(n) = &ast.exprs[obj.0 as usize] else {
            continue;
        };
        if let Some(ns) = alias.get(n) {
            ast.exprs[obj.0 as usize] = Expr::Ident(ns.clone());
        }
    }
}

/// One stmt's binding contributions to the program-wide bar.
fn collect_bindings(
    s: &Stmt,
    decl_count: &mut HashMap<String, u32>,
    blocked: &mut HashSet<String>,
) {
    match s {
        Stmt::LetDecl { name, .. } => {
            *decl_count.entry(name.clone()).or_insert(0) += 1;
        }
        Stmt::FnDecl { name, params, .. } => {
            blocked.insert(name.clone());
            for p in params {
                blocked.insert(p.name.clone());
            }
        }
        Stmt::Try { catch_param, .. } => {
            if let Some(cp) = catch_param {
                blocked.insert(cp.clone());
            }
        }
        Stmt::ForOf { var_name, .. } | Stmt::ForOfSplitIter { var_name, .. } => {
            blocked.insert(var_name.clone());
        }
        Stmt::ClassDecl {
            name,
            ctor,
            methods,
            static_methods,
            ..
        } => {
            blocked.insert(name.clone());
            if let Some(c) = ctor {
                for p in &c.params {
                    blocked.insert(p.name.clone());
                }
            }
            for m in methods.iter().chain(static_methods.iter()) {
                for p in &m.params {
                    blocked.insert(p.name.clone());
                }
            }
        }
        _ => {}
    }
}

/// Depth-first stmt walk over every body shape (the
/// `globalthis_member` gate's coverage, callback form).
fn walk_stmts(stmts: &[Stmt], f: &mut impl FnMut(&Stmt)) {
    for s in stmts {
        f(s);
        match s {
            Stmt::FnDecl { body, .. } => walk_stmts(body, f),
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                walk_stmts(body, f);
                walk_stmts(catch_body, f);
                if let Some(fin) = finally_body {
                    walk_stmts(fin, f);
                }
            }
            Stmt::ForOf { body, .. }
            | Stmt::ForOfSplitIter { body, .. }
            | Stmt::While { body, .. }
            | Stmt::DoWhile { body, .. }
            | Stmt::Labeled { body, .. } => walk_stmts(std::slice::from_ref(body), f),
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    walk_stmts(std::slice::from_ref(i), f);
                }
                walk_stmts(std::slice::from_ref(body), f);
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                walk_stmts(std::slice::from_ref(then_branch), f);
                if let Some(e) = else_branch {
                    walk_stmts(std::slice::from_ref(e), f);
                }
            }
            Stmt::Switch { cases, default, .. } => {
                for c in cases {
                    walk_stmts(&c.body, f);
                }
                if let Some(d) = default {
                    walk_stmts(d, f);
                }
            }
            Stmt::ClassDecl {
                ctor,
                methods,
                static_methods,
                static_init,
                ..
            } => {
                if let Some(c) = ctor {
                    walk_stmts(&c.body, f);
                }
                for m in methods.iter().chain(static_methods.iter()) {
                    walk_stmts(&m.body, f);
                }
                for si in static_init {
                    if let super::StaticInit::Block(b) = si {
                        walk_stmts(b, f);
                    }
                }
            }
            Stmt::Block(inner) | Stmt::Multi(inner) => walk_stmts(inner, f),
            _ => {}
        }
    }
}
