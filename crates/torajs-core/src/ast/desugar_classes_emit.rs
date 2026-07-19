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
    AccessorKind, Ast, BinOp, ClassMethod, Expr, ExprId, Param, Stmt, UnaryOp, body_ends_in_return,
    default_init_for_type, rewrite_returns_for_async, rewrite_this_in_ann,
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

/// P10.3-A3a — rewrite an async class method body into the same
/// shape `desugar_async` produces for top-level async fns: each
/// `return e` becomes `return Promise.resolve(e)`, tail-fallthrough
/// gains an explicit `return Promise.resolve(<default T>)`, and
/// the declared return type collapses to `Promise<inner_ty>`. The
/// method is recognised as async by mangled-name (`__cm_<C>__<M>`
/// or `__sm_<C>__<M>`) membership in `ast.async_fns` — the same
/// set the top-level path consults. Returns the (body, return_type)
/// pair if the method was async, or `None` for the fall-through
/// caller to emit the body unchanged.
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
    let declared_ty = method.return_type.clone().unwrap_or_else(|| {
        panic!(
            "async class method `{cname}::{}` requires an explicit return type \
             annotation `: T` or `: Promise<T>` (P10.3-A3a MVP — \
             mirrors top-level `async function f(): T` requirement)",
            method.name
        )
    });
    let inner_ty = if let Some(rest) = declared_ty.strip_prefix("Promise<")
        && let Some(inner) = rest.strip_suffix('>')
    {
        inner.to_string()
    } else {
        declared_ty.clone()
    };
    let mut new_body = method.body.clone();
    for s in &mut new_body {
        rewrite_returns_for_async(ast, s, &inner_ty);
    }
    if !body_ends_in_return(&new_body) {
        let default_init = default_init_for_type(&inner_ty);
        let default_id = ast.add_expr(default_init);
        let promise_ident = ast.add_expr(Expr::Ident("Promise".into()));
        let resolve_member = ast.add_expr(Expr::Member {
            obj: promise_ident,
            name: "resolve".into(),
        });
        let call = ast.add_expr(Expr::Call {
            callee: resolve_member,
            args: vec![default_id],
        });
        new_body.push(Stmt::Return(Some(call)));
    }
    Some((new_body, Some(format!("Promise<{inner_ty}>"))))
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
        appended.push(Stmt::FnDecl {
            name: fn_name,
            type_params: type_params.to_vec(),
            params: sm.params.clone(),
            return_type,
            body,
            is_generator: false,
            // B3a — carry the user-written MethodDefinition span
            // (mirror of the instance emit above).
            span: sm.span,
        });
    }
}
