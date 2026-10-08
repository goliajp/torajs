//! Moving a block-local `const` string into an array slot.
//!
//! `const s = a + b; xs[i] = s; … s.charCodeAt(k) …` normally pays an
//! inc for the slot's stake and a drop when the block closes: two rc
//! writes that cancel. When the binding owns its value and the rest
//! of the block only READS it — nothing after the store can release
//! the slot (no element store, no call, no write to `xs` or to any
//! heap-typed binding) and nothing hands `s` anywhere — the store
//! takes the binding's own stake and the binding is marked moved, so
//! the block close skips it. The slot keeps the cell alive for the
//! reads that follow.
//!
//! The decision is made per statement before it lowers
//! ([`note`]); the store lane spends it ([`LowerCtx::take_slot_move`]).
//! Ordering in the store lane: the out-of-bounds arm still mints and
//! releases (its kernel drops the passed stake when it throws), and
//! the binding is marked moved only after the integrity guard, whose
//! throw path must still release it.

use crate::ast::{BinOp as AstBinOp, Expr, ExprId, Stmt};
use crate::check::Type as CT;
use crate::ssa::{BlockId, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Before lowering `stmts[k]`: record its element store as a move when
/// the shape and everything after it in the block admit one.
pub(crate) fn note(ctx: &mut LowerCtx<'_>, stmts: &[Stmt], k: usize) {
    let Stmt::Expr(eid) = &stmts[k] else {
        return;
    };
    let Expr::Assign { target, value } = ctx.ast.get_expr(*eid) else {
        return;
    };
    let (Expr::Index { obj, .. }, Expr::Ident(s)) =
        (ctx.ast.get_expr(*target), ctx.ast.get_expr(*value))
    else {
        return;
    };
    let Expr::Ident(xs) = ctx.ast.get_expr(*obj) else {
        return;
    };
    if s == xs || !owns_const_str(ctx, &stmts[..k], s) {
        return;
    }
    let scan = Scan { ctx, s, xs };
    if stmts[k + 1..].iter().all(|st| scan.stmt_ok(st)) {
        ctx.proofs.slot_moves.insert(*eid, s.clone());
    }
}

/// `s` is a `const` declared earlier in this block, holding a stake of
/// its own in a plain slot nothing captures.
fn owns_const_str(ctx: &LowerCtx<'_>, before: &[Stmt], s: &str) -> bool {
    let declared_const = before.iter().any(
        |st| matches!(st, Stmt::LetDecl { mutable: false, name, is_var: false, .. } if name == s),
    );
    let in_frame = ctx
        .scope_stack
        .last()
        .is_some_and(|f| f.iter().any(|n| n == s));
    let Some(info) = ctx.locals.get(s) else {
        return false;
    };
    declared_const
        && in_frame
        && info.ty == Type::Str
        && !info.moved
        && !info.borrowed
        && !info.boxed
        && !ctx.boxed_noncopy_lets.contains(s)
        && !ctx.escape_captured_lets.contains(s)
        && !ctx.stack_alloced_locals.contains(s)
}

/// The read-only admission walk over the statements after the store.
/// Anything not listed is refused: a refusal costs the two rc writes,
/// an admission that is wrong costs a freed cell.
struct Scan<'c, 'a> {
    ctx: &'c LowerCtx<'a>,
    s: &'c str,
    xs: &'c str,
}

impl Scan<'_, '_> {
    fn stmt_ok(&self, st: &Stmt) -> bool {
        match st {
            Stmt::Expr(e) => self.expr_ok(*e),
            Stmt::LetDecl {
                name,
                init,
                is_var: false,
                ..
            } => name != self.s && name != self.xs && self.expr_ok(*init),
            Stmt::Block(v) | Stmt::Multi(v) => v.iter().all(|st| self.stmt_ok(st)),
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.expr_ok(*cond)
                    && self.stmt_ok(then_branch)
                    && else_branch.as_ref().is_none_or(|e| self.stmt_ok(e))
            }
            Stmt::Break(_) | Stmt::Continue(_) => true,
            _ => false,
        }
    }

    fn primitive(&self, e: ExprId) -> bool {
        matches!(
            self.ctx.expr_types.get(&e),
            Some(CT::Number | CT::String | CT::Boolean)
        )
    }

    fn is_s(&self, e: ExprId) -> bool {
        matches!(self.ctx.ast.get_expr(e), Expr::Ident(n) if n == self.s)
    }

    /// A string binding read in place: `s` itself or any other string
    /// local, never the array.
    fn str_recv(&self, e: ExprId) -> bool {
        matches!(self.ctx.ast.get_expr(e), Expr::Ident(n) if n != self.xs)
            && matches!(self.ctx.expr_types.get(&e), Some(CT::String))
    }

    fn expr_ok(&self, e: ExprId) -> bool {
        match self.ctx.ast.get_expr(e) {
            Expr::Ident(n) => n != self.s && n != self.xs,
            Expr::Number(_) | Expr::String(_) | Expr::Bool(_) => true,
            Expr::Member { obj, name } => name == "length" && self.str_recv(*obj),
            Expr::Call { callee, args } => {
                matches!(
                    self.ctx.ast.get_expr(*callee),
                    Expr::Member { obj, name }
                        if (name == "charCodeAt" || name == "codePointAt") && self.str_recv(*obj)
                ) && args.iter().all(|a| self.expr_ok(*a))
            }
            Expr::BinOp { op, left, right } => {
                !matches!(op, AstBinOp::LAnd | AstBinOp::LOr)
                    && self.operand_ok(*left)
                    && self.operand_ok(*right)
            }
            Expr::Unary { expr, .. } => self.primitive(*expr) && self.expr_ok(*expr),
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => self.expr_ok(*cond) && self.expr_ok(*then_branch) && self.expr_ok(*else_branch),
            // only a Copy local: overwriting a heap binding releases
            // its old value, which may be the array that holds `s`
            Expr::Assign { target, value } => {
                let copy_local = matches!(
                    self.ctx.ast.get_expr(*target),
                    Expr::Ident(n) if n != self.s && n != self.xs
                        && self.ctx.locals.get(n).is_some_and(|i| i.ty.is_copy())
                );
                copy_local && self.expr_ok(*value)
            }
            _ => false,
        }
    }

    /// A binary operand: a primitive (no user conversion hook can run),
    /// where `s` itself is a plain read.
    fn operand_ok(&self, e: ExprId) -> bool {
        self.primitive(e) && (self.is_s(e) || self.expr_ok(e))
    }
}

impl LowerCtx<'_> {
    /// The store lane's side of [`note`]: the binding whose stake the
    /// element store at `eid` takes, if the lowered store is the plain
    /// typed `Str` slot write of that binding's own value.
    pub(crate) fn take_slot_move(
        &mut self,
        eid: ExprId,
        elem_ty: Type,
        v: &Operand,
        v_raw: &Operand,
        plain: bool,
    ) -> Option<String> {
        let s = self.proofs.slot_moves.remove(&eid)?;
        let info = self.locals.get(&s)?;
        let fits = plain
            && elem_ty == Type::Str
            && v == v_raw
            && self.operand_ty(v) == Type::Str
            && !info.moved
            && !info.borrowed;
        fits.then_some(s)
    }

    /// The typed element store's integrity guard, then the move
    /// [`Self::take_slot_move`] granted: the guard's throw path runs
    /// before the binding is marked, so it still releases `s`. Returns
    /// the guarded arm, left open for the caller's own store.
    pub(crate) fn emit_integrity_guard_then_move(
        &mut self,
        arr_val: &Operand,
        idx_val: &Operand,
        moved_from: Option<String>,
    ) -> BlockId {
        let guarded = self.emit_index_integrity_guard(arr_val, idx_val);
        if let Some(info) = moved_from.and_then(|s| self.locals.get_mut(&s)) {
            info.moved = true;
        }
        guarded
    }
}
