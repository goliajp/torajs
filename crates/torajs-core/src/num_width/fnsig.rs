//! F1 (ann-width RFC §5.6) — fn-type annotation nominal hookups.
//!
//! A fn-type annotation (`(x: number) => number`, parser normal form
//! `__fn(number)->number`) is a named aggregation point exactly like
//! a class: every slot spelled with it interns one signature at
//! lowering, and every function flowing through any of those slots
//! must agree with that signature's widths. The canonical spelling
//! itself is the nominal name — `SlotKey::Class(canon)` — and the
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

use super::{Analysis, Scope, SlotKey};
use crate::ast::{Expr, ExprId, Stmt};

/// The fn-type canonical spelling of an annotation, peeling the
/// parser's `__nullable(...)` wrapper. Higher-order faces (a fn-type
/// nested inside another spelling) are not extracted — the union is
/// skipped and the inner signature keeps its parse width (loud at
/// the call site, never silent-wrong).
pub(crate) fn fn_type_canon(ann: &str) -> Option<&str> {
    let mut t = ann.trim();
    while let Some(inner) = t
        .strip_prefix("__nullable(")
        .and_then(|r| r.strip_suffix(')'))
    {
        t = inner.trim();
    }
    if t.starts_with("__fn(") {
        Some(t)
    } else {
        None
    }
}

/// Split a fn-type canonical spelling into its param spellings and
/// ret spelling: `__fn(P1|P2)->R` → (["P1","P2"], "R"). Depth-aware
/// at `(` so nested fn-types / generic args don't split. Returns
/// None on malformed spellings (the consumer keeps parse widths).
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
    let ret = rest[close + 1..].strip_prefix("->")?;
    let mut params = Vec::new();
    if !params_str.is_empty() {
        let mut d = 0i32;
        let mut last = 0usize;
        for (i, &b) in params_str.as_bytes().iter().enumerate() {
            match b {
                b'(' => d += 1,
                b')' => d -= 1,
                b'|' if d == 0 => {
                    params.push(params_str[last..i].trim());
                    last = i + 1;
                }
                _ => {}
            }
        }
        params.push(params_str[last..].trim());
    }
    Some((params, ret.trim()))
}

impl<'a> Analysis<'a> {
    /// Union an fn-type-annotated slot onto the spelling's nominal
    /// class (no-op for every other annotation).
    pub(super) fn fnsig_ann_union(&mut self, key: &SlotKey, ann: &str) {
        let Some(canon) = fn_type_canon(ann) else {
            return;
        };
        let ck = SlotKey::Class(canon.to_string());
        self.mark_containerish(&ck);
        self.mark_containerish(key);
        self.uf.union(&ck, key);
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
                            let k = SlotKey::Param(name.clone(), p.name.clone());
                            self.fnsig_ann_union(&k, ann);
                        }
                    }
                    if let Some(ann) = return_type {
                        self.fnsig_ann_union(&SlotKey::Ret(name.clone()), ann);
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
                        let fk =
                            SlotKey::Field(Box::new(SlotKey::Class(name.clone())), fname.clone());
                        self.fnsig_ann_union(&fk, ann);
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
    /// RHS, return expr, call arg).
    pub(super) fn fn_value_flow(&mut self, key: &SlotKey, eid: ExprId, scope: &Scope) {
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
        let user_params = self.user_params(&fname);
        self.mark_containerish(key);
        let rk = SlotKey::Field(Box::new(key.clone()), "__ret".to_string());
        self.mark_containerish(&rk);
        self.uf.union(&rk, &SlotKey::Ret(fname.clone()));
        for (i, p) in user_params.iter().enumerate() {
            let pk = SlotKey::Field(Box::new(key.clone()), format!("__p{i}"));
            self.mark_containerish(&pk);
            self.uf
                .union(&pk, &SlotKey::Param(fname.clone(), p.clone()));
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canon_spellings() {
        assert_eq!(
            fn_type_canon("__fn(number)->number"),
            Some("__fn(number)->number")
        );
        assert_eq!(
            fn_type_canon("__nullable(__fn(number|number)->number)"),
            Some("__fn(number|number)->number")
        );
        assert_eq!(fn_type_canon("number"), None);
        assert_eq!(fn_type_canon("Item | null"), None);
    }

    #[test]
    fn split_spellings() {
        assert_eq!(
            split_fn_type("__fn(number|number)->number"),
            Some((vec!["number", "number"], "number"))
        );
        assert_eq!(split_fn_type("__fn()->void"), Some((vec![], "void")));
        // nested fn-type param does not split at its inner `|`
        assert_eq!(
            split_fn_type("__fn(__fn(number|number)->number|number)->number"),
            Some((vec!["__fn(number|number)->number", "number"], "number"))
        );
    }
}
