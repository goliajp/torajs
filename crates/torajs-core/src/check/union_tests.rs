//! Source-level checks of the union rules: what a union slot refuses,
//! what a narrow admits, and what a loose binding still admits.

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

#[test]
fn a_value_outside_every_member_is_refused() {
    refused("let x: string | number = true;\n", "type mismatch");
    refused(
        "function f(v: string | number): string { return \"\" + v; }\nf(true);\n",
        "not assignable",
    );
}

#[test]
fn a_member_missing_on_one_member_is_refused() {
    refused(
        "function f(v: string | number): number { return v.length; }\n",
        "does not exist on every member",
    );
    accepted("function f(v: string | any[]): number { return v.length; }\n");
}

#[test]
fn typeof_narrows_each_branch() {
    accepted(
        "function f(v: string | number): string {\n\
         if (typeof v === \"string\") { return v.toUpperCase(); }\n\
         return v.toFixed(1);\n}\n",
    );
    refused(
        "function f(v: string | number): string {\n\
         if (typeof v === \"string\") { return v.toFixed(1); }\n\
         return \"\";\n}\n",
        "does not exist on every member",
    );
}

#[test]
fn an_unmodeled_guard_loosens_the_binding() {
    accepted(
        "function f(v: string | string[]): string {\n\
         if (Array.isArray(v)) return v.join(\"+\");\n\
         return v;\n}\n",
    );
    // a name on no member at all stays refused
    refused(
        "function f(v: string | number): number {\n\
         if (v === \"x\") return v.nope;\n\
         return 0;\n}\n",
        "does not exist on every member",
    );
}

#[test]
fn an_assignment_in_a_narrow_checks_the_declaration() {
    accepted(
        "function f(v: string | number): string {\n\
         if (typeof v === \"string\") { v = 5; }\n\
         return \"\" + v;\n}\n",
    );
    // a same-named binding elsewhere keeps its own declaration
    accepted(
        "function a(v: string | number): string {\n\
         if (typeof v === \"string\") { return v; }\n\
         return \"\";\n}\n\
         function b(v: string | number | boolean): string {\n\
         v = true;\n\
         return \"\" + v;\n}\n",
    );
}
