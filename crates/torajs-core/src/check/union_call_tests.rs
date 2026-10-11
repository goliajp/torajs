//! Source-level checks of a call through a union of function types:
//! each argument must fit every member that reads it, and the call
//! answers the union of the members' returns.

fn check_src(src: &str) -> Result<(), String> {
    let tokens = crate::lexer::tokenize(src).map_err(|e| format!("lex: {e}"))?;
    let mut ast = crate::parser::parse(src, &tokens).map_err(|e| format!("parse: {e}"))?;
    crate::ast::lift_arrow_fns(&mut ast);
    crate::check::check(&ast).map(|_| ())
}

fn refused(src: &str, needle: &str) {
    let err = check_src(src).expect_err("program must be refused");
    assert!(err.contains(needle), "{err}");
}

fn accepted(src: &str) {
    assert!(check_src(src).is_ok(), "{:?}", check_src(src));
}

const FNS: &str = "function inc(n: number): number { return n + 1; }\n\
                   function label(n: number): string { return \"#\" + n; }\n\
                   function len(s: string): number { return s.length; }\n\
                   function loose(x: any): any { return x; }\n\
                   let i = 3;\n";

#[test]
fn an_argument_every_member_accepts_is_admitted() {
    accepted(&format!("{FNS}console.log((i > 1 ? inc : label)(i));\n"));
    accepted(&format!(
        "{FNS}const f = i > 1 ? inc : label;\nconsole.log(f(4));\n"
    ));
    accepted(&format!(
        "{FNS}let f = i > 1 ? inc : label;\nconsole.log(f(4));\n"
    ));
}

#[test]
fn an_argument_one_member_refuses_is_refused() {
    refused(
        &format!("{FNS}const f = i > 1 ? inc : len;\nf(\"x\");\n"),
        "not assignable to the parameter of every member",
    );
    refused(
        &format!("{FNS}(i > 1 ? inc : len)(4);\n"),
        "not assignable to the parameter of every member",
    );
    refused(
        &format!("{FNS}(i > 1 ? inc : label)(\"x\");\n"),
        "not assignable to the parameter of every member",
    );
}

#[test]
fn disjoint_parameters_refuse_even_an_any_argument() {
    refused(
        &format!("{FNS}const a: any = 1;\n(i > 1 ? inc : len)(a);\n"),
        "not assignable to the parameter of every member",
    );
    accepted(&format!(
        "{FNS}const a: any = 1;\n(i > 1 ? inc : label)(a);\n"
    ));
}

#[test]
fn an_any_parameter_takes_what_the_other_member_takes() {
    accepted(&format!("{FNS}(i > 1 ? inc : loose)(4);\n"));
    accepted(&format!("{FNS}(i > 1 ? len : loose)(\"x\");\n"));
}

#[test]
fn a_missing_argument_is_left_to_the_member() {
    // `twice` reads `m` as 2 when it is left out
    accepted(&format!(
        "{FNS}function twice(n: number, m: number = 2): number {{ return n * m; }}\n\
         (i > 1 ? twice : inc)(4);\n"
    ));
}

#[test]
fn the_result_is_the_union_of_the_returns() {
    // a number | string result is not a string
    refused(
        &format!("{FNS}const s: string = (i > 1 ? inc : label)(4);\n"),
        "type mismatch",
    );
    accepted(&format!(
        "{FNS}const s: string | number = (i > 1 ? inc : label)(4);\n"
    ));
}
