//! The implicit default an optional parameter (`name?: T`) binds
//! when a call site omits it — split from `param_list.rs` at the
//! size cap (rotation 227's watch: the file sat 3 lines under 500).
//!
//! ES §9.2: an absent optional binds undefined. That is one value with
//! as many spellings as there are slot widths, and which one applies
//! follows the same materialization rule the type resolver uses —
//! asked directly, so the two cannot drift apart.
//!
//! Deliberately NOT `desugar_async`'s minter for the same question:
//! that one answers `number` with a literal F64 sentinel, precisely so
//! `num_width` widens a promise's value field to carry it. Here the
//! slot is already Any and that literal would pull it back to a raw
//! F64. The two positions want opposite things from one question.
//!
//! Until rotation 311 a TYPED optional bound a literal `null` instead,
//! on the recorded grounds that a `__nullable(T)` slot had no undefined
//! arm. It has one now on both sides — a scalar `T | null` materializes
//! as Any and spells it `ANY_UNDEF` (RFC 20260710 C4, widened to
//! let/param/return in rotation 310), a pointer-shaped one has the
//! per-type sentinel cell (RFC 20260707) — so the workaround outlived
//! its reason. It was not a harmless one: `g()` on
//! `function g(x?: string)` bound null, which made `x === undefined`
//! answer false and printed `null` where bun prints `undefined`.
//!
//! Shared by parse_fn, the ctor param list, and parse_param_list
//! (methods / fn-exprs).

use super::Parser;
use crate::ast::{Expr, ExprId};

impl<'a> Parser<'a> {
    pub(super) fn implicit_optional_default(&mut self, type_ann: Option<&str>) -> ExprId {
        // Un-annotated: the plain ident, which types as `Undefined`.
        let Some(ann) = type_ann else {
            return self.ast.add_expr(Expr::Ident("undefined".into()));
        };
        // The annotation reaching here is the `__nullable(T)` the
        // caller wrapped around the written type; T is the width that
        // has to carry the value.
        let inner = ann
            .strip_prefix("__nullable(")
            .and_then(|s| s.strip_suffix(')'))
            .unwrap_or(ann);
        // A slot that materializes as Any takes the plain ident and
        // boxes it to ANY_UNDEF. A pointer-shaped one has no such
        // encoding and needs its per-type sentinel cell, which the
        // `__undef_slot__<T>` marker asks the SSA lowering to
        // materialize (see the module doc on why the async tail's
        // minter for this same question does not fit here).
        if crate::ssa_lower_parse_type::nullable_inner_boxes(inner)
            || matches!(inner, "any" | "void" | "undefined")
        {
            return self.ast.add_expr(Expr::Ident("undefined".into()));
        }
        self.ast.add_expr(Expr::Ident(format!(
            "{}{inner}",
            crate::ast::UNDEF_SLOT_MARKER
        )))
    }
}
