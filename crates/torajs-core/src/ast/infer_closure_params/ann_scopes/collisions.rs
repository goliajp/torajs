//! Whether a program needs the scoped receiver table at all: some name
//! is declared twice with different annotations, or once with and once
//! without (a shadow), anywhere the table or the scopes see a binding.

use super::AnnScopes;
use crate::ast::infer_closure_lets::{collect_let_anns, collect_let_init_anns};
use crate::ast::stmt_nested_lists::for_each_nested_list;
use crate::ast::{Ast, Stmt};
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

    /// The binding a statement makes in a scope of its own, apart from
    /// its lists' declarations: a for-of head, a catch parameter. Only
    /// the scopes answer one, so to the flat table it is a shadow.
    pub(in crate::ast) fn heads(&mut self, s: &'a Stmt) {
        match s {
            Stmt::ForOf { var_name, .. } | Stmt::ForOfSplitIter { var_name, .. } => {
                self.unannotated(var_name)
            }
            Stmt::Try {
                catch_param: Some(p),
                ..
            } => self.unannotated(p),
            _ => {}
        }
    }

    /// A nested function's parameters and bindings, which only the
    /// scopes answer.
    pub(in crate::ast) fn nested_fn(&mut self, s: &'a Stmt) {
        let Stmt::FnDecl { params, body, .. } = s else {
            return;
        };
        for p in params.iter().filter(|p| p.name != "__this") {
            self.unannotated(&p.name);
        }
        self.shadows(body);
    }

    fn shadows(&mut self, list: &'a [Stmt]) {
        for s in list {
            match s {
                Stmt::LetDecl { name, .. } => self.unannotated(name),
                Stmt::FnDecl { .. } => self.nested_fn(s),
                _ => {
                    self.heads(s);
                    for_each_nested_list(s, &mut |l| self.shadows(l));
                }
            }
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
    // a top-level function's params and body were walked by the caller;
    // only a function declared inside another is nested
    let top = ast
        .stmts
        .iter()
        .filter(|s| !matches!(s, Stmt::FnDecl { .. }));
    for s in top {
        let one = std::slice::from_ref(s);
        collect_let_anns(ast, one, &mut all_anns, closure_hints, &mut module);
    }
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

#[cfg(test)]
mod tests {
    use crate::ast::infer_closure_params_helpers::build_ann_table;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn scoped(src: &str) -> bool {
        let toks = tokenize(src).expect("lex");
        let mut ast = parse(src, &toks).expect("parse");
        crate::ast::resolve_type_decls(&mut ast).expect("type decls");
        crate::ast::desugar_capturing_nested_fns(&mut ast);
        crate::ast::desugar_nested_fns(&mut ast);
        crate::ast::lift_arrow_fns(&mut ast);
        !build_ann_table(&ast).0.scopes.is_empty()
    }

    #[test]
    fn distinct_names_keep_the_flat_table() {
        let src = "const top: number[] = [1, 2];\n\
                   function f(a: number[]): number { const b: string[] = []; return a.map((x) => x + 1).length + b.length; }\n\
                   function g(c: string[]): number { let d: number = 0; for (const e of c) { d += e.length; } return d; }\n\
                   function h(n) { return n; }\n\
                   console.log(f(top), g(['a']), h(3));\n";
        assert!(!scoped(src));
    }

    #[test]
    fn a_name_annotated_twice_differently_takes_the_scopes() {
        let src = "function f(): number { const fs: Array<() => number> = []; fs.push(() => 1); return fs[0](); }\n\
                   function g(): string { const fs: Array<() => string> = []; fs.push(() => 'a'); return fs[0](); }\n\
                   console.log(f(), g());\n";
        assert!(scoped(src));
    }

    #[test]
    fn an_unannotated_shadow_takes_the_scopes() {
        let src = "function f(a: number[]): number { return a.length; }\n\
                   function g(a) { return a; }\n\
                   console.log(f([1]), g(2));\n";
        assert!(scoped(src));
    }
}
