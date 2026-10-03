//! The names the owned-elements census ([`super::let_owned_elems`])
//! connects into alias classes ([`super::let_owned_elems_alias`]):
//! every name the program binds, by the kind of binder.

use std::collections::{HashMap, HashSet};

use super::{Ast, Expr, ExprId, StaticInit, Stmt};

/// Every name the program binds, by the kind of binder.
pub(super) struct AliasNames {
    /// How many `let` / `const` / `var` declarations spell each name
    /// (an admitted container's element class counts as one).
    pub(super) let_counts: HashMap<String, usize>,
    /// Names some other binder declares.
    pub(super) foreign: HashSet<String>,
    /// Top-level bindings a named function body or a namespace
    /// object reads — candidates for promotion to a data global.
    pub(super) promoted: HashSet<String>,
    /// `(declared name, init)` of every `let` declaration, for the
    /// init-side edges.
    pub(super) decls: Vec<(String, ExprId)>,
    /// `(declared name, init)` of every declaration annotated as an
    /// array of owned-string arrays.
    pub(super) nest_shaped: Vec<(String, ExprId)>,
    /// Admitted containers ([`super::let_owned_elems_nest`]): the
    /// container's name → the name of its element class.
    pub(super) containers: HashMap<String, String>,
}

impl AliasNames {
    pub(super) fn aliasable(&self, n: &str) -> bool {
        self.let_counts.contains_key(n) && !self.foreign.contains(n)
    }
}

pub(super) fn collect_names(ast: &Ast) -> AliasNames {
    let mut names = AliasNames {
        let_counts: HashMap::new(),
        foreign: HashSet::new(),
        promoted: HashSet::new(),
        decls: Vec::new(),
        nest_shaped: Vec::new(),
        containers: HashMap::new(),
    };
    names_in_stmts(&ast.stmts, &mut names);
    for e in &ast.exprs {
        if let Expr::ArrowFn { params, body, .. } = e {
            names.foreign.extend(params.iter().map(|p| p.name.clone()));
            names_in_stmts(body, &mut names);
        }
    }
    let refs = crate::ast_refs::toplevel_binding_refs(ast);
    let live = crate::ast_refs::ns_live_exports(ast);
    for s in super::toplevel_stmts_flat(ast) {
        if let Stmt::LetDecl { name, .. } = s
            && (refs.named_fn_refs.contains(name) || live.contains(name))
        {
            names.promoted.insert(name.clone());
        }
    }
    super::let_owned_elems_nest::admit_containers(ast, &mut names);
    names
}

fn names_in_stmts(stmts: &[Stmt], names: &mut AliasNames) {
    for s in stmts {
        names_in_stmt(s, names);
    }
}

fn names_in_stmt(s: &Stmt, names: &mut AliasNames) {
    match s {
        Stmt::LetDecl {
            name,
            init,
            type_ann,
            ..
        } => {
            *names.let_counts.entry(name.clone()).or_insert(0) += 1;
            names.decls.push((name.clone(), *init));
            if super::let_owned_elems_nest::is_nest_ann(type_ann.as_deref()) {
                names.nest_shaped.push((name.clone(), *init));
            }
        }
        Stmt::UsingDecl { name, .. } | Stmt::YieldInto { var: name, .. } => {
            names.foreign.insert(name.clone());
        }
        Stmt::FnDecl {
            name, params, body, ..
        } => {
            names.foreign.insert(name.clone());
            names.foreign.extend(params.iter().map(|p| p.name.clone()));
            names_in_stmts(body, names);
        }
        Stmt::ClassDecl {
            name,
            ctor,
            methods,
            static_methods,
            static_init,
            ..
        } => {
            names.foreign.insert(name.clone());
            if let Some(c) = ctor {
                names
                    .foreign
                    .extend(c.params.iter().map(|p| p.name.clone()));
                names_in_stmts(&c.body, names);
            }
            for m in methods.iter().chain(static_methods) {
                names
                    .foreign
                    .extend(m.params.iter().map(|p| p.name.clone()));
                names_in_stmts(&m.body, names);
            }
            for si in static_init {
                if let StaticInit::Block(b) = si {
                    names_in_stmts(b, names);
                }
            }
        }
        Stmt::ForOf { var_name, body, .. } | Stmt::ForOfSplitIter { var_name, body, .. } => {
            names.foreign.insert(var_name.clone());
            names_in_stmt(body, names);
        }
        Stmt::Try {
            body,
            catch_param,
            catch_body,
            finally_body,
            ..
        } => {
            if let Some(p) = catch_param {
                names.foreign.insert(p.clone());
            }
            names_in_stmts(body, names);
            names_in_stmts(catch_body, names);
            if let Some(fb) = finally_body {
                names_in_stmts(fb, names);
            }
        }
        Stmt::ImportDecl {
            default,
            namespace,
            named,
            type_named,
            ..
        } => {
            names
                .foreign
                .extend(default.iter().chain(namespace).cloned());
            for (n, alias) in named.iter().chain(type_named) {
                names.foreign.insert(n.clone());
                names.foreign.extend(alias.clone());
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            names_in_stmt(then_branch, names);
            if let Some(eb) = else_branch {
                names_in_stmt(eb, names);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
            names_in_stmt(body, names)
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                names_in_stmt(i, names);
            }
            names_in_stmt(body, names);
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                names_in_stmts(&c.body, names);
            }
            if let Some(db) = default {
                names_in_stmts(db, names);
            }
        }
        Stmt::Block(inner) | Stmt::Multi(inner) => names_in_stmts(inner, names),
        Stmt::ExportDecl { inner, .. } => {
            if let Some(inner) = inner {
                names_in_stmt(inner, names);
            }
        }
        Stmt::Expr(_)
        | Stmt::Return(_)
        | Stmt::Yield(_)
        | Stmt::Throw(_)
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::TypeDecl { .. } => {}
    }
}
