//! The lower half of the bounds proof.
//!
//! `ssa_lower_bounds_proven` recognizes `i < xs.length`, which settles
//! the UPPER bound of `xs[i]` and says nothing whatever about the
//! lower one — rotation 535 shipped a fixture where `i` walks up from
//! −2 under exactly that guard and reads before the data pointer.
//!
//! For the elision that proof licenses (drop the `>= len` compare and
//! its length load) the upper half is the whole story: the negative
//! compare stays and answers `undefined`. For the ELEMENT WIDTH it is
//! not. `container_walk::seed_index_read_elem` widens a `number`
//! element to F64 because an index read can go out of bounds and owe
//! `undefined`, which an I64 slot has no bit pattern for; a read may
//! stop asking for that widening only when it can never go out of
//! bounds at all — both ends.
//!
//! So this module answers the second half: which loop guards run a
//! counter that is provably a non-negative integer at every guarded
//! read. Three things have to hold, and the third is the one that is
//! easy to miss:
//!
//! 1. the counter's declaration initializes it to a non-negative
//!    integer literal (tracked per statement sequence, so an inner
//!    shadow with a negative start is seen as itself);
//! 2. every write to it inside the loop is `i = i + <non-negative
//!    integer>` or `i++` — the guard re-establishes the upper bound
//!    on every iteration, but nothing re-establishes the lower one,
//!    so it rides on induction over the steps;
//! 3. no write to it can happen anywhere the induction cannot see. A
//!    call in the body reaching a function that assigns the same
//!    global, or a closure that captured it, would drive it negative
//!    behind the induction's back.
//!
//! The third is answered two ways. A `for (let i = …; …)` counter is
//! bound BY the loop, so nothing outside it can name that binding and
//! the induction already sees every write there is. Otherwise — a
//! `while` over a counter declared before it — the loop has to hold
//! every write to that NAME in the module, which is coarse (a sibling
//! loop's own `let i` blocks it) but sound. That census reads the
//! whole expression arena rather than walking, because a walk would
//! have to know every statement and expression shape to be complete,
//! and being incomplete here is silent-wrong.
//!
//! Miss any of them and the guard is simply not settled: the element
//! keeps its F64 seed and the program keeps answering `undefined`.
//! The direction is one-sided — a missed settlement costs an
//! optimization, never an answer.

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, BinOp, Expr, ExprId, Stmt, UnaryOp};
use crate::ssa_lower_bounds_proven::{guard_pair, stmt_taints};

/// Bindings known to hold a non-negative integer at the current point.
type Env = HashMap<String, bool>;

/// A name no source program can spell, so `stmt_taints` answers about
/// the index half of a pair alone.
const NO_ARRAY: &str = "\u{0}";

/// The census of §3: every `Ident`-targeted write in the module by
/// name, plus the names some closure captured.
struct Writes {
    sites: HashMap<String, HashSet<ExprId>>,
    captured: HashSet<String>,
}

/// Loop guard conditions (by `ExprId`) whose counter is provably a
/// non-negative integer at every read the guard dominates.
pub(super) fn settled_guards(ast: &Ast) -> HashSet<ExprId> {
    let w = census(ast);
    let mut out = HashSet::new();
    let mut env = Env::new();
    scan_seq(ast, &ast.stmts, &mut env, &w, &mut out);
    out
}

/// Read the expression arena directly. A walk would have to know
/// every statement and expression shape to be complete, and being
/// incomplete here is silent-wrong, not a lost optimization.
fn census(ast: &Ast) -> Writes {
    let mut sites: HashMap<String, HashSet<ExprId>> = HashMap::new();
    let mut captured: HashSet<String> = HashSet::new();
    for (i, e) in ast.exprs.iter().enumerate() {
        let eid = ExprId(i as u32);
        match e {
            Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => {
                if let Expr::Ident(n) = ast.get_expr(*target) {
                    sites.entry(n.clone()).or_default().insert(eid);
                }
            }
            Expr::Closure { captures, .. } => captured.extend(captures.iter().cloned()),
            _ => {}
        }
    }
    Writes { sites, captured }
}

fn scan_seq(ast: &Ast, stmts: &[Stmt], env: &mut Env, w: &Writes, out: &mut HashSet<ExprId>) {
    for s in stmts {
        // A declaration's binding takes effect after the statement,
        // so the eviction its own name triggers runs first.
        if let Stmt::LetDecl { name, init, .. } = s {
            let v = nonneg_int(ast, *init);
            env.retain(|n, _| !stmt_taints(ast, s, n, NO_ARRAY));
            env.insert(name.clone(), v);
            continue;
        }
        if let Stmt::Expr(e) = s
            && let Expr::Assign { target, value } = ast.get_expr(*e)
            && let Expr::Ident(n) = ast.get_expr(*target)
        {
            let (n, v) = (n.clone(), nonneg_int(ast, *value));
            env.retain(|k, _| !stmt_taints(ast, s, k, NO_ARRAY));
            env.insert(n, v);
            continue;
        }
        scan_stmt(ast, s, env, w, out);
        env.retain(|n, _| !stmt_taints(ast, s, n, NO_ARRAY));
    }
}

fn scan_stmt(ast: &Ast, s: &Stmt, env: &mut Env, w: &Writes, out: &mut HashSet<ExprId>) {
    match s {
        Stmt::Block(v) | Stmt::Multi(v) => scan_seq(ast, v, &mut env.clone(), w, out),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            scan_stmt(ast, then_branch, &mut env.clone(), w, out);
            if let Some(e) = else_branch {
                scan_stmt(ast, e, &mut env.clone(), w, out);
            }
        }
        Stmt::Labeled { body, .. } => scan_stmt(ast, body, env, w, out),
        Stmt::While { cond, body } => {
            if settled(ast, *cond, env, None, body, w, &|_| false) {
                out.insert(*cond);
            }
            scan_stmt(ast, body, &mut env.clone(), w, out);
        }
        // A do-while's guard runs after the body, so no pair stands
        // for it in the first place (`walk.rs` pushes none).
        Stmt::DoWhile { body, .. } => scan_stmt(ast, body, &mut env.clone(), w, out),
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            let mut inner = env.clone();
            if let Some(i) = init {
                scan_seq(ast, std::slice::from_ref(&**i), &mut inner, w, out);
            }
            // A counter the loop's own `let` binds cannot be written
            // from outside the loop — the binding is not in scope
            // there. `var` hoists out of it, so it does not count.
            let own = |name: &str| matches!(init.as_deref(), Some(Stmt::LetDecl { name: n, is_var: false, .. }) if n == name);
            if let Some(c) = cond
                && settled(ast, *c, &inner, *step, body, w, &own)
            {
                out.insert(*c);
            }
            scan_stmt(ast, body, &mut inner, w, out);
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            scan_seq(ast, body, &mut env.clone(), w, out);
            scan_seq(ast, catch_body, &mut env.clone(), w, out);
            if let Some(fb) = finally_body {
                scan_seq(ast, fb, &mut env.clone(), w, out);
            }
        }
        // Every callable body is a top-level FnDecl by now (arrows
        // lifted, methods flattened), so a fresh env per fn is the
        // whole scoping story.
        Stmt::FnDecl { body, .. } => scan_seq(ast, body, &mut Env::new(), w, out),
        Stmt::ExportDecl { inner: Some(i), .. } => scan_stmt(ast, i, env, w, out),
        // Anything else: the loops under it are simply never offered
        // a settlement.
        _ => {}
    }
}

fn settled(
    ast: &Ast,
    cond: ExprId,
    env: &Env,
    step: Option<ExprId>,
    body: &Stmt,
    w: &Writes,
    loop_owns: &dyn Fn(&str) -> bool,
) -> bool {
    let Some((i, _xs)) = guard_pair(ast, cond) else {
        return false;
    };
    if env.get(&i) != Some(&true) || w.captured.contains(&i) {
        return false;
    }
    let mut seen: HashSet<ExprId> = HashSet::new();
    if let Some(st) = step
        && !expr_steps(ast, st, &i, &mut seen)
    {
        return false;
    }
    if !stmt_steps(ast, body, &i, &mut seen) {
        return false;
    }
    // A binding the loop owns is out of everyone else's reach; any
    // other one has to have all of its writes among the steps just
    // checked.
    loop_owns(&i)
        || w.sites
            .get(&i)
            .is_none_or(|all| all.iter().all(|e| seen.contains(e)))
}

/// A non-negative integer literal.
fn nonneg_int(ast: &Ast, eid: ExprId) -> bool {
    match ast.get_expr(eid) {
        Expr::Number(n) => n.fract() == 0.0 && *n >= 0.0,
        Expr::Unary {
            op: UnaryOp::Neg, ..
        } => false,
        _ => false,
    }
}

/// Every write to `i` in this expression is a non-negative step;
/// records the write sites it admits. Unknown shapes answer false —
/// the census then has nothing to match them against either way.
fn expr_steps(ast: &Ast, eid: ExprId, i: &str, seen: &mut HashSet<ExprId>) -> bool {
    match ast.get_expr(eid) {
        Expr::Ident(_)
        | Expr::Number(_)
        | Expr::String(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::This
        | Expr::Uninit => true,
        Expr::Assign { target, value } => match ast.get_expr(*target) {
            Expr::Ident(n) if n == i => {
                if step_value(ast, *value, i) {
                    seen.insert(eid);
                    true
                } else {
                    false
                }
            }
            Expr::Ident(_) => expr_steps(ast, *value, i, seen),
            _ => expr_steps(ast, *target, i, seen) && expr_steps(ast, *value, i, seen),
        },
        Expr::PostIncr { target, is_inc } => match ast.get_expr(*target) {
            Expr::Ident(n) if n == i => {
                if *is_inc {
                    seen.insert(eid);
                    true
                } else {
                    false
                }
            }
            Expr::Ident(_) => true,
            _ => expr_steps(ast, *target, i, seen),
        },
        Expr::BinOp { left, right, .. }
        | Expr::Sequence { left, right }
        | Expr::Nullish {
            lhs: left,
            rhs: right,
        }
        | Expr::Index {
            obj: left,
            index: right,
        }
        | Expr::OptIndex {
            obj: left,
            index: right,
        } => expr_steps(ast, *left, i, seen) && expr_steps(ast, *right, i, seen),
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Spread { expr }
        | Expr::As { expr, .. } => expr_steps(ast, *expr, i, seen),
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => expr_steps(ast, *obj, i, seen),
        Expr::Call { callee, args } | Expr::OptCall { callee, args } => {
            expr_steps(ast, *callee, i, seen) && args.iter().all(|a| expr_steps(ast, *a, i, seen))
        }
        Expr::Array(elems) => elems.iter().all(|e| expr_steps(ast, *e, i, seen)),
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_steps(ast, *cond, i, seen)
                && expr_steps(ast, *then_branch, i, seen)
                && expr_steps(ast, *else_branch, i, seen)
        }
        _ => false,
    }
}

/// `i + <non-negative integer>` (either way round) or a non-negative
/// integer outright.
fn step_value(ast: &Ast, v: ExprId, i: &str) -> bool {
    match ast.get_expr(v) {
        Expr::Number(_) => nonneg_int(ast, v),
        Expr::BinOp {
            op: BinOp::Add,
            left,
            right,
        } => {
            let is_i = |e: ExprId| matches!(ast.get_expr(e), Expr::Ident(n) if n == i);
            (is_i(*left) && nonneg_int(ast, *right)) || (is_i(*right) && nonneg_int(ast, *left))
        }
        _ => false,
    }
}

fn stmt_steps(ast: &Ast, s: &Stmt, i: &str, seen: &mut HashSet<ExprId>) -> bool {
    match s {
        Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Yield(e) => expr_steps(ast, *e, i, seen),
        Stmt::Return(o) => o.is_none_or(|e| expr_steps(ast, e, i, seen)),
        Stmt::LetDecl { name, init, .. } => name != i && expr_steps(ast, *init, i, seen),
        Stmt::Block(v) | Stmt::Multi(v) => v.iter().all(|s| stmt_steps(ast, s, i, seen)),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_steps(ast, *cond, i, seen)
                && stmt_steps(ast, then_branch, i, seen)
                && else_branch
                    .as_ref()
                    .is_none_or(|e| stmt_steps(ast, e, i, seen))
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            expr_steps(ast, *cond, i, seen) && stmt_steps(ast, body, i, seen)
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            init.as_ref().is_none_or(|s| stmt_steps(ast, s, i, seen))
                && cond.is_none_or(|e| expr_steps(ast, e, i, seen))
                && step.is_none_or(|e| expr_steps(ast, e, i, seen))
                && stmt_steps(ast, body, i, seen)
        }
        Stmt::Break(_) | Stmt::Continue(_) => true,
        Stmt::Labeled { body, .. } => stmt_steps(ast, body, i, seen),
        _ => false,
    }
}
