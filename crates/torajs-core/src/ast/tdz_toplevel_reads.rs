//! §8.1.1.1 step 4 — the temporal dead zone of a top-level `let` /
//! `const`, for the one shape a compiler can settle without tracking
//! initialization at run time: a read written EARLIER in the top-level
//! statement list than the declaration itself.
//!
//! ```text
//! console.log(a);   // ReferenceError: Cannot access 'a' before …
//! const a = 5;
//! ```
//!
//! tr answered `5`. The read resolves because two independent lanes
//! give a literal-init top-level `const` a home that carries no
//! position: the checker's `pass_2` registers every literal-init
//! top-level binding into `globals` before it walks a single
//! statement (so a named fn declared BEFORE the `const` can read it —
//! which is legal, and is why the registration exists), and the
//! lowerer's inline-literal lane (`ssa_lower_ident::
//! try_inline_const_literal`) scans the whole statement list for the
//! declaration and inlines its value. Neither asks where the read is.
//! The result was the worst shape available: a wrong ANSWER, silently,
//! where every other unresolvable read is loud.
//!
//! What makes this decidable at all is the boundary the walk stops at.
//! A read sitting directly in top-level statement `i` executes before
//! statement `j > i` runs — unconditionally, for every control shape a
//! statement list can hold, because the list is linear and nothing
//! re-enters an earlier statement. The mark is therefore not a claim
//! that the read HAPPENS (it may sit in a branch nobody takes); it is
//! a claim that IF it happens, the declaration has not run. That is
//! exactly what the emitted raise says, at exactly the read's own
//! site.
//!
//! A function body is where that reasoning stops: `function g() {
//! return a }` written before the `const` reads it whenever `g` is
//! CALLED, which is normally after. So the walk enters no fn body —
//! not a `Stmt::FnDecl`, not a class body, not an arrow / fn
//! expression (`expr_children` already stops at `ArrowFn`, and the
//! statement walker never had FnDecl or ClassDecl children). Calling
//! such a fn early (`g(); const a = 5`) is still wrong, and still
//! answers `5`; settling it needs a call graph, and is registered
//! rather than guessed at here.
//!
//! Two more narrowings, both to keep a false raise impossible:
//!
//!   - **Synthesized names never participate.** Late pipeline passes
//!     APPEND declarations (`synthesize_forwarders` extends
//!     `ast.stmts` with `__forward_*` lets whose readers were written
//!     earlier), so relative position stops meaning source position
//!     for those. Every such name is `__`-prefixed and user code
//!     cannot spell one, so the prefix is dropped where the
//!     declarations are collected — which is the only place a name
//!     can enter the wanted set, so no synthesized READ can be marked
//!     either.
//!   - **A name declared anywhere inside the reading statement is
//!     skipped.** `{ let a = 1; console.log(a) }` reads the inner
//!     binding, not the top-level one. Rather than track block scopes,
//!     the subtree is asked whether it binds the name at all; a true
//!     TDZ read that happens to share a statement with an unrelated
//!     inner binding of the same name goes unmarked, which is the safe
//!     direction.
//!
//! Write targets are left alone: `a = 1` before `const a` is the same
//! ReferenceError in ES, but it lands in the assign lane, which has
//! its own unresolvable-Reference kernel — a separate cut.

use std::collections::{HashMap, HashSet};

use super::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use super::{Ast, Expr, ExprId, Stmt};

/// Fill `ast.tdz_reads`. Runs last in the AST pipeline, on the tree
/// the lowerer will read, so no later rewrite can move a marked
/// occurrence out from under its mark.
pub fn mark_toplevel_tdz_reads(ast: &mut Ast) {
    let flat = super::toplevel_stmts_flat(ast);
    let mut decl_at: HashMap<&str, usize> = HashMap::new();
    for (i, s) in flat.iter().enumerate() {
        if let Stmt::LetDecl {
            name,
            is_var: false,
            ..
        } = s
            && !name.starts_with("__")
        {
            decl_at.entry(name.as_str()).or_insert(i);
        }
    }
    if decl_at.is_empty() {
        return;
    }
    let mut marks: HashSet<ExprId> = HashSet::new();
    for (i, s) in flat.iter().enumerate() {
        let later: Vec<&str> = decl_at
            .iter()
            .filter(|(_, j)| **j > i)
            .map(|(n, _)| *n)
            .collect();
        if later.is_empty() {
            continue;
        }
        let mut shadowed: HashSet<&str> = HashSet::new();
        collect_bound_names(s, &mut shadowed);
        let wanted: HashSet<&str> = later
            .into_iter()
            .filter(|n| !shadowed.contains(n))
            .collect();
        if wanted.is_empty() {
            continue;
        }
        mark_stmt(ast, s, &wanted, &mut marks);
    }
    ast.tdz_reads = marks;
}

/// Every name the statement's own subtree binds — the walk that
/// [`mark_stmt`] takes, asked the opposite question. Fn and class
/// bodies are not entered here either: a binding inside one cannot
/// shadow anything the walk will mark.
fn collect_bound_names<'a>(s: &'a Stmt, out: &mut HashSet<&'a str>) {
    match s {
        Stmt::LetDecl { name, .. } | Stmt::UsingDecl { name, .. } => {
            out.insert(name.as_str());
        }
        Stmt::FnDecl { name, .. } | Stmt::ClassDecl { name, .. } => {
            out.insert(name.as_str());
        }
        Stmt::ForOf { var_name, .. } | Stmt::ForOfSplitIter { var_name, .. } => {
            out.insert(var_name.as_str());
        }
        Stmt::Try { catch_param, .. } => {
            if let Some(p) = catch_param {
                out.insert(p.as_str());
            }
        }
        _ => {}
    }
    for c in stmt_children_ref(s) {
        collect_bound_names(c, out);
    }
}

fn mark_stmt(ast: &Ast, s: &Stmt, wanted: &HashSet<&str>, out: &mut HashSet<ExprId>) {
    for root in stmt_exprs(s) {
        mark_expr(ast, root, wanted, out);
    }
    for c in stmt_children_ref(s) {
        mark_stmt(ast, c, wanted, out);
    }
}

fn mark_expr(ast: &Ast, eid: ExprId, wanted: &HashSet<&str>, out: &mut HashSet<ExprId>) {
    if let Expr::Ident(name) = ast.get_expr(eid) {
        if wanted.contains(name.as_str()) {
            out.insert(eid);
        }
        return;
    }
    // The target of an assignment is a Reference, not a GetValue —
    // its own raise belongs to the assign lane.
    if let Expr::Assign { target, value } = ast.get_expr(eid) {
        if !matches!(ast.get_expr(*target), Expr::Ident(_)) {
            mark_expr(ast, *target, wanted, out);
        }
        mark_expr(ast, *value, wanted, out);
        return;
    }
    for c in expr_children(ast, eid) {
        mark_expr(ast, c, wanted, out);
    }
}
