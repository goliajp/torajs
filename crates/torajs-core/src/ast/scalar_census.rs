//! The walk shared by the censuses that replace a non-escaping
//! aggregate by a scalar: `split_length_only` (a split product only
//! ever measured is its token count), `regex_match_len_only` (a
//! match result only ever null-tested or asked `m[0].length` is that
//! length) and `regex_exec_record` (an exec result read through a
//! few constant indices is its spans). Each lane supplies two answers
//! — which call produces its aggregate, and which uses of a bound one
//! are its own — and this module owns everything else: the reference
//! counts, the scope rules and the uses every lane refuses.
//!
//! ## Scope rules
//!
//! A call counts only when both it and its key are referenced exactly
//! once across the whole arena: a desugar that reuses a node (compound
//! assignment reuses its target) would otherwise carry the lane into a
//! position that needs the aggregate. Top-level bindings are left
//! alone — they are promoted to globals every function body can reach
//! — and so are declarations in a `switch` case body, whose scope runs
//! on into the following cases. A `Stmt::Multi` shares the surrounding
//! scope, so lists are flattened through it before a declaration's
//! window is taken. A function or class declaration anywhere in a list
//! is hoisted over the whole scope and disqualifies every binding in
//! it; so does a lifted closure that captured the binding, or an arrow.

use std::collections::{HashMap, HashSet};

use super::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use super::{Ast, Expr, ExprId, Stmt};

/// One scalar-replacement lane.
pub(super) trait Lane {
    /// `(call, key)` when `init` is this lane's producing call; `key`
    /// is the handle the lowering holds for the call site.
    fn produced(&self, ast: &Ast, init: ExprId) -> Option<(ExprId, ExprId)>;
    /// `Some(ok)` when `e` is a use of the binding `x` this lane rules
    /// on; `None` hands `e` to the shared walk.
    fn leaf(&self, ast: &Ast, e: ExprId, x: &str, census: &Census) -> Option<bool>;
    /// Whether a bare `x` as an `if` / loop condition is this lane's.
    fn bare_condition_ok(&self) -> bool {
        false
    }
    /// Record a binding whose every use in `window` (the statements
    /// after its declaration) the lane owns. The default records the
    /// producing call's key; a lane whose binding is re-assigned, or
    /// that needs a fact from the whole window, answers here.
    // the walk's whole state, handed once; a struct for it would be
    // built and taken apart at this one call
    #[allow(clippy::too_many_arguments)]
    fn accept(
        &self,
        _ast: &Ast,
        _name: &str,
        _init: ExprId,
        call: ExprId,
        key: ExprId,
        _window: &[&Stmt],
        census: &Census,
        found: &mut HashSet<ExprId>,
    ) {
        census.accept(call, key, found);
    }
}

/// The arena facts every lane's decision reads.
pub(super) struct Census {
    refs: HashMap<ExprId, u32>,
    /// Nodes that sit where a member access is written, deleted or
    /// called rather than read: `m = v`, `m++`, `delete m`, `m()`.
    pub(super) not_read: HashSet<ExprId>,
}

impl Census {
    pub(super) fn new(ast: &Ast) -> Self {
        Self {
            refs: reference_counts(ast),
            not_read: members_not_read(ast),
        }
    }

    /// Record `key` when `call` and `key` each have exactly one parent.
    pub(super) fn accept(&self, call: ExprId, key: ExprId, found: &mut HashSet<ExprId>) {
        if self.single_use(call, key) {
            found.insert(key);
        }
    }

    /// `call` and `key` each have exactly one parent.
    pub(super) fn single_use(&self, call: ExprId, key: ExprId) -> bool {
        self.refs.get(&call) == Some(&1) && self.refs.get(&key) == Some(&1)
    }
}

/// Check every non-top-level declaration in the program against `lane`.
pub(super) fn census_bindings(ast: &Ast, lane: &dyn Lane, census: &Census) -> HashSet<ExprId> {
    let mut found = HashSet::new();
    for s in &ast.stmts {
        walk_nested_lists(ast, s, lane, census, &mut found);
    }
    found
}

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
    lane: &dyn Lane,
    census: &Census,
    found: &mut HashSet<ExprId>,
) {
    match s {
        Stmt::Block(list) => check_list(ast, list, lane, census, found),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            check_list(ast, body, lane, census, found);
            check_list(ast, catch_body, lane, census, found);
            if let Some(f) = finally_body {
                check_list(ast, f, lane, census, found);
            }
        }
        _ => {}
    }
    for body in nested_bodies(s) {
        check_list(ast, body, lane, census, found);
        for b in body {
            walk_nested_lists(ast, b, lane, census, found);
        }
    }
    for c in stmt_children_ref(s) {
        walk_nested_lists(ast, c, lane, census, found);
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

fn check_list(
    ast: &Ast,
    list: &[Stmt],
    lane: &dyn Lane,
    census: &Census,
    found: &mut HashSet<ExprId>,
) {
    let flat = flatten(list);
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
            && let Some((call, key)) = lane.produced(ast, *init)
            && flat[i + 1..]
                .iter()
                .all(|t| stmt_uses_ok(ast, t, name, lane, census))
        {
            lane.accept(ast, name, *init, call, key, &flat[i + 1..], census, found);
        }
    }
}

/// The condition of `s` when it is a bare `x` and the lane takes that.
fn bare_condition(ast: &Ast, s: &Stmt, x: &str, lane: &dyn Lane) -> Option<ExprId> {
    let cond = match s {
        Stmt::If { cond, .. } | Stmt::While { cond, .. } | Stmt::DoWhile { cond, .. } => *cond,
        Stmt::For { cond: Some(c), .. } => *c,
        _ => return None,
    };
    (lane.bare_condition_ok() && is_x(ast, cond, x)).then_some(cond)
}

fn stmt_uses_ok(ast: &Ast, s: &Stmt, x: &str, lane: &dyn Lane, census: &Census) -> bool {
    match s {
        Stmt::FnDecl { .. } | Stmt::ClassDecl { .. } | Stmt::ExportDecl { .. } => false,
        Stmt::TypeDecl { .. } | Stmt::ImportDecl { .. } => true,
        _ => {
            let cond = bare_condition(ast, s, x, lane);
            stmt_exprs(s)
                .into_iter()
                .all(|e| Some(e) == cond || expr_uses_ok(ast, e, x, lane, census))
                && stmt_children_ref(s)
                    .into_iter()
                    .all(|c| stmt_uses_ok(ast, c, x, lane, census))
        }
    }
}

pub(super) fn is_x(ast: &Ast, e: ExprId, x: &str) -> bool {
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

/// True when every appearance of `x` under `e` is a use `lane` owns.
pub(super) fn expr_uses_ok(
    ast: &Ast,
    e: ExprId,
    x: &str,
    lane: &dyn Lane,
    census: &Census,
) -> bool {
    if let Some(ok) = lane.leaf(ast, e, x, census) {
        return ok;
    }
    match ast.get_expr(e) {
        Expr::Ident(n) => n != x,
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
            .all(|c| expr_uses_ok(ast, c, x, lane, census)),
    }
}
