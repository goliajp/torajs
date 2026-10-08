//! What the union rules know about one binding: the members it was
//! declared with, and whether it has gone loose.
//!
//! A binding in a scope frame carries this on its own `LocalInfo`
//! (`UnionBinding`), so a same-named binding elsewhere is unaffected.
//! A top-level binding a function body reads lives in `globals`, whose
//! names are unique; its loose mark is kept by name.

use crate::ast::{Ast, ExprId};
use crate::check::LocalInfo;
use crate::check_assignable::is_assignable_to_resolved;

use super::{Checker, Type};

impl Checker {
    fn scope_binding(&self, name: &str) -> Option<&LocalInfo> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }

    fn scope_binding_mut(&mut self, name: &str) -> Option<&mut LocalInfo> {
        self.scopes.iter_mut().rev().find_map(|s| s.get_mut(name))
    }

    /// The union type `name` is bound to right now, if it is one.
    pub(super) fn binding_union(&self, name: &str) -> Option<&Vec<Type>> {
        let ty = match self.scope_binding(name) {
            Some(info) => &info.ty,
            None => self.globals.get(name)?,
        };
        match ty {
            Type::Union(ms) => Some(ms),
            _ => None,
        }
    }

    /// The members `name` was declared with: what a narrow replaced,
    /// or its live union when nothing has.
    pub(crate) fn declared_union(&self, name: &str) -> Option<Vec<Type>> {
        if let Some(info) = self.scope_binding(name)
            && let Some(d) = &info.union.declared
        {
            return Some(d.clone());
        }
        self.binding_union(name).cloned()
    }

    /// Whether `name` is a loose union binding.
    pub(crate) fn is_loose(&self, name: &str) -> bool {
        match self.scope_binding(name) {
            Some(info) => info.union.loose,
            None => self.unions.loose_globals.contains(name),
        }
    }

    /// Marks the union binding `name` loose; any other binding is left
    /// alone.
    pub(crate) fn mark_loose(&mut self, name: &str) {
        match self.scope_binding_mut(name) {
            Some(info) => {
                if matches!(info.ty, Type::Union(_)) || info.union.declared.is_some() {
                    info.union.loose = true;
                }
            }
            None => {
                if matches!(self.globals.get(name), Some(Type::Union(_))) {
                    self.unions.loose_globals.insert(name.to_string());
                }
            }
        }
    }

    /// Replaces the union `name` holds with the narrow `ty`, keeping
    /// the declared members on the binding; answers the type to
    /// restore.
    pub(crate) fn apply_union_narrow(&mut self, name: &str, ty: Type) -> Option<Type> {
        if let Some(info) = self.scope_binding_mut(name)
            && info.union.declared.is_none()
            && let Type::Union(ms) = &info.ty
        {
            info.union.declared = Some(ms.clone());
        }
        self.apply_narrow(name, ty)
    }

    /// The slot an assignment to `name` is checked against: a narrowed
    /// union gives way to its declaration. The binding goes loose — the
    /// value assigned may be of any member.
    pub(crate) fn union_assign_target(&mut self, name: &str, target: Type) -> Type {
        if !matches!(target, Type::Union(_)) {
            return target;
        }
        self.mark_loose(name);
        match self.declared_union(name) {
            Some(d) => Type::Union(d),
            None => target,
        }
    }

    /// The union side of a declaration with an initializer.
    ///
    /// A boolean initializer may be a guard held in a binding
    /// (`const isS = typeof y === "string"; if (isS) y.length`, TS
    /// aliased conditions): the unions it names go loose.
    ///
    /// A union binding is narrowed to the members its initializer's
    /// type fits, as an assignment narrows it (TS §4.24 reads the
    /// initializer as the first assignment): `let x: string | number =
    /// "hi"` reads `x.toUpperCase()`.
    pub(crate) fn union_decl_narrow(&mut self, ast: &Ast, name: &str, init: ExprId) {
        let Some(init_ty) = self.expr_types.get(&init).cloned() else {
            return;
        };
        if init_ty == Type::Boolean {
            self.note_unmodeled_guard(ast, init);
        }
        let Some(members) = self.binding_union(name).cloned() else {
            return;
        };
        if init_ty == Type::Any || self.unions.types.contains_key(&init) {
            return;
        }
        let parts = match init_ty {
            Type::Nullable(t) => vec![*t, Type::Null, Type::Undefined],
            t => vec![t],
        };
        let kept: Vec<Type> = members
            .iter()
            .filter(|m| {
                parts.iter().any(|p| {
                    is_assignable_to_resolved(
                        m,
                        p,
                        &self.class_structs,
                        &self.aliases,
                        &self.generic_alias_decls,
                    )
                })
            })
            .cloned()
            .collect();
        if !kept.is_empty() && kept.len() < members.len() {
            self.apply_union_narrow(name, Type::Union(kept));
        }
    }
}
