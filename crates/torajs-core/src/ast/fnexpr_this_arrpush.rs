//! The `any[]` ELEMENT-STORE use shape — `xs.push(g)` / `xs.unshift(g)`
//! on a binding annotated exactly `any[]`.
//!
//! Sibling of [`super::fnexpr_this_arraylit`] and
//! [`super::fnexpr_this_objlit`]: those two answer for a value written
//! into a container LITERAL, this one for a value pushed into a
//! container that was declared empty. Its own file because it carries
//! a census of its own, and both candidate hosts
//! (`fnexpr_this_recvs.rs` at 478 prod lines, `fnexpr_this_args.rs` at
//! 480) had less headroom than this module needs — move-or-branch
//! before growing a tight file, never after.

use super::fnexpr_this_names::peel_as;
use super::{Expr, ExprId, Stmt};

/// The element-store methods whose EVERY argument is an element
/// value. `splice` is deliberately absent: its first two arguments
/// are a start index and a delete count, so admitting its args
/// wholesale would admit two positions this proof says nothing
/// about.
const ELEM_STORE_METHODS: &[&str] = &["push", "unshift"];

/// Binding names annotated exactly `any[]` (or the `Array<any>`
/// spelling of the same type) at EVERY declaration in the program.
///
/// Same over-removal posture as
/// [`super::fnexpr_this_recvs::collect_any_binding_names`]: a
/// same-name declaration anywhere carrying any other annotation
/// removes the name, which can only keep a promotion loud, never
/// mis-admit a typed receiver. Reassignment is NOT censused, and
/// does not need to be — what this proof rests on is the SLOT type,
/// which the annotation fixes; re-pointing the binding at another
/// `any[]` leaves the element type exactly where it was.
///
/// Walks the shared nested-list spine, so a declaration inside a
/// block / try / with is seen (rotation 437's lesson, which the
/// hand-rolled recursions in this family kept re-learning).
fn any_elem_arr_names(stmts: &[Stmt]) -> std::collections::HashSet<String> {
    fn walk(
        stmts: &[Stmt],
        ok: &mut std::collections::HashSet<String>,
        other: &mut std::collections::HashSet<String>,
    ) {
        for s in stmts {
            if let Stmt::LetDecl { name, type_ann, .. } = s {
                if matches!(type_ann.as_deref(), Some("any[]") | Some("Array<any>")) {
                    ok.insert(name.clone());
                } else {
                    other.insert(name.clone());
                }
            }
            super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| walk(inner, ok, other));
        }
    }
    let mut ok = std::collections::HashSet::new();
    let mut other = std::collections::HashSet::new();
    walk(stmts, &mut ok, &mut other);
    ok.retain(|n| !other.contains(n));
    ok
}

/// 590-03 — a bare-Ident argument of an element-store call on an
/// `any[]` binding: `const xs: any[] = []; xs.push(g)`.
///
/// Escaping proof family, and the shortest link count in it. An
/// `any[]` binding is an `Arr<Any>`, so the slot the value lands in
/// is an Any element by the receiver's own declared type — there is
/// no inference to trust and no repr to case-split, which is what
/// [`super::fnexpr_this_arraylit`]'s widening had to argue for an
/// INFERRED array. Reading such an element back yields an AnyValue
/// whichever way it is spelled (`xs[0]()`, a detached read, a
/// `new`), and every any-lane call path shifts argv on
/// FLAG_CLOSURE_RECV_FIRST.
///
/// An inline array literal receiver keeps today's loud reject: it has
/// no `any` annotation to read.
///
/// A member / index receiver (`bag.fns.push(g)`) was rejected for the
/// same-sounding but different reason — it "would need the field's
/// type, which this pass cannot see". True of THIS census, which is
/// of annotations; it is not what decides. Rotation 597: when the
/// chain roots in a binding whose properties already live in the any
/// world (`super::fnexpr_this_recvs_props`'s census — the same bar the
/// STORE position of this very slot uses), the field's type is not
/// needed, because an any-world read yields an any-world container
/// whatever the field was declared as. The named-declaration family
/// closed exactly this gap in the same rotation, and the two families
/// disagreeing about `bag.fns.push(f)` is what surfaced it: one
/// answered the holder, the other refused to compile.
pub(super) fn any_elem_push_arg_idents(
    stmts: &[Stmt],
    exprs: &[Expr],
) -> std::collections::HashSet<ExprId> {
    let names = any_elem_arr_names(stmts);
    let props_recvs =
        super::fnexpr_this_recvs_props::collect_props_receiver_binding_names(stmts, exprs);
    let mut out = std::collections::HashSet::new();
    if names.is_empty() && props_recvs.is_empty() {
        return out;
    }
    for e in exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Member { obj, name } = &exprs[callee.0 as usize] else {
            continue;
        };
        if !ELEM_STORE_METHODS.contains(&name.as_str()) {
            continue;
        }
        let obj = peel_as(exprs, *obj);
        let admits = match &exprs[obj.0 as usize] {
            Expr::Ident(recv) => names.contains(recv.as_str()),
            _ => super::fnexpr_this_recvs_props::store_receiver_admits(exprs, &props_recvs, obj),
        };
        if !admits {
            continue;
        }
        for a in args {
            super::fnexpr_this_names::slot_value_idents(exprs, *a, &mut out);
        }
    }
    out
}
