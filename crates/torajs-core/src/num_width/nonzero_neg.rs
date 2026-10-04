//! Negations whose operand is provably not zero.
//!
//! `-x` on a runtime number answers -0 when `x` is +0 (§6.1.6.1.1
//! Number::unaryMinus), which an integer slot has no bit pattern for,
//! so the width walk gives every runtime negation an F64 face and the
//! lowering emits it as a sign-preserving f64 subtract. Under a guard
//! that rules zero out — the `else` of `if (x % 2 === 0)`, the body of
//! `if (x > 0)` — that negation cannot answer -0, and when `x` is an
//! integer the answer is the exact integer `0 - x`. This module finds
//! those negations; the width walk keeps their operand's width and the
//! lowering emits the integer subtract, both reading the one answer
//! this produces, keyed by the operand's `ExprId`.
//!
//! Three things have to hold between the guard and the negation:
//!
//! 1. the guard implies `x ≠ 0` on the branch the negation sits in
//!    ([`zero_free`]), with `x` typed `number` — a string `"0"` passes
//!    `x !== 0` and still negates to -0;
//! 2. nothing writes `x` in between: the facts are positional within a
//!    statement sequence and a statement that writes the name drops
//!    them before it is walked (`stmt_taints`, the bounds proof's own
//!    eviction); a loop body starts without the facts of every name the
//!    loop writes, because the scan already evicted them at the loop;
//! 3. nothing the scan cannot see writes it: no closure captures the
//!    name, and every assignment to the name anywhere in the module
//!    sits in the body being scanned, so no other function can write
//!    it from inside a call between guard and negation. In a function
//!    the name must also be a `let` of the function's own frame, never
//!    a parameter (`arguments` aliases those) nor the spelling of a
//!    module binding: a module binding written in this very body can
//!    be written between guard and negation by a recursive call, or by
//!    another running instance across an `await` / `yield`, which 2
//!    cannot see. The module top level runs once, so its own bindings
//!    qualify. Coarse — a same-spelled local elsewhere blocks it — but
//!    sound.
//!
//! Missing any of them only keeps the F64 face, never a wrong answer.

use std::collections::{HashMap, HashSet};

use super::captures::Body;
use crate::ast::desugar_async_sm_walk::visit_expr_children;
use crate::ast::{Ast, BinOp, Expr, ExprId, Stmt, UnaryOp};
use crate::check::Type;
use crate::ssa_lower_bounds_proven::stmt_taints;

/// A name no source program can spell, so `stmt_taints` answers about
/// the name half of its pair alone.
const NO_ARRAY: &str = "\u{0}";

type Env = HashSet<String>;

struct Scan<'a> {
    ast: &'a Ast,
    types: &'a HashMap<ExprId, Type>,
    /// Names the current body may carry a fact for (rule 3).
    eligible: HashSet<String>,
    out: HashSet<ExprId>,
}

/// The operand `ExprId`s of every `-x` proven to see a non-zero `x`.
pub(super) fn collect(
    ast: &Ast,
    bodies: &[Body<'_>],
    toplevel: &HashSet<String>,
    types: &HashMap<ExprId, Type>,
) -> HashSet<ExprId> {
    let (sites, captured) = census(ast);
    let mut scan = Scan {
        ast,
        types,
        eligible: HashSet::new(),
        out: HashSet::new(),
    };
    for body in bodies {
        let mut own: HashSet<ExprId> = HashSet::new();
        body.each_expr(ast, &mut |e| {
            own.insert(e);
        });
        scan.eligible = sites
            .iter()
            .filter(|(n, at)| {
                let frame =
                    body.name.is_empty() || (body.binds_frame_local(n) && !toplevel.contains(*n));
                frame && !captured.contains(*n) && at.iter().all(|e| own.contains(e))
            })
            .map(|(n, _)| n.clone())
            .collect();
        scan.seq(body.stmts(), &mut Env::new());
    }
    scan.out
}

/// Every `Ident`-targeted write in the module by name, read off the
/// arena (a walk that missed a shape would be silent-wrong here), plus
/// the names some closure captured. A name with no write at all is a
/// `const` or never reassigned; it gets an empty site set, which every
/// body owns.
fn census(ast: &Ast) -> (HashMap<String, Vec<ExprId>>, HashSet<String>) {
    let mut sites: HashMap<String, Vec<ExprId>> = HashMap::new();
    let mut captured: HashSet<String> = HashSet::new();
    for (i, e) in ast.exprs.iter().enumerate() {
        let eid = ExprId(i as u32);
        match e {
            Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => {
                if let Expr::Ident(n) = ast.get_expr(*target) {
                    sites.entry(n.clone()).or_default().push(eid);
                }
            }
            Expr::Ident(n) => {
                sites.entry(n.clone()).or_default();
            }
            Expr::Closure { captures, .. } => captured.extend(captures.iter().cloned()),
            _ => {}
        }
    }
    (sites, captured)
}

impl Scan<'_> {
    fn seq<'s>(&mut self, stmts: impl IntoIterator<Item = &'s Stmt>, env: &mut Env) {
        for s in stmts {
            let ast = self.ast;
            env.retain(|n| !stmt_taints(ast, s, n, NO_ARRAY));
            self.stmt(s, env);
        }
    }

    fn stmt(&mut self, s: &Stmt, env: &Env) {
        match s {
            Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => self.expr(*e, env),
            Stmt::LetDecl { init, .. } => self.expr(*init, env),
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.expr(*cond, env);
                let ast = self.ast;
                let cond_stmt = Stmt::Expr(*cond);
                for (branch, holds) in [
                    (Some(&**then_branch), true),
                    (else_branch.as_deref(), false),
                ] {
                    if let Some(b) = branch {
                        let mut facts = Env::new();
                        self.guard_facts(*cond, holds, &mut facts);
                        // the cond may write a name after testing it
                        facts.retain(|n| !stmt_taints(ast, &cond_stmt, n, NO_ARRAY));
                        let mut inner = env.clone();
                        inner.extend(facts);
                        self.seq(std::iter::once(b), &mut inner);
                    }
                }
            }
            Stmt::Block(v) | Stmt::Multi(v) => self.seq(v, &mut env.clone()),
            Stmt::While { cond, body } | Stmt::DoWhile { cond, body } => {
                self.expr(*cond, env);
                self.seq(std::iter::once(&**body), &mut env.clone());
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                let mut inner = env.clone();
                if let Some(i) = init {
                    self.seq(std::iter::once(&**i), &mut inner);
                }
                for e in cond.iter().chain(step.iter()) {
                    self.expr(*e, &inner);
                }
                self.seq(std::iter::once(&**body), &mut inner);
            }
            Stmt::Labeled { body, .. } => self.seq(std::iter::once(&**body), &mut env.clone()),
            // Every other shape is walked without facts: `stmt_taints`
            // already emptied the env for it, and the negations under
            // it simply stay F64.
            _ => {}
        }
    }

    /// Record every `-x` in `eid` whose `x` the env holds.
    fn expr(&mut self, eid: ExprId, env: &Env) {
        if env.is_empty() {
            return;
        }
        if let Expr::Unary {
            op: UnaryOp::Neg,
            expr,
        } = self.ast.get_expr(eid)
            && let Expr::Ident(n) = self.ast.get_expr(*expr)
            && env.contains(n)
            && self.is_number(*expr)
        {
            self.out.insert(*expr);
        }
        visit_expr_children(self.ast, eid, &mut |c| self.expr(c, env));
    }

    fn is_number(&self, eid: ExprId) -> bool {
        matches!(self.types.get(&eid), Some(Type::Number))
    }

    /// Add the names `cond` proves non-zero when it evaluates to `holds`.
    fn guard_facts(&self, cond: ExprId, holds: bool, env: &mut Env) {
        match self.ast.get_expr(cond) {
            Expr::Unary {
                op: UnaryOp::Not,
                expr,
            } => self.guard_facts(*expr, !holds, env),
            Expr::BinOp {
                op: BinOp::LAnd,
                left,
                right,
            } if holds => {
                self.guard_facts(*left, true, env);
                self.guard_facts(*right, true, env);
            }
            Expr::BinOp {
                op: BinOp::LOr,
                left,
                right,
            } if !holds => {
                self.guard_facts(*left, false, env);
                self.guard_facts(*right, false, env);
            }
            // a truthy number is neither zero nor NaN
            Expr::Ident(_) if holds => self.add_subject(cond, env),
            Expr::BinOp { op, left, right } => {
                let (subject, op, c) = match (literal(self.ast, *left), literal(self.ast, *right)) {
                    (None, Some(c)) => (*left, *op, c),
                    (Some(c), None) => (*right, mirror(*op), c),
                    _ => return,
                };
                let op = if holds { Some(op) } else { negate(op) };
                if op.is_some_and(|op| zero_free(op, c)) {
                    self.add_subject(subject, env);
                }
            }
            _ => {}
        }
    }

    /// `x`, or `x % k` / `x & k` with `k ≠ 0` (both ±0 when `x` is; a
    /// zero `k` makes the remainder NaN, which is not 0 for any `x`): a
    /// non-zero subject makes `x` non-zero.
    fn add_subject(&self, subject: ExprId, env: &mut Env) {
        let x = match self.ast.get_expr(subject) {
            Expr::BinOp {
                op: BinOp::Mod | BinOp::BitAnd,
                left,
                right,
            } if literal(self.ast, *right).is_some_and(|k| k != 0.0) => *left,
            _ => subject,
        };
        if let Expr::Ident(n) = self.ast.get_expr(x)
            && self.eligible.contains(n)
            && self.is_number(x)
        {
            env.insert(n.clone());
        }
    }
}

/// A finite numeric literal, `-` spelling included.
fn literal(ast: &Ast, eid: ExprId) -> Option<f64> {
    let v = match ast.get_expr(eid) {
        Expr::Number(n) => *n,
        Expr::Unary {
            op: UnaryOp::Neg,
            expr,
        } => match ast.get_expr(*expr) {
            Expr::Number(n) => -*n,
            _ => return None,
        },
        _ => return None,
    };
    v.is_finite().then_some(v)
}

/// `c op x` as `x op' c`.
fn mirror(op: BinOp) -> BinOp {
    match op {
        BinOp::Lt => BinOp::Gt,
        BinOp::Gt => BinOp::Lt,
        BinOp::Le => BinOp::Ge,
        BinOp::Ge => BinOp::Le,
        other => other,
    }
}

/// The comparison that holds when `x op c` is false. A NaN subject
/// fails every ordered comparison, so the negation of `<` is `>=` or
/// NaN — which `zero_free` also excludes, since NaN is not zero.
fn negate(op: BinOp) -> Option<BinOp> {
    Some(match op {
        BinOp::Eq => BinOp::Neq,
        BinOp::Neq => BinOp::Eq,
        BinOp::LooseEq => BinOp::LooseNeq,
        BinOp::LooseNeq => BinOp::LooseEq,
        BinOp::Lt => BinOp::Ge,
        BinOp::Ge => BinOp::Lt,
        BinOp::Gt => BinOp::Le,
        BinOp::Le => BinOp::Gt,
        _ => return None,
    })
}

/// Whether `x op c` holding rules out `x` being ±0.
fn zero_free(op: BinOp, c: f64) -> bool {
    match op {
        BinOp::Eq | BinOp::LooseEq => c != 0.0,
        BinOp::Neq | BinOp::LooseNeq => c == 0.0,
        BinOp::Lt => c <= 0.0,
        BinOp::Le => c < 0.0,
        BinOp::Gt => c >= 0.0,
        BinOp::Ge => c > 0.0,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comparisons_that_exclude_zero() {
        assert!(zero_free(BinOp::Neq, 0.0));
        assert!(zero_free(BinOp::Eq, 3.0));
        assert!(!zero_free(BinOp::Eq, 0.0));
        assert!(zero_free(BinOp::Gt, 0.0));
        assert!(!zero_free(BinOp::Ge, 0.0));
        assert!(zero_free(BinOp::Ge, 1.0));
        assert!(zero_free(BinOp::Lt, 0.0));
        assert!(!zero_free(BinOp::Le, 0.0));
        assert!(zero_free(BinOp::Le, -1.0));
        // `x % 2 === 0` is false: x % 2 is not 0, and neither is x
        assert_eq!(negate(BinOp::Eq), Some(BinOp::Neq));
        assert!(zero_free(negate(BinOp::Eq).unwrap(), 0.0));
        // `x > 0` is false: x may be 0
        assert!(!zero_free(negate(BinOp::Gt).unwrap(), 0.0));
        assert_eq!(mirror(BinOp::Lt), BinOp::Gt);
    }
}
