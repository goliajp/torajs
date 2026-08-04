//! Per-class FnDecl emission for `desugar_classes` Pass 3.
//!
//! Extracted from `ast.rs::desugar_classes` (2026-06-03, god-file
//! decomp — P10.3-A3a prerequisite per `file-size.md` "先拆再加"
//! rule). Pass 3 walks each captured ClassDecl and synthesises the
//! `__cm_<C>__<M>` / `__sm_<C>__<M>` FnDecls that represent its
//! methods. Two emit halves — instance and static — live here so
//! P10.3-A3a can add async-method body rewriting in the same place
//! both halves see.
//!
//! Both helpers consume / append to caller-owned containers
//! (`appended`, accessor records) and reuse parent-module private
//! helpers (`rewrite_this_in_ann`); Rust's sub-module privacy rule
//! grants the access without widening visibility.

use super::{
    AccessorKind, Ast, BinOp, ClassMethod, Expr, ExprId, Param, Stmt, UnaryOp, rewrite_this_in_ann,
};

/// P8.2 — getter return-type inference for `get prop() { return this.x }`
/// shape, run when the user omitted the `: T` annotation. TS spec leaves
/// the getter return type inferred from body returns (per §10.1.7); plain
/// FnDecls infer via `desugar_implicit_generics` → `infer_expr_ann_with`,
/// but that helper bails on `Expr::Member` (only `length` recognised),
/// because top-level FnDecls don't have a class-field map to consult.
/// Getters do — `desugar_classes` Pass 3 has the flattened `(field_name,
/// type_ann)` slice already, so we can resolve `this.x` to `x`'s declared
/// ann. Also handles trivially-literal returns (`return 42` / `return "x"`).
/// Returns `None` when the body shape is anything else — caller falls
/// back to leaving `return_type` as `None`, preserving the existing
/// `Void` default for shapes we don't yet model.
fn infer_getter_return_ann(
    body: &[Stmt],
    fields: &[(String, String)],
    exprs: &[Expr],
) -> Option<String> {
    let ret_expr: ExprId = body.iter().rev().find_map(|s| {
        if let Stmt::Return(Some(eid)) = s {
            Some(*eid)
        } else {
            None
        }
    })?;
    infer_expr_ty_in_getter(ret_expr, fields, exprs)
}

/// Recursive type inference for getter body returns. Handles bare literals,
/// `this.<field>` member reads (resolved against the class field map),
/// arithmetic / comparison / logical / bitwise BinOps, and unary ops.
/// Returns `None` for anything we don't yet model (call, member-of-member,
/// etc.) so the caller falls back to leaving `return_type` as `None`.
fn infer_expr_ty_in_getter(
    eid: ExprId,
    fields: &[(String, String)],
    exprs: &[Expr],
) -> Option<String> {
    let e = exprs.get(eid.0 as usize)?;
    match e {
        Expr::Number(_) => Some("number".into()),
        Expr::String(_) => Some("string".into()),
        Expr::Bool(_) => Some("boolean".into()),
        Expr::Member { obj, name } => {
            let obj_e = exprs.get(obj.0 as usize)?;
            // `this` rewriting state at emit time: parser emits `Expr::This`
            // but `desugar_classes` Pass 2 (which runs before this emit
            // helper for class-body shapes) substitutes it with the synth
            // param `__this`. Match both — and the literal `Ident("this")`
            // fallback — so the helper survives ordering shifts.
            let is_this = matches!(obj_e, Expr::This)
                || matches!(obj_e, Expr::Ident(s) if s == "this" || s == "__this");
            if is_this {
                return fields
                    .iter()
                    .find(|(f, _)| f == name)
                    .map(|(_, ty)| ty.clone());
            }
            None
        }
        Expr::BinOp { op, left, right } => match op {
            // Comparison + loose-eq → always boolean.
            BinOp::Lt
            | BinOp::Gt
            | BinOp::Le
            | BinOp::Ge
            | BinOp::Eq
            | BinOp::Neq
            | BinOp::LooseEq
            | BinOp::LooseNeq => Some("boolean".into()),
            // Logical &&/|| → tora's typechecker takes the left-operand
            // type for short-circuit result; matches §13.13 semantics
            // for the common `x && y` / `x || fallback` shape.
            BinOp::LAnd | BinOp::LOr => infer_expr_ty_in_getter(*left, fields, exprs),
            // Arithmetic (non-Add) + bitwise + shift → number.
            BinOp::Sub
            | BinOp::Mul
            | BinOp::Div
            | BinOp::Mod
            | BinOp::Pow
            | BinOp::BitAnd
            | BinOp::BitOr
            | BinOp::BitXor
            | BinOp::Shl
            | BinOp::Shr
            | BinOp::UShr => Some("number".into()),
            // `+` is overloaded — string if either operand is string,
            // else number. Both must resolve for us to commit.
            BinOp::Add => {
                let l = infer_expr_ty_in_getter(*left, fields, exprs)?;
                let r = infer_expr_ty_in_getter(*right, fields, exprs)?;
                if l == "string" || r == "string" {
                    Some("string".into())
                } else {
                    Some("number".into())
                }
            }
        },
        Expr::Unary { op, .. } => match op {
            UnaryOp::Neg | UnaryOp::BitNot | UnaryOp::Plus => Some("number".into()),
            UnaryOp::Not => Some("boolean".into()),
        },
        _ => None,
    }
}

/// P10.3-A3a — rewrite an async class method body into the same shape
/// [`super::desugar_async::build_async_body`] produces for a top-level
/// async fn: each `return e` becomes `return Promise.resolve(e)`, a
/// tail fall-through gains an explicit one, and the declared return
/// type collapses to `Promise<inner_ty>`. The method is recognised as
/// async by mangled-name (`__cm_<C>__<M>` / `__sm_<C>__<M>`)
/// membership in `ast.async_fns` — the same set the top-level path
/// consults. `None` tells the caller to emit the body unchanged.
///
/// It calls that function rather than repeating it. Holding the
/// transform twice is what let the two drift: the top-level copy
/// learned to default a missing annotation to `any` (P10.7), this one
/// did not, and so `class C { async load() { return this.v } }` — an
/// everyday shape — panicked demanding an annotation, with a message
/// citing a top-level requirement that no longer existed.
fn maybe_rewrite_async_method_body(
    ast: &mut Ast,
    cname: &str,
    mangled_prefix: &str,
    method: &ClassMethod,
) -> Option<(Vec<Stmt>, Option<String>)> {
    let mangled = format!("{mangled_prefix}{cname}__{}", method.name);
    if !ast.async_fns.contains(&mangled) {
        return None;
    }
    let declared_ty = method.return_type.clone().unwrap_or_else(|| "any".into());
    let (new_body, promise_ty) =
        super::desugar_async::build_async_body(ast, method.body.clone(), &declared_ty);
    Some((new_body, Some(promise_ty)))
}

/// Emit `__cm_<C>__<M>` FnDecls for the instance methods of one
/// class. Abstract methods get a defensive `throw 7777` trap body
/// (unreachable on well-typed programs since dispatch overrides
/// route to concrete subclasses; the trap is just a safety net for
/// codegen). Accessor methods (get/set) get a `_get` / `_set`
/// suffix and an entry in the accessor record vectors so the
/// dispatch maps in check.rs / ssa_lower can recover the synth
/// name from the user-written `c.prop` / `c.prop = v`.
pub(in crate::ast) fn emit_class_instance_methods(
    ast: &mut Ast,
    methods: &[ClassMethod],
    cname: &str,
    type_params: &[String],
    this_ann: &str,
    fields: &[(String, String)],
    accessor_getter_records: &mut Vec<(String, String, String)>,
    accessor_setter_records: &mut Vec<(String, String, String)>,
    appended: &mut Vec<Stmt>,
) {
    for m in methods {
        if m.is_abstract {
            let mut params: Vec<Param> = Vec::with_capacity(m.params.len() + 1);
            params.push(Param {
                name: "__this".into(),
                type_ann: Some(this_ann.to_string()),
                default: None,
                is_rest: false,
            });
            params.extend(m.params.iter().cloned());
            let trap_eid = ast.add_expr(Expr::Number(7777.0));
            let trap_body = vec![Stmt::Throw(trap_eid)];
            appended.push(Stmt::FnDecl {
                name: format!("__cm_{cname}__{}", m.name),
                type_params: type_params.to_vec(),
                params,
                return_type: rewrite_this_in_ann(&m.return_type, this_ann),
                body: trap_body,
                is_generator: false,
                span: crate::lexer::Span { start: 0, end: 0 },
            });
            continue;
        }
        let mut params: Vec<Param> = Vec::with_capacity(m.params.len() + 1);
        params.push(Param {
            name: "__this".into(),
            type_ann: Some(this_ann.to_string()),
            default: None,
            is_rest: false,
        });
        params.extend(m.params.iter().cloned());
        // RFC 20260802 residue fix — an un-annotated SETTER param is
        // `any` per TS inference, but a None ann parses to Type::Void
        // at the SSA sig, which fails the boxed-adapter `boxable`
        // gate and silently dropped the AccessorPair's set face
        // (`gOPD(C.prototype, p).set` answered undefined and the
        // keyed write threw "readonly"). Same force-`any` posture as
        // the defaulted-param rule in parse_param_list.
        if m.accessor_kind == Some(AccessorKind::Setter) {
            for p in params.iter_mut().skip(1) {
                if p.type_ann.is_none() {
                    p.type_ann = Some("any".to_string());
                }
            }
        }
        let suffix = match m.accessor_kind {
            Some(AccessorKind::Getter) => "_get",
            Some(AccessorKind::Setter) => "_set",
            None => "",
        };
        let fn_name = format!("__cm_{cname}__{}{suffix}", m.name);
        match m.accessor_kind {
            Some(AccessorKind::Getter) => {
                accessor_getter_records.push((cname.to_string(), m.name.clone(), fn_name.clone()));
            }
            Some(AccessorKind::Setter) => {
                accessor_setter_records.push((cname.to_string(), m.name.clone(), fn_name.clone()));
            }
            None => {}
        }
        let (body, return_type) = maybe_rewrite_async_method_body(ast, cname, "__cm_", m)
            .unwrap_or_else(|| {
                // P8.2 — getter without `: T` annotation: infer from
                // body return shape (TS spec §10.1.7). Plain methods
                // keep the existing `None → Void` default.
                let effective_ret =
                    if m.accessor_kind == Some(AccessorKind::Getter) && m.return_type.is_none() {
                        infer_getter_return_ann(&m.body, fields, &ast.exprs)
                    } else {
                        m.return_type.clone()
                    };
                (
                    m.body.clone(),
                    rewrite_this_in_ann(&effective_ret, this_ann),
                )
            });
        // RFC 20260804-method-rebind-generic-body blade 2 — the
        // receiver-polymorphic twin, minted off the SAME body the
        // mono FnDecl gets (async methods clone the rewritten body).
        // Accessor faces keep their own reify lane (recorded RFC
        // follow-up), so only plain methods mint.
        if m.accessor_kind.is_none() {
            super::desugar_classes_generic_twin::mint_generic_twin(
                ast,
                &fn_name,
                &params,
                &body,
                type_params,
                m.span,
                appended,
            );
        }
        appended.push(Stmt::FnDecl {
            name: fn_name,
            type_params: type_params.to_vec(),
            params,
            return_type,
            body,
            is_generator: false,
            // B3a — carry the user-written MethodDefinition span so
            // the fn-source registry can answer `c.m.toString()`.
            span: m.span,
        });
    }
}

/// Emit `__sm_<C>__<M>` FnDecls for the static methods of one
/// class. No `__this` param — statics don't bind a receiver. The
/// enclosing class's `type_params` propagate so generic statics
/// on a generic class continue to work.
pub(in crate::ast) fn emit_class_static_methods(
    ast: &mut Ast,
    static_methods: &[ClassMethod],
    cname: &str,
    type_params: &[String],
    appended: &mut Vec<Stmt>,
) {
    for sm in static_methods {
        // RFC 20260718-accessor-reify 刀 3 — a static accessor's
        // faces carry `_get` / `_set` suffixes (mirror of the
        // instance emit above), so a same-name get/set pair no
        // longer collides in the top-level fn table. The pair is
        // recorded for class_globals' reify sweep.
        let suffix = match sm.accessor_kind {
            Some(AccessorKind::Getter) => "_get",
            Some(AccessorKind::Setter) => "_set",
            None => "",
        };
        let fn_name = format!("__sm_{cname}__{}{suffix}", sm.name);
        match sm.accessor_kind {
            Some(AccessorKind::Getter) => {
                ast.static_accessor_getters
                    .insert((cname.to_string(), sm.name.clone()), fn_name.clone());
            }
            Some(AccessorKind::Setter) => {
                ast.static_accessor_setters
                    .insert((cname.to_string(), sm.name.clone()), fn_name.clone());
            }
            None => {}
        }
        let (body, return_type) = maybe_rewrite_async_method_body(ast, cname, "__sm_", sm)
            .unwrap_or_else(|| (sm.body.clone(), sm.return_type.clone()));
        // RFC 20260802 residue fix, widened rotation 297 — force
        // `any` on EVERY un-annotated static-method param, not just
        // the setter's. An un-annotated param whose type inference
        // finds no seed (no static call site — e.g. an inner
        // ClassExpr's static method only ever dispatched through an
        // any-held class object) kept a `Type::Void` sig →
        // boxed-adapter dropout → reify skipped → DCE erased the fn
        // → the by-name dispatch answered "not a function". An
        // un-annotated param IS implicit `any` (TS semantics); a
        // direct `C.f(x)` call stays a direct call, its args just
        // box.
        let mut params = sm.params.clone();
        for p in params.iter_mut() {
            if p.type_ann.is_none() {
                p.type_ann = Some("any".to_string());
            }
        }
        // RFC 20260804-fn-this-channel knife 3a — the
        // receiver-polymorphic static twin (`__smany_`), minted off
        // the same body the mono FnDecl gets. Accessor faces keep
        // their reify lane (recorded RFC residue), so only plain
        // static methods mint; a this-free body mints nothing.
        if sm.accessor_kind.is_none() {
            super::desugar_classes_generic_twin::mint_static_generic_twin(
                ast, &fn_name, &params, &body, type_params, sm.span, appended,
            );
        }
        appended.push(Stmt::FnDecl {
            name: fn_name,
            type_params: type_params.to_vec(),
            params,
            return_type,
            body,
            is_generator: false,
            // B3a — carry the user-written MethodDefinition span
            // (mirror of the instance emit above).
            span: sm.span,
        });
    }
}
