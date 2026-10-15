//! A class declaration's share of the census: its own name, its
//! heritage and computed keys, and each callable body.

use super::super::class_computed_owner::owns;
use super::super::{ClassCtor, Expr, ExprId, StaticInit, Stmt};
use super::{Census, Scope};

impl Census<'_> {
    /// The class's own name is bound inside it (§15.7.14 step 4), and
    /// resolves to the class. The heritage and the computed keys are
    /// read in that scope; each constructor, method and static block is
    /// a function scope of its own, and a field initializer reads the
    /// class scope. The computed keys and computed static fields live
    /// in side tables keyed by the class name, which two functions may
    /// share, so a row counts only when this declaration owns it. A
    /// class body is strict code (§11.2.2).
    pub(super) fn class(&mut self, stmt: &Stmt) {
        let Stmt::ClassDecl {
            name,
            decl_id,
            parent,
            ctor,
            methods,
            static_methods,
            static_init,
            ..
        } = stmt
        else {
            return;
        };
        let strict = std::mem::replace(&mut self.strict, true);
        self.scopes.push(Scope::from([(name.clone(), false)]));
        if let Some(p) = parent {
            self.expr(*p);
        }
        let ast = self.ast;
        let owned = |c: &String, sent: &str| c == name && owns(ast, sent, *decl_id);
        let mut keys: Vec<ExprId> = ast
            .class_computed_keys
            .iter()
            .filter(|((c, sent), _)| owned(c, sent))
            .map(|(_, &e)| e)
            .collect();
        keys.sort_by_key(|e| e.0);
        for k in keys {
            self.expr(k);
        }
        if let Some(c) = ctor {
            self.ctor(c);
        }
        for m in methods.iter().chain(static_methods) {
            self.body(&m.body, &m.params);
        }
        for si in static_init {
            match si {
                StaticInit::Field(f) => self.expr(f.init),
                StaticInit::Block(b) => self.body(b, &[]),
            }
        }
        for (c, sent, init) in &ast.class_computed_static_fields {
            if owned(c, sent) {
                self.expr(*init);
            }
        }
        self.scopes.pop();
        self.strict = strict;
    }

    /// The parser folded the instance field initializers into the
    /// constructor body as `this.<n> = <init>`, each preceded by the
    /// declarations of the class expressions its initializer wrote,
    /// but they are evaluated in the class scope (§15.7.10), out of
    /// reach of the constructor's parameters and locals.
    fn ctor(&mut self, c: &ClassCtor) {
        let in_class_scope = self.field_init_stmts(&c.body);
        let scope = self.body_scope(&c.body, &c.params);
        self.scopes.push(scope);
        for (s, &field) in c.body.iter().zip(&in_class_scope) {
            if field {
                let own = self.scopes.pop();
                self.stmt(s);
                self.scopes.extend(own);
            } else {
                self.stmt(s);
            }
        }
        self.scopes.pop();
    }

    /// Which statements of a constructor body came from a field
    /// initializer: the `this.<n> = <init>` itself, and a run of class
    /// declarations right in front of one, which is where the parser
    /// puts the class expressions the initializer contains. Nothing a
    /// program writes can be a class declaration right in front of an
    /// initializer: the initializers go first, or right after the
    /// `super(...)` call.
    fn field_init_stmts(&self, body: &[Stmt]) -> Vec<bool> {
        let mut out = vec![false; body.len()];
        let mut next = false;
        for (i, s) in body.iter().enumerate().rev() {
            next = self.is_field_init(s) || (next && matches!(s, Stmt::ClassDecl { .. }));
            out[i] = next;
        }
        out
    }

    fn is_field_init(&self, s: &Stmt) -> bool {
        let Stmt::Expr(e) = s else {
            return false;
        };
        match self.ast.get_expr(*e) {
            Expr::Assign { value, .. } => self.field_inits.contains(value),
            _ => self.field_inits.contains(e),
        }
    }
}
