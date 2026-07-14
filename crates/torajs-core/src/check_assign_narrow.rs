//! Straight-line assignment narrowing (ut3 — `let b: string | null =
//! null; b = "x"; b.length`).
//!
//! TS control-flow analysis narrows a union-typed binding to the
//! assigned value's type after an assignment. The checker's V3-18
//! narrow machinery only fired on guard shapes (`if (b !== null)`),
//! so a straight-line assign left the binding `Nullable<T>` and every
//! subsequent member/arith use was rejected. This module is the
//! conservative basic-block subset of the TS behavior:
//!
//! - A narrow is only minted by a STATEMENT-level `Ident = value`
//!   assign (`check_stmt`'s `Stmt::Expr(Assign)` path via
//!   `check_assign_ident`); an assign buried in a ternary arm or a
//!   short-circuit rhs never narrows (it may not execute).
//! - Assigning `null` (or any non-narrowable value) restores the
//!   declared type immediately — the following uses see `Nullable<T>`
//!   again.
//! - Every compound-statement boundary flushes the ledger (restores
//!   every narrowed binding to its declared type): branch joins
//!   (if/switch/try segment ends), loop back-edges (while/for/for-of
//!   entry + exit), and fn-body entry. A narrow therefore never
//!   outlives the straight-line segment that established it — the
//!   checker stays sound without a fixpoint CFA.
//!
//! The ledger stores the DECLARED type so `check_assign_ident` can
//! validate later assigns against the union (`b = null` after
//! `b = "x"` is legal — the narrow must not shrink the assignable
//! surface), and so a flush restores the true declared type rather
//! than a stacked narrow.

use crate::check::{Checker, Type};
use crate::check_assignable::is_assignable_to_resolved;

impl Checker {
    /// The declared (pre-narrow) type of `name` for assignability
    /// checks — the ledger entry when the binding is currently
    /// narrowed, the live binding type otherwise.
    pub(crate) fn assign_declared_ty(&self, name: &str, live_ty: &Type) -> Type {
        self.assign_narrows
            .get(name)
            .cloned()
            .unwrap_or_else(|| live_ty.clone())
    }

    /// Statement-level `name = value` narrowing hook. A non-null
    /// value assignable to the Nullable's inner type narrows the
    /// binding to the inner type; anything else restores the
    /// declared type. No-op for non-Nullable declared types.
    pub(crate) fn apply_assign_narrow(&mut self, name: &str, value_ty: &Type) {
        let Some(live) = self.lookup(name) else {
            return;
        };
        let declared = self.assign_declared_ty(name, &live.ty);
        let Type::Nullable(inner) = declared.clone() else {
            return;
        };
        let narrowable = !matches!(
            value_ty,
            Type::Null | Type::Undefined | Type::Nullable(_) | Type::Void | Type::Any
        ) && is_assignable_to_resolved(
            &inner,
            value_ty,
            &self.class_structs,
            &self.aliases,
            &self.generic_alias_decls,
        );
        if narrowable {
            self.apply_narrow(name, (*inner).clone());
            self.assign_narrows
                .entry(name.to_string())
                .or_insert(declared);
        } else if let Some(d) = self.assign_narrows.remove(name) {
            self.apply_narrow(name, d);
        }
    }

    /// Demote-only arm for EXPRESSION-level assigns (ternary arm /
    /// sequence / short-circuit rhs — paths that may not execute):
    /// a possibly-null value kills an existing narrow immediately,
    /// but a non-null value never mints one (that stays exclusive
    /// to the statement-level hook, where execution is certain).
    pub(crate) fn assign_narrow_demote(&mut self, name: &str, value_ty: &Type) {
        if matches!(
            value_ty,
            Type::Null | Type::Undefined | Type::Nullable(_) | Type::Void | Type::Any
        ) && let Some(d) = self.assign_narrows.remove(name)
        {
            self.apply_narrow(name, d);
        }
    }

    /// Restore every assignment-narrowed binding to its declared
    /// type and clear the ledger. Called at compound-statement
    /// boundaries (see module doc). Every call site sits after the
    /// sub-structure's scope popped (or before a fn scope pushes),
    /// so the by-name restore always lands on the narrowed binding,
    /// never a shadow.
    pub(crate) fn flush_assign_narrows(&mut self) {
        if self.assign_narrows.is_empty() {
            return;
        }
        let entries: Vec<(String, Type)> = self.assign_narrows.drain().collect();
        for (name, declared) in entries {
            self.restore_narrow(&name, declared);
        }
    }
}
