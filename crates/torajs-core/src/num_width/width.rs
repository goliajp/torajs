//! Expression width classification + slot resolution for the
//! constraint walk in `walk.rs`.

use super::{Analysis, Scope, SlotKey, W, join, literal_is_f64};
use crate::ast::{BinOp, Expr, ExprId, UnaryOp};

/// Mark every slot dependency as reaching the consumer through a
/// growth op (W5). Inside an assignment-graph cycle that marking
/// makes the cycle non-i64-safe.
/// Largest literal step magnitude the counter carve-out admits
/// without a growth mark (see [`Analysis::is_int_const`]).
///
/// The horizon behind it: a tight `t += 1` loop runs 2^31 trips in
/// 0.54 s on the M-series bench box (rotation 507, three runs), so
/// 2^48 trips is one loop running uninterrupted for ~20 hours — the
/// point past which the original carve-out's "physically unreachable"
/// claim is taken to hold. `|k| ≤ 2^5` keeps `2^53 / |k| ≥ 2^48`;
/// the `i += 1` family that motivated the carve-out sits far inside
/// it, and every real loop body is slower than the tight one the
/// horizon was measured on.
const MAX_COUNTER_STEP: i64 = 1 << 5;

fn mark_growth(w: W) -> W {
    match w {
        W::Num(deps) => W::Num(deps.into_iter().map(|(k, _)| (k, true)).collect()),
        other => other,
    }
}

impl<'a> Analysis<'a> {
    pub(super) fn resolve(&self, n: &str, scope: &Scope) -> Option<SlotKey> {
        if scope.fn_name.is_empty() {
            if self.toplevel_lets.contains(n) {
                return Some(SlotKey::Global(n.to_string()));
            }
            return None;
        }
        if scope.params.contains(n) {
            return Some(SlotKey::Param(scope.fn_name.to_string(), n.to_string()));
        }
        if scope.locals.contains(n) {
            return Some(SlotKey::Local(scope.fn_name.to_string(), n.to_string()));
        }
        if self.toplevel_lets.contains(n) {
            return Some(SlotKey::Global(n.to_string()));
        }
        None
    }

    pub(super) fn add_constraint(&mut self, target: SlotKey, w: W) {
        match w {
            W::F64 => self.seeds.push(target),
            W::Num(deps) => {
                for (d, growth) in deps {
                    self.edges
                        .entry(d)
                        .or_default()
                        .push((target.clone(), growth));
                }
            }
            W::NotNum => {}
        }
    }

    /// A SMALL integer literal — the Add/Sub increment shape that
    /// keeps a self-feeding counter linear-small (`i = i + 1`
    /// family). Non-constant increments (slot / param / call /
    /// member) can be arbitrarily large per step, so they mark
    /// growth instead.
    ///
    /// The carve-out is a trip-count argument, so it is only sound
    /// while the step is small: a counter stepping by `k` leaves the
    /// exact-i64 window after `2^53 / |k|` trips, and rotation 506
    /// measured the unbounded version of this rule going wrong at
    /// the first big literal (`t += 9007199254740991` printed a
    /// wrapped negative where bun prints `18014398509481982000`).
    /// [`MAX_COUNTER_STEP`] is the largest `|k|` whose trip count to
    /// the window edge stays past the reachability horizon; a bigger
    /// literal marks growth and rides the float_demote guard, which
    /// branch_fold removes again whenever the trip count is
    /// statically bounded.
    fn is_int_const(&self, eid: ExprId) -> bool {
        let small = |n: f64| !literal_is_f64(n) && n.abs() <= MAX_COUNTER_STEP as f64;
        match self.ast.get_expr(eid) {
            Expr::Number(n) => small(*n),
            Expr::Unary {
                op: UnaryOp::Neg,
                expr,
            } => {
                matches!(self.ast.get_expr(*expr), Expr::Number(n) if small(*n) && *n != 0.0)
            }
            _ => false,
        }
    }

    /// Static width of an expression. F64 seeds mirror the union of
    /// the retired per-site heuristics (fract / out-of-range literal,
    /// Div / Pow, Math.*, NaN / Infinity) plus `-0` literals.
    pub(super) fn width_of(&self, eid: ExprId, scope: &Scope) -> W {
        match self.ast.get_expr(eid) {
            Expr::Number(n) => {
                if literal_is_f64(*n) {
                    W::F64
                } else {
                    W::Num(Vec::new())
                }
            }
            Expr::Ident(n) => {
                if n == "NaN" || n == "Infinity" {
                    return W::F64;
                }
                match self.resolve(n, scope) {
                    Some(k) => W::Num(vec![(k, false)]),
                    None => W::NotNum,
                }
            }
            Expr::BinOp { op, left, right } => self.width_of_binop(*op, *left, *right, scope),
            Expr::Unary { op, expr } => match op {
                UnaryOp::Neg => {
                    // `-0` spells Neg(Number(0)) — meaningful f64 sign
                    // state. A non-zero literal negation is the
                    // lower-time ConstI64 fold (int path), so it
                    // preserves operand width.
                    //
                    // W3 S8 — any other negation (`-x` on a runtime
                    // value) mints -0 when x == 0, so lowering floats
                    // it; the slot mirrors F64 (same mirror discipline
                    // as the Mod / Mul arms above).
                    match self.ast.get_expr(*expr) {
                        Expr::Number(n) if *n != 0.0 => self.width_of(*expr, scope),
                        _ => W::F64,
                    }
                }
                // `+x` is ToNumber — strings coerce to fractional
                // values ("3.5" → 3.5), so the result is f64-possible.
                UnaryOp::Plus => W::F64,
                UnaryOp::BitNot => W::Num(Vec::new()),
                UnaryOp::Not => W::NotNum,
            },
            Expr::Call { callee, .. } => self.width_of_call(eid, *callee, scope),
            Expr::Ternary {
                then_branch,
                else_branch,
                ..
            } => join(
                self.width_of(*then_branch, scope),
                self.width_of(*else_branch, scope),
            ),
            Expr::Nullish { lhs, rhs } => {
                join(self.width_of(*lhs, scope), self.width_of(*rhs, scope))
            }
            Expr::Sequence { right, .. } => self.width_of(*right, scope),
            Expr::Assign { value, .. } => self.width_of(*value, scope),
            // An assertion is transparent — `x as number` on an i64
            // local keeps the integer face — EXCEPT over an `any`,
            // where lowering materializes the numeric face with a
            // ToNumber whose result is f64 (`ssa_lower_any_cast`'s
            // unbox direction). The gate here is that same predicate,
            // read off the same map, so the two cannot drift.
            //
            // Where the value CAME from says nothing about this: the
            // analysis happily tracks `{v: 3}`'s field as an integer,
            // and reporting that width left containers narrow while
            // the store handed them the f64 the assertion had really
            // produced — `a.push(o.v as number)` and `a[0] = o.v as
            // number` both ended at the loud "width analysis missed
            // this write".
            Expr::As { expr, ty_ann } => {
                if ty_ann == "number"
                    && matches!(self.expr_types.get(expr), Some(crate::check::Type::Any))
                {
                    W::F64
                } else {
                    self.width_of(*expr, scope)
                }
            }
            Expr::PostIncr { target, .. } => {
                if let Expr::Ident(n) = self.ast.get_expr(*target)
                    && let Some(k) = self.resolve(n, scope)
                {
                    W::Num(vec![(k, false)])
                } else {
                    W::NotNum
                }
            }
            // W4 — Member / Index reads depend on the container's
            // elem / field lattice point; the canonical fixpoint
            // resolves them per alias class. Untrackable receivers
            // (string literals etc.) stay NotNum — their props are
            // runtime-typed independent of the width table.
            Expr::Member { .. } | Expr::Index { .. } => {
                match self.container_key_lookup(eid, scope) {
                    Some(k) => W::Num(vec![(k, false)]),
                    None => W::NotNum,
                }
            }
            // Everything else is not a tracked number source.
            _ => W::NotNum,
        }
    }

    /// [`Self::width_of`]'s BinOp arm — per-operator width facing.
    fn width_of_binop(&self, op: BinOp, left: ExprId, right: ExprId, scope: &Scope) -> W {
        match op {
            BinOp::Div | BinOp::Pow => W::F64,
            // ToInt32 firewall: bitwise / shift results are int32
            // regardless of operand width (JS spec §13.9 / §13.12).
            BinOp::BitAnd
            | BinOp::BitOr
            | BinOp::BitXor
            | BinOp::Shl
            | BinOp::Shr
            | BinOp::UShr => W::Num(Vec::new()),
            BinOp::Lt
            | BinOp::Gt
            | BinOp::Le
            | BinOp::Ge
            | BinOp::Eq
            | BinOp::Neq
            | BinOp::LooseEq
            | BinOp::LooseNeq => W::NotNum,
            BinOp::Mul => self.width_of_mul(left, right, scope),
            // Add/Sub with a literal int increment stays a linear
            // small-step counter (`i = i + 1`) — passes through
            // unmarked. A non-constant increment can be any size
            // per step, so it marks growth. Known boundary: a
            // huge-literal step (`n += 2**52`) is not marked —
            // same physical-trip-count carve-out as counters
            // (see rfc 20260611-ann-width-unification §5.5).
            BinOp::Add | BinOp::Sub => {
                let w = join(self.width_of(left, scope), self.width_of(right, scope));
                if self.is_int_const(left) || self.is_int_const(right) {
                    w
                } else {
                    mark_growth(w)
                }
            }
            // W3 — `%` can mint -0 at runtime (negative dividend
            // with zero remainder), unrepresentable in i64: any
            // slot the result reaches must stay f64. A plain
            // integer-literal dividend is non-negative (negation
            // is Unary), bounding the remainder in [+0, |b|) — it
            // keeps the pass-through join, where growth still
            // flows through (the intermediate `(n*3+1) % m`
            // diverges before the mod contracts it).
            //
            // srem runtime-0 (§5.3 follow-up close) — the divisor
            // must ALSO be a provably non-zero literal: `7 % b`
            // with a runtime-zero b is NaN per spec (the lowering
            // carve mirrors this; aarch64 sdiv-by-zero silently
            // handed the dividend back).
            BinOp::Mod => {
                let dividend_ok = matches!(
                    self.ast.get_expr(left),
                    Expr::Number(n) if !literal_is_f64(*n)
                );
                let divisor_ok = matches!(
                    self.ast.get_expr(right),
                    Expr::Number(m) if *m != 0.0 && !literal_is_f64(*m)
                );
                if dividend_ok && divisor_ok {
                    join(self.width_of(left, scope), self.width_of(right, scope))
                } else {
                    W::F64
                }
            }
            BinOp::LAnd | BinOp::LOr => {
                join(self.width_of(left, scope), self.width_of(right, scope))
            }
        }
    }

    /// Mul grows multiplicatively — any slot feeding it is
    /// a growth dependency (W5). In a cycle that means
    /// geometric blow-up past 2^53 within tens of steps.
    ///
    /// W3 C4 + S9 — int `a * b` mints -0 when one factor
    /// is zero and the other negative; a non-positive
    /// integer-literal cofactor or two non-literal factors
    /// (runtime zero × runtime negative) make that
    /// reachable, so the result must stay f64 (mirrors the
    /// lower_binop float predicate). A positive literal
    /// cofactor, a literal pair missing the zero×negative
    /// pattern, or a square (`x * x` — a value times
    /// itself is never negative×zero, 0*0 = +0) keeps the
    /// int face.
    fn width_of_mul(&self, left: ExprId, right: ExprId, scope: &Scope) -> W {
        let lit = |eid: ExprId| -> Option<i64> {
            match self.ast.get_expr(eid) {
                Expr::Number(n) if !literal_is_f64(*n) => Some(*n as i64),
                Expr::Unary {
                    op: UnaryOp::Neg,
                    expr,
                } => match self.ast.get_expr(*expr) {
                    Expr::Number(n) if !literal_is_f64(*n) => Some(-(*n as i64)),
                    _ => None,
                },
                _ => None,
            }
        };
        let square = matches!(
            (self.ast.get_expr(left), self.ast.get_expr(right)),
            (Expr::Ident(l), Expr::Ident(r)) if l == r
        );
        let minus_zero_risk = match (lit(left), lit(right)) {
            (Some(x), Some(y)) => (x == 0 && y < 0) || (x < 0 && y == 0),
            (Some(c), None) | (None, Some(c)) => c <= 0,
            (None, None) => !square,
        };
        if minus_zero_risk {
            W::F64
        } else {
            mark_growth(join(
                self.width_of(left, scope),
                self.width_of(right, scope),
            ))
        }
    }

    /// [`Self::width_of`]'s Call arm — monomorphised retargets read
    /// their Ret key; named fns read `Ret(f)`; Math.* is libm-shaped
    /// f64; other member calls and indirect fn-value calls read
    /// through the container lattice.
    fn width_of_call(&self, eid: ExprId, callee: ExprId, scope: &Scope) -> W {
        if let Some(mono) = self.retargets.get(&eid) {
            return W::Num(vec![(SlotKey::Ret(mono.clone()), false)]);
        }
        match self.ast.get_expr(callee) {
            Expr::Ident(f) if self.fn_params.contains_key(f) => {
                W::Num(vec![(SlotKey::Ret(f.clone()), false)])
            }
            // Math.* numeric intrinsics are libm-shaped f64
            // (same set the retired infer_arg_width flagged).
            // Other member calls read through the container
            // lattice (`xs.pop()` → the receiver's elem point,
            // `xs.reduce(cb)` → the callback's ret).
            // `s.charCodeAt(i)` answers NaN out of range (§22.1.3.2
            // step 5), so its slot has to be able to hold one — the
            // kernel's ABI is `f64` for exactly that reason. Matched
            // by method name without a receiver-type gate: f64 is
            // always a sound width for a Number, and a too-narrow
            // answer here is not a slow program but a broken build
            // (an FPR value reaching a GPR-only consumer).
            Expr::Member { name: m, .. } if m == "charCodeAt" => W::F64,
            // §19.2.4 / §19.2.5 / §21.1.1 — the three global
            // functions that answer a Number. Same reasoning as
            // `charCodeAt` one line up, and the same failure when it
            // is missing: they all can answer NaN or a fraction, so
            // an integer slot cannot hold what they produce, and the
            // mismatch is not a slow program but a build that dies in
            // the register allocator ("materialize_operand_gpr called
            // on ValueId(N) holding Fpr").
            //
            // They fell through to the indirect-call arm below, which
            // has no key for a global and answered NotNum — invisible
            // until a binding carried an explicit `: number`, because
            // an unannotated one takes its width from the initializer
            // instead. `const n: number = Number(s); n < 5` did not
            // compile.
            //
            // Ordered AFTER the `fn_params` arm, so a user function
            // of the same name keeps its own return width.
            Expr::Ident(f) if f == "Number" || f == "parseFloat" || f == "parseInt" => W::F64,
            Expr::Member { obj, .. } => {
                if let Expr::Ident(ns) = self.ast.get_expr(*obj)
                    && ns == "Math"
                {
                    W::F64
                } else {
                    match self.container_key_lookup(eid, scope) {
                        Some(k) => W::Num(vec![(k, false)]),
                        None => W::NotNum,
                    }
                }
            }
            // F1 — indirect call through an fn value (local
            // fn-value slot or a chained call's ret): the
            // result reads the value's `__ret` projection,
            // which the flow unions glue onto the residents'
            // Ret keys.
            _ => match self.container_key_lookup(callee, scope) {
                Some(k) => W::Num(vec![(
                    SlotKey::Field(Box::new(k), "__ret".to_string()),
                    false,
                )]),
                None => W::NotNum,
            },
        }
    }
}
