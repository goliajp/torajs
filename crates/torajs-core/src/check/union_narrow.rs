//! Narrowing a binding typed as a union (TS §4.24 type guards).
//!
//! A narrow changes no representation: the binding's slot is what its
//! declaration lowers to. What it changes is which members the member
//! check on the binding asks about ([`Checker::check_union_member`]),
//! and the type a field read off it has. A narrowed binding therefore
//! stays a `Type::Union` — possibly of one member — that travels as
//! the declared union does (`Type::union_narrowed`).
//!
//! Narrowed member by member: `typeof x === "tag"` and the
//! discriminant compare `x.k === "lit"` as the whole condition of an
//! `if` or a ternary, and a `switch (x.k)` whose cases are literals
//! (each clause sees the members its literals select). Every other
//! guard naming a union binding (a discriminant `==`, `Array.isArray(x)`,
//! `instanceof`, a user type predicate, a guard inside `&&` / `||` /
//! a loop or switch condition), and every assignment to one, marks the
//! binding loose: from there on a member read on it needs the name on
//! some declared member, not on every member. That under-refuses where
//! TS would have narrowed further, and never refuses a program TS
//! accepts. The binding's own state lives in `union_binding`.

use crate::ast::{Ast, BinOp, Expr, ExprId};

use super::{Checker, Lit, Type};

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

impl UnionFacts {
    /// The expressions typed as a dictionary whose values are numbers.
    /// Their keyed reads still answer `any` (a key may name an
    /// inherited member, or the entry may hold anything an `any` alias
    /// stored); the lowering only uses the fact to try the own entry
    /// first.
    pub(crate) fn number_dicts(&self) -> std::collections::HashSet<ExprId> {
        self.types
            .iter()
            .filter(|(_, t)| matches!(t, Type::Dict(_, v) if **v == Type::Number))
            .map(|(eid, _)| *eid)
            .collect()
    }
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
        let (eq, ne) = self.narrow_sides(&name, |ms| Some(split_by_typeof(ms, &tag)))?;
        Some(if holds_when_equal {
            (name, eq, ne)
        } else {
            (name, ne, eq)
        })
    }

    /// `x.k === v` / `x.k !== v` (either side first, `v` a literal or a
    /// `const` holding one) on a union binding `x` whose object members
    /// give `k` literal types (TS §4.24 discriminant narrowing): the
    /// binding, its type where the guard holds, and where it fails.
    /// `None` when no member's `k` is a literal type — then the guard
    /// is one this checker does not narrow.
    pub(crate) fn collect_discriminant_narrow(
        &self,
        ast: &Ast,
        cond: ExprId,
    ) -> Option<(String, Type, Type)> {
        let Expr::BinOp { op, left, right } = ast.get_expr(cond) else {
            return None;
        };
        let holds_when_equal = match op {
            BinOp::Eq => true,
            BinOp::Neq => false,
            _ => return None,
        };
        let operand = |a: ExprId, b: ExprId| {
            let (name, key) = self.discriminant_read(ast, a)?;
            Some((name, key, self.literal_operand(ast, b)?))
        };
        let (name, key, lit) = operand(*left, *right).or_else(|| operand(*right, *left))?;
        let (eq, ne) = self.narrow_sides(&name, |ms| self.split_by_literal(ms, &key, &lit))?;
        Some(if holds_when_equal {
            (name, eq, ne)
        } else {
            (name, ne, eq)
        })
    }

    /// `x.k` on a union binding `x`: the binding and `k`.
    pub(crate) fn discriminant_read(&self, ast: &Ast, e: ExprId) -> Option<(String, String)> {
        let Expr::Member { obj, name: key } = ast.get_expr(e) else {
            return None;
        };
        let Expr::Ident(name) = ast.get_expr(*obj) else {
            return None;
        };
        self.binding_union(name)?;
        Some((name.clone(), key.clone()))
    }

    /// The literal `e` spells, or the literal a `const` it reads holds.
    pub(crate) fn literal_operand(&self, ast: &Ast, e: ExprId) -> Option<Lit> {
        if let Some(lit) = Lit::of_expr(ast.get_expr(e)) {
            return Some(lit);
        }
        let Expr::Ident(n) = ast.get_expr(e) else {
            return None;
        };
        match self
            .scopes
            .iter()
            .rev()
            .find_map(|s| s.get(n))?
            .union
            .literal
        {
            Some(Type::Literal(ref lit)) => Some(lit.clone()),
            _ => None,
        }
    }

    /// The members of `members` whose `key` may be `lit`, and those
    /// whose `key` may be something else; `None` when no member gives
    /// `key` a literal type. A member whose `key` may hold several
    /// values (`"a" | "c"`, `"a" | null`, an optional `"a"`) is on each
    /// side one of them puts it; one whose `key` is not made of literal
    /// types (or that has no such field) stays on both sides.
    pub(crate) fn split_by_literal(
        &self,
        members: &[Type],
        key: &str,
        lit: &Lit,
    ) -> Option<(Vec<Type>, Vec<Type>)> {
        let mut discriminates = false;
        let (mut is, mut is_not) = (Vec::new(), Vec::new());
        for m in members {
            let (may_be, may_be_other) = match self.discriminant_values(m, key) {
                Some((lits, nullish)) => {
                    discriminates = true;
                    (lits.contains(lit), nullish || lits.iter().any(|l| l != lit))
                }
                None => (true, true),
            };
            if may_be {
                is.push(m.clone());
            }
            if may_be_other {
                is_not.push(m.clone());
            }
        }
        discriminates.then_some((is, is_not))
    }

    /// The values the field `key` of the union member `m` may hold,
    /// when its type is made of literal types ([`Type::literal_values`]).
    fn discriminant_values(&self, m: &Type, key: &str) -> Option<(Vec<Lit>, bool)> {
        let Type::Struct(fields) = super::resolve_class_ref(
            m,
            &self.class_structs,
            &self.aliases,
            &self.generic_alias_decls,
        ) else {
            return None;
        };
        let (_, t) = fields.iter().find(|(k, _)| k == key)?;
        let (lits, nullish) = t.literal_values()?;
        Some((lits.into_iter().cloned().collect(), nullish))
    }

    /// `switch (x.k)` on a union binding `x` whose object members give
    /// `k` literal types, every case value a literal: the binding and
    /// what each clause sees — the members its literal selects (the
    /// default: those no case selects), joined with what the clause
    /// before it sees when that one can fall through. Per case in
    /// order, then the default. `None` for any other switch.
    pub(crate) fn switch_narrows(
        &self,
        ast: &Ast,
        scrutinee: ExprId,
        cases: &[crate::ast::SwitchCase],
        default: Option<&[crate::ast::Stmt]>,
        default_at: usize,
    ) -> Option<(String, Vec<Type>, Type)> {
        let (name, key) = self.discriminant_read(ast, scrutinee)?;
        if self.is_loose(&name) {
            return None;
        }
        let members = self.binding_union(&name)?.clone();
        let mut selects: Vec<Vec<Type>> = Vec::with_capacity(cases.len());
        let mut case_lits: Vec<Lit> = Vec::with_capacity(cases.len());
        for c in cases {
            let lit = self.literal_operand(ast, c.value)?;
            let (is, _) = self.split_by_literal(&members, &key, &lit)?;
            selects.push(is);
            case_lits.push(lit);
        }
        // the default sees a member when its field may hold a value no
        // case names
        let unselected: Vec<Type> = members
            .iter()
            .filter(|m| {
                self.discriminant_values(m, &key)
                    .is_none_or(|(lits, nullish)| {
                        nullish || lits.iter().any(|l| !case_lits.contains(l))
                    })
            })
            .cloned()
            .collect();
        // the clauses in source order: (case index or the default, body)
        let mut order: Vec<(Option<usize>, &[crate::ast::Stmt])> = Vec::new();
        for (i, c) in cases.iter().enumerate() {
            if i == default_at {
                order.push((None, default.unwrap_or_default()));
            }
            order.push((Some(i), &c.body));
        }
        if default_at == cases.len() {
            order.push((None, default.unwrap_or_default()));
        }
        let mut per_case = vec![Type::Union(members.clone()); cases.len()];
        let mut on_default = Type::Union(members.clone());
        let mut carried: Vec<Type> = Vec::new();
        for (clause, body) in order {
            let own = match clause {
                Some(i) => &selects[i],
                None if default.is_some() => &unselected,
                None => continue,
            };
            let sees: Vec<Type> = members
                .iter()
                .filter(|m| own.contains(m) || carried.contains(m))
                .cloned()
                .collect();
            // a clause no member selects is unreachable: it keeps the
            // binding's own members
            let ty = Type::Union(if sees.is_empty() {
                members.clone()
            } else {
                sees.clone()
            });
            match clause {
                Some(i) => per_case[i] = ty,
                None => on_default = ty,
            }
            // a clause that may diverge in a form not decided here
            // carries nothing: the next clause under-refuses at worst
            let falls_through = body
                .last()
                .is_none_or(|s| self.stmt_end(ast, s) == super::End::Completes);
            carried = if falls_through { sees } else { Vec::new() };
        }
        Some((name, per_case, on_default))
    }

    /// The two sides of a guard on the union binding `name`: `split`
    /// answers them for a member list, read for the binding's current
    /// members and, where that leaves a side empty, for its declared
    /// ones — a side the narrowed type leaves empty can still be
    /// reached with a declared member, across a loop's back edge.
    fn narrow_sides(
        &self,
        name: &str,
        split: impl Fn(&[Type]) -> Option<(Vec<Type>, Vec<Type>)>,
    ) -> Option<(Type, Type)> {
        let members = self.binding_union(name)?.clone();
        let declared = self.declared_union(name).unwrap_or_else(|| members.clone());
        let (is, is_not) = split(&members)?;
        let (d_is, d_not) =
            split(&declared).unwrap_or_else(|| (declared.clone(), declared.clone()));
        let side = |ms: Vec<Type>, d: Vec<Type>| {
            Type::Union(match (ms.is_empty(), d.is_empty()) {
                (false, _) => ms,
                (true, false) => d,
                (true, true) => declared.clone(),
            })
        };
        Some((side(is, d_is), side(is_not, d_not)))
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
        let narrow = self
            .collect_typeof_narrow(ast, cond)
            .or_else(|| self.collect_discriminant_narrow(ast, cond));
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
