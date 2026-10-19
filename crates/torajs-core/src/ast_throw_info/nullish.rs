//! Which nullish values an expression may evaluate to, as the
//! lowering's guard sources see it — the AST-side mirror of
//! `ssa_lower_nullable_guard`'s source predicates and of the
//! registries the let-decl, parameter and loop-variable lowerings
//! fill, answered here so the throw analysis can run before any body
//! is lowered.
//!
//! The two must move together: a source the lowering starts guarding
//! belongs here the same day, or the guard's TypeError goes unchecked
//! at every caller of the function it sits in. Over-approximating the
//! other way only costs one cold throw check per call site.

use super::Scan;
use crate::ast::{BinOp, Expr, ExprId, UnaryOp};
use crate::check::Type;
use crate::ssa_lower_nullable_guard::spells_undef_with_generic_cell;

/// The names some assignment anywhere writes, and what the census
/// proved live.
pub(crate) type ProgramNames<'a> = (
    &'a std::collections::HashSet<String>,
    &'a crate::ssa_lower_pass_0_5::live_field_census::Live,
);

/// The nullish values a slot may hold, as a bit set: which of them a
/// narrowing has to rule out before a read off the slot is safe.
pub(crate) type Form = u8;
pub(crate) const UNDEF: Form = 1;
pub(crate) const NULL: Form = 2;
pub(crate) const ANY: Form = UNDEF | NULL;

impl Scan<'_> {
    /// A member or index read the lowering guards raises when its
    /// receiver may be nullish; a nullish-typed receiver raises
    /// through the any-member lane (§13.2.3 GetValue).
    pub(crate) fn receiver_raises(&self, obj: ExprId) -> bool {
        self.form(obj) != 0
            || matches!(
                self.expr_types.get(&obj),
                Some(Type::Undefined | Type::Null)
            )
    }

    /// A call whose callee's parameter the caller tests before the
    /// call (`live_field_census::entry`) raises there when the argument
    /// may be nullish, as the member read in the callee would.
    pub(crate) fn entry_test_raises(&self, call: ExprId, args: &[ExprId]) -> bool {
        (self.live.entry_params(call).iter())
            .any(|i| args.get(*i).is_some_and(|a| self.receiver_raises(*a)))
    }

    /// The nullish values `eid` may evaluate to; 0 when it cannot be
    /// nullish as far as the lowering's guards are concerned.
    pub(crate) fn form(&self, eid: ExprId) -> Form {
        // a member read a discriminant test proved live
        if self.live.members.contains(eid) {
            return 0;
        }
        let ty = self.expr_types.get(&eid);
        let mut f = 0;
        if matches!(ty, Some(Type::Nullable(_))) {
            f |= ANY;
        }
        // a binding or a call result the census did not prove live
        if self.live.unproven_read(eid)
            && !matches!(self.ast.get_expr(eid), Expr::Ident(n) if self.safe.contains(n))
        {
            f |= ANY;
        }
        match self.ast.get_expr(eid) {
            Expr::Ident(n) => {
                if !self.safe.contains(n)
                    && let Some(b) = self.nullish.get(n)
                {
                    f |= *b;
                }
                if n.starts_with(crate::ast::UNDEF_SLOT_MARKER) {
                    f |= UNDEF;
                }
            }
            Expr::Index { obj, .. } | Expr::OptIndex { obj, .. } => {
                if self.elem_read_can_miss(eid, *obj) {
                    f |= UNDEF;
                }
            }
            Expr::Member { obj, name } if self.member_can_miss(ty, *obj, name) => f |= UNDEF,
            Expr::OptChain { .. } => {
                if ty.is_some_and(spells_undef_with_generic_cell) {
                    f |= UNDEF;
                }
            }
            Expr::As { expr, .. } => f |= self.form(*expr),
            Expr::Call { callee, .. } => match self.ast.get_expr(*callee) {
                Expr::Ident(g) => {
                    if self.widths.returns_undef_on_fallthrough(g)
                        || self.ret_upgraded_from_any_binding(g)
                    {
                        f |= UNDEF;
                    }
                }
                Expr::Member { obj, name } => match name.as_str() {
                    "exec" | "match" => f |= NULL,
                    "find" | "findLast" | "pop" | "shift" | "at" if self.cell_elem_array(*obj) => {
                        f |= UNDEF
                    }
                    "toJSON" if matches!(self.expr_types.get(obj), Some(Type::Date)) => f |= NULL,
                    "stringify" if matches!(self.ast.get_expr(*obj), Expr::Ident(n) if n == "JSON") => {
                        f |= UNDEF
                    }
                    _ => {}
                },
                _ => {}
            },
            _ => {}
        }
        f
    }

    /// A member read of `name` off `obj`, of type `ty`, that can answer
    /// `undefined`.
    fn member_can_miss(&self, ty: Option<&Type>, obj: ExprId, name: &str) -> bool {
        // a field of a pointer-shaped type is seeded with the undefined
        // cell until something writes it
        ty.is_some_and(spells_undef_with_generic_cell)
            || (name == "description" && matches!(self.expr_types.get(&obj), Some(Type::Symbol)))
            || (self.widths.sentinel_fields()).contains(torajs_wtf8::Wtf8::new(name))
            || (name == "value"
                && matches!(self.expr_types.get(&obj), Some(Type::Promise(inner))
                    if **inner == Type::String || spells_undef_with_generic_cell(inner)))
            || matches!(self.ast.get_expr(obj), Expr::Member { obj: inner, name }
                if name == "env"
                    && matches!(self.ast.get_expr(*inner), Expr::Ident(n) if n == "process"))
    }

    /// An element read that can answer `undefined`: past the end of an
    /// array whose slots have a cell to answer with (unless the bounds
    /// walk settled both ends of this read), a string index, or a slot
    /// of a null-able exec / match result.
    fn elem_read_can_miss(&self, eid: ExprId, obj: ExprId) -> bool {
        (self.cell_elem_array(obj) && !self.widths.index_read_settled(eid))
            || matches!(self.expr_types.get(&obj), Some(Type::String))
            || self.form(obj) & NULL != 0
    }

    /// An array whose elements spell `undefined` with a sentinel a
    /// consumer guards: the generic cell, or the Str oddball.
    fn cell_elem_array(&self, obj: ExprId) -> bool {
        matches!(
            self.expr_types.get(&obj),
            Some(Type::Array(elem)) if **elem == Type::String || spells_undef_with_generic_cell(elem)
        )
    }

    /// A fn whose declared fn-typed return the lowering upgrades from
    /// an `any` binding answers the sentinel for a non-callable box —
    /// the same pair of predicates `effective_ret_ty` decides by.
    fn ret_upgraded_from_any_binding(&self, name: &str) -> bool {
        self.ast.stmts.iter().any(|s| {
            matches!(s,
                crate::ast::Stmt::FnDecl { name: n, params, return_type, body, .. }
                if n == name
                    && return_type.as_deref().is_some_and(crate::ast::is_fn_like_ann)
                    && crate::ssa_lower_body_returns_closure::body_returns_any_binding(
                        self.ast, params, body))
        })
    }
}

impl Scan<'_> {
    /// The names `cond` coming out `when` proves not to hold the paired
    /// forms.
    pub(super) fn narrowings(&self, cond: ExprId, when: bool) -> Vec<(String, Form)> {
        match self.ast.get_expr(cond) {
            Expr::BinOp {
                op: BinOp::LAnd,
                left,
                right,
            } if when => {
                let mut v = self.narrowings(*left, true);
                v.extend(self.narrowings(*right, true));
                v
            }
            Expr::BinOp {
                op: BinOp::LOr,
                left,
                right,
            } if !when => {
                let mut v = self.narrowings(*left, false);
                v.extend(self.narrowings(*right, false));
                v
            }
            Expr::Unary {
                op: UnaryOp::Not,
                expr,
            } => self.narrowings(*expr, !when),
            Expr::Ident(n) if when => vec![(n.clone(), ANY)],
            Expr::BinOp { op, left, right } => {
                let strict = match op {
                    BinOp::Eq | BinOp::Neq => true,
                    BinOp::LooseEq | BinOp::LooseNeq => false,
                    _ => return Vec::new(),
                };
                let Some((name, form)) = self
                    .nullish_test(*left, *right)
                    .or_else(|| self.nullish_test(*right, *left))
                else {
                    return Vec::new();
                };
                // the test asserting nullish-ness held: nothing ruled out
                if matches!(op, BinOp::Eq | BinOp::LooseEq) == when {
                    return Vec::new();
                }
                vec![(name, if strict { form } else { ANY })]
            }
            _ => Vec::new(),
        }
    }

    /// `x` against `undefined` / `null`, or `typeof x` against
    /// "undefined": the name and the form the literal names.
    fn nullish_test(&self, x: ExprId, lit: ExprId) -> Option<(String, Form)> {
        match (self.ast.get_expr(x), self.ast.get_expr(lit)) {
            (Expr::Ident(n), _) => match self.expr_types.get(&lit) {
                Some(crate::check::Type::Undefined) => Some((n.clone(), UNDEF)),
                Some(crate::check::Type::Null) => Some((n.clone(), NULL)),
                _ => None,
            },
            (Expr::TypeOf { expr }, Expr::String(s)) if s == "undefined" => {
                match self.ast.get_expr(*expr) {
                    Expr::Ident(n) => Some((n.clone(), UNDEF)),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}
