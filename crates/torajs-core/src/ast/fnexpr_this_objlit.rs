//! The object-literal FIELD-VALUE use shape.
//!
//! Sibling of [`super::fnexpr_this_arraylit`], and its proof is the
//! same one read off a different container. The parent
//! [`super::fnexpr_this_shapes`] keeps the catalogue and the `admits`
//! verdict; this module answers one entry of it.

use super::fnexpr_this_names::peel_as;
use super::{Expr, ExprId};

/// 590-01 — a bare-Ident VALUE of an object-literal field:
/// `const o = { f: g }`, shorthand `{ g }` included.
///
/// Escaping proof family, the same three links 589-03 wrote for an
/// array element, with the container swapped:
///
/// 1. **The field can only be typed two ways.** A `__mth(` slot —
///    the one closure repr whose call passes a receiver statically,
///    and so the one that would NOT shift — is minted exclusively for
///    a field whose value is an inline method/closure EXPRESSION
///    (`objlit_method_exprs` in [`super::objlit_nominal`]). A bare
///    Ident is not one, so its slot carries either the binding's own
///    plain closure repr (`__fn(`) or `any`.
/// 2. **Both of those shift.** Reading the field back and calling it
///    as `o.f(x)` lowers through
///    `ssa_lower_call_struct_method_dispatch`, whose
///    `takes_recv == false` arm has run behind the
///    `FLAG_CLOSURE_RECV_FIRST` gate since 398-06 — its own comment
///    says why: "the field may still HOLD a promoted closure at
///    runtime". A detached read (`const h = o.f; h()`) rides
///    `closure_local`, a call through an `any` view rides the any
///    lane; all three reach `ssa_lower_call_recv_gate`.
/// 3. **The literal's own lane does not matter.** A literal the
///    any-promote verdict boxes into a dynobj loses its nominal
///    identity altogether and every read of it is an any-lane read —
///    the shifting lane by construction.
///
/// The reprs 398-06 knife 3 left out are the exception here exactly
/// as they are for an array element, so the parent filters this set
/// by [`super::fnexpr_this_arraylit::variadic_binding_names`] — the
/// census is shared rather than re-derived, because the question
/// ("does this BINDING carry a variadic repr") is about the binding,
/// not about the container it is written into.
///
/// A computed key's key-expression is a different ExprId and is not
/// collected here: `{ [g]: 1 }` keeps failing the parity, which is
/// the fail-safe direction.
///
/// Cost of not having it, measured: `class C { g = () => ({ k: C }) }`
/// inside a function the class captures a local from — the ES5 lane,
/// where the constructor is a fn-expr needing receiver promotion —
/// was rejected, while the array spelling `[C]` and the bare `C` both
/// promoted. Flat arena scan like every sibling shape; nothing here
/// is a statement fact.
pub(super) fn objlit_field_idents(exprs: &[Expr]) -> std::collections::HashSet<ExprId> {
    let mut out = std::collections::HashSet::new();
    for e in exprs {
        let Expr::ObjectLit { fields } = e else {
            continue;
        };
        for (_, v) in fields {
            let inner = peel_as(exprs, *v);
            if matches!(&exprs[inner.0 as usize], Expr::Ident(_)) {
                out.insert(inner);
            }
        }
    }
    out
}
