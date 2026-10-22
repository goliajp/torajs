//! The loop counters of the `then` closure proof: the reads of a
//! `for (let i = …; …; …)` binding that hold a number at every
//! evaluation, whatever any type says. A counter qualifies when its
//! initializer and every value written to it are numeric by their form
//! (a numeric literal, `-x`, `x - y`, `x++`, `i + 1` of numeric operands,
//! ECMA-262 §13.5 / §13.15.3: those operators answer a Number, a BigInt
//! or a Boolean of any operands), no destructuring writes it and no
//! lifted closure captures it, so no string ever reaches it. A
//! `for (… of …)` loop's own element read is keyed by the counter the
//! lowering drives, a number too.

use std::cell::OnceCell;
use std::collections::HashMap;

use crate::ast::desugar_with::walk::{arrow_nodes, stmt_children_ref};
use crate::ast::free_vars_write::unbound_sites;
use crate::ast::{Ast, BinOp, Expr, ExprId, Stmt};
use crate::ssa::id_hash::HashSet;

/// What a write puts into the binding it targets.
enum Write {
    /// A plain assignment of this value.
    Value(ExprId),
    /// `++` / `--`: ToNumeric's answer.
    Numeric,
    /// A destructuring element: whatever the source holds.
    Unknown,
}

/// The counter sites of one program, found the first time a computed
/// key spelled as a name asks: most programs the proof opens or closes
/// never do.
pub(super) struct Counters<'a> {
    ast: &'a Ast,
    sites: OnceCell<HashSet<ExprId>>,
}

impl<'a> Counters<'a> {
    pub(super) fn new(ast: &'a Ast) -> Self {
        Counters {
            ast,
            sites: OnceCell::new(),
        }
    }

    /// Does `id` read a loop counter?
    pub(super) fn contains(&self, id: ExprId) -> bool {
        self.sites
            .get_or_init(|| counter_sites(self.ast))
            .contains(&id)
    }
}

/// The `Ident` sites that read a loop counter, and the counter keys of
/// the `for … of` element reads.
fn counter_sites(ast: &Ast) -> HashSet<ExprId> {
    let writes = writes(ast);
    let mut out = HashSet::default();
    for s in &ast.stmts {
        visit(ast, s, &writes, &mut out);
    }
    out
}

fn visit(ast: &Ast, s: &Stmt, writes: &HashMap<ExprId, Write>, out: &mut HashSet<ExprId>) {
    match s {
        Stmt::For { .. } => out.extend(counter_reads(ast, s, writes).unwrap_or_default()),
        Stmt::ForOf {
            i_ident, elem_expr, ..
        } => {
            if let Expr::Index { index, .. } = ast.get_expr(*elem_expr)
                && matches!(ast.get_expr(*index), Expr::Ident(n) if n == i_ident)
            {
                out.insert(*index);
            }
        }
        Stmt::FnDecl { body, .. } => body.iter().for_each(|c| visit(ast, c, writes, out)),
        _ => {}
    }
    for c in stmt_children_ref(s) {
        visit(ast, c, writes, out);
    }
    for a in arrow_nodes(ast, s) {
        if let Expr::ArrowFn { body, .. } = ast.get_expr(a) {
            body.iter().for_each(|c| visit(ast, c, writes, out));
        }
    }
}

/// The reads of the counter a `for` head declares, when it is one.
fn counter_reads(ast: &Ast, s: &Stmt, writes: &HashMap<ExprId, Write>) -> Option<Vec<ExprId>> {
    let Stmt::For {
        init: Some(init),
        cond,
        step,
        body,
    } = s
    else {
        return None;
    };
    let Stmt::LetDecl {
        name,
        init,
        is_var: false,
        ..
    } = init.as_ref()
    else {
        return None;
    };
    if !numeric(ast, *init, None) {
        return None;
    }
    let head: Vec<Stmt> = cond
        .iter()
        .chain(step.iter())
        .map(|e| Stmt::Expr(*e))
        .collect();
    let parts: Vec<&Stmt> = head.iter().chain(std::iter::once(body.as_ref())).collect();
    let sites = unbound_sites(ast, &parts, name)?;
    let numeric_write = |site: &ExprId| match writes.get(site) {
        None | Some(Write::Numeric) => true,
        Some(Write::Value(v)) => numeric(ast, *v, Some(name)),
        Some(Write::Unknown) => false,
    };
    let idents = (sites.iter()).all(|e| matches!(ast.get_expr(*e), Expr::Ident(_)));
    (idents && sites.iter().all(numeric_write)).then_some(sites)
}

/// Is `e`'s value a Number, a BigInt, a Boolean or null by its form,
/// reading `counter` as a number?
fn numeric(ast: &Ast, e: ExprId, counter: Option<&str>) -> bool {
    let num = |e: &ExprId| numeric(ast, *e, counter);
    match ast.get_expr(e) {
        Expr::Number(_) | Expr::BigInt { .. } | Expr::Bool(_) | Expr::Null => true,
        Expr::Unary { .. } | Expr::PostIncr { .. } => true,
        Expr::BinOp {
            op: BinOp::Add | BinOp::LAnd | BinOp::LOr,
            left,
            right,
        }
        | Expr::Nullish {
            lhs: left,
            rhs: right,
        }
        | Expr::Ternary {
            then_branch: left,
            else_branch: right,
            ..
        } => num(left) && num(right),
        Expr::BinOp { .. } => true,
        Expr::Sequence { right, .. } => num(right),
        Expr::Ident(n) => counter == Some(n.as_str()),
        _ => false,
    }
}

/// Every write of a name in the program, keyed by the `Ident` it
/// targets: a plain assignment's value, an update, or a destructuring
/// element (an assignment pattern's leaves, defaults included).
fn writes(ast: &Ast) -> HashMap<ExprId, Write> {
    let mut out = HashMap::new();
    for e in &ast.exprs {
        match e {
            Expr::Assign { target, value } => {
                let t = strip_as(ast, *target);
                if matches!(ast.get_expr(t), Expr::Ident(_)) {
                    out.insert(t, Write::Value(*value));
                }
            }
            Expr::PostIncr { target, .. } => {
                out.insert(strip_as(ast, *target), Write::Numeric);
            }
            _ => {}
        }
    }
    // a pattern's leaf answers the source, not the default it spells, so
    // this pass runs last and overrides the default's own assignment
    for e in &ast.exprs {
        if let Expr::Assign { target, .. } = e
            && !matches!(ast.get_expr(strip_as(ast, *target)), Expr::Ident(_))
        {
            pattern_leaves(ast, *target, &mut out);
        }
    }
    out
}

/// The names an assignment pattern writes. A member or index target is
/// a leaf whose own operands are reads.
fn pattern_leaves(ast: &Ast, target: ExprId, out: &mut HashMap<ExprId, Write>) {
    match ast.get_expr(target) {
        Expr::Ident(_) => {
            out.insert(target, Write::Unknown);
        }
        Expr::Array(items) => items.iter().for_each(|i| pattern_leaves(ast, *i, out)),
        Expr::ObjectLit { fields } => fields
            .iter()
            .for_each(|(_, v)| pattern_leaves(ast, *v, out)),
        Expr::Spread { expr } | Expr::As { expr, .. } => pattern_leaves(ast, *expr, out),
        Expr::Assign { target, .. } => pattern_leaves(ast, *target, out),
        _ => {}
    }
}

fn strip_as(ast: &Ast, mut e: ExprId) -> ExprId {
    while let Expr::As { expr, .. } = ast.get_expr(e) {
        e = *expr;
    }
    e
}
