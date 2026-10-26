//! A binding declared with a class or a function type that holds an
//! `any` value keeps its declared type for the TypeScript rules: what
//! tsc refuses through the declared type is refused, as through a
//! binding that holds an instance, and what it accepts still compiles.

fn check_src(src: &str) -> Result<(), String> {
    let tokens = crate::lexer::tokenize(src).map_err(|e| format!("lex: {e}"))?;
    let mut ast = crate::parser::parse(src, &tokens).map_err(|e| format!("parse: {e}"))?;
    crate::ast::desugar_classes(&mut ast);
    crate::ast::lift_arrow_fns(&mut ast);
    crate::check::check(&ast).map(|_| ())
}

const CLASS: &str = "class C { n: number = 1; m(s: number): number { return s + this.n } \
     set s(v: number) { this.n = v } get g(): number { return this.n } }\n";

fn held_class(body: &str) -> String {
    format!("{CLASS}function f(v: any) {{ const q: C = v; {body} }}\nf(new C());\n")
}

fn held_fn(body: &str) -> String {
    format!(
        "function id(x: number): number {{ return x }}\n\
         function f(v: any) {{ const g: (x: number) => number = v; {body} }}\nf(id);\n"
    )
}

fn refused(src: &str, needle: &str) {
    let err = check_src(src).expect_err("program must be refused");
    assert!(err.contains(needle), "{err}");
}

fn accepted(src: &str) {
    assert!(check_src(src).is_ok(), "{:?}", check_src(src));
}

#[test]
fn a_held_class_binding_reads_its_declared_members() {
    refused(
        &held_class("const s: string = q.n"),
        "declared String, init has Number",
    );
    refused(
        &held_class("const s: string = q.g"),
        "declared String, init has Number",
    );
    refused(
        &held_class("const s: string = q.m(1)"),
        "declared String, init has Number",
    );
    refused(
        &held_class("const z = q.m(1); const s: string = z"),
        "init has Number",
    );
}

#[test]
fn a_held_class_binding_calls_its_declared_methods() {
    refused(
        &held_class("q.m(\"x\")"),
        "argument 0: expected Number, got String",
    );
    refused(&held_class("q.m()"), "expected 1 argument(s), got 0");
}

#[test]
fn a_held_class_binding_stores_at_its_declared_types() {
    refused(
        &held_class("q.n = \"s\""),
        "field is Number, value is String",
    );
    refused(
        &held_class("q.s = \"x\""),
        "setter expects Number, value is String",
    );
}

#[test]
fn a_held_function_binding_takes_its_declared_arguments() {
    refused(
        &held_fn("g(\"x\")"),
        "argument 0: expected Number, got String",
    );
    refused(&held_fn("g()"), "expected 1 argument(s), got 0");
    refused(&held_fn("const s: string = g(1)"), "init has Number");
}

#[test]
fn what_the_declared_type_admits_still_compiles() {
    // without the setter: this harness runs no pass that gives a
    // setter's body its `void` return
    let plain = |body: &str| held_class(body).replace(" set s(v: number) { this.n = v }", "");
    accepted(&plain("console.log(q.n + q.m(2) + q.g, typeof q.m)"));
    accepted(&plain("q.n = 3"));
    accepted(&held_fn("console.log(g(2), g.length)"));
}

#[test]
fn a_generic_signature_holds_no_argument_to_one_type() {
    accepted(
        "class G { id<T>(x: T): T { return x } \
         size<T>(xs: T[] | undefined): number { return xs ? xs.length : 0 } }\n\
         function f(v: any) { const q: G = v; console.log(q.id(\"x\"), q.id(1), q.size([1])) }\n\
         f(new G());\n",
    );
}
