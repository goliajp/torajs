//! A generator body's declarations that read nothing around them, moved
//! to top level before the machine splits the body.
//!
//! This is the lift `nested_fns` performs for an ordinary body, done
//! early: by the time that pass runs, the body is spread over the
//! machine's states and a declaration written in the first one is out
//! of reach of the code after a `yield` (`function* m() { function f()
//! {} yield 0; yield f(); }` read `f` as an unknown identifier). The
//! lifted declaration keeps its source spelling for `.name`.

use std::collections::HashMap;

use crate::ast::desugar_with::walk::stmt_children;
use crate::ast::nested_fns_idents::{arrow_rebinds, rewrite_idents_in_body};
use crate::ast::{Ast, Stmt};

pub(super) struct Lifts {
    /// The program's top-level function names, read before any lift.
    pub(super) globals: Vec<String>,
    counter: u32,
    out: Vec<Stmt>,
}

impl Lifts {
    pub(super) fn new(ast: &Ast) -> Self {
        let globals = ast
            .stmts
            .iter()
            .filter_map(|s| match s {
                Stmt::FnDecl { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        Self {
            globals,
            counter: 0,
            out: Vec::new(),
        }
    }

    /// Take the declarations at `sites` out of `body` (each slot empties
    /// in place) and rename every reference to them, in `body` and in
    /// one another.
    pub(super) fn lift(&mut self, ast: &mut Ast, body: &mut [Stmt], sites: &[usize], parent: &str) {
        if sites.is_empty() {
            return;
        }
        let mut renames: HashMap<String, String> = HashMap::new();
        let mut lifted: Vec<Stmt> = Vec::new();
        for &i in sites {
            let mut decl = std::mem::replace(&mut body[i], Stmt::Block(Vec::new()));
            let Stmt::FnDecl { name, .. } = &mut decl else {
                unreachable!("a lift site is a declaration");
            };
            // the `gen` suffix keeps it apart from `nested_fns`' numbering
            let mangled = format!("__nested_{parent}_{name}_gen{}", self.counter);
            self.counter += 1;
            let shown = ast
                .renamed_binding_display
                .get(name.as_str())
                .cloned()
                .unwrap_or_else(|| name.clone());
            ast.nested_fn_names.insert(mangled.clone(), shown);
            renames.insert(std::mem::replace(name, mangled.clone()), mangled);
            lifted.push(decl);
        }
        rewrite_idents_in_body(ast, body, &renames, true);
        // the identifier walk stops at a declaration: the peers left in
        // the body (values to be, declarations kept) and the lifted ones
        // are entered here
        for s in body.iter_mut().chain(lifted.iter_mut()) {
            rename_in_decls(ast, s, &renames);
        }
        self.out.extend(lifted);
    }
}

/// Rename the references in every declaration body under `s`, at any
/// depth; a parameter or local of a body with a lifted name is that
/// body's own binding, and hides the lifted one from it.
fn rename_in_decls(ast: &mut Ast, s: &mut Stmt, renames: &HashMap<String, String>) {
    if let Stmt::FnDecl { params, body, .. } = s {
        let own: HashMap<String, String> = renames
            .iter()
            .filter(|(k, _)| !arrow_rebinds(params, body, k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if own.is_empty() {
            return;
        }
        rewrite_idents_in_body(ast, body, &own, true);
        for c in body.iter_mut() {
            rename_in_decls(ast, c, &own);
        }
        return;
    }
    for c in stmt_children(s) {
        rename_in_decls(ast, c, renames);
    }
}

impl Lifts {
    /// Append every lifted declaration to the program's top level.
    pub(super) fn finish(self, ast: &mut Ast) {
        ast.stmts.extend(self.out);
    }
}
