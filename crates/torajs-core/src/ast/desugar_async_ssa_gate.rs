//! Which async functions suspend in the SSA lowering: the function is
//! lowered as a body over one heap frame, each `await` parks the frame
//! and returns, and the microtask queue resumes it there
//! (`ssa_lower_suspend`, the split pass in torajs-egraph). Everything
//! this gate refuses keeps the generator-driven state machine
//! (`desugar_async_sm`) or the synchronous lowering behind it.
//!
//! A body that never awaits (its own awaits and `for await` loops; a
//! nested async value is a function of its own and is judged by itself)
//! runs to its end inside the call: it moves too, with no frame, its
//! return settling the result in place
//! (`ssa_lower_fn::async_setup::sync_body`).
//!
//! A function moves when all of these hold:
//!
//! - no await sits in a generator step. An await inside a `try` (any
//!   of its blocks, `await using` included, which `desugar_using`
//!   turned into one) or a `for…of` moves, and so does a `for await`,
//!   whose steps and close suspend the frame (`ssa_lower_fn::suspend::
//!   sync_step` / `any_frame`): the slots their exit protocols keep
//!   (the pending return, the throw a `finally` set aside, the
//!   iterator) are stack slots the frame carries across, and a frame
//!   dropped while it waits there unwinds past the handlers
//!   (`ssa_lower_fn::suspend::cancel`). A dynamic `import()` moves:
//!   its promise is awaited as any other. (`arguments` and
//!   `new.target` move too: the arguments object is built from the
//!   parameters on entry, before the first await, and an async
//!   function is never a constructor, so its `new.target` is
//!   undefined wherever it is read);
//! - it is no generator and no async generator.
//!
//! A generic function, a method of a generic class and a private
//! method move too: each specialized copy minted later under another
//! name takes the mark along with the body (`check_monomorph`). A
//! parameter the parser synthesized (a destructuring pattern's
//! `__destr_src_N`, an injected helper's `__`-spelled ones) is a
//! parameter of the frame like any other.
//!
//! The module body moves by the same rules; its awaits, `for await`
//! steps included, run the jobs ahead of them in place
//! (`ssa_lower_fn::suspend::in_place`), with no frame to resume.
//!
//! Declarations and class methods are marked by name (a method by its
//! mangled `__cm_` / `__sm_` name), values by ExprId; `lift_arrow_fns`
//! carries a value's mark onto its lifted name. A declaration nested in
//! another function's body (or in a block) is judged here too, and its
//! name marked when every async declaration of that name moves (the
//! name is all the later passes key on): the lift that hoists it under
//! a fresh name carries the mark over (`annexb_fn_var`'s flush), and so
//! does the rewrite of a capturing one into a function value
//! (`nested_fns_capture::rewrite`).

use super::desugar_with::walk;
use super::{Ast, ClassMethod, Expr, ExprId, Stmt, mangle_key};
use std::collections::{HashMap, HashSet};

/// `Ast::async_ssa`: the async functions this gate moved.
#[derive(Debug, Clone, Default)]
pub struct AsyncSsa {
    /// Declaration names, mangled method names, and from
    /// `lift_arrow_fns` on the lifted value names.
    pub fns: HashSet<String>,
    /// The values, by ExprId until the lift names them.
    pub value_exprs: HashSet<ExprId>,
    /// The `Promise.resolve(e)` calls `desugar_async` wrapped their
    /// returns in: the lowering returns `e` itself and the frame
    /// settles the result with it.
    pub return_calls: HashSet<ExprId>,
    /// The module body awaits at its top level and moves the same
    /// way: it is lowered into a function of its own that awaits in
    /// place, behind a `main` that calls it and drains the queue
    /// (§16.2.1.5.3 Evaluate of a module with top-level await).
    pub main: bool,
    /// A function that moved has a `for await`: its any-lane steps call
    /// the runtime's frame entries (`ssa_lower_fn::suspend::frame_step`).
    pub frame_for_await: bool,
}

pub fn mark_async_ssa_fns(ast: &mut Ast) {
    let mut names: Vec<String> = Vec::new();
    let mut for_await = false;
    for s in &ast.stmts {
        match s {
            Stmt::FnDecl {
                name,
                body,
                is_generator: false,
                ..
            } if ast.async_fns.contains(name)
                && !ast.async_generator_fns.contains(name)
                && moves(ast, body) =>
            {
                for_await |= body.iter().any(stmt_for_awaits);
                names.push(name.clone())
            }
            Stmt::ClassDecl {
                name,
                methods,
                static_methods,
                ..
            } => {
                for (prefix, list) in [("__cm_", methods), ("__sm_", static_methods)] {
                    for m in list {
                        let mangled = format!("{prefix}{name}__{}", mangle_key(&m.name));
                        if ast.async_fns.contains(&mangled) && method_moves(ast, m) {
                            for_await |= m.body.iter().any(stmt_for_awaits);
                            names.push(mangled);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let mut nested: HashMap<String, bool> = HashMap::new();
    for s in &ast.stmts {
        visit_bodies(ast, s, &mut nested);
    }
    for (name, all_move) in nested {
        let top = ast
            .stmts
            .iter()
            .any(|s| matches!(s, Stmt::FnDecl { name: n, .. } if *n == name));
        if !all_move {
            names.retain(|n| *n != name);
        } else if !top {
            names.push(name);
        }
    }
    names.sort();
    names.dedup();
    ast.async_ssa.fns.extend(names);
    let mut values: Vec<ExprId> = ast.async_fn_value_exprs.iter().copied().collect();
    values.sort_by_key(|e| e.0);
    for eid in values {
        if let Expr::ArrowFn { body, .. } = ast.get_expr(eid)
            && moves(ast, body)
        {
            for_await |= body.iter().any(stmt_for_awaits);
            ast.async_ssa.value_exprs.insert(eid);
        }
    }
    ast.async_ssa.frame_for_await = for_await;
    // the walks stop at declarations' bodies, so these are the
    // module's own awaits; a script's `await` is an identifier
    ast.async_ssa.main = !ast.sloppy_script_goal
        && ast
            .stmts
            .iter()
            .any(|s| stmt_awaits(ast, s) || stmt_for_awaits(s))
        && !ast.stmts.iter().any(|s| stmt_blocks(ast, s));
}

/// Judge every async function declaration nested in the bodies under
/// `s` (a top-level declaration is `s` itself and is judged by the
/// caller), arrows' bodies included: `out` keeps, per name, whether all
/// of them move.
fn visit_bodies(ast: &Ast, s: &Stmt, out: &mut HashMap<String, bool>) {
    match s {
        Stmt::FnDecl { body, .. } => visit_list(ast, body, out),
        Stmt::ClassDecl {
            ctor,
            methods,
            static_methods,
            ..
        } => {
            if let Some(c) = ctor {
                visit_list(ast, &c.body, out);
            }
            for m in methods.iter().chain(static_methods) {
                visit_list(ast, &m.body, out);
            }
        }
        _ => visit_nested(ast, s, out),
    }
}

fn visit_list(ast: &Ast, body: &[Stmt], out: &mut HashMap<String, bool>) {
    for s in body {
        visit_nested(ast, s, out);
    }
}

/// `s` is nested in a body or a block.
fn visit_nested(ast: &Ast, s: &Stmt, out: &mut HashMap<String, bool>) {
    if let Stmt::FnDecl {
        name,
        body,
        is_generator,
        ..
    } = s
        && ast.async_fns.contains(name)
        && !ast.async_generator_fns.contains(name)
    {
        let ok = !is_generator && moves(ast, body);
        *out.entry(name.clone()).or_insert(true) &= ok;
    }
    if matches!(s, Stmt::FnDecl { .. } | Stmt::ClassDecl { .. }) {
        return visit_bodies(ast, s, out);
    }
    for arrow in walk::arrow_nodes(ast, s) {
        if let Expr::ArrowFn { body, .. } = ast.get_expr(arrow) {
            visit_list(ast, body, out);
        }
    }
    for c in walk::stmt_children_ref(s) {
        visit_nested(ast, c, out);
    }
}

/// A method's receiver-polymorphic twin (`__cmany_` / `__smany_`) is
/// a copy of the method's desugared body, so it moves with the
/// method: that body has no `try` turning a throw into the rejection
/// and returns the bare value, which only the lowering of a moved body
/// settles the result with.
pub(super) fn mark_twin(ast: &mut Ast, method: &str, twin: &str) {
    if ast.async_ssa.fns.contains(method) {
        ast.async_ssa.fns.insert(twin.to_string());
    }
}

fn method_moves(ast: &Ast, m: &ClassMethod) -> bool {
    m.accessor_kind.is_none() && !m.is_abstract && moves(ast, &m.body)
}

fn moves(ast: &Ast, body: &[Stmt]) -> bool {
    !body.iter().any(|s| stmt_blocks(ast, s))
}

/// The body holds one of the function's own awaits, or a `for await`:
/// without one it runs to its end inside the call and needs no frame.
pub fn body_awaits(ast: &Ast, body: &[Stmt]) -> bool {
    body.iter()
        .any(|s| stmt_awaits(ast, s) || stmt_for_awaits(s))
}

/// The statement holds one of the function's own awaits.
fn stmt_awaits(ast: &Ast, s: &Stmt) -> bool {
    walk::stmt_exprs(s)
        .into_iter()
        .chain(class_exprs(ast, s))
        .any(|e| expr_awaits(ast, e))
        || walk::stmt_children_ref(s)
            .into_iter()
            .any(|c| stmt_awaits(ast, c))
}

/// The expression holds one of the function's own awaits: nested
/// function values are not entered (`expr_children` stops at them).
fn expr_awaits(ast: &Ast, eid: ExprId) -> bool {
    ast.await_value_reads.contains(&eid)
        || walk::expr_children(ast, eid)
            .into_iter()
            .any(|c| expr_awaits(ast, c))
}

/// The expressions a class declaration evaluates where it stands, in
/// the enclosing function (§15.7.14 ClassDefinitionEvaluation): its
/// `extends` operand and its computed member keys. The member bodies
/// are functions of their own.
fn class_exprs(ast: &Ast, s: &Stmt) -> Vec<ExprId> {
    let Stmt::ClassDecl {
        name,
        decl_id,
        parent,
        ..
    } = s
    else {
        return Vec::new();
    };
    let mut out: Vec<ExprId> = parent.iter().copied().collect();
    for ((owner, sentinel), &key) in &ast.class_computed_keys {
        if owner == name
            && (ast.class_computed_owner.get(sentinel)).is_some_and(|ids| ids.contains(decl_id))
        {
            out.push(key);
        }
    }
    out
}

/// The statement keeps the body off the SSA lowering: an await in a
/// generator step.
fn stmt_blocks(ast: &Ast, s: &Stmt) -> bool {
    let protocol = matches!(s, Stmt::Yield(_) | Stmt::YieldInto { .. });
    if protocol && stmt_awaits(ast, s) {
        return true;
    }
    walk::stmt_children_ref(s)
        .into_iter()
        .any(|c| stmt_blocks(ast, c))
}

/// The statement is a `for await`, whose steps await.
fn stmt_for_awaits(s: &Stmt) -> bool {
    matches!(s, Stmt::ForOf { is_await: true, .. })
        || walk::stmt_children_ref(s).into_iter().any(stmt_for_awaits)
}
