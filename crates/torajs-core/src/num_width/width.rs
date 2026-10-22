//! Expression width classification + slot resolution for the
//! constraint walk in `walk.rs`.

use super::magnitude::{Flow, MAX_COUNTER_STEP};
use super::{Analysis, Scope, SlotKeyId, W, join, literal_is_f64};
use crate::ast::{BinOp, Expr, ExprId, UnaryOp};

/// Mark every slot dependency as reaching the consumer through a
/// growth op (W5). Inside an assignment-graph cycle that marking
/// makes the cycle non-i64-safe.
fn mark_growth(w: W) -> W {
    match w {
        W::Num(deps) => W::Num(deps.into_iter().map(|(k, _)| (k, true)).collect()),
        other => other,
    }
}

impl<'a> Analysis<'a> {
    pub(super) fn add_constraint(&mut self, target: SlotKeyId, (w, m): Flow) {
        self.record_write(target, &w, m);
        match w {
            W::F64 => self.seeds.push(target),
            W::Num(deps) => {
                for (d, growth) in deps {
                    self.edges.entry(d).or_default().push((target, growth));
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
    fn is_int_const(&self, eid: ExprId, scope: &Scope) -> bool {
        let small = |n: f64| !literal_is_f64(n) && n.abs() <= MAX_COUNTER_STEP as f64;
        match self.ast.get_expr(eid) {
            Expr::Number(n) => small(*n),
            Expr::Unary {
                op: UnaryOp::Neg,
                expr,
            } => {
                matches!(self.ast.get_expr(*expr), Expr::Number(n) if small(*n) && *n != 0.0)
            }
            // A step spelled `const step = 3` is the same counter as
            // one spelled `3` (rotation 507 / 506-06): without this
            // the accumulator was marked growth, kept an f64 slot,
            // and paid a versioned loop plus a per-iteration guard
            // the literal form never sees. `mutable: false` is the
            // guarantee the value cannot change under the read; the
            // slot key is what makes a shadowing local of the same
            // name miss the table rather than borrow its value.
            Expr::Ident(n) => self
                .resolve(n, scope)
                .and_then(|k| self.const_ints.get(&k).copied())
                .is_some_and(small),
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
                    None => self.unresolved_read(n, scope),
                }
            }
            Expr::BinOp { op, left, right } => self.width_of_binop(*op, *left, *right, scope),
            Expr::Unary { op, expr } => match op {
                UnaryOp::Neg => {
                    // `-0` spells Neg(Number(0)) — meaningful f64 sign
                    // state. A non-zero literal negation is the
                    // lower-time ConstI64 fold, and a guard that rules
                    // out a zero operand (`nonzero_neg`) leaves an
                    // exact integer subtract: both keep the operand's
                    // width. W3 S8 — any other `-x` mints -0 when x is
                    // 0, so lowering floats it and the slot mirrors F64.
                    match self.ast.get_expr(*expr) {
                        Expr::Number(n) if *n != 0.0 => self.width_of(*expr, scope),
                        _ if self.nonzero_negs.contains(expr) => self.width_of(*expr, scope),
                        _ => W::F64,
                    }
                }
                // `+x` is ToNumber — strings coerce to fractional
                // values ("3.5" → 3.5), so the result is f64-possible.
                UnaryOp::Plus => W::F64,
                UnaryOp::BitNot => W::Num(Vec::new()),
                UnaryOp::Not => W::NotNum,
            },
            Expr::Call { callee, args } => self.width_of_call(eid, *callee, args, scope),
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
                } else if matches!(ty_ann.as_str(), "any" | "unknown" | "object") {
                    // The other direction, same reason. A value leaves
                    // the typed world here, and whatever of it reaches a
                    // number slot is materialized by a ToNumber whose
                    // result is an f64 — the width of what went IN says
                    // nothing about it. `let n: number = ("2.5" as any)`
                    // answered 2: the source is a Str, which is not a
                    // number source at all, so the slot kept its default
                    // I64 and truncated the 2.5 the ToNumber produced.
                    // (`as any` over something already Any takes this
                    // arm too, which is where it was already heading.)
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
                // Ahead of the lattice: an await of what is no promise
                // is its operand; and the two reads that can hand the
                // sentinel back with no key to carry it (an awaited
                // `Promise<number>`, an accessor whose getter falls
                // through — `sentinel_member_reads`) land in F64, as
                // the sentinel is a NaN bit pattern that lowering arms
                // unconditionally; an I64 slot arms nothing, and the
                // `NaN` shown for `undefined` looks exactly like the
                // taint table having missed the value.
                if let Some(w) = self.awaited_width(eid, scope) {
                    return w;
                }
                if self.sentinel_member_reads.contains(&eid) || self.namespace_number_read(eid) {
                    return W::F64;
                }
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
            // per step, so it marks growth. Whether the sum leaves
            // the exact window is the magnitude pass's question.
            BinOp::Add | BinOp::Sub => {
                let w = join(self.width_of(left, scope), self.width_of(right, scope));
                if self.is_int_const(left, scope) || self.is_int_const(right, scope) {
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
    /// cofactor or a literal pair missing the zero×negative
    /// pattern keeps the int face. A square (`x * x`) cannot
    /// mint -0 but is past 2^53 once `x` passes 2^26.5, which
    /// a counter reaches in well under a second; it keeps the
    /// int face only as a compare operand, which no slot
    /// receives (`ssa_lower_binop::square`).
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
        let minus_zero_risk = match (lit(left), lit(right)) {
            (Some(x), Some(y)) => (x == 0 && y < 0) || (x < 0 && y == 0),
            (Some(c), None) | (None, Some(c)) => c <= 0,
            (None, None) => true,
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
    fn width_of_call(&self, eid: ExprId, callee: ExprId, args: &[ExprId], scope: &Scope) -> W {
        if let Some(f) = self.direct_callee(eid, callee) {
            return W::Num(vec![(self.keys.ret(&f), false)]);
        }
        match self.ast.get_expr(callee) {
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
            //
            // Under a guard that proves the index in range the NaN
            // answer is unreachable and the code unit is an integer;
            // the lowering reads the same proof and drops that arm
            // (`bounds_walk::char_code_in_bounds`).
            Expr::Member { name: m, .. } if m == "charCodeAt" => {
                if self.char_code_in_bounds(callee, args) {
                    W::Num(Vec::new())
                } else {
                    W::F64
                }
            }
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
                } else if let Some((f, _)) = self.replayed_args(callee, args)
                    && let Some(k) = self.container_key_lookup(f, scope)
                {
                    // `f.call(t, ...)` / `f.apply(t, ...)` on an fn value
                    // answers what `f` returns, as a plain indirect call
                    W::Num(vec![(self.keys.field(k, "__ret"), false)])
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
                Some(k) => W::Num(vec![(self.keys.field(k, "__ret"), false)]),
                None => W::NotNum,
            },
        }
    }
}
