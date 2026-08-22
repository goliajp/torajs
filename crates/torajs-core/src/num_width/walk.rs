//! Constraint collection — statement / expression walks that turn the
//! module AST into seeds (statically-f64 writes) and edges (slot-to-
//! slot value flow) for the fixpoint in `mod.rs`.

use super::{Analysis, Scope, SlotKey, W};
use crate::ast::{Expr, ExprId, Stmt};

impl<'a> Analysis<'a> {
    pub(super) fn walk_stmt(&mut self, s: &Stmt, scope: &Scope) {
        match s {
            Stmt::LetDecl {
                name,
                type_ann,
                init,
                ..
            } => {
                let w = self.width_of(*init, scope);
                let key = if scope.fn_name.is_empty() {
                    SlotKey::Global(name.clone())
                } else {
                    SlotKey::Local(scope.fn_name.to_string(), name.clone())
                };
                // D5 — a binding annotated with a cyclic alias joins
                // the alias's nominal field-width point; F1 — one
                // annotated with a fn-type joins the spelling's
                // signature class.
                if let Some(ann) = type_ann {
                    self.alias_ann_union(&key, ann);
                    self.fnsig_ann_union(&key, ann);
                    // W-ESC — any-annotated binding is an escape
                    // sink (escape.rs).
                    self.seed_any_face(&key, ann);
                }
                // ②.7 — JSON.parse targets: runtime data, every
                // number-domain face seeds F64 (see json_seed.rs).
                if self.is_json_parse(*init) {
                    self.json_parse_seed(&key, type_ann.as_deref());
                }
                self.add_constraint(key.clone(), w);
                self.alias_guarded(key.clone(), *init, scope);
                self.fn_value_flow(&key, *init, scope);
                self.walk_expr(*init, scope);
            }
            Stmt::Return(maybe) => {
                if let Some(e) = maybe {
                    if !scope.fn_name.is_empty() {
                        let w = self.width_of(*e, scope);
                        let rk = SlotKey::Ret(scope.fn_name.to_string());
                        self.add_constraint(rk.clone(), w);
                        self.alias_guarded(rk.clone(), *e, scope);
                        self.fn_value_flow(&rk, *e, scope);
                    }
                    self.walk_expr(*e, scope);
                }
            }
            Stmt::Throw(e) => {
                // The thrown value goes through the one pending-throw
                // slot, so its width feeds that slot's class and comes
                // back out at whatever catch binding reads it.
                let w = self.width_of(*e, scope);
                self.add_container_constraint(SlotKey::Thrown, w);
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
            Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
                self.walk_expr(*cond, scope);
                self.walk_stmt(body, scope);
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                if let Some(i) = init {
                    self.walk_stmt(i, scope);
                }
                if let Some(c) = cond {
                    self.walk_expr(*c, scope);
                }
                if let Some(st) = step {
                    self.walk_expr(*st, scope);
                }
                self.walk_stmt(body, scope);
            }
            Stmt::ForOf {
                var_name,
                elem_expr,
                body,
                ..
            } => {
                // The element binding is fed by the indexed read the
                // parser pre-built (`src[i]`) — scalar width dep via
                // width_of (live once W4 wires the Index arm) plus a
                // guarded alias for container elements.
                let key = if scope.fn_name.is_empty() {
                    SlotKey::Global(var_name.clone())
                } else {
                    SlotKey::Local(scope.fn_name.to_string(), var_name.clone())
                };
                let w = self.width_of(*elem_expr, scope);
                self.add_constraint(key.clone(), w);
                self.alias_guarded(key, *elem_expr, scope);
                self.walk_expr(*elem_expr, scope);
                self.walk_stmt(body, scope);
            }
            Stmt::ForOfSplitIter {
                parent, sep, body, ..
            } => {
                self.walk_expr(*parent, scope);
                self.walk_expr(*sep, scope);
                self.walk_stmt(body, scope);
            }
            Stmt::Switch {
                scrutinee,
                cases,
                default,
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
                // poisons it.
                if let Some(p) = catch_param
                    && super::container_methods::in_number_domain(catch_type)
                    && let Some(k) = self.resolve(p, scope)
                {
                    self.uf.union(&SlotKey::Thrown, &k);
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

    /// Recurse into an expression collecting Assign / Call constraint
    /// edges at every nesting depth. Clones the node up front — the
    /// `&mut self` recursion can't hold a borrow into `self.ast`.
    pub(super) fn walk_expr(&mut self, eid: ExprId, scope: &Scope) {
        let e = self.ast.get_expr(eid).clone();
        match &e {
            Expr::Assign { target, value } => {
                if let Expr::Ident(n) = self.ast.get_expr(*target) {
                    let n = n.clone();
                    let w = self.width_of(*value, scope);
                    self.assign_to_name(&n, w, scope);
                    if let Some(k) = self.resolve(&n, scope) {
                        self.alias_guarded(k.clone(), *value, scope);
                        self.fn_value_flow(&k, *value, scope);
                    } else if scope.fn_name.starts_with("__") && self.by_name.contains_key(&n) {
                        // Captured container reassigned from a lifted
                        // closure — alias through the shared key.
                        self.alias_guarded(SlotKey::Captured(n), *value, scope);
                    }
                } else {
                    self.container_assign(*target, *value, scope);
                }
                self.walk_expr(*target, scope);
                self.walk_expr(*value, scope);
            }
            Expr::Call { callee, args } => {
                let callee_name: Option<String> = self.retargets.get(&eid).cloned().or_else(|| {
                    match self.ast.get_expr(*callee) {
                        Expr::Ident(n) => Some(n.clone()),
                        _ => None,
                    }
                });
                if let Some(fname) = callee_name
                    && let Some(pnames) = self.fn_params.get(&fname).cloned()
                {
                    for (i, arg) in args.iter().enumerate() {
                        if let Some(pname) = pnames.get(i) {
                            let w = self.width_of(*arg, scope);
                            let pk = SlotKey::Param(fname.clone(), pname.clone());
                            self.add_constraint(pk.clone(), w);
                            self.alias_guarded(pk.clone(), *arg, scope);
                            self.fn_value_flow(&pk, *arg, scope);
                        }
                    }
                } else if !matches!(self.ast.get_expr(*callee), Expr::Member { .. })
                    && let Some(ck) = self.container_key_lookup(*callee, scope)
                {
                    // F1 — indirect call through an fn value: args flow
                    // into the value's `__p{i}` projections (the
                    // indirect mirror of the direct Param edges; member
                    // callees keep their builtin method handling).
                    for (i, arg) in args.iter().enumerate() {
                        let w = self.width_of(*arg, scope);
                        let pk = SlotKey::Field(Box::new(ck.clone()), format!("__p{i}"));
                        self.add_constraint(pk.clone(), w);
                        // RFC 20260726-array-elem-width — a CONTAINER
                        // argument has to join the projection too, not
                        // just hand over its scalar width. The direct
                        // arm above does this; without it here, an
                        // arrow bound to a local (`let f = (xs:
                        // number[]) => xs[0]`) left the caller's array
                        // in a different class from the param reading
                        // it, so the two disagreed on element width.
                        // `fn_value_flow` at the binding's init already
                        // glued this projection onto the lifted fn's
                        // Param key, so one union reaches it.
                        self.alias_guarded(pk.clone(), *arg, scope);
                        self.fn_value_flow(&pk, *arg, scope);
                    }
                }
                self.member_call_effects(eid, *callee, args, scope);
                self.walk_expr(*callee, scope);
                for a in args {
                    self.walk_expr(*a, scope);
                }
            }
            Expr::BinOp { left, right, .. }
            | Expr::Sequence { left, right }
            | Expr::Nullish {
                lhs: left,
                rhs: right,
            } => {
                self.walk_expr(*left, scope);
                self.walk_expr(*right, scope);
            }
            Expr::Unary { expr, .. }
            | Expr::TypeOf { expr }
            | Expr::Spread { expr }
            | Expr::As { expr, .. } => self.walk_expr(*expr, scope),
            Expr::InstanceOf { expr, rhs } => {
                self.walk_expr(*expr, scope);
                self.walk_expr(*rhs, scope);
            }
            Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => self.walk_expr(*obj, scope),
            Expr::OptIndex { obj, index } | Expr::Index { obj, index } => {
                self.seed_index_read_elem(*obj, scope);
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
                // W4 shape-join, the array sibling of the ObjectLit
                // arm below (rotation 371) — every literal seeds its
                // element width unconditionally. The container walker
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
                // W4 shape-join (rotation 371) — every literal seeds
                // its field widths unconditionally. The container
                // walker only reaches literals standing in container
                // positions (let-init, known container flow), so a
                // call-argument literal never seeded: its fractional /
                // NaN initializer then truncated through the
                // same-shape layout's I64 slot (`anon_shape_unions`
                // gives the family one root; this hands the root
                // every member's evidence).
                let anon = SlotKey::Anon(eid.0);
                for (fname, e) in fields {
                    let fk = SlotKey::Field(Box::new(anon.clone()), fname.clone());
                    let w = self.width_of(*e, scope);
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
            Expr::PostIncr { target, .. } => self.walk_expr(*target, scope),
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
                for s in body {
                    self.walk_stmt(s, &inner);
                }
            }
            // Closure construction sites carry no inline body (the
            // lifted `__closure_N` FnDecl walks as a top-level stmt).
            _ => {}
        }
    }

    /// Constraint for a write to name `n` from inside `scope`. In
    /// synthetic closure bodies an unresolvable name is a captured
    /// outer binding whose defining scope is gone post-lift —
    /// broadcast to every same-named slot (conservative, F64-ward).
    pub(super) fn assign_to_name(&mut self, n: &str, w: W, scope: &Scope) {
        if let Some(key) = self.resolve(n, scope) {
            self.add_constraint(key, w);
        } else if scope.fn_name.starts_with("__")
            && let Some(keys) = self.by_name.get(n).cloned()
        {
            match w {
                W::F64 => {
                    for k in keys {
                        self.seeds.push(k);
                    }
                }
                W::Num(deps) => {
                    for (d, growth) in &deps {
                        for k in &keys {
                            self.edges
                                .entry(d.clone())
                                .or_default()
                                .push((k.clone(), *growth));
                        }
                    }
                }
                W::NotNum => {}
            }
        }
    }
}
