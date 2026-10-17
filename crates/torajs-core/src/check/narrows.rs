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

use super::*;

#[derive(Default)]
pub(crate) struct Narrows {
    pub(crate) assign: HashMap<String, Type>,
    pub(crate) member: HashMap<(String, String), Type>,
}

impl Checker {
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
