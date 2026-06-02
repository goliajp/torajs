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

use super::{AccessorKind, Ast, ClassMethod, Expr, Param, Stmt, rewrite_this_in_ann};

/// Emit `__cm_<C>__<M>` FnDecls for the instance methods of one
/// class. Abstract methods get a defensive `throw 7777` trap body
/// (unreachable on well-typed programs since dispatch overrides
/// route to concrete subclasses; the trap is just a safety net for
/// codegen). Accessor methods (get/set) get a `_get` / `_set`
/// suffix and an entry in the accessor record vectors so the
/// dispatch maps in check.rs / ssa_lower can recover the synth
/// name from the user-written `c.prop` / `c.prop = v`.
pub(super) fn emit_class_instance_methods(
    ast: &mut Ast,
    methods: &[ClassMethod],
    cname: &str,
    type_params: &[String],
    this_ann: &str,
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
        appended.push(Stmt::FnDecl {
            name: fn_name,
            type_params: type_params.to_vec(),
            params,
            return_type: rewrite_this_in_ann(&m.return_type, this_ann),
            body: m.body.clone(),
            is_generator: false,
        });
    }
}

/// Emit `__sm_<C>__<M>` FnDecls for the static methods of one
/// class. No `__this` param — statics don't bind a receiver. The
/// enclosing class's `type_params` propagate so generic statics
/// on a generic class continue to work.
pub(super) fn emit_class_static_methods(
    static_methods: &[ClassMethod],
    cname: &str,
    type_params: &[String],
    appended: &mut Vec<Stmt>,
) {
    for sm in static_methods {
        appended.push(Stmt::FnDecl {
            name: format!("__sm_{cname}__{}", sm.name),
            type_params: type_params.to_vec(),
            params: sm.params.clone(),
            return_type: sm.return_type.clone(),
            body: sm.body.clone(),
            is_generator: false,
        });
    }
}
