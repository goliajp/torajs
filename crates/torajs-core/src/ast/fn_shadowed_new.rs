//! `new N()` where `N` names a `function` declared in the enclosing
//! function scope, not the class of that name.
//!
//! ```text
//! class Widget { tag = 1 }
//! (function () {
//!   function Widget() {}
//!   const x = new Widget()   // bun: the function's instance
//!   x.constructor === Widget // tr said false, and x.tag was 1
//! })()
//! ```
//!
//! `Expr::New` carries its target as a NAME, and `desugar_classes`
//! pass 2 binds every one of them to the `__new_<C>` factory of the
//! class with that name — a global lookup, with no notion of which
//! scope the `new` is written in. The lexical shadow that should win
//! is only established later, when `desugar_nested_fns` renames the
//! nested declaration; by then the `new` is a call to a factory and
//! the rename has nothing to bind to.
//!
//! Silently: both answer to the same name, so only an identity
//! comparison or a field read separates them. With a builtin error
//! name it is worse — the program gets the intrinsic, which is how
//! test262's harness/assert-throws-custom-typeerror reads its own
//! failure message.
//!
//! This pass runs before `desugar_classes` and records the sites where
//! the name is shadowed. Pass 2 leaves those alone, and they take the
//! ordinary non-class `new` route, where the declaration's own rename
//! does reach them.
//!
//! **Every binding kind that can name a constructor**, not just
//! `function`. An earlier note here claimed a `let` / `const` /
//! parameter "binds a value rather than a constructor, and a `new`
//! through one is already the dynamic route" — measured, that was
//! wrong: `class W {}` beside `function f() { const W = function
//! (){}; new W() }` handed the program the CLASS's instance, fields
//! and all, exactly as the `function` case did.
//!
//! The three kinds do not share a scope, so they are not collected
//! into one. A `function` declaration belongs to the function scope
//! (Annex B §B.3.3 puts a block-nested one there too, which is what
//! `block_nested_fn_names` answers); a parameter belongs to it as
//! well; a `let` / `const` belongs to its BLOCK. Flattening the last
//! into the function's set would shadow a `new` written outside the
//! block that declares it. `Stmt::Multi` shares the surrounding scope
//! by definition and so is spliced rather than given a scope.
//!
//! The other scopes a binding can open are walked as such: a loop
//! head's `let` / `const` and a `for-of` variable (§14.7.4.2,
//! §14.7.5.7), the one block every clause of a `switch` shares
//! (§14.12.4), and each callable body of a class — this pass runs
//! before `desugar_classes` flattens them, so they are still inside
//! `Stmt::ClassDecl`, along with the heritage and computed keys that
//! the enclosing scope evaluates.
//!
//! **The innermost binding decides.** A scope maps each name it binds
//! to whether that binding shadows: a class declaration's name (in the
//! list that declares it, and inside its own body, §15.7.14 step 4)
//! resolves to the class, which is the route `desugar_classes` gives a
//! `new` anyway, so it is recorded as NOT shadowing — an enclosing
//! `function W` is not what `new W()` means beside a `class W`.
//!
//! In strict code (a strict program, a class body, a function with a
//! `"use strict"` prologue) a `function` declared in a block binds in
//! that block only (§14.2.2; Annex B §B.3.3 is sloppy-only), so it is
//! not a name of the enclosing function scope there.

mod class;
mod expr;
mod scope;

use super::annexb_names::{block_nested_fn_names, own_fn_names, param_names};
use super::{Ast, ExprId, Stmt};
use scope::{add_class_names, block_scope, let_const_names, shadowing, var_names};
use std::collections::{HashMap, HashSet};

/// Name → whether the binding shadows a class or builtin of that name
/// for `new`. `false` is a class binding, which the class route reads.
type Scope = HashMap<String, bool>;

/// Record every `new <name>()` whose name is shadowed by a `function`
/// declaration in an enclosing function scope.
pub fn mark_fn_shadowed_new(ast: &mut Ast) {
    // The module's own statements are not a function scope: a
    // top-level `function N` next to a `class N` is a redeclaration,
    // which the injection gate and the checker already answer.
    let stmts = std::mem::take(&mut ast.stmts);
    let field_inits = ast.class_field_init_roots.iter().copied().collect();
    let strict = !ast.program_is_sloppy();
    let mut c = Census {
        ast,
        scopes: Vec::new(),
        out: HashSet::new(),
        field_inits,
        strict,
    };
    for s in &stmts {
        c.stmt(s);
    }
    let out = c.out;
    ast.stmts = stmts;
    ast.fn_shadowed_new = out;
}

struct Census<'a> {
    ast: &'a Ast,
    scopes: Vec<Scope>,
    out: HashSet<ExprId>,
    /// Class field initializers. An instance one rides the constructor
    /// body but reads the class's scope, not the constructor's
    /// parameters and locals (§15.7.10).
    field_inits: HashSet<ExprId>,
    strict: bool,
}

impl Census<'_> {
    fn shadowed(&self, name: &str) -> bool {
        self.scopes
            .iter()
            .rev()
            .find_map(|s| s.get(name))
            .copied()
            .unwrap_or(false)
    }

    /// A function scope: its `function` declarations (and, in sloppy
    /// code, the ones its blocks hoist under Annex B), its parameters,
    /// the `let` / `const` and classes its body binds at top level
    /// (the body is that block), and every `var` at any depth.
    fn body(&mut self, body: &[Stmt], params: &[super::Param]) {
        let scope = self.body_scope(body, params);
        let strict = self.strict;
        self.strict = strict || self.ast.body_says_use_strict(body);
        self.scopes.push(scope);
        for s in body {
            self.stmt(s);
        }
        self.scopes.pop();
        self.strict = strict;
    }

    fn body_scope(&self, body: &[Stmt], params: &[super::Param]) -> Scope {
        let mut names = own_fn_names(body);
        if !self.strict && !self.ast.body_says_use_strict(body) {
            names.extend(block_nested_fn_names(body));
        }
        names.extend(param_names(params));
        names.extend(let_const_names(body));
        var_names(body, &mut names);
        let mut scope = shadowing(names);
        add_class_names(body, &mut scope);
        scope
    }

    /// A block's own bindings, plus `extra` (a catch parameter).
    fn block(&mut self, list: &[Stmt], extra: Option<&str>) {
        let mut scope = block_scope(list);
        if let Some(p) = extra {
            scope.insert(p.to_string(), true);
        }
        self.scopes.push(scope);
        for s in list {
            self.stmt(s);
        }
        self.scopes.pop();
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Throw(e) | Stmt::Yield(e) => {
                self.expr(*e)
            }
            Stmt::YieldInto { value, .. } => self.expr(*value),
            Stmt::LetDecl { init, .. } | Stmt::UsingDecl { init, .. } => self.expr(*init),
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.expr(*cond);
                self.stmt(then_branch);
                if let Some(e) = else_branch {
                    self.stmt(e);
                }
            }
            Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
                self.expr(*cond);
                self.stmt(body);
            }
            // A `let` / `const` head binds in a scope around the whole
            // loop (§14.7.4.2); a `var` one is the function's.
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                let head = init
                    .as_deref()
                    .map(std::slice::from_ref)
                    .unwrap_or_default();
                self.scopes.push(shadowing(let_const_names(head)));
                if let Some(i) = init {
                    self.stmt(i);
                }
                if let Some(c) = cond {
                    self.expr(*c);
                }
                if let Some(s) = step {
                    self.expr(*s);
                }
                self.stmt(body);
                self.scopes.pop();
            }
            // The loop variable is bound in the body (§14.7.5.7).
            Stmt::ForOf {
                var_name,
                elem_expr,
                body,
                ..
            } => {
                self.expr(*elem_expr);
                self.loop_body(var_name, body);
            }
            Stmt::ForOfSplitIter {
                var_name,
                parent,
                sep,
                body,
            } => {
                self.expr(*parent);
                self.expr(*sep);
                self.loop_body(var_name, body);
            }
            // A block is a scope of its own for `let` / `const`.
            Stmt::Block(b) => self.block(b, None),
            Stmt::Multi(b) => {
                // Shares the surrounding scope — its bindings were already
                // collected by whoever owns that scope.
                for s in b {
                    self.stmt(s);
                }
            }
            // Each of the three is a block (§14.15), and the catch block
            // also binds its parameter. An async function's body is
            // wrapped in one before this pass runs.
            Stmt::Try {
                body,
                catch_param,
                catch_body,
                finally_body,
                ..
            } => {
                self.block(body, None);
                self.block(catch_body, catch_param.as_deref());
                if let Some(f) = finally_body {
                    self.block(f, None);
                }
            }
            Stmt::Switch {
                scrutinee,
                cases,
                default,
                ..
            } => {
                self.expr(*scrutinee);
                // Every clause shares one block (§14.12.4), so a `let` /
                // `const` in one clause is in scope in all of them.
                let mut scope = Scope::new();
                for list in cases.iter().map(|c| &c.body[..]).chain(default.as_deref()) {
                    scope.extend(block_scope(list));
                }
                self.scopes.push(scope);
                for c in cases {
                    self.expr(c.value);
                    for s in &c.body {
                        self.stmt(s);
                    }
                }
                for s in default.iter().flatten() {
                    self.stmt(s);
                }
                self.scopes.pop();
            }
            Stmt::Labeled { body, .. } => self.stmt(body),
            Stmt::FnDecl { body, params, .. } => self.body(body, params),
            Stmt::ExportDecl { inner, .. } => {
                if let Some(i) = inner {
                    self.stmt(i);
                }
            }
            Stmt::ClassDecl { .. } => self.class(stmt),
            Stmt::Return(None)
            | Stmt::Break(_)
            | Stmt::Continue(_)
            | Stmt::TypeDecl { .. }
            | Stmt::ImportDecl { .. } => {}
        }
    }

    fn loop_body(&mut self, var_name: &str, body: &Stmt) {
        self.scopes
            .push(Scope::from([(var_name.to_string(), true)]));
        self.stmt(body);
        self.scopes.pop();
    }
}
