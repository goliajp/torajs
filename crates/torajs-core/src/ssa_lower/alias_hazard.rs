//! Which borrow-shaped `let` inits must take a stake after all.
//!
//! A binding whose init reads a slot someone else owns (`const old =
//! p.k`, `const e = xs[i]`, `const b = outer`) binds as a borrow: no
//! inc going in, no drop at scope close
//! ([`crate::ssa_lower_stmt_let_decl_share::init_is_alias`]). That is
//! only sound while the owner keeps its stake. Overwriting the slot
//! (`p.k = new N(n)`), shrinking the container (`xs.pop()`), rebinding
//! the root (`b = new Box()`), or calling code that may do any of these
//! releases the value the binding still names, and every later read of
//! it — a `return old`, a push, an argument, a field store — touches a
//! freed cell.
//!
//! This pass walks each body once, before it lowers, and records the
//! inits of borrow-shaped lets whose binding is READ after something
//! that may release a slot. Those bind owned (`+1` now, their own drop
//! at scope close), the same shape a body-reassigned alias takes; every
//! other alias keeps the free borrow. The walk follows evaluation order:
//! an assignment's value runs before the store; a value handed to a
//! consumer (an argument, an element, a receiver) is taken only after
//! every later operand and the call itself have run; a branch that
//! returns or throws does not reach the code after the `if`, though it
//! does reach the `catch` / `finally` around it; and a loop is walked
//! twice so a release late in the body reaches the reads at its top.

mod decls;
mod expr;
mod flow;
mod mentions;
mod release;
mod scan;

use std::collections::HashSet;

use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::check::Type as CT;
use crate::ssa::Type;
use crate::ssa_lower::LowerCtx;
use scan::Scan;

type ExprTypes = std::collections::HashMap<ExprId, CT>;

/// Record, for the body about to lower, the inits of borrow-shaped lets
/// (this frame only — nested function bodies are primed when they
/// lower) whose binding is read after a possible release of its source.
/// Runs once the parameters are bound, so a method's receiver layout is
/// known.
pub(crate) fn prime<'s>(ctx: &mut LowerCtx<'_>, body: impl Iterator<Item = &'s Stmt>) {
    let list: Vec<&Stmt> = body.collect();
    let mut out = HashSet::new();
    with_env(ctx, &list, |env| visit_list(env, &list, &mut out));
    ctx.proofs.released_alias_inits = out;
}

/// The same question for a for-of variable bound as a borrow of the
/// element: one iteration's body, which is the binding's whole scope
/// (the next iteration binds the next element).
pub(crate) fn loop_var_read_after_release(ctx: &LowerCtx<'_>, body: &Stmt, name: &str) -> bool {
    with_env(ctx, &[body], |env| {
        let mut scan = Scan::new(env, name, None);
        scan.stmt(body);
        scan.verdict()
    })
}

fn with_env<R>(ctx: &LowerCtx<'_>, frame: &[&Stmt], f: impl FnOnce(&Env) -> R) -> R {
    let field_slot = |obj: ExprId, field: &str| field_slot_ty(ctx, obj, field);
    let env = Env {
        ast: ctx.ast,
        types: ctx.expr_types,
        leaf_names: decls::leaf_names(ctx, frame),
        field_slot: &field_slot,
        shadow: &ctx.scans.proto_shadow,
    };
    f(&env)
}

/// The slot type of `obj.field` when the receiver's class layout is
/// known: the method's own receiver, or a value the checker typed as a
/// non-generic class instance.
fn field_slot_ty(ctx: &LowerCtx<'_>, obj: ExprId, field: &str) -> Option<Type> {
    let recv_ty = match ctx.ast.get_expr(obj) {
        Expr::This => ctx.locals.get("__this")?.ty,
        Expr::Ident(n) if n == "__this" => ctx.locals.get(n)?.ty,
        _ => match ctx.expr_types.get(&obj)? {
            CT::ClassRef(n) => *ctx.aliases.get(n)?,
            _ => return None,
        },
    };
    let Type::Obj(sid) = recv_ty else {
        return None;
    };
    let key = torajs_wtf8::Wtf8::new(field);
    ctx.struct_layouts[sid.0 as usize]
        .iter()
        .find(|(p, _)| p.as_wtf8() == key)
        .map(|(_, t)| *t)
}

struct Env<'a> {
    ast: &'a Ast,
    types: &'a ExprTypes,
    leaf_names: HashSet<String>,
    field_slot: &'a dyn Fn(ExprId, &str) -> Option<Type>,
    /// The built-in methods, statics and symbol hooks the program
    /// writes: a call or a spread the typed tier routes through a
    /// written one may run user code.
    shadow: &'a crate::builtin_proto_shadow::ShadowSet,
}

/// `Multi` shares its parent's scope, so its lets live to the end of
/// the enclosing list: flatten it in place.
fn flatten<'s>(list: &[&'s Stmt], out: &mut Vec<&'s Stmt>) {
    for s in list {
        match s {
            Stmt::Multi(inner) => {
                let refs: Vec<&Stmt> = inner.iter().collect();
                flatten(&refs, out);
            }
            _ => out.push(s),
        }
    }
}

fn visit_list(env: &Env, list: &[&Stmt], out: &mut HashSet<ExprId>) {
    let mut flat = Vec::new();
    flatten(list, &mut flat);
    for (k, s) in flat.iter().enumerate() {
        if let Stmt::LetDecl { name, init, .. } = s
            && let Some(root) = alias_root(env.ast, *init)
        {
            let mut scan = Scan::new(env, name, root);
            scan.seq(&flat[k + 1..]);
            if scan.verdict() {
                out.insert(*init);
            }
        }
        visit_nested(env, s, out);
    }
}

fn visit_nested(env: &Env, s: &Stmt, out: &mut HashSet<ExprId>) {
    let one = |s: &Stmt, out: &mut HashSet<ExprId>| visit_list(env, &[s], out);
    match s {
        Stmt::Block(b) => visit_list(env, &b.iter().collect::<Vec<_>>(), out),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            one(then_branch, out);
            if let Some(e) = else_branch {
                one(e, out);
            }
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::Labeled { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::ForOfSplitIter { body, .. } => one(body, out),
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            // the head's binding lives through cond, body and step
            if let Some(i) = init.as_deref() {
                if let Stmt::LetDecl { name, init, .. } = i
                    && let Some(root) = alias_root(env.ast, *init)
                {
                    let mut scan = Scan::new(env, name, root);
                    scan.looped(|sc| {
                        if let Some(c) = cond {
                            sc.expr(*c);
                        }
                        sc.stmt(body);
                        if let Some(st) = step {
                            sc.expr(*st);
                        }
                    });
                    if scan.verdict() {
                        out.insert(*init);
                    }
                }
                visit_nested(env, i, out);
            }
            one(body, out);
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            visit_list(env, &body.iter().collect::<Vec<_>>(), out);
            visit_list(env, &catch_body.iter().collect::<Vec<_>>(), out);
            if let Some(f) = finally_body {
                visit_list(env, &f.iter().collect::<Vec<_>>(), out);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                visit_list(env, &c.body.iter().collect::<Vec<_>>(), out);
            }
            if let Some(d) = default {
                visit_list(env, &d.iter().collect::<Vec<_>>(), out);
            }
        }
        _ => {}
    }
}

/// The binding a borrow-shaped init ultimately reads through, or `None`
/// for a shape that never binds as a borrow. `Some(None)` is a borrow
/// rooted in no rebindable name (`this.k`).
fn alias_root(ast: &Ast, init: ExprId) -> Option<Option<String>> {
    // a cast over an element or member read binds as that read does
    let leaf = crate::ssa_lower_return_borrow::peel_passthrough(ast, init);
    match ast.get_expr(init) {
        Expr::Member { .. } | Expr::Index { .. } | Expr::Ident(_) => {}
        _ if matches!(ast.get_expr(leaf), Expr::Member { .. } | Expr::Index { .. }) => {}
        _ => return None,
    }
    let mut e = leaf;
    loop {
        match ast.get_expr(e) {
            Expr::Member { obj, .. }
            | Expr::Index { obj, .. }
            | Expr::OptChain { obj, .. }
            | Expr::OptIndex { obj, .. } => e = *obj,
            Expr::As { expr, .. } => e = *expr,
            Expr::Ident(n) => return Some(Some(n.clone())),
            _ => return Some(None),
        }
    }
}
