//! A member read on a union receiver (TS §4.13 property access): the
//! property must exist on every member, or the access is a compile
//! error. The value rides the any lane, which would answer any name at
//! runtime — so this check, not the lane, is what keeps `x.length` on
//! a `string | number` from reading `undefined` off the number.
//!
//! The nullish members are skipped, the posture a `T | null` receiver
//! already has: reading through a null is the runtime TypeError, not a
//! compile stop.

use crate::ast::{Ast, Expr, ExprId};

use super::{Checker, Type};

/// The `Object.prototype` members every non-nullish value has.
const OBJECT_PROTO: &[&str] = &[
    "toString",
    "toLocaleString",
    "valueOf",
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "constructor",
];

impl Checker {
    /// Refuses `obj.name` when `obj` was typed as a union and `name`
    /// is missing from one of its members.
    pub(crate) fn check_union_member(
        &self,
        ast: &Ast,
        obj: ExprId,
        name: &str,
    ) -> Result<(), String> {
        let Some(Type::Union(members)) = self.unions.types.get(&obj) else {
            return Ok(());
        };
        if OBJECT_PROTO.contains(&name) {
            return Ok(());
        }
        let solid: Vec<&Type> = members.iter().filter(|m| !m.is_nullish_value()).collect();
        let Some(missing) = solid.iter().find(|m| !self.has_member(ast, m, name)) else {
            return Ok(());
        };
        // a loose binding may be narrowed by a guard this checker does
        // not model: one declared member having the name is enough
        if let Expr::Ident(n) = ast.get_expr(obj)
            && self.is_loose(n)
            && (self.declared_union(n).unwrap_or_default().iter())
                .chain(solid.iter().copied())
                .any(|m| !m.is_nullish_value() && self.has_member(ast, m, name))
        {
            return Ok(());
        }
        Err(format!(
            "property `{name}` does not exist on every member of the union \
             {members:?} (missing on {missing:?})"
        ))
    }

    /// [`Self::check_union_member`] for the callee of `obj.name(..)`
    /// when `obj` is a union binding: several call routes claim a
    /// method call off its syntax without typing the member read.
    pub(crate) fn check_union_method_call(
        &mut self,
        ast: &Ast,
        callee: ExprId,
    ) -> Result<(), String> {
        if let Expr::Member { obj, name } = ast.get_expr(callee)
            && let Expr::Ident(binding) = ast.get_expr(*obj)
            && self.binding_union(binding).is_some()
        {
            self.type_of(ast, *obj)?;
            self.check_union_member(ast, *obj, name)?;
        }
        Ok(())
    }

    /// TS §4.15.1 argument assignability for the union parameters of a
    /// named function: an argument the any lane would carry anyway
    /// must still be one of the parameter's members.
    pub(crate) fn check_union_args(
        &self,
        ast: &Ast,
        callee: ExprId,
        args: &[ExprId],
    ) -> Result<(), String> {
        let Expr::Ident(f) = ast.get_expr(callee) else {
            return Ok(());
        };
        let fn_ty = match self.scopes.iter().rev().find_map(|s| s.get(f)) {
            Some(info) => &info.ty,
            None => match self.globals.get(f) {
                Some(t) => t,
                None => return Ok(()),
            },
        };
        let Type::Function(params, _) = fn_ty else {
            return Ok(());
        };
        for (i, (p, a)) in params.iter().zip(args).enumerate() {
            if !matches!(p, Type::Union(_)) {
                continue;
            }
            let given = match ast.get_expr(*a) {
                Expr::Ident(n) if self.is_loose(n) => continue,
                _ => self.unions.types.get(a).or_else(|| self.expr_types.get(a)),
            };
            if let Some(given) = given
                && !crate::check_assignable::is_assignable_to_resolved(
                    p,
                    given,
                    &self.class_structs,
                    &self.aliases,
                    &self.generic_alias_decls,
                )
            {
                return Err(format!(
                    "argument {} of `{f}`: {given:?} is not assignable to {p:?}",
                    i + 1
                ));
            }
        }
        Ok(())
    }

    fn has_member(&self, ast: &Ast, t: &Type, name: &str) -> bool {
        let resolved = super::resolve_class_ref(
            t,
            &self.class_structs,
            &self.aliases,
            &self.generic_alias_decls,
        );
        if let Type::Struct(fields) = &resolved
            && fields.iter().any(|(k, _)| k == name)
        {
            return true;
        }
        if let Type::ClassRef(c) = t {
            if crate::ast::accessor_lookup::accessor_getter_in_chain(ast, c, name).is_some()
                || crate::ast::accessor_lookup::accessor_setter_in_chain(ast, c, name).is_some()
            {
                return true;
            }
            // a generic class's methods are registered under its bare name
            let mut cur = c.split('<').next();
            let mut hops = ast.class_parents.len() + 1;
            while let Some(cname) = cur
                && hops > 0
            {
                if self.globals.contains_key(&format!("__cm_{cname}__{name}")) {
                    return true;
                }
                cur = ast.class_parents.get(cname).and_then(|p| p.as_deref());
                hops -= 1;
            }
        }
        crate::check_type_of_member::member_resolves(&resolved, name)
    }
}
