//! Which async functions suspend in the SSA lowering: the function is
//! lowered as a body over one heap frame, each `await` parks the frame
//! and returns, and the microtask queue resumes it there
//! (`ssa_lower_suspend`, the split pass in torajs-egraph). Everything
//! this gate refuses keeps the generator-driven state machine
//! (`desugar_async_sm`) or the synchronous lowering behind it.
//!
//! A body that never awaits (its own awaits; a nested async value is
//! a function of its own and is judged by itself) runs to its end
//! inside the call: it moves too, with no frame, its return settling
//! the result in place (`ssa_lower_fn::async_setup::sync_body`).
//!
//! A function moves when all of these hold:
//!
//! - no await sits inside a `class` body, and the function has no
//!   `for await` (its steps await without spelling one; they still
//!   drain the queue in place). An await inside a `try` (any of its
//!   blocks, `await using` included, which `desugar_using` turned into
//!   one) or a `for…of` moves: the slots their exit protocols keep
//!   (the pending return, the throw a `finally` set aside, the
//!   iterator) are stack slots the frame carries across, and a frame
//!   dropped while it waits there unwinds past the handlers
//!   (`ssa_lower_fn::suspend::cancel`);
//! - it makes no dynamic `import()`: the imported module's top-level
//!   await and a suspended caller's resumption do not agree yet.
//!   (`arguments` and `new.target` move: the arguments object is built
//!   from the parameters on entry, before the first await, and an
//!   async function is never a constructor, so its `new.target` is
//!   undefined wherever it is read);
//! - no parameter is a synthesized `__` one, and it is no generator,
//!   no async generator and not generic (a specialized copy of a
//!   generic body is minted later under another name).
//!
//! The module body moves by the same rules, except that a `for await`
//! or a dynamic `import()` in it moves too: it awaits in place
//! (`ssa_lower_fn::suspend::in_place`), with no frame to resume.
//!
//! Declarations and class methods are marked by name (a method by its
//! mangled `__cm_` / `__sm_` name), values by ExprId; `lift_arrow_fns`
//! carries a value's mark onto its lifted name.

use super::desugar_with::walk;
use super::{Ast, ClassMethod, Expr, ExprId, Param, Stmt, mangle_key};
use std::collections::HashSet;

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
}

pub fn mark_async_ssa_fns(ast: &mut Ast) {
    let mut names: Vec<String> = Vec::new();
    for s in &ast.stmts {
        match s {
            Stmt::FnDecl {
                name,
                type_params,
                params,
                body,
                is_generator: false,
                ..
            } if type_params.is_empty()
                && ast.async_fns.contains(name)
                && !ast.async_generator_fns.contains(name)
                && moves(ast, params, body) =>
            {
                names.push(name.clone())
            }
            Stmt::ClassDecl {
                name,
                type_params,
                methods,
                static_methods,
                ..
            } if type_params.is_empty() => {
                for (prefix, list) in [("__cm_", methods), ("__sm_", static_methods)] {
                    for m in list {
                        let mangled = format!("{prefix}{name}__{}", mangle_key(&m.name));
                        if ast.async_fns.contains(&mangled) && method_moves(ast, m) {
                            names.push(mangled);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    ast.async_ssa.fns.extend(names);
    let mut values: Vec<ExprId> = ast.async_fn_value_exprs.iter().copied().collect();
    values.sort_by_key(|e| e.0);
    for eid in values {
        if let Expr::ArrowFn { params, body, .. } = ast.get_expr(eid)
            && moves(ast, params, body)
        {
            ast.async_ssa.value_exprs.insert(eid);
        }
    }
    // the walks stop at declarations' bodies, so these are the
    // module's own awaits; a script's `await` is an identifier
    ast.async_ssa.main = !ast.sloppy_script_goal
        && ast
            .stmts
            .iter()
            .any(|s| stmt_awaits(ast, s) || stmt_for_awaits(s))
        && !ast.stmts.iter().any(|s| stmt_blocks(ast, s, Owner::Module));
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
    m.accessor_kind.is_none()
        && !m.is_abstract
        && m.type_params.is_empty()
        && !m.name.lossy().starts_with("__priv_")
        && moves(ast, &m.params, &m.body)
}

fn moves(ast: &Ast, params: &[Param], body: &[Stmt]) -> bool {
    !params.iter().any(|p| p.name.starts_with("__"))
        && !body.iter().any(|s| stmt_blocks(ast, s, Owner::Function))
}

/// The body holds one of the function's own awaits: without one it
/// runs to its end inside the call and needs no frame.
pub fn body_awaits(ast: &Ast, body: &[Stmt]) -> bool {
    body.iter().any(|s| stmt_awaits(ast, s))
}

/// The statement holds one of the function's own awaits.
fn stmt_awaits(ast: &Ast, s: &Stmt) -> bool {
    walk::stmt_exprs(s).into_iter().any(|e| expr_awaits(ast, e))
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

/// Whose body [`stmt_blocks`] judges.
#[derive(Clone, Copy, PartialEq)]
enum Owner {
    Function,
    Module,
}

/// The statement keeps the body off the SSA lowering: an await in a
/// class body or a generator step, a `for` whose head makes a function,
/// or in a function a `for await` (its steps await without spelling
/// one, and only the state machine suspends them) or a dynamic
/// `import()` anywhere. The module body awaits both in place.
fn stmt_blocks(ast: &Ast, s: &Stmt, owner: Owner) -> bool {
    let protocol = matches!(
        s,
        Stmt::ClassDecl { .. } | Stmt::Yield(_) | Stmt::YieldInto { .. }
    );
    if (protocol && stmt_awaits(ast, s))
        || (owner == Owner::Function && matches!(s, Stmt::ForOf { is_await: true, .. }))
    {
        return true;
    }
    if loop_head_closes_over(ast, s) {
        return true;
    }
    // the module body awaits an import's promise in place, as any other
    if owner == Owner::Function
        && walk::stmt_exprs(s)
            .into_iter()
            .any(|e| expr_reserved(ast, e))
    {
        return true;
    }
    walk::stmt_children_ref(s)
        .into_iter()
        .any(|c| stmt_blocks(ast, c, owner))
}

/// The statement is a `for await`, whose steps await.
fn stmt_for_awaits(s: &Stmt) -> bool {
    matches!(s, Stmt::ForOf { is_await: true, .. })
        || walk::stmt_children_ref(s).into_iter().any(stmt_for_awaits)
}

/// A `for` whose head makes a function: §14.7.4.4 gives each iteration
/// its own copy of the `let` bindings, and a closure made in the
/// initializer or the update sees a particular copy. The statement
/// lowering shares one binding across those positions, which the state
/// machine's per-iteration fields do not.
fn loop_head_closes_over(ast: &Ast, s: &Stmt) -> bool {
    let Stmt::For { init, step, .. } = s else {
        return false;
    };
    init.as_deref().is_some_and(|i| stmt_makes_fn(ast, i))
        || step.is_some_and(|e| expr_makes_fn(ast, e))
}

fn stmt_makes_fn(ast: &Ast, s: &Stmt) -> bool {
    walk::stmt_exprs(s)
        .into_iter()
        .any(|e| expr_makes_fn(ast, e))
        || walk::stmt_children_ref(s)
            .into_iter()
            .any(|c| stmt_makes_fn(ast, c))
}

fn expr_makes_fn(ast: &Ast, eid: ExprId) -> bool {
    matches!(ast.get_expr(eid), Expr::ArrowFn { .. })
        || walk::expr_children(ast, eid)
            .into_iter()
            .any(|c| expr_makes_fn(ast, c))
}

/// A dynamic `import()` anywhere in the expression, nested function
/// bodies included.
fn expr_reserved(ast: &Ast, eid: ExprId) -> bool {
    match ast.get_expr(eid) {
        Expr::Ident(n) => n == "__torajs_dyn_import",
        Expr::ArrowFn { body, .. } => body.iter().any(|s| stmt_reserved(ast, s)),
        _ => walk::expr_children(ast, eid)
            .into_iter()
            .any(|c| expr_reserved(ast, c)),
    }
}

fn stmt_reserved(ast: &Ast, s: &Stmt) -> bool {
    walk::stmt_exprs(s)
        .into_iter()
        .any(|e| expr_reserved(ast, e))
        || walk::stmt_children_ref(s)
            .into_iter()
            .any(|c| stmt_reserved(ast, c))
}
