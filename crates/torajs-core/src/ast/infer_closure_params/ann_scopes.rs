//! The receiver table read at one site: the annotations of the
//! bindings visible there, scope by scope (§9.1 Environment Records).
//!
//! The flat table is keyed by bare name, so two functions that each
//! declare an `fs` of a different type share one entry, and the arrows
//! one of them pushes are typed from the other's element type: the
//! checker then refuses a program bun runs (`fs: Array<() => number>`
//! in one function, `fs: Array<() => string>` in the next). Such a
//! program reads each name through the scopes enclosing the site
//! instead: the block, the loop head, the catch clause, the function
//! (its parameters and hoisted `var`s), and for a lifted arrow the
//! scope the arrow was written in, up to the module. A declaration
//! without an annotation shadows like any other and answers what its
//! initializer gives it, the way the module's own bindings are read:
//! a literal, `new Map<K, V>()`, or a call to a function that declares
//! its return; any other initializer answers nothing.
//!
//! The walk is paid only by a program in which some name is declared
//! twice with different annotations, and there only once a site asks;
//! everywhere else the flat table already gives every site the answer
//! the scopes would. A site the walk does not reach reads the module's
//! scope.

use super::super::desugar_with::walk::{expr_children, stmt_exprs};
use super::super::infer_closure_lets::lifted_closure_name;
use super::super::{Ast, Expr, ExprId, Stmt};
use std::cell::OnceCell;
use std::collections::HashMap;

mod collisions;
mod compound;
mod declare;
pub(in crate::ast) use collisions::{Collisions, module_and_scopes};

/// The module's scope, the first one the walk makes.
const ROOT: u32 = 0;

pub(in crate::ast) struct AnnScopes {
    flat: HashMap<String, String>,
    /// Set when some name collides: the walk, made when the first site
    /// asks. The AST is not changed between the table and its last
    /// reader, so a walk made then answers as one made up front.
    scoped: Option<Pending>,
}

struct Pending {
    declared_rets: HashMap<String, String>,
    walked: OnceCell<Walked>,
}

struct Walked {
    scopes: Vec<Scope>,
    site: HashMap<ExprId, u32>,
}

struct Scope {
    parent: Option<u32>,
    names: HashMap<String, Option<String>>,
}

/// The bindings one site sees.
#[derive(Clone, Copy)]
pub(in crate::ast) struct Anns<'a> {
    flat: &'a HashMap<String, String>,
    walked: Option<&'a Walked>,
    scope: Option<u32>,
}

impl<'a> Anns<'a> {
    /// A site the walk did not reach reads the module's bindings; a
    /// name no enclosing scope declares is not bound there, whatever
    /// another function calls the same name.
    pub(in crate::ast) fn get(&self, name: &str) -> Option<&'a String> {
        let Some(w) = self.walked else {
            return self.flat.get(name);
        };
        let mut s = Some(self.scope.unwrap_or(ROOT));
        while let Some(i) = s {
            let sc = &w.scopes[i as usize];
            if let Some(ann) = sc.names.get(name) {
                return ann.as_ref();
            }
            s = sc.parent;
        }
        None
    }

    pub(in crate::ast) fn contains_key(&self, name: &str) -> bool {
        self.get(name).is_some()
    }
}

impl AnnScopes {
    pub(in crate::ast) fn flat(flat: HashMap<String, String>) -> Self {
        AnnScopes { flat, scoped: None }
    }

    /// `declared_rets` feeds the initializer inference of the
    /// unannotated bindings, the one the flat table applies to the
    /// module's.
    pub(in crate::ast) fn scoped(
        flat: HashMap<String, String>,
        declared_rets: HashMap<String, String>,
    ) -> Self {
        let walked = OnceCell::new();
        let scoped = Some(Pending {
            declared_rets,
            walked,
        });
        AnnScopes { flat, scoped }
    }

    /// The bindings `site` of `ast` sees.
    pub(in crate::ast) fn at(&self, ast: &Ast, site: ExprId) -> Anns<'_> {
        let walked =
            (self.scoped.as_ref()).map(|p| p.walked.get_or_init(|| walk(ast, &p.declared_rets)));
        Anns {
            flat: &self.flat,
            walked,
            scope: walked.and_then(|w| w.site.get(&site).copied()),
        }
    }
}

fn walk(ast: &Ast, declared_rets: &HashMap<String, String>) -> Walked {
    let mut b = Builder {
        ast,
        declared_rets,
        scopes: Vec::new(),
        site: HashMap::new(),
        closure_site: HashMap::new(),
        fn_scope: HashMap::new(),
        unannotated: Vec::new(),
    };
    let root = b.new_scope(None);
    b.hoist_vars(root, &ast.stmts);
    b.declare(root, &ast.stmts);
    for s in &ast.stmts {
        if let Stmt::FnDecl { name, .. } = s {
            let fs = b.function(s, root);
            b.fn_scope.insert(name.as_str(), fs);
        } else {
            b.stmt(s, root);
        }
    }
    // a lifted arrow's captures are the bindings of the scope it
    // was written in
    for (name, at) in &b.closure_site {
        if let Some(&fs) = b.fn_scope.get(name.as_str()) {
            b.scopes[fs as usize].parent = Some(*at);
        }
    }
    b.infer_unannotated();
    Walked {
        scopes: b.scopes,
        site: b.site,
    }
}

struct Builder<'a> {
    ast: &'a Ast,
    declared_rets: &'a HashMap<String, String>,
    scopes: Vec<Scope>,
    site: HashMap<ExprId, u32>,
    closure_site: HashMap<String, u32>,
    fn_scope: HashMap<&'a str, u32>,
    /// The bindings declared without an annotation, each with its scope
    /// and initializer, inferred once every scope has its parent.
    unannotated: Vec<(u32, &'a str, ExprId)>,
}

impl<'a> Builder<'a> {
    fn new_scope(&mut self, parent: Option<u32>) -> u32 {
        self.scopes.push(Scope {
            parent,
            names: HashMap::new(),
        });
        (self.scopes.len() - 1) as u32
    }

    /// The last annotated declaration of a name in one scope wins, as
    /// in the flat table; an unannotated one only shadows.
    fn bind(&mut self, scope: u32, name: &str, ann: Option<String>) {
        let slot = self.scopes[scope as usize]
            .names
            .entry(name.to_string())
            .or_insert(None);
        if ann.is_some() {
            *slot = ann;
        }
    }

    fn function(&mut self, s: &'a Stmt, parent: u32) -> u32 {
        let Stmt::FnDecl { params, body, .. } = s else {
            unreachable!()
        };
        let fs = self.new_scope(Some(parent));
        for p in params {
            self.bind(fs, &p.name, p.type_ann.clone());
        }
        // a default is evaluated in the parameters' scope (§10.2.11)
        for d in params.iter().filter_map(|p| p.default) {
            self.expr(d, fs);
        }
        self.hoist_vars(fs, body);
        self.declare(fs, body);
        for st in body {
            self.stmt(st, fs);
        }
        fs
    }

    /// A list with a scope of its own, under `parent`.
    fn block(&mut self, list: &'a [Stmt], parent: u32) -> u32 {
        let bs = self.new_scope(Some(parent));
        self.declare(bs, list);
        for s in list {
            self.stmt(s, bs);
        }
        bs
    }

    fn stmt(&mut self, s: &'a Stmt, scope: u32) {
        // a loop head's own expressions read its bindings
        if !matches!(s, Stmt::For { .. }) {
            for e in stmt_exprs(s) {
                self.expr(e, scope);
            }
        }
        match s {
            Stmt::FnDecl { .. } => {
                self.function(s, scope);
            }
            Stmt::Block(list) => {
                self.block(list, scope);
            }
            Stmt::Multi(list) => {
                for m in list {
                    self.stmt(m, scope);
                }
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.block(std::slice::from_ref(then_branch), scope);
                if let Some(e) = else_branch {
                    self.block(std::slice::from_ref(e), scope);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
                self.block(std::slice::from_ref(body), scope);
            }
            Stmt::For { init, body, .. } => self.for_loop(s, init.as_deref(), body, scope),
            Stmt::ForOf {
                var_name,
                var_type_ann,
                body,
                ..
            } => self.for_of(var_name, var_type_ann.clone(), body, scope),
            Stmt::ForOfSplitIter { var_name, body, .. } => self.for_of(var_name, None, body, scope),
            Stmt::Try {
                body,
                catch_param,
                catch_type,
                catch_body,
                finally_body,
                ..
            } => {
                let catch = (catch_param.as_deref(), catch_type.clone(), &catch_body[..]);
                self.try_stmt(body, catch, finally_body.as_deref(), scope)
            }
            Stmt::Switch { cases, default, .. } => {
                let lists = cases.iter().map(|c| &c.body[..]).chain(default.as_deref());
                self.switch(lists.collect(), scope)
            }
            _ => {}
        }
    }

    fn expr(&mut self, e: ExprId, scope: u32) {
        let node = self.ast.get_expr(e);
        if matches!(node, Expr::Call { .. } | Expr::Assign { .. }) {
            self.site.insert(e, scope);
        }
        if matches!(node, Expr::Closure { .. } | Expr::Ident(_))
            && let Some(name) = lifted_closure_name(self.ast, e)
        {
            self.closure_site.entry(name).or_insert(scope);
        }
        for c in expr_children(self.ast, e) {
            self.expr(c, scope);
        }
    }
}
