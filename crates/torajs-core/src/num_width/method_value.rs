//! A class method read as a VALUE (`const nn = d.n`) may later be
//! called bare through its boxed adapter, where a missing argument
//! arrives as `undefined`. An F64 parameter slot keeps that as its
//! sentinel; an I64 one cannot represent it, so the detached-call
//! verdict (`ssa_lower_module_metadata::collect_this_free_fids`)
//! refuses the method and every bare call throws. Seeding the method's
//! number parameters F64 at such a read pays the width only where a
//! program actually detaches the method. The same reads name the
//! methods whose every parameter may hold its slot's undefined
//! sentinel ([`detached_methods`], for the sentinel-parameter table):
//! `typeof s`, `s.length`, `x + 1` on a missing argument then answer
//! as they do for `undefined`, not for the sentinel's bit pattern.

use std::collections::HashSet;

use crate::ast::{Expr, ExprId, Param, PropKey, Stmt};
use crate::check::Type;

use super::Analysis;

impl<'a> Analysis<'a> {
    /// `obj.name` in value position, `obj` a class instance: the
    /// method body is `__cm_<C>__<name>` on the class or the nearest
    /// ancestor declaring it.
    pub(super) fn seed_method_value_escape(&mut self, obj: ExprId, name: &str) {
        let Some((fname, params)) = self.method_body_of(obj, name) else {
            return;
        };
        for p in params {
            if p.name != "__this"
                && matches!(p.type_ann.as_deref(), None | Some("number" | "i64" | "f64"))
            {
                self.seeds.push(self.keys.param(&fname, &p.name));
            }
        }
    }

    /// The declared body behind `obj.name` when `obj` is a class
    /// instance: `__cm_<C>__<name>` on the class or the nearest
    /// ancestor declaring it, with its parameter list.
    fn method_body_of(&self, obj: ExprId, name: &str) -> Option<(String, &'a [Param])> {
        let Some(Type::ClassRef(class)) = self.expr_types.get(&obj) else {
            return None;
        };
        let key = crate::ast::mangle_key(PropKey::from(name).as_wtf8()).into_owned();
        let mut owner = Some(class.clone());
        while let Some(c) = owner {
            let fname = format!("__cm_{c}__{key}");
            let params = self.ast.stmts.iter().find_map(|s| match s {
                Stmt::FnDecl { name, params, .. } if *name == fname => Some(params.as_slice()),
                _ => None,
            });
            if let Some(params) = params {
                return Some((fname, params));
            }
            owner = self.ast.class_parents.get(&c).cloned().flatten();
        }
        None
    }
}

/// Every class method some expression reads as a VALUE — a member
/// read on a class instance that is not a call's callee. Read before
/// the walk, because the sentinel tables close before it runs.
pub(super) fn detached_methods(a: &Analysis<'_>) -> HashSet<String> {
    let callees: HashSet<ExprId> = a
        .ast
        .exprs
        .iter()
        .filter_map(|e| match e {
            Expr::Call { callee, .. } | Expr::OptCall { callee, .. } => Some(*callee),
            _ => None,
        })
        .collect();
    let mut out = HashSet::new();
    for (i, e) in a.ast.exprs.iter().enumerate() {
        let Expr::Member { obj, name } = e else {
            continue;
        };
        if callees.contains(&ExprId(i as u32)) {
            continue;
        }
        if let Some((fname, _)) = a.method_body_of(*obj, name) {
            out.insert(fname);
        }
    }
    out
}
