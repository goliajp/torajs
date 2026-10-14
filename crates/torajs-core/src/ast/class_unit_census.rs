//! The reference census behind a unit class's admission
//! (`class_unit`): where a class's name occurs, and what else a
//! subtree holds that the whitelist refuses. Split out of `class_unit`
//! ahead of K5, which grows the admission side. Verbatim move.

use super::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use super::{Ast, Expr, ExprId, StaticInit, Stmt};

pub(super) fn names_class(e: &Expr, name: &str) -> bool {
    matches!(e, Expr::Ident(n) if n == name)
        || matches!(e, Expr::New { class_name, .. } if class_name == name)
}

pub(super) fn returns_value(s: &Stmt) -> bool {
    match s {
        Stmt::Return(Some(_)) => true,
        Stmt::FnDecl { .. } => false,
        other => stmt_children_ref(other).into_iter().any(returns_value),
    }
}

/// Occurrences of the class name (and of `arguments`) in a subtree,
/// every nested function and class body included.
#[derive(Default)]
pub(super) struct Occurrences {
    pub(super) name: usize,
    pub(super) arguments: bool,
    /// A `function` expression (not an arrow): its own `this`.
    pub(super) fn_exprs: bool,
    /// Leave class bodies out. A class body is hoisted out of the
    /// container, so a name it holds stops reading the container's
    /// binding: for the question "does every reference stay in the
    /// container" such an occurrence is NOT a local one.
    pub(super) skip_class_bodies: bool,
    /// A class declaration anywhere in the subtree (a class expression
    /// spliced beside its use site included), whatever
    /// `skip_class_bodies` says about its body.
    pub(super) holds_class: bool,
}

impl Occurrences {
    /// The static bodies of a class: static methods and accessors,
    /// static field initializers, static blocks.
    pub(super) fn statics(
        &mut self,
        ast: &Ast,
        static_methods: &[super::ClassMethod],
        static_init: &[StaticInit],
        name: &str,
    ) {
        for m in static_methods {
            self.list(ast, &m.body, name);
        }
        for si in static_init {
            match si {
                StaticInit::Block(b) => self.list(ast, b, name),
                StaticInit::Field(f) => self.expr(ast, f.init, name),
            }
        }
    }

    pub(super) fn list(&mut self, ast: &Ast, stmts: &[Stmt], name: &str) {
        for s in stmts {
            self.stmt(ast, s, name);
        }
    }

    pub(super) fn stmt(&mut self, ast: &Ast, s: &Stmt, name: &str) {
        if matches!(s, Stmt::ClassDecl { .. }) {
            self.holds_class = true;
        }
        match s {
            Stmt::FnDecl { body, .. } => self.list(ast, body, name),
            Stmt::ClassDecl {
                ctor,
                methods,
                static_methods,
                static_init,
                ..
            } if !self.skip_class_bodies => {
                if let Some(c) = ctor {
                    self.list(ast, &c.body, name);
                }
                for m in methods.iter().chain(static_methods) {
                    self.list(ast, &m.body, name);
                }
                for i in static_init {
                    match i {
                        StaticInit::Block(b) => self.list(ast, b, name),
                        StaticInit::Field(f) => self.expr(ast, f.init, name),
                    }
                }
            }
            _ => {}
        }
        // An arrow's body is reached through its own node.
        for e in stmt_exprs(s) {
            self.expr(ast, e, name);
        }
        for c in stmt_children_ref(s) {
            self.stmt(ast, c, name);
        }
    }

    pub(super) fn expr(&mut self, ast: &Ast, eid: ExprId, name: &str) {
        let e = ast.get_expr(eid);
        if names_class(e, name) {
            self.name += 1;
        }
        if matches!(e, Expr::Ident(n) if n == "arguments") {
            self.arguments = true;
        }
        if let Expr::ArrowFn { body, .. } = e {
            self.fn_exprs |= ast.fn_expr_exprs.contains(&eid);
            self.list(ast, body, name);
            return;
        }
        for c in expr_children(ast, eid) {
            self.expr(ast, c, name);
        }
    }
}

/// The references to `name` a unit class's STATIC bodies hold — static
/// methods and accessors (their parameter defaults included), static
/// field initializers, static blocks — each an `Ident` the unit lane
/// spells as `__unit`. Inside the class body the name is the class
/// scope's own binding (§15.7.14 step 4.a), this evaluation's class
/// object, and every static body has that object in hand: a static
/// method or accessor face holds it, a field initializer runs in
/// `__cunit_<C>`, a block takes it as a parameter. `None` when one of
/// them cannot be spelled that way: a `new C(…)` (the name as a
/// constructor callee), a scope in those bodies that binds the name
/// again, or a class declared in them (hoisted, its bodies leave
/// `__unit`'s reach).
pub(super) fn static_self_refs(
    ast: &Ast,
    static_methods: &[super::ClassMethod],
    static_init: &[StaticInit],
    name: &str,
) -> Option<Vec<ExprId>> {
    let mut walk = SelfRefs {
        name,
        ids: Vec::new(),
        ok: true,
    };
    for m in static_methods {
        walk.params(ast, &m.params);
        walk.list(ast, &m.body);
    }
    for si in static_init {
        match si {
            StaticInit::Field(f) => walk.expr(ast, f.init),
            StaticInit::Block(b) => walk.list(ast, b),
        }
    }
    walk.ok.then_some(walk.ids)
}

/// W8 — a static body of the admitted class `cls` naming the class
/// names this evaluation's class object, which it holds as `__unit`.
pub(super) fn spell_static_self_refs(ast: &mut Ast, cls: &Stmt) {
    let Stmt::ClassDecl {
        name,
        static_methods,
        static_init,
        ..
    } = cls
    else {
        return;
    };
    for id in static_self_refs(ast, static_methods, static_init, name).unwrap_or_default() {
        ast.exprs[id.0 as usize] = Expr::Ident(super::class_unit::UNIT_PARAM.into());
    }
}

struct SelfRefs<'a> {
    name: &'a str,
    ids: Vec<ExprId>,
    ok: bool,
}

impl SelfRefs<'_> {
    fn params(&mut self, ast: &Ast, params: &[super::Param]) {
        for p in params {
            self.ok &= p.name != self.name;
            if let Some(d) = p.default {
                self.expr(ast, d);
            }
        }
    }

    fn list(&mut self, ast: &Ast, stmts: &[Stmt]) {
        for s in stmts {
            self.stmt(ast, s);
        }
    }

    fn stmt(&mut self, ast: &Ast, s: &Stmt) {
        if matches!(s, Stmt::ClassDecl { .. })
            || super::desugar_generators_alpha::rebinds_in_stmt(s, self.name)
        {
            self.ok = false;
            return;
        }
        if let Stmt::FnDecl { params, body, .. } = s {
            self.params(ast, params);
            self.list(ast, body);
        }
        for e in stmt_exprs(s) {
            self.expr(ast, e);
        }
        for c in stmt_children_ref(s) {
            self.stmt(ast, c);
        }
    }

    fn expr(&mut self, ast: &Ast, eid: ExprId) {
        match ast.get_expr(eid) {
            Expr::Ident(n) if n == self.name => self.ids.push(eid),
            Expr::New { class_name, .. } if class_name == self.name => self.ok = false,
            Expr::ArrowFn { params, body, .. } => {
                self.params(ast, params);
                self.list(ast, body);
                return;
            }
            _ => {}
        }
        for c in expr_children(ast, eid) {
            self.expr(ast, c);
        }
    }
}
