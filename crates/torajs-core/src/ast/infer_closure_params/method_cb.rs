//! An arrow handed to a method — `o.arm((res) => res(1))` — takes the
//! parameter types the method declares for that slot, the same
//! contextual typing an arrow handed to a plain function already gets
//! (`apply_user_fn_callee_hint`). The receiver's static type pins which
//! declaration that is (TS reads `o.arm`'s type, never its runtime
//! value), and three spellings of it name one:
//!
//! - a receiver annotated with an object type whose field is a
//!   function type: `o: { arm: (f: …) => void }`;
//! - a receiver whose type is a class: the class's own method
//!   (`__cm_<C>__<m>`), not one it inherits;
//! - an unannotated binding the program declares once, initialized with
//!   an object literal: the literal's method.
//!
//! Anything else keeps the default the arrow gets with no context. A
//! spelling that names a type parameter the handler cannot see is not
//! projected.

use super::super::infer_closure_lets::{field_ann_of, lifted_closure_name};
use super::super::infer_closure_typevars::{UserFnHintSinks, apply_user_fn_callee_hint};
use super::super::{Ast, Expr, ExprId, PropKey, Stmt, mangle_key};
use super::SoleBindings;
use crate::num_width::{fn_type_canon, split_fn_type};

/// One `recv.m(args)` site with an arrow among its arguments.
pub(super) struct MethodSite<'s> {
    call: ExprId,
    obj: ExprId,
    method: &'s str,
    args: &'s [ExprId],
    closure_args: &'s [(usize, String)],
}

impl<'s> MethodSite<'s> {
    pub(super) fn new(
        call: ExprId,
        obj: ExprId,
        method: &'s str,
        args: &'s [ExprId],
        closure_args: &'s [(usize, String)],
    ) -> Self {
        MethodSite {
            call,
            obj,
            method,
            args,
            closure_args,
        }
    }
}

pub(super) fn seed_method_cb_anns(
    ast: &Ast,
    site: &MethodSite<'_>,
    obj_ann: Option<&str>,
    sole: &SoleBindings<'_>,
    sinks: UserFnHintSinks<'_>,
) {
    let declared = match obj_ann {
        Some(ann) => field_fn_params(ast, ann, site.method)
            .or_else(|| class_method_params(ast, ann, site.method)),
        None => literal_method_params(ast, site.obj, site.method, sole),
    };
    let Some((pos_anns, tps)) = declared else {
        return;
    };
    if pos_anns
        .iter()
        .flatten()
        .any(|a| sole.names_type_param(a, &tps))
    {
        return;
    }
    let tps = (!tps.is_empty()).then_some(tps.as_slice());
    apply_user_fn_callee_hint(
        ast,
        site.call,
        site.args,
        site.closure_args,
        &pos_anns,
        tps,
        sinks,
    );
}

type Declared = (Vec<Option<String>>, Vec<String>);

/// `o: { arm: (f: F) => void }` — the field's own parameter spellings.
fn field_fn_params(ast: &Ast, ann: &str, method: &str) -> Option<Declared> {
    let field = field_ann_of(ast, ann, method)?;
    let canon = fn_type_canon(&field)?;
    let (params, _) = split_fn_type(&canon)?;
    Some((
        params.into_iter().map(|p| Some(p.to_string())).collect(),
        Vec::new(),
    ))
}

/// `c: C` — the method `C` itself declares. A getter of that name
/// answers the call with whatever it returns, so it takes none.
fn class_method_params(ast: &Ast, ann: &str, method: &str) -> Option<Declared> {
    let ann = ann.trim();
    if !ann
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
    {
        return None;
    }
    let key = PropKey::from(method);
    let name = format!("__cm_{ann}__{}", mangle_key(key.as_wtf8()));
    let getter = format!("{name}_get");
    if ast
        .stmts
        .iter()
        .any(|s| matches!(s, Stmt::FnDecl { name: n, .. } if *n == getter))
    {
        return None;
    }
    fn_decl_params(ast, &name)
}

/// `const o = { arm(f) { … } }` bound once — the literal's method, when
/// nothing else in the literal can supply that key.
fn literal_method_params(
    ast: &Ast,
    obj: ExprId,
    method: &str,
    sole: &SoleBindings<'_>,
) -> Option<Declared> {
    let Expr::Ident(n) = ast.get_expr(obj) else {
        return None;
    };
    let decl = sole.sole(n)?;
    if decl.ann.is_some() {
        return None;
    }
    let Expr::ObjectLit { fields } = ast.get_expr(decl.init?) else {
        return None;
    };
    // a spread, an accessor or a computed key can supply the key too
    if fields
        .iter()
        .any(|(k, _)| k.as_wtf8().as_str().is_none_or(|s| s.starts_with("__")))
    {
        return None;
    }
    let mut hits = fields
        .iter()
        .filter(|(k, _)| k.as_wtf8().as_str() == Some(method));
    let (_, value) = hits.next()?;
    if hits.next().is_some() {
        return None;
    }
    fn_decl_params(ast, &lifted_closure_name(ast, *value)?)
}

/// The user parameters of the lifted function `name`, with its type
/// parameters.
fn fn_decl_params(ast: &Ast, name: &str) -> Option<Declared> {
    ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl {
            name: n,
            params,
            type_params,
            ..
        } if n == name => Some((
            params
                .iter()
                .filter(|p| p.name != "__env" && p.name != "__this")
                .map(|p| p.type_ann.clone())
                .collect(),
            type_params.clone(),
        )),
        _ => None,
    })
}
