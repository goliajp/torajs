//! Which `s.split(sep)` calls only ever have their product measured —
//! the census the split-count lane consults (rotation 672).
//!
//! `s.split(sep)` builds an array of views, and a program that then
//! reads nothing but its `.length` pays for every cell, its release
//! and the pool round-trip to learn one number. The array is never
//! observable in these shapes — no element is read, its identity is
//! never compared, nothing is written into it — so the lowering
//! replaces it by the number (scalar replacement of an aggregate that
//! does not escape): `__torajs_str_split_count`, which counts the
//! tokens without building them.
//!
//! Two shapes qualify:
//!
//! - `R.split(S).length` — the product is a temporary whose one use is
//!   the length read.
//! - `let` / `const` `x = R.split(S)` where every use of `x` in its
//!   scope is `x.length` in a read position. A write (`x.length = n`,
//!   `x.length++`, `delete x.length`), a call through it
//!   (`x.length()`), any other member, index or bare use, a lifted
//!   closure that captured `x`, an arrow, or a function or class
//!   declaration anywhere in the scope disqualifies it: a shape the
//!   walk can not prove is a length read counts as a use.
//!
//! Only a one-argument call qualifies (a limit slices the product).
//! Whether the receiver and separator are strings is not known here;
//! the lowering takes the lane only when both are, so an entry for
//! any other call is inert.
//!
//! ## Shape
//!
//! Keyed by the separator argument's ExprId — the one handle
//! `ssa_lower_str_str_split::lower_split` holds for its call site.
//! A call counts only when both it and its separator are referenced
//! exactly once across the whole arena: a desugar that reuses a node
//! (compound assignment reuses its target) would otherwise carry the
//! count lane into a position that needs the array. Top-level
//! bindings are left alone — they are promoted to globals every
//! function body can reach — and so are declarations in a `switch`
//! case body, whose scope runs on into the following cases. A
//! `Stmt::Multi` shares the surrounding scope, so lists are flattened
//! through it before a declaration's window is taken.

use std::collections::{HashMap, HashSet};

use super::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use super::{Ast, Expr, ExprId, Stmt};

/// Record the separator ExprIds of the one-argument split calls whose
/// product is only ever measured. Runs at the end of the desugar
/// pipeline, after every pass that could add a use.
pub fn analyze_split_length_only(ast: &mut Ast) {
    let refs = reference_counts(ast);
    let not_read = members_not_read(ast);
    let mut found: HashSet<ExprId> = HashSet::new();
    for i in 0..ast.exprs.len() {
        let eid = ExprId(i as u32);
        if let Expr::Member { obj, name } = ast.get_expr(eid)
            && name == "length"
            && !not_read.contains(&eid)
            && let Some(sep) = one_arg_split_sep(ast, *obj)
        {
            accept(&refs, *obj, sep, &mut found);
        }
    }
    let stmts = ast.stmts.clone();
    for s in &stmts {
        walk_nested_lists(ast, s, &refs, &mut found);
    }
    ast.split_length_only_seps = found;
}

/// The nodes that sit where a member access is written, deleted or
/// called rather than read: `m = v`, `m++`, `delete m`, `m()`.
fn members_not_read(ast: &Ast) -> HashSet<ExprId> {
    let mut out = HashSet::new();
    for e in &ast.exprs {
        match e {
            Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => {
                out.insert(*target);
            }
            Expr::Delete { expr } => {
                out.insert(*expr);
            }
            Expr::Call { callee, .. } | Expr::OptCall { callee, .. } => {
                out.insert(*callee);
            }
            _ => {}
        }
    }
    out
}

fn accept(refs: &HashMap<ExprId, u32>, call: ExprId, sep: ExprId, found: &mut HashSet<ExprId>) {
    if refs.get(&call) == Some(&1) && refs.get(&sep) == Some(&1) {
        found.insert(sep);
    }
}

/// `R.split(S)` with exactly one non-spread argument: `S`.
fn one_arg_split_sep(ast: &Ast, call: ExprId) -> Option<ExprId> {
    let Expr::Call { callee, args } = ast.get_expr(call) else {
        return None;
    };
    let Expr::Member { name, .. } = ast.get_expr(*callee) else {
        return None;
    };
    match args.as_slice() {
        [sep] if name == "split" && !matches!(ast.get_expr(*sep), Expr::Spread { .. }) => {
            Some(*sep)
        }
        _ => None,
    }
}

/// How many parent edges point at each ExprId: every child of every
/// arena node (dead ones included — an over-count only rejects more)
/// plus every statement root, function and method bodies included.
fn reference_counts(ast: &Ast) -> HashMap<ExprId, u32> {
    let mut refs: HashMap<ExprId, u32> = HashMap::new();
    for i in 0..ast.exprs.len() {
        let eid = ExprId(i as u32);
        for c in expr_children(ast, eid) {
            *refs.entry(c).or_insert(0) += 1;
        }
        if let Expr::ArrowFn { body, .. } = ast.get_expr(eid) {
            for s in body {
                count_stmt_roots(s, &mut refs);
            }
        }
    }
    for s in &ast.stmts {
        count_stmt_roots(s, &mut refs);
    }
    refs
}

fn count_stmt_roots(s: &Stmt, refs: &mut HashMap<ExprId, u32>) {
    for e in stmt_exprs(s) {
        *refs.entry(e).or_insert(0) += 1;
    }
    for c in stmt_children_ref(s) {
        count_stmt_roots(c, refs);
    }
    for body in nested_bodies(s) {
        for b in body {
            count_stmt_roots(b, refs);
        }
    }
}

/// The statement lists a function or class declaration hides from
/// [`stmt_children_ref`] (it stops at a nested function on purpose).
fn nested_bodies(s: &Stmt) -> Vec<&Vec<Stmt>> {
    match s {
        Stmt::FnDecl { body, .. } => vec![body],
        Stmt::ClassDecl {
            methods,
            static_methods,
            ..
        } => methods
            .iter()
            .chain(static_methods)
            .map(|m| &m.body)
            .collect(),
        Stmt::ExportDecl {
            inner: Some(inner), ..
        } => nested_bodies(inner),
        _ => Vec::new(),
    }
}

/// Visit every statement list below `s` that is a scope of its own —
/// block bodies, loop bodies, try arms, function and method bodies —
/// and check the declarations in it. The top-level list itself is
/// never checked (its bindings are globals).
fn walk_nested_lists(
    ast: &Ast,
    s: &Stmt,
    refs: &HashMap<ExprId, u32>,
    found: &mut HashSet<ExprId>,
) {
    match s {
        Stmt::Block(list) => check_list(ast, list, refs, found),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            check_list(ast, body, refs, found);
            check_list(ast, catch_body, refs, found);
            if let Some(f) = finally_body {
                check_list(ast, f, refs, found);
            }
        }
        _ => {}
    }
    for body in nested_bodies(s) {
        check_list(ast, body, refs, found);
        for b in body {
            walk_nested_lists(ast, b, refs, found);
        }
    }
    for c in stmt_children_ref(s) {
        walk_nested_lists(ast, c, refs, found);
    }
}

/// `list` flattened through `Stmt::Multi`, which shares the scope it
/// sits in.
fn flatten(list: &[Stmt]) -> Vec<&Stmt> {
    let mut out = Vec::new();
    for s in list {
        match s {
            Stmt::Multi(inner) => out.extend(flatten(inner)),
            _ => out.push(s),
        }
    }
    out
}

fn check_list(ast: &Ast, list: &[Stmt], refs: &HashMap<ExprId, u32>, found: &mut HashSet<ExprId>) {
    let flat = flatten(list);
    // A function or class declaration anywhere in the list is hoisted
    // over the whole scope and can reach every binding in it.
    if flat
        .iter()
        .any(|s| matches!(s, Stmt::FnDecl { .. } | Stmt::ClassDecl { .. }))
    {
        return;
    }
    for (i, s) in flat.iter().enumerate() {
        if let Stmt::LetDecl {
            name, init, is_var, ..
        } = s
            && !*is_var
            && let Some(sep) = one_arg_split_sep(ast, *init)
            && flat[i + 1..]
                .iter()
                .all(|t| stmt_only_measures(ast, t, name))
        {
            accept(refs, *init, sep, found);
        }
    }
}

fn stmt_only_measures(ast: &Ast, s: &Stmt, x: &str) -> bool {
    match s {
        Stmt::FnDecl { .. } | Stmt::ClassDecl { .. } | Stmt::ExportDecl { .. } => false,
        Stmt::TypeDecl { .. } | Stmt::ImportDecl { .. } => true,
        _ => {
            stmt_exprs(s)
                .into_iter()
                .all(|e| expr_only_measures(ast, e, x))
                && stmt_children_ref(s)
                    .into_iter()
                    .all(|c| stmt_only_measures(ast, c, x))
        }
    }
}

fn is_x(ast: &Ast, e: ExprId, x: &str) -> bool {
    matches!(ast.get_expr(e), Expr::Ident(n) if n == x)
}

/// A member, index or optional access whose receiver is `x` itself.
fn through_x(ast: &Ast, e: ExprId, x: &str) -> bool {
    match ast.get_expr(e) {
        Expr::Member { obj, .. } | Expr::Index { obj, .. } | Expr::OptChain { obj, .. } => {
            is_x(ast, *obj, x)
        }
        _ => false,
    }
}

/// True when every appearance of `x` under `e` is `x.length` read as
/// a value.
fn expr_only_measures(ast: &Ast, e: ExprId, x: &str) -> bool {
    match ast.get_expr(e) {
        Expr::Ident(n) => n != x,
        Expr::Member { obj, name } if name == "length" && is_x(ast, *obj, x) => true,
        Expr::Assign { target, .. } if is_x(ast, *target, x) || through_x(ast, *target, x) => false,
        Expr::PostIncr { target, .. } | Expr::Delete { expr: target }
            if through_x(ast, *target, x) =>
        {
            false
        }
        Expr::Call { callee, .. } | Expr::OptCall { callee, .. } if through_x(ast, *callee, x) => {
            false
        }
        Expr::Closure { captures, .. } => !captures.iter().any(|c| c == x),
        Expr::ArrowFn { .. } => false,
        _ => expr_children(ast, e)
            .into_iter()
            .all(|c| expr_only_measures(ast, c, x)),
    }
}
