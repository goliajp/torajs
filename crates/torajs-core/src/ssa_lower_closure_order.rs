//! Pass 2B closure-body lowering order — construction before body.
//!
//! A lifted closure's body lowering reads `closure_captures`, which its
//! CONSTRUCTION site (an `Expr::Closure` node in the enclosing fn's
//! body) populates. M2.A ordered the bodies by reverse append order,
//! which equals parent-before-child only while the arena holds inner
//! expressions before outer ones — true for parser output, broken by
//! `desugar_capturing_nested_fns`: a nested `function` declaration it
//! routes becomes a function expression minted at the arena TAIL, so
//! the child closure appends after its parent and reverse order lowers
//! the child's body first.
//!
//! This pass derives the real dependency from the AST instead: each
//! closure has exactly one construction site, so "whose body holds my
//! construction" is a parent function — the relation is a forest, no
//! cycles. A stable topological pass over the reverse-append order
//! keeps today's order wherever it already satisfies parent-first
//! (parser-shaped nesting is untouched) and defers only a child that
//! appears ahead of its parent.

use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::ssa::FuncId;
use std::collections::{HashMap, HashSet};

/// What a closure's construction site knows of the bindings it
/// captures, which the closure's body, lowered after it, reads.
pub(crate) struct CaptureFacts {
    /// 556-02 — the captures that were VARIADIC bindings in the constructing
    /// frame (`variadic_locals` there). The body preamble re-registers them so a
    /// call through one takes the boxed dual entry (the fixed-prefix
    /// CallIndirect fed a rest body its argv slots as plain params, exit 139).
    pub(crate) variadic: Vec<String>,
    /// Annotated captures and their annotations (the body's `let_anns`, `binding_ann`).
    pub(crate) anns: Vec<(String, String)>,
    /// Captures holding a namespace-static read ([`crate::ssa_lower_ns_static_alias`]).
    pub(crate) ns_static: Vec<(String, (i64, bool))>,
}

/// T-15.g.5 extension: closure construction can also live at module
/// top-level (`let cb = function(v) { return v + cap }` directly in
/// implicit main). Top-level construction only runs when synthesize_
/// main lowers, so closure bodies that depend on top-level captures
/// must lower AFTER main, not just after user fns. Pipeline now:
/// Pass 2A user fns → Pass 3 main → Pass 2B closure bodies (reverse).
/// `(stmt index, fid)` pairs for top-level FnDecls.
type DeclIndices = Vec<(usize, FuncId)>;

pub(crate) fn partition_closure_decls(
    ast: &Ast,
    decl_indices: DeclIndices,
) -> (DeclIndices, DeclIndices) {
    let (user_decls, mut closure_decls): (Vec<_>, Vec<_>) =
        decl_indices
            .into_iter()
            .partition(|(stmt_idx, _)| match &ast.stmts[*stmt_idx] {
                Stmt::FnDecl { name, .. } => !name.starts_with("__closure_"),
                _ => true,
            });
    closure_decls.reverse();
    // RFC 20260912 — a generator's iterator methods that read captures
    // through the wrapper's env depend on the body that constructs the
    // wrapper (its `closure_captures` entry) and are depended on by the
    // closures their own bodies construct: they take their place in the
    // closure order, resolved by the topological pass below.
    let (gen_env_methods, user_decls): (Vec<_>, Vec<_>) =
        user_decls
            .into_iter()
            .partition(|(stmt_idx, _)| match &ast.stmts[*stmt_idx] {
                Stmt::FnDecl { name, .. } => ast.gen_env_methods.contains_key(name),
                _ => false,
            });
    closure_decls.extend(gen_env_methods);
    // Reverse append equals parent-before-child only for parser-shaped
    // nesting; a closure minted by the capturing-nested-fn route sits
    // at the arena tail and needs the topological pass to lower after
    // the closure that constructs it.
    let closure_decls = order_closure_bodies(ast, closure_decls);
    (user_decls, closure_decls)
}

/// Reorder `closure_decls` (already reverse-append) so every closure
/// lowers after the closure whose body holds its construction site.
/// Construction sites in user fns or implicit main need nothing: those
/// lower in Pass 2A / Pass 3, before every closure body.
pub(crate) fn order_closure_bodies(
    ast: &Ast,
    closure_decls: Vec<(usize, FuncId)>,
) -> Vec<(usize, FuncId)> {
    // parent[name] = closure whose body constructs `name`.
    let mut parent: HashMap<String, String> = HashMap::new();
    for (stmt_idx, _) in &closure_decls {
        let Stmt::FnDecl { name, body, .. } = &ast.stmts[*stmt_idx] else {
            continue;
        };
        let mut constructed: Vec<String> = Vec::new();
        for s in body {
            closures_in_stmt(ast, s, &mut constructed);
        }
        for c in constructed {
            parent.insert(c, name.clone());
        }
    }
    // RFC 20260912 — a gen-env iterator method's parent is whichever
    // closure body constructs its wrapper (none when a user fn or main
    // does: those lowered already). Its own body was scanned above, so
    // the closures it constructs already name it as their parent.
    for (stmt_idx, _) in &closure_decls {
        let Stmt::FnDecl { name, .. } = &ast.stmts[*stmt_idx] else {
            continue;
        };
        let Some(info) = ast.gen_env_methods.get(name) else {
            continue;
        };
        if let Some(p) = parent.get(&info.wrapper_fn_name(ast)).cloned() {
            parent.insert(name.clone(), p);
        }
    }
    // A clone of a lifted body (551-03 any-widen, the knife 2 construct
    // face) reads the original's env layout, which is known once the
    // original's construction site lowered: it waits for the same
    // parent.
    for (clone, orig) in &ast.widen_clone_origin {
        if let Some(p) = parent.get(orig).cloned() {
            parent.insert(clone.clone(), p);
        }
    }
    let names: Vec<Option<&str>> = closure_decls
        .iter()
        .map(|(i, _)| match &ast.stmts[*i] {
            Stmt::FnDecl { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    // Stable Kahn over the incoming order: emit the first entry whose
    // parent is already emitted (or is not a closure). The relation is
    // a forest, so this always drains.
    let mut emitted: HashSet<String> = HashSet::new();
    let mut done: Vec<bool> = vec![false; closure_decls.len()];
    let mut out: Vec<(usize, FuncId)> = Vec::with_capacity(closure_decls.len());
    while out.len() < closure_decls.len() {
        let mut progressed = false;
        for k in 0..closure_decls.len() {
            if done[k] {
                continue;
            }
            let ready = match names[k].and_then(|n| parent.get(n)) {
                Some(p) => emitted.contains(p),
                None => true,
            };
            if ready {
                done[k] = true;
                if let Some(n) = names[k] {
                    emitted.insert(n.to_string());
                }
                out.push(closure_decls[k]);
                progressed = true;
            }
        }
        if !progressed {
            // A parent recorded in the map but absent from this decl
            // set (can't lower here) — emit the remainder in incoming
            // order rather than loop forever.
            for k in 0..closure_decls.len() {
                if !done[k] {
                    done[k] = true;
                    out.push(closure_decls[k]);
                }
            }
        }
    }
    out
}

/// Collect the names of every `Expr::Closure` construction site inside
/// `s`, including sites buried in nested expressions.
fn closures_in_stmt(ast: &Ast, s: &Stmt, out: &mut Vec<String>) {
    match s {
        Stmt::Expr(eid) | Stmt::Return(Some(eid)) | Stmt::Yield(eid) | Stmt::Throw(eid) => {
            closures_in_expr(ast, *eid, out)
        }
        Stmt::YieldInto { value, .. } => closures_in_expr(ast, *value, out),
        Stmt::Return(None) | Stmt::Break(_) | Stmt::Continue(_) => {}
        Stmt::LetDecl { init, .. } | Stmt::UsingDecl { init, .. } => {
            closures_in_expr(ast, *init, out)
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            closures_in_expr(ast, *cond, out);
            closures_in_stmt(ast, then_branch, out);
            if let Some(eb) = else_branch {
                closures_in_stmt(ast, eb, out);
            }
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            closures_in_expr(ast, *cond, out);
            closures_in_stmt(ast, body, out);
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
            ..
        } => {
            closures_in_expr(ast, *scrutinee, out);
            for c in cases {
                closures_in_expr(ast, c.value, out);
                for s in &c.body {
                    closures_in_stmt(ast, s, out);
                }
            }
            if let Some(db) = default {
                for s in db {
                    closures_in_stmt(ast, s, out);
                }
            }
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init {
                closures_in_stmt(ast, i, out);
            }
            if let Some(c) = cond {
                closures_in_expr(ast, *c, out);
            }
            if let Some(st) = step {
                closures_in_expr(ast, *st, out);
            }
            closures_in_stmt(ast, body, out);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for st in stmts {
                closures_in_stmt(ast, st, out);
            }
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            closures_in_expr(ast, *parent, out);
            closures_in_expr(ast, *sep, out);
            closures_in_stmt(ast, body, out);
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => {
            closures_in_expr(ast, *elem_expr, out);
            closures_in_stmt(ast, body, out);
        }
        Stmt::Labeled { body, .. } => closures_in_stmt(ast, body, out),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for s in body {
                closures_in_stmt(ast, s, out);
            }
            for s in catch_body {
                closures_in_stmt(ast, s, out);
            }
            if let Some(fb) = finally_body {
                for s in fb {
                    closures_in_stmt(ast, s, out);
                }
            }
        }
        Stmt::FnDecl { body, .. } => {
            for s in body {
                closures_in_stmt(ast, s, out);
            }
        }
        Stmt::ExportDecl { inner, .. } => {
            if let Some(inner) = inner {
                closures_in_stmt(ast, inner, out);
            }
        }
        Stmt::TypeDecl { .. } | Stmt::ClassDecl { .. } | Stmt::ImportDecl { .. } => {}
    }
}

fn closures_in_expr(ast: &Ast, eid: ExprId, out: &mut Vec<String>) {
    match ast.get_expr(eid) {
        Expr::Closure { fn_name, .. } => out.push(fn_name.clone()),
        Expr::ArrowFn { body, .. } => {
            for s in body {
                closures_in_stmt(ast, s, out);
            }
        }
        Expr::Elision
        | Expr::Ident(_)
        | Expr::String(_)
        | Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Uninit
        | Expr::Regex { .. }
        | Expr::This
        | Expr::NewTarget => {}
        Expr::BinOp { left, right, .. }
        | Expr::Sequence { left, right }
        | Expr::Nullish {
            lhs: left,
            rhs: right,
        } => {
            closures_in_expr(ast, *left, out);
            closures_in_expr(ast, *right, out);
        }
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Delete { expr }
        | Expr::Spread { expr }
        | Expr::As { expr, .. } => closures_in_expr(ast, *expr, out),
        Expr::InstanceOf { expr, rhs } => {
            closures_in_expr(ast, *expr, out);
            closures_in_expr(ast, *rhs, out);
        }
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => closures_in_expr(ast, *obj, out),
        Expr::Call { callee, args } | Expr::OptCall { callee, args } => {
            closures_in_expr(ast, *callee, out);
            for a in args {
                closures_in_expr(ast, *a, out);
            }
        }
        Expr::Assign { target, value } => {
            closures_in_expr(ast, *target, out);
            closures_in_expr(ast, *value, out);
        }
        Expr::Index { obj, index } | Expr::OptIndex { obj, index } => {
            closures_in_expr(ast, *obj, out);
            closures_in_expr(ast, *index, out);
        }
        Expr::Array(elems) => {
            for e in elems {
                closures_in_expr(ast, *e, out);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, e) in fields {
                closures_in_expr(ast, *e, out);
            }
        }
        Expr::New { args, .. } | Expr::Super { args } => {
            for a in args {
                closures_in_expr(ast, *a, out);
            }
        }
        Expr::NewDynamic { callee, args } => {
            closures_in_expr(ast, *callee, out);
            for a in args {
                closures_in_expr(ast, *a, out);
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            closures_in_expr(ast, *cond, out);
            closures_in_expr(ast, *then_branch, out);
            closures_in_expr(ast, *else_branch, out);
        }
        Expr::PostIncr { target, .. } => closures_in_expr(ast, *target, out),
    }
}
