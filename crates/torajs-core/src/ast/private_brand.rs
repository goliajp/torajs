//! RFC 20260916-private-element-brand slice 2a — the private METHOD /
//! ACCESSOR brand (§7.3.33 PrivateMethodOrAccessorAdd, §7.3.34
//! InitializeInstanceElements).
//!
//! A class's private methods and accessors reify on its prototype, so
//! "the prototype chain has `#m`" used to be the brand — true for an
//! instance whose `super(…)` has not returned yet, and for
//! `Object.create(C.prototype)`. The spec installs the brand at a
//! point in time: a base class right after allocation, a derived
//! class right after `super(…)` returns. So each `Tag::Obj` instance
//! carries an install cursor in the high half of its +8 slot, counting
//! the brands installed so far along its class chain in root-to-leaf
//! order; a class owning private methods holds one position in that
//! order, and its brand is present exactly when the object is an
//! instance of the class and the cursor is past that position.
//!
//! Chains that touch a value-returning constructor
//! (`Ast::ctor_return_override`) or an exotic builtin parent keep the
//! previous behaviour for now: their `this` may be an adopted object
//! with no cursor of this class, which needs the foreign-object store
//! of a later slice.

use super::desugar_classes_super::ClassIndexEntry;
use super::{Ast, Expr, ExprId, Stmt};
use std::collections::HashMap;

/// Runtime kernel names the class desugar mints.
pub(crate) const INSTALL: &str = "__torajs_priv_install";
pub(crate) const CHECK: &str = "__torajs_priv_brand";

/// Plan the brand positions, then guard every access site. Runs once
/// the class parents are known and before any class rewrite: the
/// `super(…)` install consults the plan, and the access guard only
/// touches member receivers no class rewrite has renamed yet.
pub(super) fn run(ast: &mut Ast, class_index: &[ClassIndexEntry]) {
    plan(ast, class_index);
    guard_access_sites(ast, class_index);
}

/// Record, for every class owning private methods or accessors on a
/// chain this slice covers, the position of its brand in the chain's
/// install order (`Ast::private_brand_pos`).
fn plan(ast: &mut Ast, class_index: &[ClassIndexEntry]) {
    let parent: HashMap<&str, Option<&str>> = class_index
        .iter()
        .map(|(_, c, _, p, ..)| (c.as_str(), p.as_deref()))
        .collect();
    let owns = |c: &str| {
        class_index.iter().any(|(_, n, _, _, _, _, _, methods, _)| {
            n == c && methods.iter().any(|m| m.name.starts_with("__priv_"))
        })
    };
    let mut out = HashMap::new();
    for (_, cname, ..) in class_index {
        if !owns(cname) {
            continue;
        }
        let mut pos = 0u32;
        let mut covered = true;
        let mut cur = Some(cname.as_str());
        while let Some(c) = cur {
            if ast.ctor_return_override.contains(c)
                || super::desugar_classes_builtin_heritage::exotic_root_parent(ast, c).is_some()
            {
                covered = false;
                break;
            }
            cur = parent.get(c).copied().flatten();
            if let Some(p) = cur
                && owns(p)
            {
                pos += 1;
            }
        }
        if covered {
            out.insert(cname.clone(), pos);
        }
    }
    ast.private_brand_pos = out;
}

/// `__torajs_priv_install(this, pos, pos + 1)` for `cname`, or `None`
/// when the class holds no brand position.
pub(crate) fn install_call(ast: &mut Ast, cname: &str, recv: Expr) -> Option<ExprId> {
    let pos = *ast.private_brand_pos.get(cname)?;
    let callee = ast.add_expr(Expr::Ident(INSTALL.into()));
    let recv = ast.add_expr(recv);
    let from = ast.add_expr(Expr::Number(f64::from(pos)));
    let to = ast.add_expr(Expr::Number(f64::from(pos + 1)));
    Some(ast.add_expr(Expr::Call {
        callee,
        args: vec![recv, from, to],
    }))
}

/// Route the receiver of every access to a private method or accessor
/// of a covered class — the read, the write, the call — through
/// `__torajs_priv_brand(obj, "C", pos)`, which checks the brand
/// (§7.3.31 / §7.3.32 step 1) and answers the receiver unchanged. The
/// receiver is still evaluated once and the node keeps its shape, so
/// every downstream pass (the class Pass 2 call rewrite, `.call`
/// devirtualisation, promise chains, the static-`this` table keyed on
/// the receiver's ExprId) sees what it saw before; the checker types
/// the call as its first operand.
fn guard_access_sites(ast: &mut Ast, class_index: &[ClassIndexEntry]) {
    let mut owner: HashMap<String, String> = HashMap::new();
    for (_, cname, _, _, _, _, _, methods, _) in class_index {
        if !ast.private_brand_pos.contains_key(cname) {
            continue;
        }
        for m in methods {
            if let Some(n) = m.name.as_str()
                && n.starts_with("__priv_")
            {
                owner.insert(n.to_string(), cname.clone());
            }
        }
    }
    if owner.is_empty() {
        return;
    }
    let sites: Vec<(usize, ExprId, String)> = ast
        .exprs
        .iter()
        .enumerate()
        .filter_map(|(i, e)| match e {
            Expr::Member { obj, name } => owner.get(name).map(|c| (i, *obj, c.clone())),
            _ => None,
        })
        .collect();
    for (i, obj, cname) in sites {
        let pos = ast.private_brand_pos[&cname];
        let callee = ast.add_expr(Expr::Ident(CHECK.into()));
        let class = ast.add_expr(Expr::String(cname.as_str().into()));
        let pos = ast.add_expr(Expr::Number(f64::from(pos)));
        let checked = ast.add_expr(Expr::Call {
            callee,
            args: vec![obj, class, pos],
        });
        if let Expr::Member { obj, .. } = &mut ast.exprs[i] {
            *obj = checked;
        }
    }
}

/// The install statements for a factory body, placed right after the
/// mint.
///
/// §10.2.2 step 6.b installs a base class's brand as its [[Construct]]
/// begins, and for an instance of a derived class that is the moment
/// the innermost `super(…)` reaches the root; no code between the mint
/// and that moment can observe `this` (a derived constructor cannot
/// touch it before `super`), so the root's install goes at the mint for
/// every class of the chain — including the root's own factory, which
/// no `super` reaches. Every other class installs at its `super(…)`.
///
/// A factory that calls no constructor at all belongs to a chain with
/// none, so no `super` site exists and no user code runs between the
/// installs: the whole chain installs here, root first.
pub(crate) fn factory_install_stmts(ast: &mut Ast, cname: &str, calls_ctor: bool) -> Vec<Stmt> {
    let mut chain = vec![cname.to_string()];
    while let Some(Some(p)) = ast.class_parents.get(chain.last().unwrap()) {
        chain.push(p.clone());
    }
    chain.reverse();
    if calls_ctor {
        chain.truncate(1);
    }
    chain
        .iter()
        .filter_map(|c| install_call(ast, c, Expr::Ident("__this".into())))
        .map(Stmt::Expr)
        .collect()
}
