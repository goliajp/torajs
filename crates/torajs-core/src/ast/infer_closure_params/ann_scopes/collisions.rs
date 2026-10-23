//! Whether a program needs the scoped receiver table at all: some name
//! is declared twice with different annotations, or once with and once
//! without (a shadow), anywhere the table or the scopes see a binding.

use super::AnnScopes;
use crate::ast::Ast;
use crate::ast::infer_closure_lets::{collect_let_anns, collect_let_init_anns};
use std::collections::HashMap;

/// Whether the flat table can answer every site: no name is declared
/// with two different annotations, or with one and without one.
/// `__this` is excluded — it names a different object in every
/// function and is answered per function before the table is read.
#[derive(Default)]
pub(in crate::ast) struct Collisions<'a> {
    conflict: bool,
    unannotated: Vec<&'a str>,
}

impl<'a> Collisions<'a> {
    /// `old` is what the flat table held for the name before `ann`.
    pub(in crate::ast) fn annotated(&mut self, old: Option<String>, ann: &str) {
        self.conflict |= old.is_some_and(|o| o != ann);
    }

    pub(in crate::ast) fn unannotated(&mut self, name: &'a str) {
        self.unannotated.push(name);
    }

    pub(in crate::ast) fn param(&mut self, name: &'a str, old: Option<String>, ann: &str) {
        if name != "__this" {
            self.annotated(old, ann);
        }
    }

    /// The module's own declarations, `m`: an unannotated one whose
    /// initializer gave it an annotation (`inferred`) is not a shadow.
    pub(in crate::ast) fn module(&mut self, m: Collisions<'a>, inferred: &HashMap<String, String>) {
        self.conflict |= m.conflict;
        let shadows = m
            .unannotated
            .into_iter()
            .filter(|n| !inferred.contains_key(*n));
        self.unannotated.extend(shadows);
    }

    pub(in crate::ast) fn any(&self, flat: &HashMap<String, String>) -> bool {
        self.conflict || self.unannotated.iter().any(|n| flat.contains_key(*n))
    }
}

/// The module's own bindings joined to the functions' `all_anns`, an
/// explicit annotation over one its initializer gives; then the table
/// as the sites read it.
pub(in crate::ast) fn module_and_scopes<'a>(
    ast: &'a Ast,
    mut all_anns: HashMap<String, String>,
    closure_hints: &mut HashMap<String, String>,
    declared_rets: &HashMap<String, String>,
    mut coll: Collisions<'a>,
) -> AnnScopes {
    let mut module = Collisions::default();
    collect_let_anns(ast, &ast.stmts, &mut all_anns, closure_hints, &mut module);
    let mut inferred: HashMap<String, String> = HashMap::new();
    collect_let_init_anns(ast, &ast.stmts, declared_rets, &mut inferred, &mut module);
    coll.module(module, &inferred);
    for (k, v) in inferred {
        let held = all_anns.entry(k).or_insert_with(|| v.clone());
        coll.annotated(Some(held.clone()), &v);
    }
    if coll.any(&all_anns) {
        AnnScopes::scoped(ast, all_anns, declared_rets)
    } else {
        AnnScopes::flat(all_anns)
    }
}
