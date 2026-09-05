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
/// this one different is that the cell ESCAPES, so the proof has to be
/// about every path the caller can reach it by rather than about this
/// syntactic position. Those paths all honor the receiver channel: the
/// any lane shifts argv on FLAG_CLOSURE_RECV_FIRST (`__torajs_any_call`
/// / `invoke_with_this` / the NewDynamic kernel), and 398-06 put the
/// same runtime test on the TYPED indirect lanes, so a value that flows
/// through a concrete slot answers correctly too.
///
/// The single lane with no gate is `emit_fnsig_callee`'s bare
/// `CallIndirect`, and it is reached only when the callee's static type
/// is a spelled-out `Type::FnSig` — which is precisely what the RETURN
/// annotation decides. So the boundary carries the whole proof: an
/// absent annotation infers straight through, an explicit `any` says it
/// outright, and a concrete signature (`function take(f: any): (a:
/// number) => string`) hands the caller a typed callee and is rejected
/// here.
///
/// The binding's OWN annotation was required alongside this from the
/// day the shape landed (`7c259d91f`), on the theory that it was what
/// put the returned cell in the any lane to begin with. It is not —
/// the promoter keeps every admitted binding on the runtime gate,
/// which is why the array-element and `any`-parameter shapes never
/// asked for it either. 593 measured the difference across every
/// read-back path (immediate call, through a variable, into an array
/// element or an object field, on to another function, one more
/// boundary out, `typeof`) and found the annotated and unannotated
/// bindings answering alike — including the argv-shift witnesses that
/// a zero-parameter callee cannot show.
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

/// Every name a `LetDecl` annotates exactly `any`. It was the binding
/// half of the return-shape proof above until 593 showed that half was
/// not load-bearing; what still reads it is
/// [`super::fnexpr_this_routed`]'s rebindable test, where an explicit
/// `any` says the slot was declared wide on purpose. A name declared
/// twice lands here off either decl, and the `decls.len() != 1` guard
/// is what turns that away.
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
