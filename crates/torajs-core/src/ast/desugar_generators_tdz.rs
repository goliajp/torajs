//! The temporal dead zone of a generator / async machine body's own
//! `let` / `const` (RFC 20260915 registered item 2, §8.1.1.1 step 4).
//!
//! Inside a machine every `let` becomes a field of the iterator,
//! initialised the moment the object exists, so a read that reaches
//! it before the declaration ran answered the field's zero instead of
//! throwing: `await x; { const r = z; let z = 1 }` printed 0 where bun
//! refuses (rotation 645 p5 / p5b), and `function f() { return x + 1 }
//! f(); await using x = null` was the one shape blade 5 kept on the
//! synchronous lowering for. The final `mark_tdz_reads` pass cannot
//! help: it runs on the lowered tree, where the lifted field has
//! neither a declaration nor a position left. So the question is
//! asked HERE, on the raw body, before the let-lift — and settled in
//! two ways, by what can be known:
//!
//! 1. **Decidable reads** — a read written earlier in the same
//!    statement list than its declaration (the rule `tdz_reads`
//!    documents), the declaration's own initializer, and the reads a
//!    for-of head already marked at parse time. Reaching one means
//!    the declaration has not run, unconditionally, so the read is
//!    replaced IN PLACE by the raise itself:
//!    `(() : any => { throw __new_ReferenceError(...) })()`. A write
//!    target evaluates its right-hand side first, as §13.15.2 does.
//!
//! 2. **Closure reads** — a function value written BEFORE the
//!    declaration (`function f() { return x }` hoisted above `let x`,
//!    an arrow in an earlier statement, an arrow inside the
//!    declaration's own initializer) reads whenever it is CALLED,
//!    which no statement order settles. The binding gets a runtime
//!    initialised-flag, exactly the "uninitialized" marker an engine
//!    keeps in the binding slot, spelled as one more lifted local:
//!
//!    ```text
//!    let __tdz3_x: boolean = false;   // at the head of x's list
//!    …                                 // (re-entered each time the
//!    let x = init;                     //  list is: a loop body)
//!    __tdz3_x = true;
//!    ```
//!
//!    and each such read becomes `(__tdz3_x ? undefined : <raise>, x)`
//!    — a `Sequence` whose value is still `x`, so the read keeps its
//!    type; a write becomes `__tdz3_x ? (x = v) : (v, <raise>)`. The
//!    lift turns the flag into a field like any other local, and the
//!    frame link hands it to nested closures the way it hands them
//!    `x`. A closure written AFTER the declaration in its list never
//!    runs before that list's initialisation and is left alone — for
//!    the loop shape `for (;;) { if (g) g(); let x = 1; g = () => x }`
//!    the field's current value is the earlier iteration's binding,
//!    which is what the spec's per-iteration environment holds too.
//!
//! Shadowing keeps the safe direction throughout: a closure that
//! binds the name itself (a parameter, a `let` anywhere in its body)
//! is not entered at all, so a false raise is impossible and a dead
//! zone read sharing a closure with an unrelated inner binding of the
//! same name stays as it is today.

use std::collections::HashSet;

use super::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use super::dflt_param_tdz::tdz_throw_iife;
use super::{Ast, Expr, ExprId, Stmt};

/// Entry — settle the dead zone of every binding `body` declares,
/// before the let-lift takes their declarations away.
pub(super) fn rewrite_tdz(ast: &mut Ast, body: &mut Vec<Stmt>) {
    let mut marks: HashSet<ExprId> = HashSet::new();
    super::tdz_reads::run_nested(ast, body, &mut marks);
    for s in body.iter() {
        collect_parser_marks(ast, s, &mut marks);
    }
    if !marks.is_empty() {
        for s in body.iter() {
            rewrite_decided_stmt(ast, s, &marks);
        }
    }
    super::desugar_generators_tdz_flag::flag_closure_reads(ast, body);
}

/// The reads the parser marked (`forof_binding::mark_forof_head_tdz`:
/// `for (let x of [x])`) that sit in this body — a lifted `this.x` is
/// no longer an `Ident`, so the lowerer's mark lookup would miss them.
fn collect_parser_marks(ast: &Ast, s: &Stmt, out: &mut HashSet<ExprId>) {
    if let Stmt::FnDecl { body, .. } = s {
        for b in body {
            collect_parser_marks(ast, b, out);
        }
        return;
    }
    for root in stmt_exprs(s) {
        collect_parser_marks_expr(ast, root, out);
    }
    for c in stmt_children_ref(s) {
        collect_parser_marks(ast, c, out);
    }
}

fn collect_parser_marks_expr(ast: &Ast, eid: ExprId, out: &mut HashSet<ExprId>) {
    match ast.get_expr(eid) {
        Expr::Ident(_) => {
            if ast.tdz_reads.contains(&eid) {
                out.insert(eid);
            }
        }
        Expr::ArrowFn { body, .. } => {
            for b in body {
                collect_parser_marks(ast, b, out);
            }
        }
        _ => {
            for c in expr_children(ast, eid) {
                collect_parser_marks_expr(ast, c, out);
            }
        }
    }
}

fn rewrite_decided_stmt(ast: &mut Ast, s: &Stmt, marks: &HashSet<ExprId>) {
    if let Stmt::FnDecl { body, .. } = s {
        for b in body {
            rewrite_decided_stmt(ast, b, marks);
        }
        return;
    }
    for root in stmt_exprs(s) {
        rewrite_decided_expr(ast, root, marks);
    }
    for c in stmt_children_ref(s) {
        rewrite_decided_stmt(ast, c, marks);
    }
}

/// What a marked occurrence is the operand of — the parent decides
/// which node the raise replaces.
enum Decided {
    Read,
    Write { target: ExprId, value: ExprId },
    Incr { target: ExprId },
    Arrow(Vec<Stmt>),
    Other,
}

fn rewrite_decided_expr(ast: &mut Ast, eid: ExprId, marks: &HashSet<ExprId>) {
    let shape = match ast.get_expr(eid) {
        Expr::Ident(_) if marks.contains(&eid) => Decided::Read,
        Expr::Assign { target, value } if marks.contains(target) => Decided::Write {
            target: *target,
            value: *value,
        },
        Expr::PostIncr { target, .. } if marks.contains(target) => {
            Decided::Incr { target: *target }
        }
        Expr::ArrowFn { body, .. } => Decided::Arrow(body.clone()),
        _ => Decided::Other,
    };
    match shape {
        Decided::Read => {
            let name = ident_name(ast, eid);
            let call = tdz_throw_iife(ast, &name, None);
            ast.exprs[eid.0 as usize] = call;
        }
        Decided::Write { target, value } => {
            rewrite_decided_expr(ast, value, marks);
            let name = ident_name(ast, target);
            let call = tdz_throw_iife(ast, &name, Some(value));
            ast.exprs[eid.0 as usize] = call;
        }
        Decided::Incr { target } => {
            let name = ident_name(ast, target);
            let call = tdz_throw_iife(ast, &name, None);
            ast.exprs[eid.0 as usize] = call;
        }
        Decided::Arrow(body) => {
            for b in &body {
                rewrite_decided_stmt(ast, b, marks);
            }
        }
        Decided::Other => {
            for c in expr_children(ast, eid) {
                rewrite_decided_expr(ast, c, marks);
            }
        }
    }
}

fn ident_name(ast: &Ast, eid: ExprId) -> String {
    match ast.get_expr(eid) {
        Expr::Ident(n) => n.clone(),
        other => unreachable!("tdz mark on a non-identifier: {other:?}"),
    }
}
