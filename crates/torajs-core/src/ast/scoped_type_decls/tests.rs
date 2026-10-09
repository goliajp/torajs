use super::hoist_scoped_type_decls;
use crate::ast::{Ast, Expr, Stmt};

fn hoisted(src: &str) -> Ast {
    let toks = crate::lexer::tokenize(src).expect("lex");
    let mut ast = crate::parser::parse(src, &toks).expect("parse");
    hoist_scoped_type_decls(&mut ast);
    ast
}

/// Every `let` annotation in the program, by binding name, wherever it
/// sits (nested statement lists and arrow bodies included).
fn let_anns(ast: &Ast) -> Vec<(String, Option<String>)> {
    fn walk(stmts: &[Stmt], out: &mut Vec<(String, Option<String>)>) {
        for s in stmts {
            if let Stmt::LetDecl { name, type_ann, .. } = s {
                out.push((name.clone(), type_ann.clone()));
            }
            crate::ast::stmt_nested_lists::for_each_nested_list(s, &mut |inner| walk(inner, out));
        }
    }
    let mut out = Vec::new();
    walk(&ast.stmts, &mut out);
    for e in &ast.exprs {
        if let Expr::ArrowFn { body, .. } = e {
            walk(body, &mut out);
        }
    }
    out
}

fn ann_of(ast: &Ast, binding: &str) -> String {
    let_anns(ast)
        .into_iter()
        .find(|(n, _)| n == binding)
        .and_then(|(_, a)| a)
        .unwrap_or_else(|| panic!("no annotated binding {binding}"))
}

fn top_type_names(ast: &Ast) -> Vec<String> {
    ast.stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::TypeDecl { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_nested_declaration_shadows_the_outer_one_in_its_scope_only() {
    let ast = hoisted(
        "type K = string;
         function f(): number { type K = number; const x: K = 41; return x + 1; }
         const s: K = \"a\";
         { type K = boolean; const b: K = true; { const c: K[] = []; } }",
    );
    assert_eq!(ann_of(&ast, "s"), "K");
    assert_eq!(ann_of(&ast, "x"), "K$$scope1");
    assert_eq!(ann_of(&ast, "b"), "K$$scope2");
    assert_eq!(ann_of(&ast, "c"), "K$$scope2[]");
    assert_eq!(top_type_names(&ast), ["K", "K$$scope1", "K$$scope2"]);
}

#[test]
fn a_function_signature_sees_its_body_declarations() {
    let ast = hoisted(
        "function f(p: P): R { interface P { v: number } type R = P[]; const q: R = [p]; return q; }",
    );
    let Some(Stmt::FnDecl {
        params,
        return_type,
        ..
    }) = ast.stmts.first()
    else {
        panic!("fn first")
    };
    assert_eq!(params[0].type_ann.as_deref(), Some("P$$scope1"));
    assert_eq!(return_type.as_deref(), Some("R$$scope1"));
    assert_eq!(ann_of(&ast, "q"), "R$$scope1");
    let r = ast.stmts.iter().find_map(|s| match s {
        Stmt::TypeDecl { name, fields, .. } if name == "R$$scope1" => Some(fields[0].1.clone()),
        _ => None,
    });
    assert_eq!(r.as_deref(), Some("P$$scope1[]"));
}

#[test]
fn a_type_parameter_shadows_a_nested_declaration() {
    let ast = hoisted(
        "function f() { type T = number; function g<T>(x: T): T { const y: T = x; return y; } const z: T = 1; }",
    );
    assert_eq!(ann_of(&ast, "y"), "T");
    assert_eq!(ann_of(&ast, "z"), "T$$scope1");
}

#[test]
fn a_declaration_naming_an_enclosing_type_parameter_stays_put() {
    let ast = hoisted("function f<T>(x: T) { type Pair = T[]; const p: Pair = [x]; }");
    assert_eq!(ann_of(&ast, "p"), "Pair");
    assert_eq!(top_type_names(&ast), Vec::<String>::new());
}

#[test]
fn a_declaration_that_stays_put_still_shadows_an_outer_one() {
    let ast = hoisted(
        "function f() { type Pair = string; function g<T>(x: T) { type Pair = T[]; const p: Pair = [x]; } const q: Pair = \"\"; }",
    );
    assert_eq!(ann_of(&ast, "p"), "Pair");
    assert_eq!(ann_of(&ast, "q"), "Pair$$scope1");
}

#[test]
fn arrow_bodies_and_switch_clauses_are_scopes() {
    let ast = hoisted(
        "const h = (n: number) => { type N = string; const m: N = \"\" + n; return m; };
         switch (1) { case 1: type S = number; break; default: const t: S = 2; }",
    );
    assert_eq!(ann_of(&ast, "m"), "N$$scope1");
    assert_eq!(ann_of(&ast, "t"), "S$$scope1");
}
