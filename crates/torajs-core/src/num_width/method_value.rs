//! A class method read as a VALUE (`const nn = d.n`) may later be
//! called bare through its boxed adapter, where a missing argument
//! arrives as `undefined`. An F64 parameter slot keeps that as its
//! sentinel; an I64 one cannot represent it, so the detached-call
//! verdict (`ssa_lower_module_metadata::collect_this_free_fids`)
//! refuses the method and every bare call throws. Seeding the method's
//! number parameters F64 at such a read pays the width only where a
//! program actually detaches the method.

use crate::ast::{ExprId, PropKey, Stmt};
use crate::check::Type;

use super::{Analysis, SlotKey};

impl<'a> Analysis<'a> {
    /// `obj.name` in value position, `obj` a class instance: the
    /// method body is `__cm_<C>__<name>` on the class or the nearest
    /// ancestor declaring it.
    pub(super) fn seed_method_value_escape(&mut self, obj: ExprId, name: &str) {
        let Some(Type::ClassRef(class)) = self.expr_types.get(&obj) else {
            return;
        };
        let key = crate::ast::mangle_key(PropKey::from(name).as_wtf8()).into_owned();
        let mut owner = Some(class.clone());
        while let Some(c) = owner {
            let fname = format!("__cm_{c}__{key}");
            let params = self.ast.stmts.iter().find_map(|s| match s {
                Stmt::FnDecl { name, params, .. } if *name == fname => Some(params),
                _ => None,
            });
            if let Some(params) = params {
                for p in params {
                    if p.name != "__this"
                        && matches!(p.type_ann.as_deref(), None | Some("number" | "i64" | "f64"))
                    {
                        self.seeds
                            .push(SlotKey::Param(fname.clone(), p.name.clone()));
                    }
                }
                return;
            }
            owner = self.ast.class_parents.get(&c).cloned().flatten();
        }
    }
}
