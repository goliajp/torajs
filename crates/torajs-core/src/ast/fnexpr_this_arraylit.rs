//! The array-literal element use shape.
//!
//! Split out of `fnexpr_this_shapes.rs` under the file-size rule
//! before widening it: that file stood at 472 lines with 28 to
//! spare, and the widening brings a census of its own. The parent
//! keeps the catalogue of shapes and the `admits` verdict; this
//! module answers one of them.

use super::fnexpr_this_names::peel_as;
use super::{Expr, ExprId, Stmt};

/// 397-01 — an element of an array literal that initializes an
/// exactly-`any` binding: `const arr: any = [g]`.
///
/// Escaping proof family (the explicit-`any` argument / any-boundary
/// return shapes): the binding is `any`, so the whole array lives in
/// the any world and every read of an element stays there — an
/// `arr[0](7)` rides the any-index call lane, whose closure leg
/// dispatches through `invoke_with_this` (the 399-05 fix), and a
/// detached read's plain call seeds `undefined`; both shift argv on
/// FLAG_CLOSURE_RECV_FIRST. The annotation must be spelled on the
/// binding itself — an inferred array type rides the typed lanes,
/// whose element calls do not shift.
///
/// Bare Ident elements and their `as` shells both admit (the shell
/// changes the checker's view, never the value, and the binding's
/// own `any` is what decides the lane).
pub(super) fn any_arraylit_elem_idents(
    stmts: &[Stmt],
    exprs: &[Expr],
) -> std::collections::HashSet<ExprId> {
    fn walk(stmts: &[Stmt], exprs: &[Expr], out: &mut std::collections::HashSet<ExprId>) {
        for s in stmts {
            if let Stmt::LetDecl { type_ann, init, .. } = s
                && type_ann.as_deref() == Some("any")
            {
                let init = peel_as(exprs, *init);
                if let Expr::Array(elems) = &exprs[init.0 as usize] {
                    for e in elems {
                        let inner = peel_as(exprs, *e);
                        if matches!(&exprs[inner.0 as usize], Expr::Ident(_)) {
                            out.insert(inner);
                        }
                    }
                }
            }
            super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| walk(inner, exprs, out));
        }
    }
    let mut out = std::collections::HashSet::new();
    walk(stmts, exprs, &mut out);
    out
}
