//! Expression width classification + slot resolution for the
//! constraint walk in `walk.rs`.

use super::{Analysis, Scope, SlotKey, W, join, literal_is_f64};
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

    /// A literal whose value an i64 slot holds exactly — the Add/Sub
    /// increment shape that keeps a self-feeding counter linear-small
    /// (`i = i + 1` family). Non-constant increments (slot / param /
    /// call / member) can be arbitrarily large per step, so they mark
    /// growth instead.
    fn is_int_const(&self, eid: ExprId) -> bool {
        match self.ast.get_expr(eid) {
            Expr::Number(n) => !literal_is_f64(*n),
            Expr::Unary {
                op: UnaryOp::Neg,
                expr,
            } => {
                matches!(self.ast.get_expr(*expr), Expr::Number(n) if !literal_is_f64(*n) && *n != 0.0)
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
            Expr::BinOp { op, left, right } => match op {
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
                // Mul grows multiplicatively — any slot feeding it is
                // a growth dependency (W5). In a cycle that means
                // geometric blow-up past 2^53 within tens of steps.
                BinOp::Mul => mark_growth(join(
                    self.width_of(*left, scope),
                    self.width_of(*right, scope),
                )),
                // Add/Sub with a literal int increment stays a linear
                // small-step counter (`i = i + 1`) — passes through
                // unmarked. A non-constant increment can be any size
                // per step, so it marks growth. Known boundary: a
                // huge-literal step (`n += 2**52`) is not marked —
                // same physical-trip-count carve-out as counters
                // (see rfc 20260611-ann-width-unification §5.5).
                BinOp::Add | BinOp::Sub => {
                    let w = join(self.width_of(*left, scope), self.width_of(*right, scope));
                    if self.is_int_const(*left) || self.is_int_const(*right) {
                        w
                    } else {
                        mark_growth(w)
                    }
                }
                // Mod passes growth through: the intermediate value
                // (`(n*3+1) % m`) already diverges between f64 and
                // i64 before the mod contracts it.
                BinOp::Mod | BinOp::LAnd | BinOp::LOr => {
                    join(self.width_of(*left, scope), self.width_of(*right, scope))
                }
            },
            Expr::Unary { op, expr } => match op {
                UnaryOp::Neg => {
                    // `-0` spells Neg(Number(0)) — meaningful f64 sign
                    // state. Other negations preserve operand width.
                    if let Expr::Number(n) = self.ast.get_expr(*expr)
                        && *n == 0.0
                    {
                        W::F64
                    } else {
                        self.width_of(*expr, scope)
                    }
                }
                // `+x` is ToNumber — strings coerce to fractional
                // values ("3.5" → 3.5), so the result is f64-possible.
                UnaryOp::Plus => W::F64,
                UnaryOp::BitNot => W::Num(Vec::new()),
                UnaryOp::Not => W::NotNum,
            },
            Expr::Call { callee, .. } => {
                if let Some(mono) = self.retargets.get(&eid) {
                    return W::Num(vec![(SlotKey::Ret(mono.clone()), false)]);
                }
                match self.ast.get_expr(*callee) {
                    Expr::Ident(f) => {
                        if self.fn_params.contains_key(f) {
                            W::Num(vec![(SlotKey::Ret(f.clone()), false)])
                        } else {
                            W::NotNum
                        }
                    }
                    // Math.* numeric intrinsics are libm-shaped f64
                    // (same set the retired infer_arg_width flagged).
                    Expr::Member { obj, .. } => {
                        if let Expr::Ident(ns) = self.ast.get_expr(*obj)
                            && ns == "Math"
                        {
                            W::F64
                        } else {
                            W::NotNum
                        }
                    }
                    _ => W::NotNum,
                }
            }
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
            Expr::As { expr, .. } => self.width_of(*expr, scope),
            Expr::PostIncr { target, .. } => {
                if let Expr::Ident(n) = self.ast.get_expr(*target)
                    && let Some(k) = self.resolve(n, scope)
                {
                    W::Num(vec![(k, false)])
                } else {
                    W::NotNum
                }
            }
            // Member / Index reads keep their annotation-derived width
            // (container-width face is W4 scope); everything else is
            // not a tracked number source.
            _ => W::NotNum,
        }
    }
}
