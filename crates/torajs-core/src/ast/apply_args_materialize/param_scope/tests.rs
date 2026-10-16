use super::split_param_scope;
use crate::ast::{Ast, Expr, Stmt};

fn run(src: &str) -> Ast {
    let toks = crate::lexer::tokenize(src).expect("lex");
    let mut ast = crate::parser::parse(src, &toks).expect("parse");
    split_param_scope(&mut ast);
    ast
}

fn fn_body<'a>(ast: &'a Ast, name: &str) -> &'a [Stmt] {
    ast.stmts
        .iter()
        .find_map(|s| match s {
            Stmt::FnDecl { name: n, body, .. } if n == name => Some(body.as_slice()),
            _ => None,
        })
        .expect("function")
}

fn let_names(body: &[Stmt]) -> Vec<String> {
    body.iter()
        .filter_map(|s| match s {
            Stmt::LetDecl { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect()
}

fn returned(ast: &Ast, body: &[Stmt]) -> String {
    let Some(Stmt::Return(Some(e))) = body.last() else {
        panic!("no return");
    };
    match ast.get_expr(*e) {
        Expr::Ident(n) => n.clone(),
        other => panic!("not an ident: {other:?}"),
    }
}

#[test]
fn a_body_binding_a_default_reads_takes_its_own_name() {
    let ast = run("function f(y = K) { let K = 2; return K; }");
    let body = fn_body(&ast, "f");
    assert_eq!(let_names(body), ["K__ps0"]);
    assert_eq!(returned(&ast, body), "K__ps0");
    assert_eq!(ast.renamed_binding_display["K__ps0"], "K");
}

#[test]
fn an_inner_block_keeps_its_own_binding() {
    let ast = run("function f(y = K) { let K = 1; { let K = 2; K; } return K; }");
    let body = fn_body(&ast, "f");
    let Stmt::Block(inner) = &body[1] else {
        panic!("block");
    };
    assert_eq!(let_names(inner), ["K"]);
    let Stmt::Expr(e) = &inner[1] else {
        panic!("expr");
    };
    assert!(matches!(ast.get_expr(*e), Expr::Ident(n) if n == "K"));
    assert_eq!(returned(&ast, body), "K__ps0");
}

#[test]
fn a_var_shadowing_a_captured_parameter_starts_as_its_value() {
    let ast = run("function f(a, g = () => a) { var a = 2; return a; }");
    let body = fn_body(&ast, "f");
    assert_eq!(let_names(body), ["a__ps0", "a__ps0"]);
    let Stmt::LetDecl { init, .. } = &body[0] else {
        panic!("seed");
    };
    assert!(matches!(ast.get_expr(*init), Expr::Ident(n) if n == "a"));
    assert_eq!(returned(&ast, body), "a__ps0");
}

#[test]
fn a_var_shadowing_a_parameter_without_a_closure_stays() {
    let ast = run("function f(a, b = a) { var a; return a; }");
    assert_eq!(let_names(fn_body(&ast, "f")), ["a"]);
}

#[test]
fn a_body_no_default_reaches_is_untouched() {
    let ast =
        run("function f(y = 1) { let K = 2; return K; } function g() { let K = 3; return K; }");
    assert_eq!(let_names(fn_body(&ast, "f")), ["K"]);
    assert_eq!(let_names(fn_body(&ast, "g")), ["K"]);
}

fn ctor<'a>(ast: &'a Ast, class: &str) -> &'a crate::ast::ClassCtor {
    ast.stmts
        .iter()
        .find_map(|s| match s {
            Stmt::ClassDecl {
                name,
                ctor: Some(c),
                ..
            } if name == class => Some(c),
            _ => None,
        })
        .expect("constructor")
}

#[test]
fn a_parameter_a_field_initializer_names_moves_out_of_its_way() {
    let ast = run("const k = 1; class E { v = k; constructor(k) { this.w = k; } }");
    let c = ctor(&ast, "E");
    assert_eq!(c.params[0].name, "k__fi0");
    let names: Vec<&str> = c
        .body
        .iter()
        .filter_map(|s| match s {
            Stmt::Expr(e) => match ast.get_expr(*e) {
                Expr::Assign { value, .. } => match ast.get_expr(*value) {
                    Expr::Ident(n) => Some(n.as_str()),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(names, ["k", "k__fi0"]);
}

#[test]
fn a_constructor_with_no_name_in_common_is_untouched() {
    let ast = run("const a = 1; class N { x = a; y; constructor(b) { let c = b; this.y = c; } }");
    let c = ctor(&ast, "N");
    assert_eq!(c.params[0].name, "b");
    assert_eq!(let_names(&c.body), ["c"]);
}
