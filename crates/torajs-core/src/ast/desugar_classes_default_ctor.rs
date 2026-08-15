//! The implicit constructor a derived class gets when it did not
//! declare one — ES §15.7.10 `constructor(...args) { super(...args) }`.
//!
//! Split out of [`super::desugar_classes`] when that file reached the
//! 500-line limit: the pass is self-contained (one `&mut Ast` plus the
//! mutable class index, no shared state with the rest of Pass 1) and
//! moving it whole beats extracting a helper inside a file with no
//! room left for another signature and doc.

use super::*;

/// ES §15.7.10 — a ctor-less derived class carries the implicit
/// default ctor `constructor(...args) { super(...args) }`. The static
/// world spells it concretely: params are the nearest explicit
/// ancestor ctor's params verbatim, the body a single
/// `super(<param names>)` (Pass 1.5 rewrites it into the
/// `__cm_<Parent>__ctor` call like any user-written super, and the
/// factory / `C.length` / spread lanes all follow from the filled
/// ctor). Pre-fix the factory elided the ctor call entirely —
/// `new B(7)` on `class B extends A {}` rejected loud with
/// "expected 0 argument(s)", and a zero-param parent ctor's body
/// silently never ran.
///
/// Skipped (keeping today's behavior) when the whole chain is
/// ctor-less (nothing to forward), when the class or any ancestor on
/// the walk is generic (ctor param anns may reference that class's
/// type params), or when the ancestor ctor is variadic (rest
/// forwarding needs a spread — apply_rest_args territory). The walk
/// is hop-capped by the class count; malformed parent chains fall
/// through to `compute_full_fields`' existing declaration-order
/// panic.
pub(super) fn synthesize_derived_default_ctors(
    ast: &mut Ast,
    class_index: &mut [super::desugar_classes_super::ClassIndexEntry],
) {
    struct ChainInfo {
        parent: Option<String>,
        /// Only a ctor the USER wrote answers "what does this class
        /// take?" — see `ctor_params` below.
        ctor_params: Option<Vec<Param>>,
        /// Whether the ancestor has a ctor of ANY kind, which is a
        /// different question: a parser-synth field-init ctor forwards
        /// no parameters but still has to RUN.
        has_ctor: bool,
        generic: bool,
    }
    let by_name: std::collections::HashMap<String, ChainInfo> = class_index
        .iter()
        .map(|(_, cname, tp, parent, _, _, ctor, _, _)| {
            (
                cname.clone(),
                ChainInfo {
                    parent: parent.clone(),
                    // A ctor the parser synthesized out of field
                    // initializers is not an answer to "what does this
                    // class take?" — the class has the implicit default
                    // ctor, so a descendant forwarding through it must
                    // keep walking to the nearest ctor a user wrote.
                    ctor_params: ctor
                        .as_ref()
                        .filter(|_| !ast.field_init_synth_ctors.contains(cname))
                        .map(|c| c.params.clone()),
                    has_ctor: ctor.is_some(),
                    generic: !tp.is_empty(),
                },
            )
        })
        .collect();
    let max_hops = class_index.len();
    for (_, cname, type_params, parent, _, _, ctor, _, _) in class_index.iter_mut() {
        // A derived class with field initializers and no `constructor`
        // still needs the implicit one. The parser turns its field
        // initializers into a ctor so they run at all, and that ctor
        // used to stop this synthesis at the door — so the class was
        // left with no `super(...)` and the base's own initializers
        // never ran (`class B { v = 5 }` / `class D extends B { e = 6 }`
        // answered `d.v === 0`), while a base ctor's parameters had
        // nowhere to arrive (`new D(9)` rejected as "expected 0
        // argument(s)"). `append_no_super_throw` next door already
        // spells out the rule this restores: such a class simply has
        // the implicit default ctor, and that one does call super.
        if (ctor.is_some() && !ast.field_init_synth_ctors.contains(cname))
            || parent.is_none()
            || !type_params.is_empty()
        {
            continue;
        }
        let mut cur = parent.clone();
        let mut found: Option<Vec<Param>> = None;
        let mut any_ancestor_ctor = false;
        for _ in 0..max_hops {
            let Some(info) = cur.as_ref().and_then(|n| by_name.get(n)) else {
                break;
            };
            if info.generic {
                break;
            }
            any_ancestor_ctor |= info.has_ctor;
            if let Some(params) = &info.ctor_params {
                found = Some(params.clone());
                break;
            }
            cur = info.parent.clone();
        }
        // Nothing to forward is not the same as nothing to call. A
        // chain of classes that only ever declared field initializers
        // has no parameters anywhere and still needs the `super()`,
        // or the base's initializers never run.
        if found.is_none() && !any_ancestor_ctor {
            continue;
        }
        let params = found.unwrap_or_default();
        let args: Vec<ExprId> = params
            .iter()
            .map(|p| {
                let pid = ast.add_expr(Expr::Ident(p.name.clone()));
                // A rest param forwards as a SPREAD (the same
                // double-wrap story as build_factory_body's ctor
                // relay): `super(a)` against a rest-tailed ancestor
                // would re-pack into `super([a])`. Pre-fix this
                // shape was skipped outright, which left the derived
                // default ctor without its super call — the base
                // initializers silently never ran.
                if p.is_rest {
                    ast.add_expr(Expr::Spread { expr: pid })
                } else {
                    pid
                }
            })
            .collect();
        let super_eid = ast.add_expr(Expr::Super { args });
        match ctor {
            // Field initializers are already sitting in the synthesized
            // body; the super call goes in FRONT of them, which is both
            // the spec order (§10.2.11 — they run once `this` exists)
            // and the only order under which one of them may read an
            // inherited field. The parameters become the ancestor's, so
            // `new D(9)` reaches the base ctor.
            Some(c) => {
                c.params = params;
                c.body.insert(0, Stmt::Expr(super_eid));
            }
            None => {
                *ctor = Some(ClassCtor {
                    params,
                    body: vec![Stmt::Expr(super_eid)],
                });
            }
        }
        ast.derived_default_ctor_classes.insert(cname.clone());
    }
}
