//! Null narrowing across `if` / ternary guards: which binding a
//! condition narrows, and the narrow's application and restore.

use super::*;

impl Checker {
    /// V3-18 wedge — detect a flow-narrowing cond shape on the
    /// form `<ident> !== null` / `null !== <ident>` (and === for
    /// the inverse polarity). Returns (binding-name, inner-type,
    /// then-narrows). Polarity = true means the then-branch
    /// narrows, false means the else-branch.
    pub(crate) fn collect_null_narrow(
        &self,
        ast: &Ast,
        cond: ExprId,
    ) -> Option<(String, Type, bool)> {
        // `a && b` is true only when both are, `a || b` false only when
        // both are: an operand's narrow on that side is the whole
        // condition's (`if (x === null || x.w < 2) return`), unless the
        // right operand, run after the left, writes the binding
        if let Expr::BinOp {
            op: op @ (BinOp::LAnd | BinOp::LOr),
            left,
            right,
        } = ast.get_expr(cond)
        {
            let side = *op == BinOp::LAnd;
            let from = |e| self.collect_null_narrow(ast, e).filter(|n| n.2 == side);
            return match from(*left) {
                Some(n) if !crate::check_assigns_to::expr_writes(ast, *right, &n.0) => Some(n),
                _ => from(*right),
            };
        }
        // Cond shape 1 — `<ident> !== null` / `null !== <ident>`
        // (and `===` for the inverse polarity). The historical
        // narrow shape, kept verbatim.
        if let Expr::BinOp { op, left, right } = ast.get_expr(cond) {
            let polarity = match op {
                BinOp::Neq | BinOp::LooseNeq => Some(true),
                BinOp::Eq | BinOp::LooseEq => Some(false),
                _ => None,
            };
            if let Some(polarity) = polarity {
                let name = match (
                    crate::check_assign_narrow::guard_operand(ast, *left),
                    crate::check_assign_narrow::guard_operand(ast, *right),
                ) {
                    (Expr::Ident(n), Expr::Null) => Some(n.clone()),
                    (Expr::Null, Expr::Ident(n)) => Some(n.clone()),
                    // Chunk 629 — `x !== undefined` narrows the same
                    // way: `Nullable<T>` ≡ `T | null | undefined`
                    // (P1.7) and the checker's Nullable narrow has
                    // always been the P1.7 collapse (`!== null`
                    // doesn't exclude undefined either). The
                    // undefined literal parses as Ident("undefined").
                    (Expr::Ident(a), Expr::Ident(b)) if b == "undefined" && a != "undefined" => {
                        Some(a.clone())
                    }
                    (Expr::Ident(a), Expr::Ident(b)) if a == "undefined" && b != "undefined" => {
                        Some(b.clone())
                    }
                    _ => None,
                };
                if let Some(name) = name {
                    let info = self.lookup(&name)?;
                    // The DECLARED type, not the live one — see
                    // `Checker::assign_declared_ty`.
                    if let Type::Nullable(inner) = self.assign_declared_ty(&name, &info.ty) {
                        return Some((name, *inner, polarity));
                    }
                }
            }
        }
        // Cond shape 2 (truthy-narrow wedge) — bare ident or
        // `!ident` where ident is Nullable<T>. Per JS spec
        // §7.1.2 ToBoolean, `null` is falsy, so `if (s) ...`
        // narrows the then-branch to T (or the else-branch via
        // `!s`). For Nullable<Number> the then-branch also
        // excludes 0 and for Nullable<String> it excludes "",
        // but that just makes the value *more* constrained — it
        // is still a valid T, which is all the narrow promises.
        // Other primitives (number, string, boolean, struct) on
        // their own are not Nullable here, so this hook only
        // fires when the binding's declared type is Nullable.
        let (target, polarity) = match crate::check_assign_narrow::guard_operand(ast, cond) {
            Expr::Ident(n) => (n.clone(), true),
            Expr::Unary {
                op: crate::ast::UnaryOp::Not,
                expr,
            } => {
                if let Expr::Ident(n) = crate::check_assign_narrow::guard_operand(ast, *expr) {
                    (n.clone(), false)
                } else {
                    return None;
                }
            }
            _ => return None,
        };
        let info = self.lookup(&target)?;
        if let Type::Nullable(inner) = self.assign_declared_ty(&target, &info.ty) {
            Some((target, *inner, polarity))
        } else {
            None
        }
    }

    /// V3-18 wedge — narrow the binding `name` to `inner_ty`
    /// in the innermost scope that owns it; return the previous
    /// type so it can be restored after the narrowed branch.
    pub(crate) fn apply_narrow(&mut self, name: &str, inner_ty: Type) -> Option<Type> {
        for s in self.scopes.iter_mut().rev() {
            if let Some(info) = s.get_mut(name) {
                let prev = info.ty.clone();
                // A narrow may not move the binding out of the lane
                // its slot lives in — see `narrow_within_lane`.
                info.ty = crate::check::narrow_within_lane(&prev, inner_ty);
                if matches!(prev, Type::Nullable(_))
                    && !matches!(info.ty, Type::Nullable(_))
                    && info.null_declared.is_none()
                {
                    info.null_declared = Some(prev.clone());
                }
                return Some(prev);
            }
        }
        None
    }

    pub(crate) fn restore_narrow(&mut self, name: &str, prev_ty: Type) {
        for s in self.scopes.iter_mut().rev() {
            if let Some(info) = s.get_mut(name) {
                info.ty = prev_ty;
                return;
            }
        }
    }
}
