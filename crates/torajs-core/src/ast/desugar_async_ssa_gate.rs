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
//! Every async function moves but a generator: an await inside a `try`
//! (any of its blocks, `await using` included, which `desugar_using`
//! turned into one) or a `for…of` moves, and so does a `for await`,
//! whose steps and close suspend the frame (`ssa_lower_fn::suspend::
//! sync_step` / `any_frame`): the slots their exit protocols keep (the
//! pending return, the throw a `finally` set aside, the iterator) are
//! stack slots the frame carries across, and a frame dropped while it
//! waits there unwinds past the handlers (`ssa_lower_fn::suspend::
//! cancel`). A dynamic `import()` moves: its promise is awaited as any
//! other. (`arguments` and `new.target` move too: the arguments object
//! is built from the parameters on entry, before the first await, and
//! an async function is never a constructor, so its `new.target` is
//! undefined wherever it is read.) An async generator stays with the
//! generator lowering, which lifts its awaits itself
//! (`desugar_generators_async`).
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
//! Declarations are marked by name, values by ExprId; `lift_arrow_fns`
//! carries a value's mark onto its lifted name. A declaration nested in
//! another function's body (or in a block) is judged here too, and its
//! name marked when every async declaration of that name moves (the
//! name is all the later passes key on): the lift that hoists it under
//! a fresh name carries the mark over (`annexb_fn_var`'s flush), and so
//! does the rewrite of a capturing one into a function value
//! (`nested_fns_capture::rewrite`).
//!
//! A class method is marked by its declaration ([`MethodKey`]), at any
//! depth: a nested class takes its final name only in `desugar_classes`
//! (a collision rename's `__hc<N>_`, the capturing lane's `__cc<N>_`),
//! where [`name_moved_methods`] marks the mangled `__cm_` / `__sm_`
//! name it will emit. A top-level class keeps its name, and its methods
//! are marked by name here as well, for the state machine that runs
//! before the classes are desugared (`desugar_async_sm_methods`).

mod nested;

use super::desugar_with::walk;
use super::{Ast, ClassMethod, Expr, ExprId, PropKey, Stmt, mangle_key};
use std::collections::HashSet;

/// A class method by the declaration that holds it: the class's
/// `decl_id`, whether the method is static, and its `mangle_key`.
pub type MethodKey = (u32, bool, String);

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
    /// The parser's async methods ([`record_async_method`]). Two
    /// classes of one name (in two functions) share their mangled name
    /// until the hoist renames one, so the name cannot say which of
    /// them is async.
    pub async_methods: HashSet<MethodKey>,
    /// The methods that moved.
    pub methods: HashSet<MethodKey>,
}

/// The parser's record of an async method of the class `class`
/// (declaration `decl_id`): by its mangled name in `async_fns`, which
/// every "is this async" reader keys on, and by declaration.
pub fn record_async_method(
    ast: &mut Ast,
    (class, decl_id): (&str, u32),
    is_static: bool,
    member: &PropKey,
) {
    let sym = mangle_key(member);
    let prefix = if is_static { "__sm_" } else { "__cm_" };
    ast.async_fns.insert(format!("{prefix}{class}__{sym}"));
    let key = (decl_id, is_static, sym.into_owned());
    ast.async_ssa.async_methods.insert(key);
}

pub fn mark_async_ssa_fns(ast: &mut Ast) {
    let mut census = nested::Census::default();
    for s in &ast.stmts {
        census.top(ast, s);
    }
    let mut names = census.names();
    for s in &ast.stmts {
        names.extend(moved_names(&census.methods, s));
    }
    names.sort();
    names.dedup();
    ast.async_ssa.fns.extend(names);
    let mut for_await = census.for_await;
    ast.async_ssa.methods.extend(census.methods);
    let mut values: Vec<ExprId> = ast.async_fn_value_exprs.iter().copied().collect();
    values.sort_by_key(|e| e.0);
    for eid in values {
        if let Expr::ArrowFn { body, .. } = ast.get_expr(eid) {
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
            .any(|s| stmt_awaits(ast, s) || stmt_for_awaits(s));
}

/// The mangled names, under the name it has now, of the moved methods
/// (`moved`) of the class `s` declares.
fn moved_names(moved: &HashSet<MethodKey>, s: &Stmt) -> Vec<String> {
    let Stmt::ClassDecl {
        name,
        decl_id,
        methods,
        static_methods,
        ..
    } = s
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (is_static, list) in [(false, methods), (true, static_methods)] {
        let prefix = if is_static { "__sm_" } else { "__cm_" };
        for m in list {
            let sym = mangle_key(&m.name);
            if moved.contains(&(*decl_id, is_static, sym.to_string())) {
                out.push(format!("{prefix}{name}__{sym}"));
            }
        }
    }
    out
}

/// `desugar_classes`, once the hoist has given every class its final
/// name and before the methods are emitted under it: mark each moved
/// method's mangled name, and record it as async under that name.
pub(super) fn name_moved_methods(ast: &mut Ast) {
    let mut names: Vec<String> = Vec::new();
    for top in &ast.stmts {
        nested::each_stmt(ast, top, &mut |s| {
            names.extend(moved_names(&ast.async_ssa.methods, s))
        });
    }
    for n in names {
        ast.async_fns.insert(n.clone());
        ast.async_ssa.fns.insert(n);
    }
}

/// The capturing lane installs the method `m` of the declaration
/// `decl_id` as a function value (`capturing_classes::lower_to_es5`),
/// after `desugar_async` settled the values it knew of: an async one is
/// settled here the same way, and takes the value mark when it moved.
pub(super) fn method_value(
    ast: &mut Ast,
    (decl_id, is_static): (u32, bool),
    m: &mut ClassMethod,
) -> ExprId {
    let params = std::mem::take(&mut m.params);
    let mut return_type = m.return_type.take();
    let mut body = std::mem::take(&mut m.body);
    let key = (decl_id, is_static, mangle_key(&m.name).into_owned());
    let is_async = ast.async_ssa.async_methods.contains(&key);
    let ssa = ast.async_ssa.methods.contains(&key);
    if is_async {
        let declared = return_type.unwrap_or_else(|| "any".into());
        let (settled, promise_ty) =
            super::desugar_async::build_async_body(ast, body, &declared, ssa);
        body = settled;
        return_type = Some(promise_ty);
    }
    let eid = ast.add_expr(Expr::ArrowFn {
        params,
        return_type,
        body,
    });
    if is_async {
        ast.async_fn_value_exprs.insert(eid);
    }
    if ssa {
        ast.async_ssa.value_exprs.insert(eid);
    }
    eid
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

/// An async method moves unless it has no body of its own to move (an
/// accessor or an abstract method).
fn method_moves(m: &ClassMethod) -> bool {
    m.accessor_kind.is_none() && !m.is_abstract
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

/// The statement is a `for await`, whose steps await.
fn stmt_for_awaits(s: &Stmt) -> bool {
    matches!(s, Stmt::ForOf { is_await: true, .. })
        || walk::stmt_children_ref(s).into_iter().any(stmt_for_awaits)
}
