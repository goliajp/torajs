//! `new N()` where `N` names a `function` declared in the enclosing
//! function scope, not the class of that name.
//!
//! ```text
//! class Widget { tag = 1 }
//! (function () {
//!   function Widget() {}
//!   const x = new Widget()   // bun: the function's instance
//!   x.constructor === Widget // tr said false, and x.tag was 1
//! })()
//! ```
//!
//! `Expr::New` carries its target as a NAME, and `desugar_classes`
//! pass 2 binds every one of them to the `__new_<C>` factory of the
//! class with that name — a global lookup, with no notion of which
//! scope the `new` is written in. The lexical shadow that should win
//! is only established later, when `desugar_nested_fns` renames the
//! nested declaration; by then the `new` is a call to a factory and
//! the rename has nothing to bind to.
//!
//! Silently: both answer to the same name, so only an identity
//! comparison or a field read separates them. With a builtin error
//! name it is worse — the program gets the intrinsic, which is how
//! test262's harness/assert-throws-custom-typeerror reads its own
//! failure message.
//!
//! This pass runs before `desugar_classes` and records the sites where
//! the name is shadowed. Pass 2 leaves those alone, and they take the
//! ordinary non-class `new` route, where the declaration's own rename
//! does reach them.
//!
//! **Every binding kind that can name a constructor**, not just
//! `function`. An earlier note here claimed a `let` / `const` /
//! parameter "binds a value rather than a constructor, and a `new`
//! through one is already the dynamic route" — measured, that was
//! wrong: `class W {}` beside `function f() { const W = function
//! (){}; new W() }` handed the program the CLASS's instance, fields
//! and all, exactly as the `function` case did.
//!
//! The three kinds do not share a scope, so they are not collected
//! into one. A `function` declaration belongs to the function scope
//! (Annex B §B.3.3 puts a block-nested one there too, which is what
//! `block_nested_fn_names` answers); a parameter belongs to it as
//! well; a `let` / `const` belongs to its BLOCK. Flattening the last
//! into the function's set would shadow a `new` written outside the
//! block that declares it. `Stmt::Multi` shares the surrounding scope
//! by definition and so is spliced rather than given a scope.
//!
//! Recorded gaps: a `for` / `for-of` loop variable is a binding too
//! and is not collected yet; a class METHOD body is not walked
//! either, since this pass runs before `desugar_classes` flattens them
//! and the bodies are still inside `Stmt::ClassDecl`.

use super::annexb_names::{block_nested_fn_names, own_fn_names, param_names};
use super::desugar_with::walk::stmt_children_ref;
use super::{Ast, Expr, ExprId, Stmt};
use std::collections::HashSet;

/// Record every `new <name>()` whose name is shadowed by a `function`
/// declaration in an enclosing function scope.
pub fn mark_fn_shadowed_new(ast: &mut Ast) {
    // The module's own statements are not a function scope: a
    // top-level `function N` next to a `class N` is a redeclaration,
    // which the injection gate and the checker already answer.
    let stmts = std::mem::take(&mut ast.stmts);
    let mut scopes: Vec<HashSet<String>> = Vec::new();
    let mut out: HashSet<ExprId> = HashSet::new();
    for s in &stmts {
        walk_stmt(ast, s, &mut scopes, &mut out);
    }
    ast.stmts = stmts;
    ast.fn_shadowed_new = out;
}

/// The names a function body binds with `function`, at its top level
/// or inside one of its blocks — Annex B §B.3.3 puts the block ones in
/// the function scope too, which is the same set `desugar_nested_fns`
/// lifts and renames.
fn body_fn_names(body: &[Stmt]) -> HashSet<String> {
    let mut names = own_fn_names(body);
    names.extend(block_nested_fn_names(body));
    names
}

/// The `let` / `const` names a statement list binds DIRECTLY. `Multi`
/// shares the surrounding scope, so its entries belong to this list;
/// a nested `Block` gets its own scope and is not descended into here.
fn let_const_names(stmts: &[Stmt]) -> HashSet<String> {
    let mut out = HashSet::new();
    for s in stmts {
        match s {
            Stmt::LetDecl {
                name,
                is_var: false,
                ..
            }
            | Stmt::UsingDecl { name, .. } => {
                out.insert(name.clone());
            }
            Stmt::Multi(inner) => out.extend(let_const_names(inner)),
            _ => {}
        }
    }
    out
}

/// Every `var` the body binds, at any depth — `var` is function-scoped,
/// so a `var W` inside a nested block shadows the whole body and not
/// just that block. The walk stops at a nested function: its `var`s
/// are its own.
fn var_names(stmts: &[Stmt], out: &mut HashSet<String>) {
    for s in stmts {
        if let Stmt::LetDecl {
            name, is_var: true, ..
        } = s
        {
            out.insert(name.clone());
        }
        if matches!(s, Stmt::FnDecl { .. }) {
            continue;
        }
        for c in stmt_children_ref(s) {
            var_names(std::slice::from_ref(c), out);
        }
    }
}

/// A function scope: its `function` declarations, its parameters, the
/// `let` / `const` its body binds at top level (the body is that
/// block), and every `var` at any depth.
fn walk_body(
    ast: &Ast,
    body: &[Stmt],
    params: &[super::Param],
    scopes: &mut Vec<HashSet<String>>,
    out: &mut HashSet<ExprId>,
) {
    let mut names = body_fn_names(body);
    names.extend(param_names(params));
    names.extend(let_const_names(body));
    var_names(body, &mut names);
    scopes.push(names);
    for s in body {
        walk_stmt(ast, s, scopes, out);
    }
    scopes.pop();
}

/// A block: the `let` / `const` it binds directly, plus `extra` (a
/// catch parameter).
fn walk_block(
    ast: &Ast,
    list: &[Stmt],
    extra: Option<&str>,
    scopes: &mut Vec<HashSet<String>>,
    out: &mut HashSet<ExprId>,
) {
    let mut names = let_const_names(list);
    names.extend(extra.map(str::to_string));
    scopes.push(names);
    for s in list {
        walk_stmt(ast, s, scopes, out);
    }
    scopes.pop();
}

fn walk_stmt(ast: &Ast, stmt: &Stmt, scopes: &mut Vec<HashSet<String>>, out: &mut HashSet<ExprId>) {
    match stmt {
        Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Throw(e) | Stmt::Yield(e) => {
            walk_expr(ast, *e, scopes, out)
        }
        Stmt::YieldInto { value, .. } => walk_expr(ast, *value, scopes, out),
        Stmt::LetDecl { init, .. } | Stmt::UsingDecl { init, .. } => {
            walk_expr(ast, *init, scopes, out)
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            walk_expr(ast, *cond, scopes, out);
            walk_stmt(ast, then_branch, scopes, out);
            if let Some(e) = else_branch {
                walk_stmt(ast, e, scopes, out);
            }
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            walk_expr(ast, *cond, scopes, out);
            walk_stmt(ast, body, scopes, out);
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init {
                walk_stmt(ast, i, scopes, out);
            }
            if let Some(c) = cond {
                walk_expr(ast, *c, scopes, out);
            }
            if let Some(s) = step {
                walk_expr(ast, *s, scopes, out);
            }
            walk_stmt(ast, body, scopes, out);
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => {
            walk_expr(ast, *elem_expr, scopes, out);
            walk_stmt(ast, body, scopes, out);
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            walk_expr(ast, *parent, scopes, out);
            walk_expr(ast, *sep, scopes, out);
            walk_stmt(ast, body, scopes, out);
        }
        // A block is a scope of its own for `let` / `const`.
        Stmt::Block(b) => walk_block(ast, b, None, scopes, out),
        Stmt::Multi(b) => {
            // Shares the surrounding scope — its bindings were already
            // collected by whoever owns that scope.
            for s in b {
                walk_stmt(ast, s, scopes, out);
            }
        }
        // Each of the three is a block (§14.15), and the catch block
        // also binds its parameter. An async function's body is
        // wrapped in one before this pass runs.
        Stmt::Try {
            body,
            catch_param,
            catch_body,
            finally_body,
            ..
        } => {
            walk_block(ast, body, None, scopes, out);
            walk_block(ast, catch_body, catch_param.as_deref(), scopes, out);
            if let Some(f) = finally_body {
                walk_block(ast, f, None, scopes, out);
            }
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
            ..
        } => {
            walk_expr(ast, *scrutinee, scopes, out);
            for c in cases {
                walk_expr(ast, c.value, scopes, out);
                for s in &c.body {
                    walk_stmt(ast, s, scopes, out);
                }
            }
            for s in default.iter().flatten() {
                walk_stmt(ast, s, scopes, out);
            }
        }
        Stmt::Labeled { body, .. } => walk_stmt(ast, body, scopes, out),
        Stmt::FnDecl { body, params, .. } => walk_body(ast, body, params, scopes, out),
        Stmt::ExportDecl { inner, .. } => {
            if let Some(i) = inner {
                walk_stmt(ast, i, scopes, out);
            }
        }
        Stmt::Return(None)
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::TypeDecl { .. }
        | Stmt::ClassDecl { .. }
        | Stmt::ImportDecl { .. } => {}
    }
}

fn walk_expr(ast: &Ast, eid: ExprId, scopes: &mut Vec<HashSet<String>>, out: &mut HashSet<ExprId>) {
    match ast.get_expr(eid) {
        Expr::New {
            class_name, args, ..
        } => {
            if scopes.iter().any(|s| s.contains(class_name)) {
                out.insert(eid);
            }
            for a in args {
                walk_expr(ast, *a, scopes, out);
            }
        }
        Expr::ArrowFn { body, params, .. } => walk_body(ast, body, params, scopes, out),
        Expr::BinOp { left, right, .. }
        | Expr::Sequence { left, right }
        | Expr::Assign {
            target: left,
            value: right,
        }
        | Expr::Index {
            obj: left,
            index: right,
        }
        | Expr::OptIndex {
            obj: left,
            index: right,
        }
        | Expr::Nullish {
            lhs: left,
            rhs: right,
        }
        | Expr::InstanceOf {
            expr: left,
            rhs: right,
        } => {
            walk_expr(ast, *left, scopes, out);
            walk_expr(ast, *right, scopes, out);
        }
        Expr::Unary { expr, .. }
        | Expr::Member { obj: expr, .. }
        | Expr::As { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Delete { expr }
        | Expr::Spread { expr }
        | Expr::OptChain { obj: expr, .. }
        | Expr::PostIncr { target: expr, .. } => walk_expr(ast, *expr, scopes, out),
        Expr::Call { callee, args }
        | Expr::NewDynamic { callee, args }
        | Expr::OptCall { callee, args } => {
            walk_expr(ast, *callee, scopes, out);
            for a in args {
                walk_expr(ast, *a, scopes, out);
            }
        }
        Expr::Array(items) => {
            for e in items {
                walk_expr(ast, *e, scopes, out);
            }
        }
        Expr::Super { args } => {
            for a in args {
                walk_expr(ast, *a, scopes, out);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, e) in fields {
                walk_expr(ast, *e, scopes, out);
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            walk_expr(ast, *cond, scopes, out);
            walk_expr(ast, *then_branch, scopes, out);
            walk_expr(ast, *else_branch, scopes, out);
        }
        Expr::Ident(_)
        | Expr::String(_)
        | Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Uninit
        | Expr::Elision
        | Expr::Regex { .. }
        | Expr::Closure { .. }
        | Expr::This
        | Expr::NewTarget => {}
    }
}
