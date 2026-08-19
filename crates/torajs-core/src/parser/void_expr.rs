//! `void <expr>` — ES §13.5.2. Split out of `expr_prec.rs` (r443):
//! that file is at its size cap, so growing it in place was not
//! available (the delete_expr sibling precedent).

use super::Parser;
use crate::ast::{Expr, ExprId};
use crate::lexer::Token;

impl Parser<'_> {
    /// V3-18 m1.h.30 — `void <expr>` evaluates expr (for side
    /// effects) then yields `undefined`. Desugars to
    /// `Expr::Sequence { left: <expr>, right: Expr::Ident
    /// ("undefined") }` so `void 0` is the same value as the
    /// `undefined` Ident everywhere: Type::Undefined at check
    /// time (binop undef-id hints fire), ConstPtrNull at SSA.
    /// RC-4 F1b-1: the earlier String("undefined") stand-in
    /// made `x !== void 0` a *content* compare (str_eq) — a
    /// real "undefined" string compared equal to the undefined
    /// literal, and a null-slot Str operand SIGSEGV'd inside
    /// str_eq (test262 S15.5.4.10 family).
    pub(super) fn parse_void_expr(&mut self) -> Result<ExprId, String> {
        debug_assert!(matches!(self.peek(), Token::Void));
        self.pos += 1;
        let inner = self.parse_unary()?;
        // RFC 20260713-array-proto-residual blade 5 — a pure
        // literal operand folds to the plain `undefined` ident
        // (ES §13.5.2 evaluates then discards; literals have no
        // effects). The Sequence wrapper defeated every
        // undefined-shape probe downstream (any-literal pack /
        // let-binding lanes tagged `void 0` as null — printed
        // "null", typeof "object"). Effectful operands keep the
        // Sequence (evaluation order preserved).
        if matches!(
            self.ast.get_expr(inner),
            Expr::Number(_) | Expr::String(_) | Expr::Bool(_) | Expr::Null
        ) || matches!(self.ast.get_expr(inner), Expr::Ident(n) if n == "undefined")
        {
            let id = self.ast.add_expr(Expr::Ident("undefined".into()));
            // The fold erases the `void`, but an assignment TARGET
            // must still see it (§13.15.1 — invalid
            // AssignmentTargetType is a parse-time SyntaxError,
            // while `undefined = x` fails at runtime).
            self.void_folds.insert(id.0);
            return Ok(id);
        }
        let undef = self.ast.add_expr(Expr::Ident("undefined".into()));
        return Ok(self.ast.add_expr(Expr::Sequence {
            left: inner,
            right: undef,
        }));
    }
}
