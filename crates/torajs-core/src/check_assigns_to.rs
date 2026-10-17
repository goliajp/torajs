//! Where a guarded region writes the guarded reference.
//!
//! A null guard narrows a binding (`q !== null`) or a member path
//! (`t.next !== null`) for the statement it guards. The checker walks
//! that statement once, in execution order, so a write inside it can
//! retire the narrow at the write itself (`q = q.next` puts `q` back to
//! its declared `T | null` for the rest of the walk) — exactly the
//! control-flow narrowing TS performs — as long as no use the walk
//! already typed under the narrow can run again after the write
//! without passing the guard first.
//!
//! Two shapes break that: a write inside a loop nested in the region
//! (the loop's next iteration re-runs uses typed before the write) and
//! a function body in the region that mentions the reference's root
//! binding (its uses run whenever it is called, not where the walk
//! types them; a declaration or class body counts whatever it
//! mentions). [`scan`] tells the three cases apart.
//!
//! Member narrows are keyed by the canonical source path of their
//! receiver ([`member_path`]); a write invalidates every narrow whose
//! key it overlaps ([`path_overlaps`]).

use crate::ast::desugar_async_sm_walk::{visit_expr_children, visit_stmt_exprs};
use crate::ast::{Ast, Expr, ExprId, Stmt};

/// Chunk 789 — canonical source path for a member-narrow receiver:
/// an Ident (`h`), a Member chain (`h.o.p`), or an integer-literal
/// Index (`arr[0]` — chunk 790; a computed index is not stable
/// across re-evaluation). `None` for every other shape, which stays
/// un-narrowed (loud).
pub(crate) fn member_path(ast: &Ast, eid: ExprId) -> Option<String> {
    match ast.get_expr(eid) {
        Expr::Ident(n) => Some(n.clone()),
        Expr::Member { obj, name } => Some(format!("{}.{name}", member_path(ast, *obj)?)),
        Expr::Index { obj, index } => match ast.get_expr(*index) {
            Expr::Number(v) if v.fract() == 0.0 && *v >= 0.0 => {
                Some(format!("{}[{}]", member_path(ast, *obj)?, *v as u64))
            }
            _ => None,
        },
        _ => None,
    }
}

/// Chunk 789 — is the narrow keyed at `key_path` rooted at (or equal
/// to) the assigned path? `h = ...` invalidates "h.o"-rooted keys;
/// `h.o = ...` invalidates "h.o" itself and deeper "h.o.p" roots.
pub(crate) fn path_overlaps(key_path: &str, assigned: &str) -> bool {
    key_path == assigned
        || key_path.starts_with(&format!("{assigned}."))
        || key_path.starts_with(&format!("{assigned}["))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Region {
    /// nothing in the region writes the reference
    NoWrite,
    /// every write is one the single walk sees before any use it
    /// could invalidate
    StraightLine,
    /// a write inside a nested loop, or a function body mentioning the
    /// root in a region that writes
    Unsafe,
}

#[derive(Default)]
struct Scan {
    write: bool,
    nested_write: bool,
    has_fn: bool,
}

/// The reference a guard narrows: `root` is the binding the path
/// starts at, `hit` matches a write's target expression.
pub(crate) struct Guarded<'a> {
    pub(crate) root: &'a str,
    pub(crate) hit: &'a dyn Fn(ExprId) -> bool,
}

/// Classify the writes to `g` in the guarded statement `s`.
pub(crate) fn scan(ast: &Ast, s: &Stmt, g: &Guarded) -> Region {
    let mut out = Scan::default();
    visit_stmt_exprs(s, &mut |e| scan_expr(ast, e, g, &mut out));
    let top_write = out.write;
    out.write = false;
    scan_stmts(ast, s, false, g, &mut out);
    match (top_write, out.nested_write || out.has_fn) {
        (false, _) => Region::NoWrite,
        (true, false) => Region::StraightLine,
        (true, true) => Region::Unsafe,
    }
}

/// Does evaluating `exprs` or `s` write what `hit` matches?
pub(crate) fn writes(ast: &Ast, exprs: &[ExprId], s: &Stmt, hit: &dyn Fn(ExprId) -> bool) -> bool {
    let g = Guarded { root: "", hit };
    let mut out = Scan::default();
    for e in exprs {
        scan_expr(ast, *e, &g, &mut out);
    }
    visit_stmt_exprs(s, &mut |e| scan_expr(ast, e, &g, &mut out));
    out.write
}

fn scan_expr(ast: &Ast, e: ExprId, g: &Guarded, out: &mut Scan) {
    match ast.get_expr(e) {
        Expr::Assign { target, .. } | Expr::PostIncr { target, .. } if (g.hit)(*target) => {
            out.write = true;
        }
        Expr::Closure { captures, .. } if captures.iter().any(|c| c == g.root) => {
            out.has_fn = true;
        }
        // a write in an arrow body is a write of the region
        Expr::ArrowFn { body, .. } => {
            if body.iter().any(|s| stmt_mentions(ast, s, g.root)) {
                out.has_fn = true;
            }
            for s in body {
                visit_stmt_exprs(s, &mut |e| scan_expr(ast, e, g, out));
            }
        }
        _ => {}
    }
    visit_expr_children(ast, e, &mut |c| scan_expr(ast, c, g, out));
}

fn stmt_mentions(ast: &Ast, s: &Stmt, root: &str) -> bool {
    let mut hit = false;
    visit_stmt_exprs(s, &mut |e| hit |= expr_mentions(ast, e, root));
    hit
}

fn expr_mentions(ast: &Ast, e: ExprId, root: &str) -> bool {
    let own = match ast.get_expr(e) {
        Expr::Ident(n) => n == root,
        Expr::Closure { captures, .. } => captures.iter().any(|c| c == root),
        Expr::ArrowFn { body, .. } => body.iter().any(|s| stmt_mentions(ast, s, root)),
        _ => false,
    };
    let mut hit = own;
    visit_expr_children(ast, e, &mut |c| hit |= expr_mentions(ast, c, root));
    hit
}

/// Statement-level pass: declarations with bodies, and writes inside
/// nested loops.
fn scan_stmts(ast: &Ast, s: &Stmt, in_loop: bool, g: &Guarded, out: &mut Scan) {
    let is_loop = matches!(
        s,
        Stmt::While { .. }
            | Stmt::DoWhile { .. }
            | Stmt::For { .. }
            | Stmt::ForOf { .. }
            | Stmt::ForOfSplitIter { .. }
    );
    if is_loop && !in_loop {
        let mut inner = Scan::default();
        visit_stmt_exprs(s, &mut |e| scan_expr(ast, e, g, &mut inner));
        out.nested_write |= inner.write;
    }
    let in_loop = in_loop || is_loop;
    let mut each = |b: &Stmt| scan_stmts(ast, b, in_loop, g, out);
    match s {
        Stmt::FnDecl { .. } | Stmt::ClassDecl { .. } => out.has_fn = true,
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            each(then_branch);
            if let Some(b) = else_branch {
                each(b);
            }
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::ForOfSplitIter { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::Labeled { body, .. } => each(body),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                each(i);
            }
            each(body);
        }
        Stmt::Switch { cases, default, .. } => {
            cases.iter().flat_map(|c| &c.body).for_each(&mut each);
            default.iter().flatten().for_each(&mut each);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => stmts.iter().for_each(&mut each),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            body.iter().for_each(&mut each);
            catch_body.iter().for_each(&mut each);
            finally_body.iter().flatten().for_each(&mut each);
        }
        Stmt::ExportDecl { inner, .. } => {
            if let Some(i) = inner {
                each(i);
            }
        }
        Stmt::Expr(_)
        | Stmt::LetDecl { .. }
        | Stmt::UsingDecl { .. }
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::Throw(_)
        | Stmt::TypeDecl { .. }
        | Stmt::Return(_)
        | Stmt::Yield(_)
        | Stmt::YieldInto { .. }
        | Stmt::ImportDecl { .. } => {}
    }
}

/// `name = …` / `name++`.
pub(crate) fn binding_hit<'a>(ast: &'a Ast, name: &'a str) -> impl Fn(ExprId) -> bool + 'a {
    move |t| matches!(ast.get_expr(t), Expr::Ident(n) if n == name)
}

/// A write whose target path overlaps the receiver or the member
/// itself (`t = …`, `t.next = …`, `t.next.v = …` does not).
pub(crate) fn member_hit<'a>(
    ast: &'a Ast,
    recv: &'a str,
    field: &'a str,
) -> impl Fn(ExprId) -> bool + 'a {
    let full = format!("{recv}.{field}");
    move |t| {
        member_path(ast, t).is_some_and(|tp| path_overlaps(recv, &tp) || path_overlaps(&full, &tp))
    }
}

#[cfg(test)]
mod tests {
    // a recursive object type: the unit pipeline runs no class desugar
    // (the class-receiver path is covered by the conformance fixtures)
    const NODE: &str = "type Node = { v: number; next: Node | null };\n\
        function g(n: Node): number { return n.v; }\n\
        const a: Node = { v: 1, next: { v: 2, next: null } };\n";

    fn check_src(body: &str) -> Result<(), String> {
        let src = format!("{NODE}{body}\n");
        let tokens = crate::lexer::tokenize(&src).map_err(|e| format!("lex: {e}"))?;
        let mut ast = crate::parser::parse(&src, &tokens).map_err(|e| format!("parse: {e}"))?;
        crate::ast::lift_arrow_fns(&mut ast);
        crate::check::check(&ast).map(|_| ())
    }

    fn accepted(body: &str) {
        assert!(check_src(body).is_ok(), "{body}: {:?}", check_src(body));
    }

    fn refused(body: &str) {
        assert!(check_src(body).is_err(), "{body}: must be refused");
    }

    #[test]
    fn a_loop_guard_survives_a_write_it_sees_first() {
        accepted("let t = a; while (t.next !== null) t = t.next;");
        accepted(
            "let t: Node = a; let q: Node | null = a.next; \
             while (q !== null) { t = q; q = q.next; }",
        );
        accepted(
            "let q: Node | null = a; let s = 0; while (q !== null) \
             { s += g(q); q = q.next; if (q === null) break; s += g(q); }",
        );
        accepted(
            "let q: Node | null = a; while (q !== null) \
             { if (q.v === 2) { q = q.next; continue; } g(q); q = q.next; }",
        );
    }

    #[test]
    fn a_write_retires_the_narrow_where_it_happens() {
        refused("let q: Node | null = a; if (q !== null) { q = q.next; g(q); }");
        refused(
            "let q: Node | null = a; if (q !== null) { q = q.next; } \
             else { throw 1; } g(q);",
        );
        refused(
            "let q: Node | null = a; let k = 0; while (q !== null) \
             { if (k > 0) { q = q.next; } g(q); k++; }",
        );
        refused("const t = a; while (t.next !== null) { t.next = t.next.next; g(t.next); }");
    }

    #[test]
    fn a_write_a_use_can_outrun_keeps_the_old_refusal() {
        refused(
            "let q: Node | null = a; let k = 0; while (q !== null) \
             { while (k < 3) { g(q); q = q.next; k++; } }",
        );
        refused(
            "let t = a; let k = 0; while (t.next !== null) \
             { while (k < 2) { g(t.next); t = t.next; k++; } }",
        );
        refused(
            "let t = a; let k = 0; if (t.next !== null) \
             { while (k < 2) { g(t.next); t = t.next; k++; } }",
        );
        refused(
            "let q: Node | null = a.next; if (q !== null) { const f = () => g(q); q = q.next; f(); }",
        );
    }

    #[test]
    fn an_exit_or_a_fill_narrows_what_follows() {
        accepted(
            "function h(x: Node | null): number { if (x === null || x.v < 2) return -1; return g(x); }",
        );
        accepted(
            "function h(x: Node | null): number { if (x === null) x = { v: 9, next: null }; return g(x); }",
        );
        refused(
            "function h(x: Node | null, k: boolean): number \
             { if (x === null && k) return -1; return g(x); }",
        );
        refused(
            "function h(x: Node | null, k: boolean): number \
             { if (k) x = { v: 9, next: null }; return g(x); }",
        );
    }

    #[test]
    fn a_filled_binding_can_still_be_written_but_not_ahead_of_a_loop() {
        accepted(
            "function h(x: Node | null): number \
             { if (x === null) x = { v: 9, next: null }; const r = g(x); x = null; return r; }",
        );
        refused(
            "function h(x: Node | null, k: number): number \
             { if (x === null) x = { v: 9, next: null }; while (k > 0) { g(x); x = x.next; k--; } return 0; }",
        );
        accepted(
            "function h(x: Node | null, k: number): number \
             { if (x === null) x = { v: 9, next: null }; while (k > 0) { g(x); k--; } return 0; }",
        );
    }
}
