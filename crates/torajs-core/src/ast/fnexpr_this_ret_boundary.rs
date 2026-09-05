//! The RETURN-BOUNDARY shape of a knife-2 promotion candidate:
//! `return <bare name>` out of a function that does not re-type it.
//!
//! One of [`super::fnexpr_this_shapes`]'s escape shapes, kept in its
//! own file because the proof it carries is about the BOUNDARY (what
//! the caller's static type for the returned value ends up being)
//! rather than about the syntactic position, and because the shapes
//! file sat one edit away from the 500-line bar. Moving a whole family
//! out is what the file-size debt table asks the next editor to do
//! first; this is that move.

use super::{Expr, ExprId, Stmt};

/// `return <bare name>` out of a function whose return type is
/// inferred or spelled exactly `any`.
///
/// Returning the name never CALLS the binding — the same one-liner that
/// admits a member's object and the right of `instanceof`. What makes
/// this one different is that the cell ESCAPES, so the proof it needs
/// is the explicit-`any` argument shape's rather than theirs: the value
/// has to cross into the any lane and STAY there, because every
/// any-lane call path honors the receiver channel (`__torajs_any_call`
/// / `invoke_with_this` / the NewDynamic kernel all shift argv on
/// FLAG_CLOSURE_RECV_FIRST) while a typed indirect call does not.
///
/// Two things have to hold for that crossing, and this classifier only
/// checks the second — the caller pairs it with the binding's own `any`
/// annotation, which is what makes the RETURNED expression an any-lane
/// cell in the first place:
///
/// 1. the binding is annotated `any`, and
/// 2. the boundary does not re-type it — an absent return annotation
///    infers the `any` straight through, and an explicit `any` says it
///    outright. A concrete signature (`function take(f: any): (a:
///    number) => string`) is what this rejects: it hands the caller a
///    typed callee, whose call path never reads the flag.
///
/// The excluded halves stay loud, which is where they already are:
/// every program that returns a `this`-using binding is rejected today,
/// so nothing that answers correctly can be pulled in.
///
/// Bare Ident only, like the `instanceof` shape — `return C as any` is
/// a different node and is not measured here.
pub(super) fn any_boundary_return_idents(
    stmts: &[Stmt],
    exprs: &[Expr],
) -> std::collections::HashSet<ExprId> {
    fn walk(
        stmts: &[Stmt],
        exprs: &[Expr],
        admits: bool,
        out: &mut std::collections::HashSet<ExprId>,
    ) {
        for s in stmts {
            if let Stmt::Return(Some(eid)) = s
                && admits
                && matches!(&exprs[eid.0 as usize], Expr::Ident(_))
            {
                out.insert(*eid);
            }
            // A `return` belongs to the nearest enclosing function, so
            // the FnDecl arm re-derives `admits` while every other
            // compound form carries it through. Top level starts false:
            // a `return` cannot appear there, and guessing in the
            // admitting direction is the unsafe one.
            match s {
                Stmt::FnDecl {
                    return_type, body, ..
                } => walk(
                    body,
                    exprs,
                    return_type.as_deref().is_none_or(|a| a == "any"),
                    out,
                ),
                _ => super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
                    walk(inner, exprs, admits, out)
                }),
            }
        }
    }
    let mut out = std::collections::HashSet::new();
    walk(stmts, exprs, false, &mut out);
    out
}

/// Every name a `LetDecl` annotates exactly `any` — the binding half of
/// the return-shape proof above. A name declared twice lands here off
/// either decl, and the `decls.len() != 1` guard is what turns that
/// away.
pub(super) fn collect_any_ann_decl_names(
    stmts: &[Stmt],
    out: &mut std::collections::HashSet<String>,
) {
    for s in stmts {
        if let Stmt::LetDecl { name, type_ann, .. } = s
            && type_ann.as_deref() == Some("any")
        {
            out.insert(name.clone());
        }
        super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
            collect_any_ann_decl_names(inner, out)
        });
    }
}
