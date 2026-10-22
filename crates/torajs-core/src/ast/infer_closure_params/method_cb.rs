//! An arrow handed to a method — `o.arm((res) => res(1))` — takes the
//! parameter types the method declares for that slot, the same
//! contextual typing an arrow handed to a plain function already gets
//! (`apply_user_fn_callee_hint`). The receiver's static type pins which
//! declaration that is (TS reads `o.arm`'s type, never its runtime
//! value), and three spellings of it name one:
//!
//! - a binding the program declares once, annotated with an object type
//!   whose field is a function type and initialized with an object
//!   literal: `const o: { arm: (f: …) => void } = { arm(f) { … } }`.
//!   The literal's method is what runs, and the checker holds it to the
//!   annotation; a value of another kind stored under the annotation (a
//!   class instance, an `any`) runs a method of its own, which TS lets
//!   call the arrow with anything;
//! - a receiver whose type is a class: the class's own method
//!   (`__cm_<C>__<m>`), not one it inherits;
//! - an unannotated binding the program declares once, initialized with
//!   an object literal: the literal's method.
//!
//! Anything else keeps the default the arrow gets with no context. A
//! spelling that names a type parameter the handler cannot see is not
//! projected, and neither is a declaration the value may not run: a
//! subclass overriding the method (TS checks method parameters
//! bivariantly, so the override may call the arrow with anything), a
//! write to a property of that name anywhere, or a write to the
//! literal's binding.

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
    if written(ast, None, site.method) {
        return;
    }
    let declared = match obj_ann {
        Some(ann) => field_fn_params(ast, ann, site, sole)
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

/// `const o: { arm: (f: F) => void } = { arm(f) { … } }` — the field's
/// own parameter spellings, when the literal supplies the method. The
/// method runs with the parameters it writes itself, so a position the
/// literal annotates (`arm(f: (x: string | number) => void)`) keeps
/// that spelling, and one past the literal's parameters has none.
fn field_fn_params(
    ast: &Ast,
    ann: &str,
    site: &MethodSite<'_>,
    sole: &SoleBindings<'_>,
) -> Option<Declared> {
    let field = field_ann_of(ast, ann, site.method)?;
    let canon = fn_type_canon(&field)?;
    let (params, _) = split_fn_type(&canon)?;
    let Expr::Ident(n) = ast.get_expr(site.obj) else {
        return None;
    };
    let decl = sole.sole(n)?;
    if decl.ann.is_none() || written(ast, Some(n), site.method) {
        return None;
    }
    let (own, tps) = literal_method(ast, decl.init?, site.method)?;
    let anns = (own.into_iter().enumerate())
        .map(|(i, a)| a.or_else(|| params.get(i).map(|p| p.to_string())))
        .collect();
    Some((anns, tps))
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
    if overridden(ast, ann, method) {
        return None;
    }
    fn_decl_params(ast, &name)
}

/// Does a class descending from `class` declare its own `method`
/// (a method or an accessor)?
fn overridden(ast: &Ast, class: &str, method: &str) -> bool {
    let key = PropKey::from(method);
    let m = mangle_key(key.as_wtf8());
    ast.class_parents.keys().any(|c| {
        c != class && descends(ast, c, class) && {
            let own = format!("__cm_{c}__{m}");
            ast.stmts.iter().any(|s| match s {
                Stmt::FnDecl { name, .. } => name
                    .strip_prefix(own.as_str())
                    .is_some_and(|r| matches!(r, "" | "_get" | "_set")),
                _ => false,
            })
        }
    })
}

/// Is `class` a proper ancestor of `c`?
fn descends<'a>(ast: &'a Ast, mut c: &'a str, class: &str) -> bool {
    while let Some(Some(p)) = ast.class_parents.get(c) {
        if p == class {
            return true;
        }
        c = p;
    }
    false
}

/// Does the program assign to a property named `method` (`o.m = …`,
/// `o["m"] = …`), or to the binding `obj`?
fn written(ast: &Ast, obj: Option<&str>, method: &str) -> bool {
    ast.exprs.iter().any(|e| {
        let Expr::Assign { target, .. } = e else {
            return false;
        };
        match ast.get_expr(*target) {
            Expr::Member { name, .. } => name == method,
            Expr::Index { index, .. } => {
                matches!(ast.get_expr(*index), Expr::String(s) if s.as_str() == Some(method))
            }
            Expr::Ident(n) => obj == Some(n.as_str()),
            _ => false,
        }
    })
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
    if decl.ann.is_some() || written(ast, Some(n), method) {
        return None;
    }
    literal_method(ast, decl.init?, method)
}

/// The parameters of the method the object literal `lit` supplies for
/// `method`, when nothing else in the literal can supply that key.
fn literal_method(ast: &Ast, lit: ExprId, method: &str) -> Option<Declared> {
    let Expr::ObjectLit { fields } = ast.get_expr(lit) else {
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
