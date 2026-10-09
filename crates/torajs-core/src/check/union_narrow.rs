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
//! some declared member, not on every member. That under-refuses where
//! TS would have narrowed further, and never refuses a program TS
//! accepts. The binding's own state lives in `union_binding`.

use crate::ast::{Ast, BinOp, Expr, ExprId};

use super::{Checker, Type};

/// The union facts the checker keeps beside its plain type tables.
#[derive(Default)]
pub(crate) struct UnionFacts {
    /// The expressions whose type is a union or a dictionary, with
    /// that type. `expr_types` holds what the value travels as (`Any`);
    /// this keeps the members (the value type), for the member check on
    /// a union receiver, the write check on a dictionary and the
    /// census's `union` bucket.
    pub(crate) types: std::collections::HashMap<ExprId, Type>,
    /// The loose union bindings that live in `globals` — a scope
    /// binding keeps its mark on its own `LocalInfo`.
    pub(crate) loose_globals: std::collections::HashSet<String>,
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
        Type::Literal(l) => return typeof_tag(&l.base()),
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
        if !t.has_lane_fold() {
            if !self.unions.types.is_empty() {
                self.unions.types.remove(&eid);
            }
            self.expr_types.insert(eid, t.clone());
            return t;
        }
        let t = t.decl_view();
        if matches!(t, Type::Union(_) | Type::Dict(..)) {
            self.unions.types.insert(eid, t.clone());
        } else {
            self.unions.types.remove(&eid);
        }
        self.expr_types.insert(eid, t.clone().lane());
        match ast.get_expr(eid) {
            Expr::Ident(n) if self.is_loose(n) => t.lane(),
            _ => t,
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
        let declared = self
            .declared_union(&name)
            .unwrap_or_else(|| members.clone());
        let (is, is_not) = split_by_typeof(&members, &tag);
        let (d_is, d_not) = split_by_typeof(&declared, &tag);
        // a side the narrowed type leaves empty can still be reached
        // with a declared member, across a loop's back edge
        let side = |ms: Vec<Type>, d: Vec<Type>| {
            Type::Union(match (ms.is_empty(), d.is_empty()) {
                (false, _) => ms,
                (true, false) => d,
                (true, true) => declared.clone(),
            })
        };
        let (eq, ne) = (side(is, d_is), side(is_not, d_not));
        Some(if holds_when_equal {
            (name, eq, ne)
        } else {
            (name, ne, eq)
        })
    }

    /// Marks loose every union binding `cond` names (a guard this
    /// checker does not narrow).
    pub(crate) fn note_unmodeled_guard(&mut self, ast: &Ast, cond: ExprId) {
        let mut stack = vec![cond];
        while let Some(e) = stack.pop() {
            if let Expr::Ident(n) = ast.get_expr(e) {
                self.mark_loose(n);
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
}

/// `members` split by what `typeof` answers for each: `tag`, or
/// something else. A member `typeof` cannot classify stays on both
/// sides.
fn split_by_typeof(members: &[Type], tag: &str) -> (Vec<Type>, Vec<Type>) {
    let is = members
        .iter()
        .filter(|m| typeof_tag(m).is_none_or(|t| t == tag))
        .cloned()
        .collect();
    let is_not = members
        .iter()
        .filter(|m| typeof_tag(m) != Some(tag))
        .cloned()
        .collect();
    (is, is_not)
}
