//! Narrowing a binding typed as a union (TS §4.24 type guards).
//!
//! A union's value rides the any lane, so a narrow changes no
//! representation: what it changes is which members the member check
//! on the binding asks about ([`Checker::check_union_member`]). A
//! narrowed binding therefore stays a `Type::Union` — possibly of one
//! member — and still folds to `Any` wherever a value is typed.
//!
//! Only `typeof x === "tag"` as the whole condition of an `if` or a
//! ternary is narrowed member by member. Every other guard naming a
//! union binding (a discriminant compare, `Array.isArray(x)`,
//! `instanceof`, a user type predicate, a guard inside `&&` / `||` /
//! a loop or switch condition), and every assignment to one, marks the
//! binding loose: from there on a member read on it needs the name on
//! some member, not on every member. That under-refuses where TS would
//! have narrowed further, and never refuses a program TS accepts.

use crate::ast::{Ast, BinOp, Expr, ExprId};

use super::{Checker, Type};

/// The union facts the checker keeps beside its plain type tables.
#[derive(Default)]
pub(crate) struct UnionFacts {
    /// The expressions whose type is a union, with that union.
    /// `expr_types` holds what the value travels as (`Any`); this keeps
    /// the members, for the member check on a union receiver and for
    /// the census's `union` bucket.
    pub(crate) types: std::collections::HashMap<ExprId, Type>,
    /// Union bindings under a guard the checker does not narrow, or
    /// assigned to: their member reads take the loose rule. Keyed by
    /// name, so a same-named binding elsewhere is loosened too.
    pub(crate) loose: std::collections::HashSet<String>,
    /// The declared union of a binding a `typeof` guard has narrowed,
    /// so an assignment in the narrowed segment is checked against
    /// the declaration, not the narrow.
    pub(crate) declared: std::collections::HashMap<String, Type>,
}

/// What `typeof` answers for a value of type `t` (ES §13.5.3), when
/// the type decides it.
fn typeof_tag(t: &Type) -> Option<&'static str> {
    Some(match t {
        Type::Number => "number",
        Type::String => "string",
        Type::Boolean => "boolean",
        Type::BigInt => "bigint",
        Type::Symbol => "symbol",
        Type::Undefined | Type::Void => "undefined",
        Type::Function(..) | Type::Object(_) => "function",
        Type::TypeVar(_) | Type::Any | Type::Union(_) | Type::Never => return None,
        _ => "object",
    })
}

impl Checker {
    /// Records `t` for `eid` (its lane in `expr_types`, the union
    /// itself in `unions.types`) and answers the view a declared slot
    /// compares: the top union kept, unless `eid` reads a loose
    /// binding, whose surviving members are unknown — that value fits
    /// a slot the way an `any` does.
    pub(crate) fn record_type(&mut self, ast: &Ast, eid: ExprId, t: Type) -> Type {
        if !t.has_union() {
            if !self.unions.types.is_empty() {
                self.unions.types.remove(&eid);
            }
            self.expr_types.insert(eid, t.clone());
            return t;
        }
        let t = t.decl_view();
        if matches!(t, Type::Union(_)) {
            self.unions.types.insert(eid, t.clone());
        } else {
            self.unions.types.remove(&eid);
        }
        self.expr_types.insert(eid, t.clone().lane());
        match ast.get_expr(eid) {
            Expr::Ident(n) if self.unions.loose.contains(n) => t.lane(),
            _ => t,
        }
    }

    /// The union type `name` is bound to right now, if it is one.
    pub(super) fn binding_union(&self, name: &str) -> Option<&Vec<Type>> {
        let ty = match self.scopes.iter().rev().find_map(|s| s.get(name)) {
            Some(info) => &info.ty,
            None => self.globals.get(name)?,
        };
        match ty {
            Type::Union(ms) => Some(ms),
            _ => None,
        }
    }

    /// `typeof x === "tag"` (also `!==`, `==`, `!=`, either side
    /// first) on a union binding: the binding, its type where the
    /// guard holds, and where it fails.
    pub(crate) fn collect_typeof_narrow(
        &self,
        ast: &Ast,
        cond: ExprId,
    ) -> Option<(String, Type, Type)> {
        let Expr::BinOp { op, left, right } = ast.get_expr(cond) else {
            return None;
        };
        let holds_when_equal = match op {
            BinOp::Eq | BinOp::LooseEq => true,
            BinOp::Neq | BinOp::LooseNeq => false,
            _ => return None,
        };
        let operand = |a: ExprId, b: ExprId| match (ast.get_expr(a), ast.get_expr(b)) {
            (Expr::TypeOf { expr }, Expr::String(tag)) => match ast.get_expr(*expr) {
                Expr::Ident(n) => Some((n.clone(), tag.to_string())),
                _ => None,
            },
            _ => None,
        };
        let (name, tag) = operand(*left, *right).or_else(|| operand(*right, *left))?;
        let members = self.binding_union(&name)?.clone();
        let (is, is_not): (Vec<Type>, Vec<Type>) = members
            .iter()
            .cloned()
            .partition(|m| typeof_tag(m).is_none_or(|t| t == tag));
        // a member typeof cannot classify stays on both sides
        let unknown: Vec<Type> = members
            .iter()
            .filter(|m| typeof_tag(m).is_none())
            .cloned()
            .collect();
        let is_not = [is_not, unknown].concat();
        let side = |ms: Vec<Type>| {
            if ms.is_empty() {
                Type::Union(members.clone())
            } else {
                Type::Union(ms)
            }
        };
        let (eq, ne) = (side(is), side(is_not));
        Some(if holds_when_equal {
            (name, eq, ne)
        } else {
            (name, ne, eq)
        })
    }

    /// Installs a `typeof` narrow on `name`, remembering the declared
    /// union; answers the type to restore. A binding already narrowed
    /// holds a subset of the remembered union and keeps it; any other
    /// is a fresh declaration (another binding of the same name).
    pub(crate) fn apply_union_narrow(&mut self, name: &str, ty: Type) -> Option<Type> {
        if let Some(cur) = self.binding_union(name).cloned() {
            let known = match self.unions.declared.get(name) {
                Some(Type::Union(d)) => cur.iter().all(|m| d.contains(m)),
                _ => false,
            };
            if !known {
                self.unions
                    .declared
                    .insert(name.to_string(), Type::Union(cur));
            }
        }
        self.apply_narrow(name, ty)
    }

    /// Marks loose every union binding `cond` names (a guard this
    /// checker does not narrow).
    pub(crate) fn note_unmodeled_guard(&mut self, ast: &Ast, cond: ExprId) {
        let mut stack = vec![cond];
        while let Some(e) = stack.pop() {
            if let Expr::Ident(n) = ast.get_expr(e)
                && self.binding_union(n).is_some()
            {
                self.unions.loose.insert(n.clone());
            }
            crate::ast::desugar_async_sm_walk::visit_expr_children(ast, e, &mut |c| stack.push(c));
        }
    }

    /// The condition of an `if` / ternary: the `typeof` narrow it
    /// installs, or (when it is no such guard) the loose mark on the
    /// unions it names.
    pub(crate) fn union_guard(&mut self, ast: &Ast, cond: ExprId) -> Option<(String, Type, Type)> {
        let narrow = self.collect_typeof_narrow(ast, cond);
        if narrow.is_none() {
            self.note_unmodeled_guard(ast, cond);
        }
        narrow
    }

    /// The slot an assignment to `name` is checked against: a narrowed
    /// union gives way to its declaration. The binding goes loose — the
    /// value assigned may be of any member.
    pub(crate) fn union_assign_target(&mut self, name: &str, target: Type) -> Type {
        let Type::Union(cur) = &target else {
            return target;
        };
        self.unions.loose.insert(name.to_string());
        // a narrow is a subset of its declaration; a union that is not
        // one of the remembered union's subsets belongs to another
        // binding of the same name
        match self.unions.declared.get(name) {
            Some(Type::Union(d)) if cur.iter().all(|m| d.contains(m)) => Type::Union(d.clone()),
            _ => target,
        }
    }
}
