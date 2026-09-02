//! The EXPANDO store face — `var o = { a: 1 }; o.f = function () { …
//! this … }`, the plainest way JavaScript hangs a method on an object
//! that did not declare one.
//!
//! [`super::fnexpr_this_faces::collect_store_face`] already promotes a
//! store whose receiver is a props receiver (an `any` / array / empty
//! object-literal binding) or a `.prototype` / `.constructor` chain.
//! A binding initialized from a NON-empty object literal is none of
//! those: it carries a nominal struct type, and a store into one of
//! its DECLARED fields lands in a typed slot, where the read comes
//! back as a concrete function signature and the call is a typed
//! indirect — which does not shift argv on
//! `FLAG_CLOSURE_RECV_FIRST`. Promoting there would shift every
//! argument of a call that never learned about the extra leading
//! parameter: the silent wrong B-4 narrow-surface forbids.
//!
//! A name the literal never declared is the opposite. It has no typed
//! slot to land in, so the store goes to the object's expando dict and
//! the read comes back as a NaN box — the same any lane every other
//! admitted store receiver relies on, and every call path out of it
//! honours the recv-first flag. Measured before it was written: with
//! the closure arriving from the any lane instead of written inline
//! (`o.f = mk()` where `mk(): any`), HEAD already answers `this === o`
//! through exactly this channel.
//!
//! So the census records what each object-literal binding DECLARES,
//! and the admission is "the stored key is not one of them". A
//! computed key admits unless it is a string literal naming a declared
//! field — the shape `obj[Symbol.toPrimitive] = function () { … }`
//! test262 writes for the ToPrimitive families.
//!
//! Name-keyed and coarse like the rest of the family: a second
//! declaration of the name, or any assignment to the name itself,
//! drops it. Both are over-refusals, and an over-refusal costs one
//! loud reject.

use super::{Expr, ExprId, Stmt};

/// Object-literal bindings by name, each paired with the field names
/// its literal declares.
pub(super) struct ExpandoRecvs {
    fields: std::collections::HashMap<String, std::collections::HashSet<String>>,
}

impl ExpandoRecvs {
    pub(super) fn scan(stmts: &[Stmt], exprs: &[Expr]) -> Self {
        let mut fields = std::collections::HashMap::new();
        let mut poisoned = std::collections::HashSet::new();
        scan_stmts(stmts, exprs, &mut fields, &mut poisoned);
        for e in exprs {
            if let Expr::Assign { target, .. } = e
                && let Expr::Ident(n) = &exprs[target.0 as usize]
            {
                poisoned.insert(n.clone());
            }
        }
        fields.retain(|n, _| !poisoned.contains(n));
        Self { fields }
    }

    /// `true` when a store to `target` lands in an object-literal
    /// binding's expando dict rather than one of its declared slots.
    pub(super) fn admits(&self, exprs: &[Expr], target: ExprId) -> bool {
        match &exprs[target.0 as usize] {
            Expr::Member { obj, name } => self
                .declared(exprs, *obj)
                .is_some_and(|decl| !decl.contains(name.as_str())),
            Expr::Index { obj, index } => {
                self.declared(exprs, *obj)
                    .is_some_and(|decl| match &exprs[index.0 as usize] {
                        Expr::String(k) => k.as_str().is_none_or(|k| !decl.contains(k)),
                        _ => true,
                    })
            }
            _ => false,
        }
    }

    /// The census is keyed by BINDING NAME, so the receiver has to be
    /// read through `as any` the same way
    /// [`super::fnexpr_this_faces::collect_store_face`]'s admissions
    /// are: `(o as any).f = function () { …this… }` names the same
    /// binding `o.f = …` does. The declared-field bar below is what
    /// keeps the cast from buying anything extra — a key the literal
    /// DID declare stays loud whichever spelling reaches it.
    fn declared(&self, exprs: &[Expr], obj: ExprId) -> Option<&std::collections::HashSet<String>> {
        match &exprs[super::fnexpr_this_faces::peel_any_cast(exprs, obj).0 as usize] {
            Expr::Ident(n) => self.fields.get(n.as_str()),
            _ => None,
        }
    }
}

fn scan_stmts(
    stmts: &[Stmt],
    exprs: &[Expr],
    fields: &mut std::collections::HashMap<String, std::collections::HashSet<String>>,
    poisoned: &mut std::collections::HashSet<String>,
) {
    for s in stmts {
        if let Stmt::LetDecl {
            name,
            type_ann,
            init,
            ..
        } = s
        {
            // An annotated binding is somebody else's census: `any`
            // and `T[]` are the props receiver's, and a written-out
            // struct type is a slot table this walk cannot read.
            match (type_ann.as_deref(), &exprs[init.0 as usize]) {
                (None, Expr::ObjectLit { fields: lit }) => {
                    if fields
                        .insert(
                            name.clone(),
                            lit.iter()
                                .filter_map(|(k, _)| k.as_str().map(str::to_string))
                                .collect(),
                        )
                        .is_some()
                    {
                        poisoned.insert(name.clone());
                    }
                }
                _ => {
                    poisoned.insert(name.clone());
                }
            }
        }
        super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
            scan_stmts(inner, exprs, fields, poisoned)
        });
    }
}
