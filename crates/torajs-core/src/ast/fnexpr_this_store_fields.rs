//! The field names a `this.<key> = <fn>` store may promote through.
//!
//! Split out of `fnexpr_this_faces.rs` under the function-size rule
//! before the census grew a second source: that file stood at 458
//! lines with 42 to spare, and the shape this answers is its own
//! question — [`super::fnexpr_this_faces::collect_store_face`] asks
//! "is this store POSITION a face", this asks "does the KEY have a
//! slot the promotion survives".

use super::{Expr, Stmt};
use crate::ast::PropKey;

/// The verdict for one `this.<key> =` store key, over the whole
/// program.
///
/// Two ways to earn it, and they are the same argument from opposite
/// ends. A key every `TypeDecl` types `any` (or a closure-repr
/// signature) has a slot the promotion survives. A key **no nominal
/// type declares at all** has no slot to land in: the store goes to
/// the receiver's expando dict and the read is a NaN box on every
/// path — which is `fnexpr_this_expando`'s argument, stated there for
/// object literals ("a name the literal never declared … has no typed
/// slot to land in") and applied here to `this`.
///
/// The second half is what lets a plain function constructor promote:
///
/// ```js
/// let k = function () { this.q = k };  // `q` is declared nowhere
/// new k();
/// ```
///
/// Before this, `q` was in no `TypeDecl`, so the store was not a face,
/// so the body's own read of `k` was an unadmitted use, so knife 2
/// declined and the program died on `fnexpr this in unclaimed
/// receiver position` — while the same program without the self-read
/// compiled. See `.claude/rfcs/20260905-fnexpr-self-read-receiver/`.
///
/// **Nominal field names come from two places and both are consulted.**
/// `TypeDecl` is the obvious one (`desugar_classes` flattens each
/// class into one). The other is OBJECT LITERALS: `{ q: 1 }` infers a
/// `Struct` type carrying a field `q` and emits no `TypeDecl` at all,
/// and `objlit_nominal` hands an object-literal method a `__this`
/// typed as that struct — so a store into it WOULD land in a typed
/// slot. A census reading only `TypeDecl`s would have admitted that
/// and shifted argv on the typed indirect call. Any key an object
/// literal spells anywhere in the program is therefore excluded,
/// whatever its type: object-literal field types are not spelled in
/// the tree this pass walks, so there is nothing to read them off.
///
/// Coarse and name-keyed like every census in this family. One
/// unrelated `{ q: … }` anywhere refuses every `this.q` store; an
/// over-refusal costs one loud reject, a mispair costs the argument
/// shift.
pub(super) struct ThisStoreKeys {
    /// Declared by a `TypeDecl`, and typed `any` / closure-repr by
    /// every `TypeDecl` that declares it.
    any_typed: std::collections::HashSet<PropKey>,
    /// Named by ANY `TypeDecl` field or ANY object-literal field —
    /// i.e. a key that some nominal type in this program spells.
    nominal: std::collections::HashSet<PropKey>,
}

impl ThisStoreKeys {
    pub(super) fn admits(&self, key: &PropKey) -> bool {
        self.any_typed.contains(key) || !self.nominal.contains(key)
    }
}

/// Take the census — see [`ThisStoreKeys`] for the two admissions and
/// the counterexample that decides the second one's shape.
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
pub(super) fn this_store_keys(stmts: &[Stmt], exprs: &[Expr]) -> ThisStoreKeys {
    let mut admitted: std::collections::HashSet<PropKey> = std::collections::HashSet::new();
    let mut other_typed: std::collections::HashSet<PropKey> = std::collections::HashSet::new();
    let mut nominal: std::collections::HashSet<PropKey> = std::collections::HashSet::new();
    for e in exprs {
        if let Expr::ObjectLit { fields } = e {
            nominal.extend(fields.iter().map(|(f, _)| f.clone()));
        }
    }
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
            nominal.insert(fname.clone());
            if fty == "any" || fn_shaped {
                admitted.insert(fname.clone());
            } else {
                other_typed.insert(fname.clone());
            }
        }
    }
    admitted.retain(|f| !other_typed.contains(f));
    ThisStoreKeys {
        any_typed: admitted,
        nominal,
    }
}
