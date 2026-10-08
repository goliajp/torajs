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
use crate::ast::{Ast, Expr, ExprId};

impl<'a> Parser<'a> {
    pub(super) fn implicit_optional_default(&mut self, type_ann: Option<&str>) -> ExprId {
        implicit_optional_default(&mut self.ast, type_ann)
    }
}

/// The default an optional parameter binds when the argument is
/// absent — the parser's `x?: T` rule, as a free function so a desugar
/// minting an optional parameter (the async wrapper, RFC 20260805
/// blade 1) spells the same value the parser would: the plain
/// `undefined` where the slot materializes as Any, the per-type
/// `__undef_slot__T` sentinel where it is pointer-shaped.
pub(crate) fn implicit_optional_default(ast: &mut Ast, type_ann: Option<&str>) -> ExprId {
    let Some(ann) = type_ann else {
        return ast.add_expr(Expr::Ident("undefined".into()));
    };
    let inner = ann
        .strip_prefix("__nullable(")
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(ann);
    if crate::ssa_lower_parse_type::nullable_inner_boxes(inner)
        || crate::ssa_lower_parse_type::slot_is_any(inner)
    {
        return ast.add_expr(Expr::Ident("undefined".into()));
    }
    ast.add_expr(Expr::Ident(format!(
        "{}{inner}",
        crate::ast::UNDEF_SLOT_MARKER
    )))
}
