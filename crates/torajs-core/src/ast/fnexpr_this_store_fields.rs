//! The field names a `this.<key> = <fn>` store may promote through.
//!
//! Split out of `fnexpr_this_faces.rs` under the function-size rule
//! before the census grew a second source: that file stood at 458
//! lines with 42 to spare, and the shape this answers is its own
//! question — [`super::fnexpr_this_faces::collect_store_face`] asks
//! "is this store POSITION a face", this asks "does the KEY have a
//! slot the promotion survives". The move is verbatim; the widening
//! is the commit after it.

use super::Stmt;
use crate::ast::PropKey;

/// The field names a `this.<name> = <fn-expr>` store may promote
/// through: declared by at least one `TypeDecl`, and typed exactly
/// `any` by EVERY `TypeDecl` that declares them.
///
/// `desugar_classes` flattens a class into a `TypeDecl` plus flat
/// member FnDecls, so a field initializer and a constructor store are
/// the same node by the time this pass looks: `Expr::Assign` onto
/// `__this.<name>`. What the promote needs is the proof every other
/// store receiver here carries — that no receiver-unaware call path
/// can reach the stored closure — and an `any` slot supplies it: the
/// value comes back out as a NaN box, and every any-lane call path
/// shifts argv on FLAG_CLOSURE_RECV_FIRST. A slot typed with a
/// concrete function signature is the opposite: the call goes down the
/// typed indirect lane, which does not.
///
/// The census is name-keyed and deliberately coarse, like the binding
/// censuses next door. `__this` names whatever receiver the enclosing
/// body has, and this pass sees flat FnDecls rather than the class each
/// came from, so one class typing `m` as `any` while another types it
/// as a signature makes the name ambiguous — and an ambiguous name is
/// refused for both. Over-refusal costs today's answer; a mispair would
/// cost the argument shift.
pub(super) fn any_typed_this_fields(stmts: &[Stmt]) -> std::collections::HashSet<PropKey> {
    let mut admitted: std::collections::HashSet<PropKey> = std::collections::HashSet::new();
    let mut other_typed: std::collections::HashSet<PropKey> = std::collections::HashSet::new();
    for s in stmts {
        let Stmt::TypeDecl { fields, .. } = s else {
            continue;
        };
        for (fname, fty) in fields {
            // 398-06 knife 3 — a CONCRETE fixed-arity function
            // signature joins `any` in the admitted set: its typed
            // indirect call lanes (closure_local / fn_indirect /
            // struct_method_dispatch) now run receiverless calls
            // behind the runtime FLAG_CLOSURE_RECV_FIRST gate, so a
            // promoted closure read back out of the slot shifts argv
            // on every path, same as the any lane always did. A class
            // field spells its signature with the closure-repr marker
            // (`__cls(P)->(R)`); a rest-tail signature and the
            // argc-carrying repr (`__clsargc`) stay out — their calls
            // dispatch through the boxed variadic adapter, a path
            // this bar has not audited.
            let fn_shaped =
                (fty.starts_with("__fn(") || fty.starts_with("__cls(")) && !fty.contains("__rest");
            if fty == "any" || fn_shaped {
                admitted.insert(fname.clone());
            } else {
                other_typed.insert(fname.clone());
            }
        }
    }
    admitted.retain(|f| !other_typed.contains(f));
    admitted
}
