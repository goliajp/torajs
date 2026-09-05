//! The `: any` BINDING-INIT use shape — `const v: any = k`.
//!
//! The base case the family never had. Every admitted any-lane escape
//! so far put the value in a slot reached THROUGH something: an
//! `any[]` element ([`super::fnexpr_this_arrpush`]), an object-literal
//! field ([`super::fnexpr_this_objlit`]), an `any` parameter
//! ([`super::fnexpr_this_args`]). Writing it straight into an
//! `any`-annotated binding — the shortest spelling of the same
//! escape — was still an unclaimed position, so
//! `const v: any = k` refused to compile while `const v: any = [k]`
//! did not.
//!
//! The proof is the shortest in the family, and it needs no census of
//! the program at all. The slot's type is fixed by the annotation ON
//! THIS DECLARATION, so every read of the binding hands back an
//! AnyValue however it is spelled, and every any-lane call path
//! shifts argv on `FLAG_CLOSURE_RECV_FIRST`. Nothing about the
//! binding's later uses can change that: unlike the unannotated alias
//! ([`super::fnexpr_this_alias`], whose slot holds the raw closure
//! repr and whose direct call would therefore eat an argument), an
//! `: any` slot has no typed call lane to fall into. That is why this
//! admits outright where the alias needed a fixpoint.
//!
//! A same-name declaration elsewhere is likewise irrelevant: what is
//! admitted is the init expression of THIS declaration, and it is
//! this declaration's own annotation that types the slot the value
//! lands in.

use super::fnexpr_this_names::peel_as;
use super::{Expr, ExprId, Stmt};

/// The bare-Ident initializer of every binding annotated exactly
/// `any` (the `as` suffix peeled — a cast changes the static type of
/// the read, never where the value is stored).
///
/// Walks the shared nested-list spine, so a declaration inside a
/// block / try / with is seen (rotation 437's lesson).
pub(super) fn any_ann_init_idents(
    stmts: &[Stmt],
    exprs: &[Expr],
) -> std::collections::HashSet<ExprId> {
    let mut out = std::collections::HashSet::new();
    walk(stmts, exprs, &mut out);
    out
}

fn walk(stmts: &[Stmt], exprs: &[Expr], out: &mut std::collections::HashSet<ExprId>) {
    for s in stmts {
        if let Stmt::LetDecl { type_ann, init, .. } = s
            && type_ann.as_deref() == Some("any")
        {
            let inner = peel_as(exprs, *init);
            if matches!(&exprs[inner.0 as usize], Expr::Ident(_)) {
                out.insert(inner);
            }
        }
        super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| walk(inner, exprs, out));
    }
}
