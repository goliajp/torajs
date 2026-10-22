//! F1 (ann-width RFC §5.6) — fn-type annotation nominal hookups.
//!
//! A fn-type annotation (`(x: number) => number`, parser normal form
//! `__fn(number)->(number)`) is a named aggregation point exactly like
//! a class: every slot spelled with it interns one signature at
//! lowering, and every function flowing through any of those slots
//! must agree with that signature's widths. The canonical spelling
//! itself is the nominal name — `self.keys.class(&canon)` — and the
//! signature's number faces project through `Field(class, "__ret")`
//! / `Field(class, "__p{i}")`.
//!
//! Two hookup kinds:
//! - annotation-driven (`fnsig_ann_union` / `fnsig_nominal_unions`):
//!   slots annotated with a fn-type union onto its Class key (let /
//!   param / ret / named-type fields) — the alias.rs shape.
//! - flow-driven (`fn_value_flow`): a function *taken as a value*
//!   (top-level fn ident or closure construction flowing into a
//!   slot) unions its Ret / Param keys with the destination slot's
//!   projections. The width join then runs over every resident of
//!   the signature: one f64-possible resident floats the whole
//!   class, all-integral residents keep the narrow ABI. Through the
//!   existing congruence closure any alias of the slot answers the
//!   same projection, so un-annotated carriers (`let f = half`) need
//!   no annotation at all.
//!
//! The flow union deliberately does NOT mark the function's own Ret /
//! Param keys containerish: doing so would activate the guarded
//! ret-flow edges of *scalar* calls (`let q = h(); q = q / 4` must
//! not glue q's width back onto Ret(h) — the S5 one-way edge).

use super::{Analysis, Scope, SlotKeyId};
use crate::ast::{Expr, ExprId, Stmt};

/// The fn-type canonical spelling of an annotation, peeling the
/// parser's `__nullable(...)` wrapper. Higher-order faces (a fn-type
/// nested inside another spelling) are not extracted — the union is
/// skipped and the inner signature keeps its parse width (loud at
/// the call site, never silent-wrong).
///
/// `__cls(P)->(R)` (the bug-327 C2 / chunk 553 env-first retag of an
/// `__fn(P)->(R)` slot, and struct-field closure spellings) is the
/// SAME nominal signature — it normalizes to the `__fn(` spelling so
/// retagged slots and their residents keep unioning onto one class.
/// Pre-553 `__cls(` fell out of the F1 aggregation entirely: a
/// retagged slot's f64-possible faces stopped floating the interned
/// signature and the caller read the ret off the wrong register lane.
pub(crate) fn fn_type_canon(ann: &str) -> Option<String> {
    let mut t = ann.trim();
    while let Some(inner) = t
        .strip_prefix("__nullable(")
        .and_then(|r| r.strip_suffix(')'))
    {
        t = inner.trim();
    }
    if t.starts_with("__fn(") {
        Some(t.to_string())
    } else {
        t.strip_prefix("__cls(").map(|rest| format!("__fn({rest}"))
    }
}

/// Split a fn-type canonical spelling into its param spellings and
/// ret spelling: `__fn(P1|P2)->(R)` → (["P1","P2"], "R"). Returns None
/// on malformed spellings (the consumer keeps parse widths).
///
/// The param split runs on the shared `check_type_ann::split_top_pipe`
/// so a multi-argument generic param (`__fn(Map<string|number>)->(void)`)
/// nests instead of splitting — see that function's r381 note. The
/// close-paren scan below stays paren-only on purpose: it is looking
/// for the `)` that closes `__fn(`, and no generic argument can hide
/// one from it.
pub(crate) fn split_fn_type(canon: &str) -> Option<(Vec<&str>, &str)> {
    let rest = canon.strip_prefix("__fn(")?;
    let bytes = rest.as_bytes();
    let mut depth = 1i32;
    let mut close = None;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close?;
    let params_str = &rest[..close];
    let ret = crate::type_ann_fnsig::ret_of_tail(&rest[close + 1..])?;
    let params: Vec<&str> = crate::check_type_ann::split_top_pipe(params_str)
        .into_iter()
        .map(str::trim)
        .collect();
    Some((params, ret.trim()))
}

impl<'a> Analysis<'a> {
    /// Union an fn-type-annotated slot onto the spelling's nominal
    /// class (no-op for every other annotation).
    ///
    /// An array spelling `T[]` (any depth) carries the fn-type on its
    /// ELEMENT, so the element key is what joins the class: `let fs:
    /// (() => number)[]` used to canonicalize the whole `__fn(…)->(…)[]`
    /// string into a class of its own that no resident ever joined,
    /// and `fs[0]()` read an f64 return off the integer lane
    /// (rotation 639, 638-13).
    pub(super) fn fnsig_ann_union(&mut self, key: SlotKeyId, ann: &str) {
        let mut spelling = ann.trim();
        let mut k = key;
        while let Some(inner) = spelling.strip_suffix("[]") {
            spelling = inner.trim();
            k = self.keys.elem(k);
        }
        let Some(canon) = fn_type_canon(spelling) else {
            return;
        };
        let ck = self.keys.class(&canon);
        self.mark_containerish(ck);
        self.mark_containerish(k);
        self.uf.union(&self.keys, ck, k);
    }

    /// Annotation-driven nominal hookups for fn-type spellings, the
    /// signature analog of `alias_nominal_unions`: fn params / rets
    /// and named-type fields annotated with a fn-type glue onto its
    /// Class key.
    pub(super) fn fnsig_nominal_unions(&mut self) {
        let ast = self.ast;
        for stmt in &ast.stmts {
            match stmt {
                Stmt::FnDecl {
                    name,
                    params,
                    return_type,
                    ..
                } => {
                    for p in params {
                        if let Some(ann) = &p.type_ann {
                            let k = self.keys.param(name, &p.name);
                            self.fnsig_ann_union(k, ann);
                        }
                    }
                    if let Some(ann) = return_type {
                        self.fnsig_ann_union(self.keys.ret(name), ann);
                    }
                }
                Stmt::TypeDecl {
                    name,
                    type_params,
                    fields,
                } => {
                    if !type_params.is_empty() {
                        continue;
                    }
                    for (fname, ann) in fields {
                        let fk = self.keys.field(self.keys.class(name), fname);
                        self.fnsig_ann_union(fk, ann);
                    }
                }
                _ => {}
            }
        }
    }

    /// Flow-driven hookup: a function value (top-level fn ident or
    /// closure construction) flowing into `key` unions its Ret /
    /// Param keys with the destination's `__ret` / `__p{i}`
    /// projections. Call at every value-flow site (let init, assign
    /// RHS, return expr, call arg). A choice (`c ? f : g`, `f || g`)
    /// flows each function it may answer.
    pub(super) fn fn_value_flow(&mut self, key: SlotKeyId, eid: ExprId, scope: &Scope) {
        for leaf in crate::ast_closure_param_tag_member::passthrough_leaves(self.ast, eid) {
            self.fn_leaf_flow(key, leaf, scope);
        }
    }

    fn fn_leaf_flow(&mut self, key: SlotKeyId, eid: ExprId, scope: &Scope) {
        let fname = match self.ast.get_expr(eid) {
            Expr::Ident(n)
                if self.resolve(n, scope).is_none() && self.fn_params.contains_key(n) =>
            {
                n.clone()
            }
            Expr::Closure { fn_name, .. } if self.fn_params.contains_key(fn_name) => {
                fn_name.clone()
            }
            _ => return,
        };
        let user_params = self.closure_arg_params(&fname, eid);
        self.fn_flows
            .push((key, fname.clone(), user_params.clone()));
        self.mark_containerish(key);
        let rk = self.keys.field(key, "__ret");
        self.mark_containerish(rk);
        self.uf.union(&self.keys, rk, self.keys.ret(&fname));
        for (i, p) in user_params.iter().enumerate() {
            let pk = self.keys.field(key, format!("__p{i}"));
            self.mark_containerish(pk);
            self.uf.union(&self.keys, pk, self.keys.param(&fname, p));
        }
    }

    /// A callback argument that names no function but reads a slot
    /// holding function values (`p.then(o.cb)`; every local of a
    /// generator or async body is such a read, since the state machine
    /// keeps it as `this.<name>`): that slot's `face` projection
    /// (`__p{i}` / `__ret`), which [`Self::fn_value_flow`] glued onto
    /// every function stored there. Without it the callee's caller is
    /// invisible and its parameters keep the integer default, so a
    /// settled 0.5 reached the closure as 0.
    pub(super) fn callback_projection(
        &mut self,
        cb_arg: ExprId,
        face: &str,
        scope: &Scope,
    ) -> Option<SlotKeyId> {
        if self.callee_fn_name(cb_arg).is_some() {
            return None;
        }
        let k = self.container_key_lookup(cb_arg, scope)?;
        let pk = self.keys.field(k, face);
        self.mark_containerish(pk);
        Some(pk)
    }

    /// A function's user-facing param names: a lifted closure's first
    /// param is the `__env` pointer — the user arity starts after it.
    /// Every positional hookup (callback acc/elem wiring, `__p{i}`
    /// projections) must index THESE, not the raw `fn_params` list.
    pub(super) fn user_params(&self, fname: &str) -> Vec<String> {
        let params = match self.fn_params.get(fname) {
            Some(ps) => ps.as_slice(),
            None => return Vec::new(),
        };
        if params.first().is_some_and(|p| p == "__env") {
            params[1..].to_vec()
        } else {
            params.to_vec()
        }
    }

    /// The params a CALLBACK position feeds: `user_params` minus a
    /// leading hidden `__this`. A receiver-first callee (a promoted
    /// fn-expr, a `bind_this_param`-ed declaration's shell) carries the
    /// receiver as its first param, but every HOF lane threads that
    /// slot separately (`sig_skip`) and lines the element / acc /
    /// settled value up with the param AFTER it. Wiring the element
    /// class onto `__this` left the real first param on its
    /// annotation's default width: `[2.5].every(C3, o)` read 2 and
    /// `[2.5].map(function (this, v) …, o)` answered 4 (rotation 649).
    pub(super) fn callback_params(&self, fname: &str) -> Vec<String> {
        let ps = self.user_params(fname);
        if ps.first().is_some_and(|p| p == "__this") {
            ps[1..].to_vec()
        } else {
            ps
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canon_spellings() {
        assert_eq!(
            fn_type_canon("__fn(number)->(number)").as_deref(),
            Some("__fn(number)->(number)")
        );
        assert_eq!(
            fn_type_canon("__nullable(__fn(number|number)->(number))").as_deref(),
            Some("__fn(number|number)->(number)")
        );
        // env-first retag spelling normalizes onto the same class
        assert_eq!(
            fn_type_canon("__cls(number)->(number)").as_deref(),
            Some("__fn(number)->(number)")
        );
        assert_eq!(fn_type_canon("number"), None);
        assert_eq!(fn_type_canon("Item | null"), None);
    }

    #[test]
    fn split_spellings() {
        assert_eq!(
            split_fn_type("__fn(number|number)->(number)"),
            Some((vec!["number", "number"], "number"))
        );
        assert_eq!(split_fn_type("__fn()->(void)"), Some((vec![], "void")));
        // An array OF fns is not a fn type — the outer `[]` sits
        // outside the bracketed return, so the split declines.
        assert_eq!(split_fn_type("__fn()->(void)[]"), None);
        // nested fn-type param does not split at its inner `|`
        assert_eq!(
            split_fn_type("__fn(__fn(number|number)->(number)|number)->(number)"),
            Some((vec!["__fn(number|number)->(number)", "number"], "number"))
        );
    }
}
