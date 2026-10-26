//! Constraint collection — statement / expression walks that turn the
//! module AST into seeds (statically-f64 writes) and edges (slot-to-
//! slot value flow) for the fixpoint in `mod.rs`.

use super::magnitude::Flow;
use super::{Analysis, Scope};
use crate::ast::{BinOp, Expr, ExprId, Stmt};

impl<'a> Analysis<'a> {
    pub(super) fn walk_stmt(&mut self, s: &Stmt, scope: &Scope) {
        match s {
            Stmt::LetDecl {
                name,
                type_ann,
                init,
                ..
            } => self.walk_let_decl(name, type_ann.as_deref(), *init, scope),
            Stmt::Return(Some(e)) => {
                if !scope.fn_name.is_empty() {
                    let w = self.flow(*e, scope);
                    let rk = self.keys.ret(scope.fn_name);
                    self.add_constraint(rk, w);
                    self.alias_guarded(rk, *e, scope);
                    self.fn_value_flow(rk, *e, scope);
                }
                self.walk_expr(*e, scope);
            }
            Stmt::Throw(e) => {
                // The thrown value goes through the one pending-throw
                // slot, so its width feeds that slot's class and comes
                // back out at whatever catch binding reads it.
                let w = self.flow(*e, scope);
                self.add_container_constraint(self.keys.thrown(), w);
                self.walk_expr(*e, scope);
            }
            Stmt::Expr(e) | Stmt::Yield(e) => self.walk_expr(*e, scope),
            Stmt::YieldInto { value, .. } => self.walk_expr(*value, scope),
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.walk_expr(*cond, scope);
                self.walk_stmt(then_branch, scope);
                if let Some(eb) = else_branch {
                    self.walk_stmt(eb, scope);
                }
            }
            Stmt::Labeled { body, .. } => {
                self.walk_stmt(body, scope);
            }
            Stmt::While { .. } | Stmt::DoWhile { .. } | Stmt::For { .. } => {
                self.walk_loop(s, scope)
            }
            Stmt::ForOf {
                var_name,
                elem_expr,
                body,
                is_await,
                ..
            } => {
                // The element binding is fed by the indexed read the
                // parser pre-built (`src[i]`) — scalar width dep via
                // width_of (live once W4 wires the Index arm) plus a
                // guarded alias for container elements.
                let key = if scope.fn_name.is_empty() {
                    self.keys.global(var_name)
                } else {
                    self.keys.local(scope.fn_name, var_name)
                };
                let w = self.flow(*elem_expr, scope);
                self.add_constraint(key, w);
                // a `Map<K, number>` values() source steps an f64 on the
                // typed lane (`ssa_lower_map_num_lane`), sentinel included
                if let Expr::Index { obj, .. } = self.ast.get_expr(*elem_expr)
                    && matches!(self.expr_types.get(obj),
                        Some(crate::check::Type::MapIter(t)) if **t == crate::check::Type::Number)
                {
                    self.add_constraint(key, super::magnitude::f64_flow());
                }
                // a `for await` over anything but an array steps through
                // the any lane (`ssa_lower_stmt_for_of`): the value is the
                // runtime's, at no width the program's slots can vouch for
                if *is_await
                    && let Expr::Index { obj, .. } = self.ast.get_expr(*elem_expr)
                    && !matches!(self.expr_types.get(obj), Some(crate::check::Type::Array(_)))
                {
                    self.add_constraint(key, super::magnitude::f64_flow());
                }
                self.alias_guarded(key, *elem_expr, scope);
                self.walk_expr(*elem_expr, scope);
                self.mag_loop(None, scope, |a| a.walk_stmt(body, scope));
            }
            Stmt::ForOfSplitIter {
                parent, sep, body, ..
            } => {
                self.walk_expr(*parent, scope);
                self.walk_expr(*sep, scope);
                self.mag_loop(None, scope, |a| a.walk_stmt(body, scope));
            }
            Stmt::Switch {
                scrutinee,
                cases,
                default,
                ..
            } => {
                self.walk_expr(*scrutinee, scope);
                for c in cases {
                    self.walk_expr(c.value, scope);
                    for cs in &c.body {
                        self.walk_stmt(cs, scope);
                    }
                }
                if let Some(d) = default {
                    for ds in d {
                        self.walk_stmt(ds, scope);
                    }
                }
            }
            Stmt::Try {
                body,
                catch_param,
                catch_type,
                catch_body,
                finally_body,
                ..
            } => {
                // The catch binding reads the same raw slot every throw
                // writes, so the two share one class — the promise
                // `value` shape. Gated on the number domain for the
                // same reason that one is: a slot carries any type, and
                // gluing a non-numeric face into the numeric class
                // poisons it. A written `number` binds at f64 whatever
                // the throws wrote (the binding reads the slot's tag and
                // widens an integer), so it is an f64 write of its own
                // and leaves the throw sites' class alone.
                if let Some(p) = catch_param
                    && super::container_methods::in_number_domain(catch_type)
                    && let Some(k) = self.resolve(p, scope)
                {
                    if catch_type.as_deref().map(str::trim) == Some("number") {
                        self.add_constraint(k, super::magnitude::f64_flow());
                    } else {
                        self.uf.union(&self.keys, self.keys.thrown(), k);
                    }
                }
                for bs in body {
                    self.walk_stmt(bs, scope);
                }
                for cs in catch_body {
                    self.walk_stmt(cs, scope);
                }
                if let Some(fb) = finally_body {
                    for fs in fb {
                        self.walk_stmt(fs, scope);
                    }
                }
            }
            Stmt::Block(v) | Stmt::Multi(v) => {
                for bs in v {
                    self.bounds_evict(bs);
                    self.walk_stmt(bs, scope);
                }
            }
            Stmt::ExportDecl {
                inner,
                default_expr,
                ..
            } => {
                if let Some(i) = inner {
                    self.walk_stmt(i, scope);
                }
                if let Some(de) = default_expr {
                    self.walk_expr(*de, scope);
                }
            }
            // Nested FnDecl (pre-desugar shape) — own scope, walked
            // when iterating ast.stmts only if top-level; nested ones
            // are rare pre-lift and their writes go through capture
            // machinery the lowerer rejects separately.
            _ => {}
        }
    }

    /// What a binding declaration contributes: the slot's own width
    /// constraint plus every hookup its annotation and initializer
    /// imply. Split out of `walk_stmt` — it is the one arm that
    /// touches five of the analysis's channels at once.
    fn walk_let_decl(&mut self, name: &str, type_ann: Option<&str>, init: ExprId, scope: &Scope) {
        let w = self.flow(init, scope);
        let key = if scope.fn_name.is_empty() {
            self.keys.global(name)
        } else {
            self.keys.local(scope.fn_name, name)
        };
        // D5 — a binding annotated with a cyclic alias joins the
        // alias's nominal field-width point; F1 — one annotated with
        // a fn-type joins the spelling's signature class.
        if let Some(ann) = type_ann {
            self.alias_ann_union(key, ann);
            self.fnsig_ann_union(key, ann);
            // W-ESC — any-annotated binding is an escape sink
            // (escape.rs).
            self.seed_any_face(key, ann, true);
        }
        // ②.7 — JSON.parse targets: runtime data, every number-domain
        // face seeds F64 (see json_seed.rs).
        if self.is_json_parse(init) {
            self.json_parse_seed(key, type_ann);
        } else if let Some(ann) = type_ann
            && self.expr_types.get(&init) == Some(&crate::check::Type::Any)
        {
            self.any_object_seed(init, key, ann);
        } else if let Some(ann) = type_ann {
            self.any_member_seed(init, key, ann);
        }
        self.add_constraint(key, w);
        self.alias_guarded(key, init, scope);
        self.fn_value_flow(key, init, scope);
        self.walk_expr(init, scope);
    }

    /// Recurse into an expression collecting Assign / Call constraint
    /// edges at every nesting depth. Clones the node up front — the
    /// `&mut self` recursion can't hold a borrow into `self.ast`.
    pub(super) fn walk_expr(&mut self, eid: ExprId, scope: &Scope) {
        let e = self.ast.get_expr(eid).clone();
        match &e {
            Expr::Assign { target, value } => {
                if let Expr::Ident(n) = self.ast.get_expr(*target) {
                    let n = n.clone();
                    let w = self.flow(*value, scope);
                    self.assign_to_name(&n, w, scope);
                    if let Some(k) = self.resolve(&n, scope) {
                        let any = self.expr_types.get(value) == Some(&crate::check::Type::Any);
                        for ann in self.binding_anns(k) {
                            if any {
                                self.any_object_seed(*value, k, &ann);
                            } else {
                                self.any_member_seed(*value, k, &ann);
                            }
                        }
                        self.alias_guarded(k, *value, scope);
                        self.fn_value_flow(k, *value, scope);
                    } else if scope.fn_name.starts_with("__") && self.named(&n).is_some() {
                        // Captured container reassigned from a lifted
                        // closure — alias through the shared key.
                        self.alias_guarded(self.keys.captured(&n), *value, scope);
                    }
                } else {
                    self.container_assign(*target, *value, scope);
                }
                self.walk_expr(*target, scope);
                self.walk_expr(*value, scope);
            }
            Expr::Call { callee, args } => self.walk_call_site(eid, *callee, args, scope),
            Expr::BinOp { op, left, right } => {
                if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul) {
                    self.record_arith(eid, scope);
                }
                self.walk_expr(*left, scope);
                self.walk_expr(*right, scope);
            }
            Expr::Sequence { left, right }
            | Expr::Nullish {
                lhs: left,
                rhs: right,
            } => {
                self.walk_expr(*left, scope);
                self.walk_expr(*right, scope);
            }
            // `delete xs[i]` leaves a hole, and a number[] hole holds the
            // F64 sentinel: seed the element slot like an unproven read
            // (an I64 slot has no word for it; the kernel refuses).
            Expr::Delete { expr } if matches!(self.ast.get_expr(*expr), Expr::Index { .. }) => {
                if let Expr::Index { obj, index } = self.ast.get_expr(*expr) {
                    let (obj, index) = (*obj, *index);
                    self.seed_index_read_elem(obj, scope, false);
                    self.walk_expr(obj, scope);
                    self.walk_expr(index, scope);
                }
            }
            Expr::Unary { expr, .. }
            | Expr::Delete { expr }
            | Expr::TypeOf { expr }
            | Expr::Spread { expr } => self.walk_expr(*expr, scope),
            Expr::As { expr, ty_ann } => self.walk_cast(*expr, ty_ann, scope),
            Expr::InstanceOf { expr, rhs } => {
                self.walk_expr(*expr, scope);
                self.walk_expr(*rhs, scope);
            }
            Expr::Member { obj, name } => {
                self.member_read_effects(eid, *obj, name, scope);
                self.walk_expr(*obj, scope);
            }
            Expr::OptChain { obj, .. } => self.walk_expr(*obj, scope),
            Expr::OptIndex { obj, index } | Expr::Index { obj, index } => {
                let in_bounds = self.bounds_record(eid, *obj, *index);
                self.seed_index_read_elem(*obj, scope, in_bounds);
                self.walk_expr(*obj, scope);
                self.walk_expr(*index, scope);
            }
            Expr::OptCall { callee, args } => {
                self.walk_expr(*callee, scope);
                for a in args {
                    self.walk_expr(*a, scope);
                }
            }
            Expr::Array(elems) => {
                // The array sibling of the ObjectLit arm below — every
                // literal seeds its element width unconditionally. The container walker
                // only reaches literals standing in container
                // positions, so an array literal handed straight to a
                // call never seeded: `JSON.stringify([1, 2.5])` took
                // its element type from the first element and read the
                // fractional one back through the I64 layout as its
                // own bits.
                //
                // The seeding is the container walker's own Array arm
                // rather than a copy of it — that one already knows
                // what a spread element means (union with the source's
                // element class) and what a nested container element
                // means, and both of those are wrong to guess at here.
                // Reaching it twice is harmless: the width join and
                // the unions are idempotent.
                self.container_key_of(eid, scope);
                for e in elems {
                    self.walk_expr(*e, scope);
                }
            }
            Expr::ObjectLit { fields } => {
                // Every literal seeds its own field widths
                // unconditionally. The container walker only reaches
                // literals standing in container positions (let-init,
                // known container flow), so a call-argument literal
                // never seeded and its fractional / NaN initializer
                // truncated through an I64 slot.
                let anon = self.keys.anon(eid.0);
                for (fname, e) in fields {
                    let fk = self.keys.field(anon, fname);
                    let w = self.flow(*e, scope);
                    self.add_container_constraint(fk, w);
                    self.walk_expr(*e, scope);
                }
            }
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                self.walk_expr(*cond, scope);
                self.walk_expr(*then_branch, scope);
                self.walk_expr(*else_branch, scope);
            }
            Expr::New { args, .. } | Expr::Super { args } => {
                for a in args {
                    self.walk_expr(*a, scope);
                }
            }
            Expr::PostIncr { target, is_inc, .. } => {
                self.record_step(*target, *is_inc, scope);
                self.walk_expr(*target, scope);
            }
            Expr::ArrowFn { params, body, .. } => {
                // Pre-lift arrow (rare at this stage): walk its body
                // under an extended scope so writes to outer slots
                // still register; its own params shadow.
                let inner = Scope {
                    fn_name: scope.fn_name,
                    params: scope
                        .params
                        .iter()
                        .cloned()
                        .chain(params.iter().map(|p| p.name.clone()))
                        .collect(),
                    locals: scope.locals.clone(),
                };
                // The pairs standing here were proven at the site
                // that BUILDS the closure, not the one that calls it;
                // the body must not see them. Lowering agrees by
                // giving every function body a fresh stack.
                let outer = std::mem::take(&mut self.bounds_stack);
                self.mag_opaque(|a| body.iter().for_each(|s| a.walk_stmt(s, &inner)));
                self.bounds_stack = outer;
            }
            // Closure construction sites carry no inline body (the
            // lifted `__closure_N` FnDecl walks as a top-level stmt).
            _ => {}
        }
    }

    /// Constraint for a write to name `n` from inside `scope`. In a
    /// synthetic body a name `resolve` cannot pin (no capture binding,
    /// see `captures.rs`) may be any outer binding of that spelling —
    /// broadcast to every same-named slot (conservative, F64-ward).
    pub(super) fn assign_to_name(&mut self, n: &str, w: Flow, scope: &Scope) {
        if let Some(key) = self.resolve(n, scope) {
            self.add_constraint(key, w);
        } else if scope.fn_name.starts_with("__")
            && let Some(keys) = self.named(n).cloned()
        {
            for k in keys {
                self.add_constraint(k, w.clone());
            }
        }
    }
}
