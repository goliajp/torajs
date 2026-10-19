//! The scanner behind `fn_throw_info`: one function body at a time,
//! carrying the throw bit and callee list it builds, the fn-valued
//! names a call through is a call to an unknown target, and the
//! bindings whose guarded member / index reads can raise — with the
//! path narrowing that rules such a read out.
//!
//! A member or index read off a receiver that may be nullish lowers
//! to a runtime guard (`ssa_lower_nullable_guard`) that records a
//! TypeError. A function whose only raise was such a guard stayed out
//! of `may_throw`, so its callers pruned their check, the callee
//! answered the return sentinel and the throw surfaced at exit
//! instead of in the caller's `try`. The sources mirror the lowering's
//! registries (`nullish.rs`); a narrowing (`if (t === undefined)
//! return`, `t && t.k`, `while (t !== null)`) admits a binding on the
//! paths its test rules out, and only for a name no assignment
//! anywhere in the program writes, so the value the test saw is the
//! value the read sees.

use super::nullish::{ANY, Form, UNDEF};
use crate::ast::{Ast, Expr, ExprId, Param, Stmt};
use crate::num_width::WidthTable;
use std::collections::{HashMap, HashSet};

pub(crate) struct Scan<'a> {
    pub(crate) ast: &'a Ast,
    pub(crate) expr_types: &'a HashMap<ExprId, crate::check::Type>,
    pub(crate) widths: &'a WidthTable,
    pub(crate) direct: bool,
    pub(crate) called: Vec<String>,
    /// Fn-valued bindings visible to calls in this body: every param
    /// name (a call through one is necessarily fn-typed — the checker
    /// rejects calling a number), plus body `let`s as the stmt scan
    /// encounters them (declaration-before-use holds post-var-hoist).
    pub(crate) fn_values: HashSet<String>,
    /// Bindings that may hold a nullish value, and which forms.
    pub(crate) nullish: HashMap<String, Form>,
    /// Nullish bindings the path so far proved non-nullish.
    pub(crate) safe: HashSet<String>,
    /// Names some assignment anywhere in the program writes.
    assigned: &'a HashSet<String>,
    /// Receivers of a constructor's leading field writes, which no
    /// integrity level can refuse yet (`ast::fresh_this`).
    pub(crate) fresh_recv: HashSet<ExprId>,
    /// Parameters and `let`s some declaration annotates with an object
    /// type (`obj_cast::object_ann`): an `any` value assigned to one is
    /// cast. Never narrowed by a shadowing declaration.
    pub(crate) obj_slots: HashSet<String>,
    /// The scanned fn, for the width table's slot lookups.
    pub(crate) fn_name: String,
    /// A keyed read whose key may name a non-index property; it raises
    /// only in a program where such a property can run code.
    pub(crate) nonint_read: bool,
    /// A keyed store that can give an array such a property.
    pub(crate) code_write: bool,
}

impl<'a> Scan<'a> {
    pub(crate) fn new(
        ast: &'a Ast,
        name: &str,
        params: &[Param],
        expr_types: &'a HashMap<ExprId, crate::check::Type>,
        widths: &'a WidthTable,
        assigned: &'a HashSet<String>,
    ) -> Self {
        // the replace lane's view twin takes a capture group's view,
        // and a group that did not participate is the undefined
        // sentinel (`seed_undef_sentinel_params`)
        let view_twin = ast.is_replace_view_fn(name);
        let mut nullish = HashMap::new();
        for p in params {
            let ann = p.type_ann.as_deref();
            if ann.is_some_and(|a| a.starts_with("__nullable(")) {
                nullish.insert(p.name.clone(), ANY);
            } else if widths.param_takes_undef_sentinel(name, &p.name)
                || (view_twin && ann == Some("string"))
            {
                nullish.insert(p.name.clone(), UNDEF);
            }
        }
        Scan {
            ast,
            expr_types,
            widths,
            direct: false,
            called: Vec::new(),
            fn_values: params.iter().map(|p| p.name.clone()).collect(),
            nullish,
            safe: HashSet::new(),
            assigned,
            fresh_recv: HashSet::new(),
            obj_slots: (params.iter())
                .filter(|p| {
                    (p.type_ann.as_deref()).is_some_and(|a| super::obj_cast::object_ann(ast, a))
                })
                .map(|p| p.name.clone())
                .collect(),
            fn_name: name.to_string(),
            nonint_read: false,
            code_write: false,
        }
    }

    /// A block: bindings and narrowings made inside die with it.
    pub(crate) fn stmts(&mut self, stmts: &[Stmt]) {
        self.scoped(|sc| sc.seq(stmts));
    }

    /// A single statement in a scope of its own (a branch arm, a loop
    /// body).
    fn one(&mut self, s: &Stmt) {
        self.stmts(std::slice::from_ref(s));
    }

    fn scoped(&mut self, f: impl FnOnce(&mut Self)) {
        let nullish = self.nullish.clone();
        let safe = self.safe.clone();
        f(self);
        self.nullish = nullish;
        self.safe = safe;
    }

    /// Statements in order; an `if` whose arm leaves the sequence
    /// proves the other outcome of its test for what follows.
    fn seq(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.stmt(s);
            if let Stmt::If {
                cond,
                then_branch,
                else_branch,
            } = s
            {
                if diverges(then_branch) {
                    self.admit(*cond, false);
                }
                if else_branch.as_deref().is_some_and(diverges) {
                    self.admit(*cond, true);
                }
            }
        }
    }

    /// Walk `f` with `cond` known to have come out `when`.
    pub(crate) fn under(&mut self, cond: ExprId, when: bool, f: impl FnOnce(&mut Self)) {
        let safe = self.safe.clone();
        self.admit(cond, when);
        f(self);
        self.safe = safe;
    }

    fn admit(&mut self, cond: ExprId, when: bool) {
        for (name, excluded) in self.narrowings(cond, when) {
            if self.assigned.contains(&name) {
                continue;
            }
            if let Some(&f) = self.nullish.get(&name)
                && f & !excluded == 0
            {
                self.safe.insert(name);
            }
        }
    }

    /// Whether storing `value` into `target` casts into an object
    /// layout: an `any` value into a slot of an object type, or an
    /// object literal, laid out at the slot's declared type, with an
    /// `any` member that type gives an object type
    /// (`obj_cast::lit_casts_member`).
    pub(crate) fn store_casts(&self, target: ExprId, value: ExprId) -> bool {
        use super::obj_cast::{class_field_ann, lit_casts_member, lit_into_binding_casts};
        let (ast, types) = (self.ast, self.expr_types);
        if types.get(&value) == Some(&crate::check::Type::Any)
            && (super::obj_cast::object_ty(types.get(&target))
                || matches!(ast.get_expr(target), Expr::Ident(n) if self.obj_slots.contains(n)))
        {
            return true;
        }
        match ast.get_expr(target) {
            Expr::Ident(n) => lit_into_binding_casts(ast, types, n, value),
            Expr::Member { obj, name } => class_field_ann(ast, types, *obj, name)
                .is_some_and(|a| lit_casts_member(ast, types, &a, value)),
            _ => false,
        }
    }

    /// Whether a call through `callee` passes an object literal laid
    /// out at a parameter's declared type with an `any` member that
    /// type gives an object type (`obj_cast::lit_arg_casts_member`).
    pub(crate) fn args_cast(&self, callee: ExprId, args: &[ExprId]) -> bool {
        matches!(self.ast.get_expr(callee), Expr::Ident(n)
            if super::obj_cast::lit_arg_casts_member(self.ast, self.expr_types, n, args))
    }

    /// A binding takes the forms its init can answer, or the ones a
    /// written `T | null` / `T | undefined` says outright — the same
    /// split `record_binding_flags` makes.
    fn declare(&mut self, name: &str, type_ann: Option<&str>, init: ExprId) {
        let mut f = self.form(init);
        if type_ann.is_some_and(|a| a.starts_with("__nullable(")) {
            f |= ANY;
        }
        if type_ann == Some("string")
            && matches!(self.expr_types.get(&init), Some(crate::check::Type::Any))
        {
            f |= ANY;
        }
        // an `any` initializer of an object-typed binding, or of an
        // array of objects, is cast
        let any_init = matches!(self.expr_types.get(&init), Some(crate::check::Type::Any));
        if type_ann.is_some_and(|a| super::obj_cast::object_ann(self.ast, a)) {
            self.obj_slots.insert(name.to_string());
            self.direct |= any_init;
        } else if type_ann.is_some_and(|a| super::obj_cast::cast_binding_ann(self.ast, a)) {
            self.direct |= any_init;
        }
        if let Some(a) = type_ann {
            self.direct |= super::obj_cast::lit_casts_member(self.ast, self.expr_types, a, init);
        }
        if f != 0 {
            self.nullish.insert(name.to_string(), f);
        } else {
            self.nullish.remove(name);
        }
        self.safe.remove(name);
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Throw(eid) => {
                self.direct = true;
                self.expr(*eid);
            }
            Stmt::Expr(eid) | Stmt::Return(Some(eid)) | Stmt::Yield(eid) => self.expr(*eid),
            Stmt::YieldInto { value, .. } => self.expr(*value),
            Stmt::Return(None) | Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::Labeled { body, .. } => self.stmt(body),
            Stmt::LetDecl {
                name,
                type_ann,
                init,
                ..
            }
            | Stmt::UsingDecl {
                name,
                type_ann,
                init,
                ..
            } => {
                self.expr(*init);
                self.fn_values.insert(name.clone());
                self.declare(name, type_ann.as_deref(), *init);
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.expr(*cond);
                self.under(*cond, true, |sc| sc.one(then_branch));
                if let Some(eb) = else_branch {
                    self.under(*cond, false, |sc| sc.one(eb));
                }
            }
            Stmt::While { cond, body } => {
                self.expr(*cond);
                self.under(*cond, true, |sc| sc.one(body));
            }
            Stmt::DoWhile { body, cond } => {
                self.one(body);
                self.expr(*cond);
            }
            Stmt::Switch {
                scrutinee,
                cases,
                default,
                ..
            } => {
                self.expr(*scrutinee);
                for c in cases {
                    self.expr(c.value);
                    self.stmts(&c.body);
                }
                if let Some(db) = default {
                    self.stmts(db);
                }
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => self.scoped(|sc| {
                if let Some(i) = init {
                    sc.stmt(i);
                }
                if let Some(c) = cond {
                    sc.expr(*c);
                }
                if let Some(st) = step {
                    sc.expr(*st);
                }
                match cond {
                    Some(c) => sc.under(*c, true, |sc| sc.one(body)),
                    None => sc.one(body),
                }
            }),
            Stmt::Block(stmts) => self.stmts(stmts),
            // shares the surrounding scope (the variant's doc)
            Stmt::Multi(stmts) => self.seq(stmts),
            Stmt::ForOfSplitIter {
                parent, sep, body, ..
            } => {
                self.expr(*parent);
                self.expr(*sep);
                self.one(body);
            }
            // A `for..of` iterates its source under the same §7.4.2
            // protocol a spread does and has the same blind spot
            // (`scan_spread`'s doc). The source is the receiver of the
            // parser-minted element read (`src[i]`, see the variant doc).
            Stmt::ForOf {
                var_name,
                src_ident,
                elem_expr,
                body,
                ..
            } => {
                if let Expr::Index { obj, .. } = self.ast.get_expr(*elem_expr) {
                    crate::ast_throw_info_call::scan_spread(self, *obj);
                }
                self.expr(*elem_expr);
                self.scoped(|sc| {
                    // the loop variable holds what the container's
                    // slots hold (`note_sentinel_loop_var`)
                    if sc.widths.container_elem_takes_sentinel(src_ident) {
                        sc.nullish.insert(var_name.clone(), UNDEF);
                    } else {
                        sc.nullish.remove(var_name);
                    }
                    sc.safe.remove(var_name);
                    sc.stmt(body);
                });
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                self.stmts(body);
                self.stmts(catch_body);
                if let Some(fb) = finally_body {
                    self.stmts(fb);
                }
            }
            // desugar_classes runs before throw analysis; classes split
            // into FnDecls each get their own throw-info pass
            Stmt::FnDecl { .. }
            | Stmt::TypeDecl { .. }
            | Stmt::ClassDecl { .. }
            | Stmt::ImportDecl { .. } => {}
            Stmt::ExportDecl { inner, .. } => {
                if let Some(inner) = inner {
                    self.stmt(inner);
                }
            }
        }
    }
}

/// True when control cannot run past `s` into the statement after it.
fn diverges(s: &Stmt) -> bool {
    match s {
        Stmt::Return(_) | Stmt::Throw(_) | Stmt::Break(_) | Stmt::Continue(_) => true,
        Stmt::Block(stmts) | Stmt::Multi(stmts) => stmts.iter().any(diverges),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => diverges(then_branch) && else_branch.as_deref().is_some_and(diverges),
        Stmt::Labeled { body, .. } => diverges(body),
        _ => false,
    }
}
