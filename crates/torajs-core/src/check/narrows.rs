//! The checker's flow-narrowing ledgers, and the member-path guard
//! narrow that writes one of them.
//!
//! - `assign` — straight-line assignment narrows, `name → declared
//!   (pre-narrow) type` for bindings narrowed by a statement-level
//!   `b = <non-null>` assign (see check_assign_narrow.rs). Flushed
//!   (restored to declared) at every compound-statement boundary, so
//!   a narrow never crosses a branch join or a loop back-edge.
//! - `member` — RFC 20260710 C5 member-path narrows minted by an
//!   `if (o.cb) ...` guard: `(receiver path, field name)` → the
//!   field's Nullable inner type. `check_type_of_member` consults it
//!   before the declared field type; any assignment to the receiver
//!   or the member invalidates its entry (`check_type_of_assign`).
//! - a binding's `flow_declared` (`LocalInfo`) — the declared type
//!   behind a narrow that a later write may retire
//!   (`crate::check_assigns_to`); the methods below set it around a
//!   guarded region, snapshot it across `if` branches, and put a
//!   binding a loop writes back to it on the way into the loop.

use super::*;

#[derive(Default)]
pub(crate) struct Narrows {
    pub(crate) assign: HashMap<String, Type>,
    pub(crate) member: HashMap<(String, String), Type>,
}

/// What [`Checker::open_guard`] displaced (`None`: no such binding).
pub(crate) type GuardSave = Option<(String, Option<Type>)>;

/// The live type of each binding carrying a `flow_declared` type, by
/// scope depth and name.
pub(crate) type GuardTypes = Vec<((usize, String), Type)>;

impl Checker {
    fn binding(&self, name: &str) -> Option<&LocalInfo> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }

    fn binding_mut(&mut self, name: &str) -> Option<&mut LocalInfo> {
        self.scopes.iter_mut().rev().find_map(|s| s.get_mut(name))
    }

    /// Record `declared` for the binding `name` resolves to while a
    /// guarded region that writes it is walked.
    pub(crate) fn open_guard(&mut self, name: &str, declared: Type) -> GuardSave {
        let info = self.binding_mut(name)?;
        Some((name.to_string(), info.flow_declared.replace(declared)))
    }

    pub(crate) fn close_guard(&mut self, saved: GuardSave) {
        if let Some((name, prev)) = saved
            && let Some(info) = self.binding_mut(&name)
        {
            info.flow_declared = prev;
        }
    }

    /// Mark the binding `name` as narrowed in front of `declared` from
    /// here to the end of its scope.
    pub(crate) fn set_flow_declared(&mut self, name: &str, declared: Type) {
        if let Some(info) = self.binding_mut(name) {
            info.flow_declared = Some(declared);
        }
    }

    pub(crate) fn guard_declared(&self, name: &str) -> Option<&Type> {
        self.binding(name)?.flow_declared.as_ref()
    }

    /// The live types of the bindings carrying a `flow_declared` type:
    /// what a branch may change (a write there retires the narrow), so
    /// `if` gives each branch the state before it and joins the ones
    /// that fall through.
    pub(crate) fn guard_snapshot(&self) -> GuardTypes {
        let mut out = Vec::new();
        for (d, scope) in self.scopes.iter().enumerate() {
            for (n, info) in scope {
                if info.flow_declared.is_some() {
                    out.push(((d, n.clone()), info.ty.clone()));
                }
            }
        }
        out
    }

    pub(crate) fn guard_restore(&mut self, snap: &GuardTypes) {
        for ((d, n), t) in snap {
            if let Some(info) = self.scopes.get_mut(*d).and_then(|s| s.get_mut(n)) {
                info.ty = t.clone();
            }
        }
    }

    /// Two branches that both fall through: a binding they leave
    /// differently is back to its declared type.
    pub(crate) fn guard_join(&self, a: &GuardTypes, b: &GuardTypes) -> GuardTypes {
        let other: HashMap<_, _> = b.iter().cloned().collect();
        let declared = |(d, n): &(usize, String)| {
            let info = self.scopes.get(*d)?.get(n)?;
            info.flow_declared.clone()
        };
        let join = |(k, t): &((usize, String), Type)| match other.get(k) {
            Some(u) if u == t => t.clone(),
            _ => declared(k).unwrap_or_else(|| t.clone()),
        };
        a.iter().map(|e| (e.0.clone(), join(e))).collect()
    }

    /// A loop runs its uses again after its writes, which the single
    /// walk does not see: a binding narrowed in front of its
    /// `flow_declared` type that the loop (`exprs` and `body`) writes
    /// enters the loop at that declared type.
    pub(crate) fn enter_loop(&mut self, ast: &Ast, exprs: &[ExprId], body: &Stmt) {
        for scope in self.scopes.iter_mut() {
            for (name, info) in scope.iter_mut() {
                let Some(declared) = &info.flow_declared else {
                    continue;
                };
                let hit = crate::check_assigns_to::binding_hit(ast, name);
                if info.ty != *declared && crate::check_assigns_to::writes(ast, exprs, body, &hit) {
                    info.ty = declared.clone();
                }
            }
        }
    }

    /// RFC 20260710 C5 — detect a member-path narrowing cond on a
    /// truthiness / nullish-eq guard over `recv.field` where `recv`
    /// canonicalizes to a stable source path (chunk 789: an Ident
    /// or a Member chain `h.o` — see `member_path` for why Index
    /// receivers are excluded) and the receiver's struct field is
    /// declared `Nullable<T>`:
    /// `if (o.cb)` / `if (!o.cb)` / `o.cb !== null|undefined` /
    /// `o.cb === null|undefined`. Returns ((recv-path, field),
    /// inner, then-narrows). Mirrors [`Self::collect_null_narrow`]'s
    /// binding shapes; the narrow lands in `narrows.member` instead
    /// of a scope slot (a member path has no binding to retype).
    pub(crate) fn collect_member_narrow(
        &mut self,
        ast: &Ast,
        cond: ExprId,
    ) -> Option<((String, String), Type, bool)> {
        let (member_eid, polarity) = match ast.get_expr(cond) {
            Expr::Member { .. } => (cond, true),
            Expr::Unary {
                op: crate::ast::UnaryOp::Not,
                expr,
            } if matches!(ast.get_expr(*expr), Expr::Member { .. }) => (*expr, false),
            Expr::BinOp { op, left, right } => {
                let polarity = match op {
                    BinOp::Neq | BinOp::LooseNeq => true,
                    BinOp::Eq | BinOp::LooseEq => false,
                    _ => return None,
                };
                let is_nullish = |e: ExprId| {
                    matches!(ast.get_expr(e), Expr::Null)
                        || matches!(ast.get_expr(e), Expr::Ident(n) if n == "undefined")
                };
                if matches!(ast.get_expr(*left), Expr::Member { .. }) && is_nullish(*right) {
                    (*left, polarity)
                } else if matches!(ast.get_expr(*right), Expr::Member { .. }) && is_nullish(*left) {
                    (*right, polarity)
                } else {
                    return None;
                }
            }
            _ => return None,
        };
        let Expr::Member { obj, name: field } = ast.get_expr(member_eid) else {
            return None;
        };
        let (obj, field) = (*obj, field.clone());
        // Chunk 789 — canonical receiver path (Ident / Member chain /
        // int-literal Index); receiver type through the full type_of
        // so nested receivers (`h.o`, `arr[0]`) resolve like any
        // member read (including narrows already in force).
        let recv = crate::check_assigns_to::member_path(ast, obj)?;
        let recv_ty = self.type_of(ast, obj).ok()?;
        let fields = match recv_ty {
            Type::Struct(fields) => fields,
            Type::Object(alias) => match self.aliases.get(alias) {
                Some(Type::Struct(fields)) => fields.clone(),
                _ => return None,
            },
            // a class instance's declared data field narrows the same
            // way, unless it is a scalar whose slot stays boxed (the
            // narrowed type would misdescribe the slot, see
            // `narrow_within_lane`)
            Type::ClassRef(class) => match self.class_structs.get(&class) {
                Some(Type::Struct(fields)) => {
                    let fty = fields.iter().find(|(n, _)| *n == field).map(|(_, t)| t)?;
                    return match fty {
                        Type::Nullable(inner) if !rides_any_lane(fty) => {
                            Some(((recv, field), (**inner).clone(), polarity))
                        }
                        _ => None,
                    };
                }
                _ => return None,
            },
            _ => return None,
        };
        let fty = fields.iter().find(|(n, _)| *n == field).map(|(_, t)| t)?;
        if let Type::Nullable(inner) = fty {
            Some(((recv, field), (**inner).clone(), polarity))
        } else {
            None
        }
    }

    /// RFC 20260710 C5 — install a member-path narrow; returns the
    /// previous entry (shadow-restore across nested guards).
    pub(crate) fn apply_member_narrow(
        &mut self,
        key: &(String, String),
        inner_ty: Type,
    ) -> Option<Type> {
        self.narrows.member.insert(key.clone(), inner_ty)
    }

    /// Restore a member-path narrow to its pre-branch state.
    pub(crate) fn restore_member_narrow(&mut self, key: &(String, String), prev: Option<Type>) {
        match prev {
            Some(t) => {
                self.narrows.member.insert(key.clone(), t);
            }
            None => {
                self.narrows.member.remove(key);
            }
        }
    }
}
