//! A write through a base-typed receiver is held to the setter the base
//! declares, however a subclass redeclares the property.

use torajs_core::ast::Ast;
use torajs_core::{check, lexer, parser};

fn desugared(src: &str) -> Ast {
    let tokens = lexer::tokenize(src).expect("fixture lexes");
    let mut ast = parser::parse(src, &tokens).expect("fixture parses");
    ast.source = src.to_string();
    ast.warm_newline_cache();
    crate::ast_pipeline::run_ast_prelude(&mut ast).expect("prelude");
    crate::ast_pipeline::run_ast_desugar_pipeline(&mut ast).expect("desugar");
    ast
}

fn check(src: &str) -> Result<(), String> {
    check::check_with_arity(&desugared(src)).map(|_| ())
}

const BASE: &str = "class B {\n  _v: number = 0;\n  get v(): number { return this._v; }\n  set v(x: number) { this._v = x; }\n}\n";

fn program(sub: &str, write: &str) -> String {
    format!(
        "{BASE}class D extends B {{\n{sub}\n}}\nfunction put(b: B): void {{ b.v = {write}; }}\nput(new D());\n"
    )
}

#[test]
fn a_setter_redeclared_with_another_param_spelling_keeps_the_base_check() {
    let sub = "  get v(): number { return this._v; }\n  set v(x) { this._v = x; }";
    let e = check(&program(sub, "\"x\"")).expect_err("a string against `set v(x: number)`");
    assert!(e.contains("setter expects Number, value is String"), "{e}");
    check(&program(sub, "1")).expect("a number is what the base setter takes");
}

#[test]
fn a_getter_only_redeclaration_keeps_the_base_check() {
    let sub = "  get v(): number { return this._v + 1; }";
    let e = check(&program(sub, "\"x\"")).expect_err("a string against `set v(x: number)`");
    assert!(e.contains("setter expects Number, value is String"), "{e}");
    check(&program(sub, "1")).expect("a number is what the base setter takes");
}

#[test]
fn a_same_spelling_override_keeps_the_base_check() {
    let sub = "  get v(): number { return this._v; }\n  set v(x: number) { this._v = x * 2; }";
    let e = check(&program(sub, "\"x\"")).expect_err("a string against `set v(x: number)`");
    assert!(e.contains("setter expects Number, value is String"), "{e}");
}
