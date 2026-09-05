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

/// 589-03 — an element of ANY array literal, not only one
/// initializing an exactly-`any` binding.
///
/// [`any_arraylit_elem_idents`]'s narrowing sentence — "an inferred
/// array type rides the typed lanes, whose element calls do not
/// shift" — was true when 397-01 wrote it and is not true now.
/// 398-06 put the typed indirect lanes behind the runtime
/// `FLAG_CLOSURE_RECV_FIRST` gate (`ssa_lower_call_recv_gate`, reached
/// from `fn_indirect`, `closure_local` and `struct_method_dispatch`),
/// so an element read back out of a closure-repr array and called
/// shifts argv exactly as the any lane always did. The bar predates
/// the gate, the same way the rest-param bar rotation 416 removed
/// predated the recv slot.
///
/// A closure value in an array element can be typed only two ways —
/// `any`, or a closure repr — and both now shift. The reprs 398-06
/// knife 3 deliberately left out are the exception, so
/// [`variadic_binding_names`] takes them back out.
///
/// Cost of the old bar, measured: a class that captures an enclosing
/// local goes down the ES5 lane, where its constructor is a fn-expr
/// needing receiver promotion — and
/// `class C { g = () => [C, a] }` lost it, while
/// `class C { g = () => C }`, `[C]` and `id(C)` all kept it. Flat
/// arena scan like every sibling shape here (only
/// `any_arraylit_elem_idents` walks statements, because the
/// annotation it reads is a statement fact).
pub(super) fn arraylit_elem_idents(exprs: &[Expr]) -> std::collections::HashSet<ExprId> {
    let mut out = std::collections::HashSet::new();
    for e in exprs {
        let Expr::Array(elems) = e else { continue };
        for el in elems {
            let inner = peel_as(exprs, *el);
            if matches!(&exprs[inner.0 as usize], Expr::Ident(_)) {
                out.insert(inner);
            }
        }
    }
    out
}

/// The bindings [`arraylit_elem_idents`] must NOT admit — the ones
/// whose closure carries a variadic repr.
///
/// 398-06 knife 3 admitted `__fn(` / `__cls(`-shaped slots and left
/// out the rest-tail signature and the argc-carrying repr
/// (`__clsargc`): those dispatch through the boxed variadic adapter,
/// "a path this bar has not audited". The same exclusion, read off
/// the candidate's own parameter list rather than a slot annotation,
/// because an array element's type is not spelled anywhere this pass
/// can see.
///
/// `argc` / `argv` are the promote loop's own censuses of bindings
/// whose bodies materialize real argc/argv; the rest-param half is
/// found by resolving each binding to its lifted `FnDecl`.
///
/// [`any_arraylit_elem_idents`] is deliberately NOT filtered by this:
/// an exactly-`any` binding was admitted before this rule existed and
/// stays admitted, so no program that compiled stops compiling.
pub(super) fn variadic_binding_names(
    stmts: &[Stmt],
    exprs: &[Expr],
    argc: &std::collections::HashSet<String>,
    argv: &std::collections::HashSet<String>,
) -> std::collections::HashSet<String> {
    let rest_fns: std::collections::HashSet<&str> = stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl { name, params, .. } if params.iter().any(|p| p.is_rest) => {
                Some(name.as_str())
            }
            _ => None,
        })
        .collect();
    let mut out: std::collections::HashSet<String> = argc.union(argv).cloned().collect();
    let mut note = |name: &String, init: ExprId| {
        if let Expr::Closure { fn_name, .. } = &exprs[peel_as(exprs, init).0 as usize]
            && rest_fns.contains(fn_name.as_str())
        {
            out.insert(name.clone());
        }
    };
    fn walk(stmts: &[Stmt], note: &mut dyn FnMut(&String, ExprId)) {
        for s in stmts {
            if let Stmt::LetDecl { name, init, .. } = s {
                note(name, *init);
            }
            super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| walk(inner, note));
        }
    }
    walk(stmts, &mut note);
    for e in exprs {
        if let Expr::Assign { target, value } = e
            && let Expr::Ident(n) = &exprs[target.0 as usize]
        {
            note(n, *value);
        }
    }
    out
}
