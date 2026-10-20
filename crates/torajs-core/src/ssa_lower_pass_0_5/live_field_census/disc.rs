//! Member reads proven live by a discriminant field.
//!
//! A class's nullable field `f` may hold a live value at every
//! construction site except those passing certain constants for a field
//! `d`: after `if (x.d === C) return` has ruled those constants out, a
//! read of `x.f` sees a live value although the field as a whole is
//! not live. Both fields are written in the constructor prefix (and in
//! the same place of its width twins) and nowhere else, so neither
//! changes after construction, and `x` is a binding nothing writes. A
//! construction site whose `d` is not a known constant is never ruled
//! out. A constant is a literal, or a top-level binding of one that
//! nothing writes or shadows, compared by value (`===` on two equal
//! primitives other than NaN holds).
//!
//! A fact holds from the end of the test to the end of the statement
//! list the test is in, and inside the `else` branch; a declaration of
//! the name ends it, from the start of the statement that holds the
//! declaration. Each function starts with no fact.

use crate::ssa::id_hash::{HashMap, HashSet};
use std::collections::hash_map::Entry;

use crate::ast::{BinOp, Expr, ExprId, Stmt};
use crate::check::Type;

use super::walk::nested_stmts;
use super::{Census, PrefixWrite};

mod facts;
mod sites;

use facts::Facts;

/// A constant a discriminant field is compared against.
#[derive(Clone, PartialEq)]
pub(super) enum Key<'a> {
    Num(u64),
    Str(&'a torajs_wtf8::Wtf8Buf),
    Bool(bool),
}

/// A read `x.f` where tests of `x`'s discriminant fields have ruled
/// constants out.
struct Disc<'a> {
    class: &'a str,
    field: &'a str,
    ruled_out: Vec<(&'a str, Key<'a>)>,
}

/// What the width twins of a class's factory and constructor write:
/// the twin constructors' prefixes, and the seeds the twin factories
/// hand them.
#[derive(Default)]
struct Twins<'a> {
    ctors: usize,
    writes: Vec<PrefixWrite<'a>>,
    seeds: HashSet<ExprId>,
}

/// The reads made under a fact, the top-level constants, and the twins
/// of the classes read.
#[derive(Default)]
pub(super) struct Discs<'a> {
    reads: HashMap<ExprId, Disc<'a>>,
    /// What each read needs live (`sites`).
    plans: HashMap<ExprId, Option<Vec<ExprId>>>,
    consts: Consts<'a>,
    twins: HashMap<&'a str, Twins<'a>>,
}

/// A test `x.d === K` failing: binding, field, constant.
type Test<'a> = (&'a str, &'a str, Key<'a>);

impl<'a> Census<'a> {
    pub(super) fn discs(&self) -> Discs<'a> {
        let mut out = Discs::default();
        for s in &self.ast.stmts {
            let Stmt::LetDecl { name: n, init, .. } = s else {
                continue;
            };
            let Some(k) = literal(self.ast.get_expr(*init)) else {
                continue;
            };
            // one binder of the name in the whole program, never written
            let n = n.as_str();
            let alone = self.let_inits.get(n).is_some_and(|v| v.len() == 1)
                && !self.assigned.contains(n)
                && !self.bound.contains(n)
                && !self.ctor_params.contains_key(n)
                && !self.scopes.param_names.contains(n)
                && !self.fns.contains_key(n);
            if alone {
                out.consts.insert(n, k);
            }
        }
        let bodies = (self.fns.values().map(|(_, b)| *b))
            .chain(self.ctors.iter().map(|c| c.body))
            .chain(std::iter::once(&self.ast.stmts[..]));
        for body in bodies {
            let mut facts = Facts::default();
            body.iter().for_each(|s| self.stmt(s, &mut facts, &mut out));
        }
        let classes: HashSet<&str> = out.reads.values().map(|d| d.class).collect();
        out.twins = classes.into_iter().map(|c| (c, self.twins(c))).collect();
        out
    }

    /// `st` under `facts` and `extra`, which end with it.
    fn nested(&self, st: &'a Stmt, facts: &mut Facts<'a>, extra: &[Test<'a>], out: &mut Discs<'a>) {
        let at = facts.mark();
        for (x, d, k) in extra {
            facts.push(x, d, k.clone());
        }
        self.stmt(st, facts, out);
        facts.restore(at);
    }

    /// `f` under the facts less `names`, which a declaration anywhere
    /// inside the statement shadows from its start.
    fn inner(
        &self,
        facts: &mut Facts<'a>,
        names: &[&'a str],
        f: impl FnOnce(&Self, &mut Facts<'a>),
    ) {
        let at = facts.mark();
        names.iter().for_each(|n| facts.remove(n));
        f(self, facts);
        facts.restore(at);
    }

    fn stmt(&self, s: &'a Stmt, facts: &mut Facts<'a>, out: &mut Discs<'a>) {
        let mut names = Vec::new();
        if !facts.is_empty() {
            declares(s, &mut names);
        }
        match s {
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.reads(*cond, facts, out);
                let mut tests = Vec::new();
                self.tests(*cond, &out.consts, &mut tests);
                self.inner(facts, &names, |c, f| {
                    c.nested(then_branch, f, &[], out);
                    if let Some(e) = else_branch {
                        c.nested(e, f, &tests, out);
                    }
                });
                if exits(then_branch) {
                    tests.into_iter().for_each(|(x, d, k)| facts.push(x, d, k));
                }
            }
            Stmt::LetDecl { init: e, .. }
            | Stmt::Expr(e)
            | Stmt::Throw(e)
            | Stmt::Return(Some(e)) => self.reads(*e, facts, out),
            Stmt::Multi(v) => v.iter().for_each(|st| self.stmt(st, facts, out)),
            Stmt::Block(v) => self.inner(facts, &names, |c, f| {
                v.iter().for_each(|st| c.stmt(st, f, out));
            }),
            Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
                self.reads(*cond, facts, out);
                self.inner(facts, &names, |c, f| c.nested(body, f, &[], out));
            }
            Stmt::For { .. }
            | Stmt::ForOf { .. }
            | Stmt::ForOfSplitIter { .. }
            | Stmt::Labeled { .. }
            | Stmt::Try { .. }
            | Stmt::Switch { .. } => self.inner(facts, &names, |c, f| {
                nested_stmts(s, &mut |st| c.nested(st, f, &[], out));
            }),
            _ => {}
        }
        names.into_iter().for_each(|n| facts.remove(n));
    }

    /// The `x.d === K` tests `cond` failing makes fail, through `||`.
    fn tests(&self, cond: ExprId, consts: &Consts<'a>, out: &mut Vec<Test<'a>>) {
        let Expr::BinOp { op, left, right } = self.ast.get_expr(cond) else {
            return;
        };
        match op {
            BinOp::LOr => {
                self.tests(*left, consts, out);
                self.tests(*right, consts, out);
            }
            BinOp::Eq | BinOp::LooseEq => {
                for (m, k) in [(*left, *right), (*right, *left)] {
                    if let Expr::Member { obj, name } = self.ast.get_expr(m)
                        && let Expr::Ident(x) = self.ast.get_expr(*obj)
                        && let Some(k) = key(self.ast.get_expr(k), consts)
                    {
                        out.push((x.as_str(), name.as_str(), k));
                    }
                }
            }
            _ => {}
        }
    }

    /// Record every `x.f` read in `e` for which a fact on `x` holds.
    fn reads(&self, e: ExprId, facts: &Facts<'a>, out: &mut Discs<'a>) {
        if facts.is_empty() {
            return;
        }
        if let Expr::Member { obj, name } = self.ast.get_expr(e)
            && let Expr::Ident(x) = self.ast.get_expr(*obj)
            && let Some(ruled_out) = facts.get(x.as_str())
            && !self.assigned.contains(x.as_str())
            && let Some(Type::ClassRef(c)) = self.ty(*obj)
        {
            let d = Disc {
                class: c.split('<').next().unwrap_or(c),
                field: name,
                ruled_out: ruled_out.clone(),
            };
            // an expression met under two sets of facts keeps neither
            match out.reads.entry(e) {
                Entry::Occupied(mut o) if o.get().ruled_out != d.ruled_out => {
                    o.get_mut().ruled_out.clear()
                }
                Entry::Occupied(_) => {}
                Entry::Vacant(v) => {
                    v.insert(d);
                }
            }
        }
        crate::ast::desugar_async_sm_walk::visit_expr_children(self.ast, e, &mut |c| {
            self.reads(c, facts, out)
        });
    }
}

type Consts<'a> = HashMap<&'a str, Key<'a>>;

/// The constant `e` spells: a literal, or a top-level constant.
fn key<'a>(e: &'a Expr, consts: &Consts<'a>) -> Option<Key<'a>> {
    match e {
        Expr::Ident(n) => consts.get(n.as_str()).cloned(),
        _ => literal(e),
    }
}

fn literal(e: &Expr) -> Option<Key<'_>> {
    match e {
        Expr::Number(v) if !v.is_nan() => Some(Key::Num(if *v == 0.0 { 0 } else { v.to_bits() })),
        Expr::String(s) => Some(Key::Str(s)),
        Expr::Bool(b) => Some(Key::Bool(*b)),
        _ => None,
    }
}

/// Control never reaches the statement after `s`.
fn exits(s: &Stmt) -> bool {
    match s {
        Stmt::Return(_) | Stmt::Throw(_) | Stmt::Break(_) | Stmt::Continue(_) => true,
        Stmt::Block(v) | Stmt::Multi(v) => v.iter().any(exits),
        Stmt::If {
            then_branch,
            else_branch: Some(e),
            ..
        } => exits(then_branch) && exits(e),
        _ => false,
    }
}

/// The names `s` or a statement nested in it declares.
fn declares<'s>(s: &'s Stmt, out: &mut Vec<&'s str>) {
    match s {
        Stmt::LetDecl { name: n, .. }
        | Stmt::UsingDecl { name: n, .. }
        | Stmt::YieldInto { var: n, .. }
        | Stmt::ForOf { var_name: n, .. }
        | Stmt::ForOfSplitIter { var_name: n, .. }
        | Stmt::Try {
            catch_param: Some(n),
            ..
        } => out.push(n),
        _ => {}
    }
    nested_stmts(s, &mut |st| declares(st, out));
}
