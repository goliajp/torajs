//! Magnitude bounds for the integer face of a Number.
//!
//! Number arithmetic is f64: an integer result is exact only up to
//! [`EXACT_INT_MAX`] in magnitude, and past it each `+ - *` rounds. The
//! i64 representation computes exactly, so it answers what JS answers
//! only while every value it holds stays inside that window. This pass
//! bounds `|value|` for every slot and for every `+ - *` the walk saw; a
//! slot whose bound leaves the window is seeded F64, and an operator
//! whose bound leaves it lowers in f64 (`arith_in_window` on the table),
//! where float_demote narrows it back wherever interval facts allow.
//!
//! A slot's bound is the largest of its writes (an `i++` / `i--` is a
//! write of `i + 1`); one that is not a `let` binding is at least
//! [`UNRECORDED_REACH`], what a runtime-supplied index or length may
//! hold. A slot on an assignment cycle whose writes each add at most a
//! constant to one value of the cycle is a counter, bounded by how far
//! [`COUNTER_HORIZON`] steps take it from its largest entry, or by the
//! tests of the loops it counts up in (`bound.rs`). Any other cycle is
//! unbounded.

mod bound;

use super::{Analysis, Scope, SlotKey, W};
use crate::ast::{BinOp, Expr, ExprId, UnaryOp};

pub(super) use bound::settle;

/// Every integer up to this magnitude is a Number; past it they round.
pub(crate) const EXACT_INT_MAX: i64 = 1 << 53;

/// Step executions taken as physically unreachable. A tight `t += 1`
/// loop runs 2^31 trips in 0.54 s on the M-series bench box, so 2^48
/// trips is one loop running uninterrupted for about 20 hours.
pub(super) const COUNTER_HORIZON: i64 = 1 << 48;

/// What a slot may hold beyond its recorded writes: an index or a
/// length the runtime hands a callback.
const UNRECORDED_REACH: u128 = 1 << 32;

const TOP: u128 = u128::MAX;

/// An upper bound on `|value|` of an expression, over slot bounds.
#[derive(Clone, Debug)]
pub(super) enum Mag {
    Const(u128),
    Slot(SlotKey),
    Sum(Box<Mag>, Box<Mag>),
    /// `a - b`, bounded as a sum; kept apart so a step can be told to
    /// count up.
    Diff(Box<Mag>, Box<Mag>),
    Prod(Box<Mag>, Box<Mag>),
    /// `a % b`: no larger than either side.
    Min(Box<Mag>, Box<Mag>),
    Max(Vec<Mag>),
    /// A read of this property name, which may run a getter.
    Prop(String),
}

impl Mag {
    pub(super) fn top() -> Self {
        Mag::Const(TOP)
    }
}

/// A write's width with its magnitude bound.
pub(super) type Flow = (W, Mag);

/// A write statically f64: its bound is never read.
pub(super) fn f64_flow() -> Flow {
    (W::F64, Mag::top())
}

/// What the walk recorded: every write's bound by target with the
/// innermost loop it runs in, every `+ - *` operator's bound, and the
/// guard each loop's test establishes.
#[derive(Default)]
pub(super) struct MagRecord {
    writes: Vec<(SlotKey, Mag, Option<usize>)>,
    arith: Vec<(ExprId, Mag)>,
    /// Indexed by loop id.
    guards: Vec<Option<Guard>>,
    /// The loops enclosing the walk's current point, innermost last.
    open: Vec<usize>,
}

/// A loop whose body runs only while `key < below` held at its test.
struct Guard {
    /// The function the loop is in (`""` for the top level).
    fn_name: String,
    key: SlotKey,
    below: Mag,
}

/// `X * x + k` over the value `X` of the cycle being bounded; `x` is
/// [`TOP`] once the cycle value is multiplied by itself.
#[derive(Clone, Copy)]
struct Lin {
    x: u128,
    k: u128,
}

fn lin(m: &Mag, outside: &impl Fn(&SlotKey) -> Option<u128>) -> Lin {
    match m {
        Mag::Const(c) => Lin { x: 0, k: *c },
        Mag::Slot(s) => match outside(s) {
            Some(k) => Lin { x: 0, k },
            None => Lin { x: 1, k: 0 },
        },
        Mag::Sum(a, b) | Mag::Diff(a, b) => {
            let (a, b) = (lin(a, outside), lin(b, outside));
            Lin {
                x: a.x.saturating_add(b.x),
                k: a.k.saturating_add(b.k),
            }
        }
        Mag::Prod(a, b) => {
            let (a, b) = (lin(a, outside), lin(b, outside));
            let x = if a.x > 0 && b.x > 0 {
                TOP
            } else {
                a.x.saturating_mul(b.k)
                    .saturating_add(b.x.saturating_mul(a.k))
            };
            Lin {
                x,
                k: a.k.saturating_mul(b.k),
            }
        }
        Mag::Min(a, b) => {
            let (a, b) = (lin(a, outside), lin(b, outside));
            match (a.x, b.x) {
                (0, 0) => Lin {
                    x: 0,
                    k: a.k.min(b.k),
                },
                (_, 0) => b,
                _ => a,
            }
        }
        Mag::Prop(_) => unreachable!("property reads resolve before bounding"),
        Mag::Max(v) => v.iter().fold(Lin { x: 0, k: 0 }, |acc, m| {
            let l = lin(m, outside);
            Lin {
                x: acc.x.max(l.x),
                k: acc.k.max(l.k),
            }
        }),
    }
}

impl Analysis<'_> {
    /// What writing `eid` contributes: its width and its bound.
    pub(super) fn flow(&self, eid: ExprId, scope: &Scope) -> Flow {
        (self.width_of(eid, scope), self.mag_of(eid, scope))
    }

    /// The bound of `eid`'s value, mirroring how `width_of` resolves
    /// each read; anything it cannot bound is [`Mag::top`].
    pub(super) fn mag_of(&self, eid: ExprId, scope: &Scope) -> Mag {
        let bin = |f: fn(Box<Mag>, Box<Mag>) -> Mag, l: ExprId, r: ExprId| {
            f(
                Box::new(self.mag_of(l, scope)),
                Box::new(self.mag_of(r, scope)),
            )
        };
        match self.ast.get_expr(eid) {
            Expr::Number(n) if n.is_finite() => Mag::Const(n.abs().ceil() as u128),
            Expr::Bool(_) | Expr::Null => Mag::Const(1),
            Expr::Ident(n) => match self.resolve(n, scope) {
                Some(k) => Mag::Slot(k),
                None if scope.fn_name.starts_with("__") && self.by_name.contains_key(n) => {
                    Mag::Max(self.by_name[n].iter().cloned().map(Mag::Slot).collect())
                }
                None => Mag::top(),
            },
            Expr::BinOp { op, left, right } => match op {
                BinOp::Add => bin(Mag::Sum, *left, *right),
                BinOp::Sub => bin(Mag::Diff, *left, *right),
                BinOp::Mul => bin(Mag::Prod, *left, *right),
                BinOp::Mod => bin(Mag::Min, *left, *right),
                BinOp::Div | BinOp::Pow => Mag::top(),
                BinOp::BitAnd
                | BinOp::BitOr
                | BinOp::BitXor
                | BinOp::Shl
                | BinOp::Shr
                | BinOp::UShr => Mag::Const(1 << 32),
                BinOp::LAnd | BinOp::LOr => {
                    Mag::Max(vec![self.mag_of(*left, scope), self.mag_of(*right, scope)])
                }
                _ => Mag::Const(1),
            },
            Expr::Unary { op, expr } => match op {
                // spelled `0 - x`, so a negated step never reads as counting up
                UnaryOp::Neg => {
                    Mag::Diff(Box::new(Mag::Const(0)), Box::new(self.mag_of(*expr, scope)))
                }
                UnaryOp::Plus => Mag::top(),
                UnaryOp::BitNot => Mag::Const(1 << 32),
                UnaryOp::Not => Mag::Const(1),
            },
            Expr::Ternary {
                then_branch,
                else_branch,
                ..
            } => Mag::Max(vec![
                self.mag_of(*then_branch, scope),
                self.mag_of(*else_branch, scope),
            ]),
            Expr::Nullish { lhs, rhs } => {
                Mag::Max(vec![self.mag_of(*lhs, scope), self.mag_of(*rhs, scope)])
            }
            Expr::Sequence { right, .. } => self.mag_of(*right, scope),
            Expr::Assign { value, .. } => self.mag_of(*value, scope),
            Expr::As { expr, .. } => self.mag_of(*expr, scope),
            Expr::PostIncr { target, .. } => self.mag_of(*target, scope),
            Expr::Call { callee, .. } => {
                let named = self.retargets.get(&eid).cloned().or_else(|| {
                    match self.ast.get_expr(*callee) {
                        Expr::Ident(f) if self.fn_params.contains_key(f) => Some(f.clone()),
                        _ => None,
                    }
                });
                match named {
                    Some(f) => Mag::Slot(SlotKey::Ret(f)),
                    None => self
                        .container_key_lookup(eid, scope)
                        .map_or(Mag::top(), Mag::Slot),
                }
            }
            Expr::Member { name, .. } | Expr::OptChain { name, .. } => {
                let field = self
                    .container_key_lookup(eid, scope)
                    .map_or(Mag::top(), Mag::Slot);
                Mag::Max(vec![field, Mag::Prop(name.clone())])
            }
            Expr::Index { .. } | Expr::OptIndex { .. } => self
                .container_key_lookup(eid, scope)
                .map_or(Mag::top(), Mag::Slot),
            _ => Mag::top(),
        }
    }

    /// Record a write of `m` into `target`.
    pub(super) fn record_mag(&mut self, target: SlotKey, m: Mag) {
        let at = self.mag.open.last().copied();
        self.mag.writes.push((target, m, at));
    }

    /// Record `++` / `--` on `target` as the write of a unit step.
    pub(super) fn record_step(&mut self, target: ExprId, up: bool, scope: &Scope) {
        if let Some(k) = self.container_key_lookup(target, scope) {
            let (s, one) = (Box::new(Mag::Slot(k.clone())), Box::new(Mag::Const(1)));
            self.record_mag(
                k,
                if up {
                    Mag::Sum(s, one)
                } else {
                    Mag::Diff(s, one)
                },
            );
        }
    }

    /// Walk a loop's repeated part (`inner`) as one loop, guarded by
    /// what its test `cond` holds whenever the body runs.
    pub(super) fn mag_loop(
        &mut self,
        cond: Option<ExprId>,
        scope: &Scope,
        inner: impl FnOnce(&mut Self),
    ) {
        let guard = cond.and_then(|c| self.loop_guard(c, scope));
        self.mag.open.push(self.mag.guards.len());
        self.mag.guards.push(guard);
        inner(self);
        self.mag.open.pop();
    }

    /// `key < below` when `c` holds: a relational test of a binding
    /// against anything, or a conjunction with one.
    fn loop_guard(&self, c: ExprId, scope: &Scope) -> Option<Guard> {
        let Expr::BinOp { op, left, right } = self.ast.get_expr(c) else {
            return None;
        };
        let (k, b) = match op {
            BinOp::LAnd => {
                return self
                    .loop_guard(*left, scope)
                    .or_else(|| self.loop_guard(*right, scope));
            }
            BinOp::Lt | BinOp::Le => (*left, *right),
            BinOp::Gt | BinOp::Ge => (*right, *left),
            _ => return None,
        };
        let Expr::Ident(n) = self.ast.get_expr(k) else {
            return None;
        };
        Some(Guard {
            fn_name: scope.fn_name.to_string(),
            key: self.resolve(n, scope)?,
            below: Mag::Sum(Box::new(self.mag_of(b, scope)), Box::new(Mag::Const(1))),
        })
    }

    /// Record the operator `eid` (a `+ - *`) for the window check. One
    /// the checker types other than a Number never takes the integer
    /// path, so it is left out (and out of the window, were it to).
    pub(super) fn record_arith(&mut self, eid: ExprId, scope: &Scope) {
        if matches!(self.expr_types.get(&eid), Some(t) if !matches!(t, crate::check::Type::Number))
        {
            return;
        }
        let m = self.mag_of(eid, scope);
        self.mag.arith.push((eid, m));
    }
}
