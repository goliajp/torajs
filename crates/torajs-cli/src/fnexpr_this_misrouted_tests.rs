//! Which call sites of an unclaimed function expression the desugar
//! pipeline refuses for handing it a receiver its body would ignore.
//!
//! Every refused program is paired with a control that differs only at
//! the call site and must pass the pipeline, so the refusal is pinned
//! to that site rather than to some other stage.

use torajs_core::{lexer, parser};

const KEEP: &str = "function keep(_: any) {}\n";

fn pipeline_ok(src: &str) -> bool {
    let src = format!("{src}\n{KEEP}");
    let tokens = lexer::tokenize(&src).expect("fixture lexes");
    let mut ast = parser::parse(&src, &tokens).expect("fixture parses");
    ast.source = src.clone();
    ast.warm_newline_cache();
    crate::ast_pipeline::run_ast_prelude(&mut ast).expect("prelude");
    crate::ast_pipeline::run_ast_desugar_pipeline(&mut ast).is_ok()
}

/// `body` holds `CALL`; the refused program puts `call` there, the
/// control puts `0`.
fn refused_at(body: &str, call: &str) {
    let control = body.replace("CALL", "0");
    assert!(pipeline_ok(&control), "control refused:\n{control}");
    let refused = body.replace("CALL", call);
    assert!(!pipeline_ok(&refused), "accepted:\n{refused}");
}

const METHOD: &str = "class C { tag = 0; m() { const f: any = function (this: any) { return this }; keep(f); const g = f; return CALL } }\nconsole.log(new C().m());";

#[test]
fn new_through_an_alias_in_a_method_is_refused() {
    refused_at(METHOD, "new g()");
}

#[test]
fn a_plain_call_through_an_alias_in_a_method_is_refused() {
    refused_at(METHOD, "g()");
}

#[test]
fn call_with_a_foreign_receiver_is_refused() {
    refused_at(METHOD, "g.call(5)");
    refused_at(METHOD, "g.apply(5, [])");
}

#[test]
fn call_without_a_receiver_in_a_method_is_refused() {
    refused_at(METHOD, "g.call()");
    refused_at(METHOD, "g.call(undefined)");
}

#[test]
fn an_arrow_calling_the_alias_answers_for_the_method() {
    refused_at(METHOD, "(() => g())()");
}

#[test]
fn a_function_expression_passing_its_own_this_is_refused() {
    let body = "class C { m() { const f: any = function () { return this }; keep(f); const g = f; const h = function () { return CALL }; keep(h); return h.call(1) } }\nconsole.log(new C().m());";
    refused_at(body, "g.call(this)");
}

#[test]
fn a_call_before_super_in_a_derived_constructor_is_refused() {
    let ctor = "class B { constructor() {} }\nclass D extends B { r: any; constructor() { const f = function () { return this }; keep(f); const g = f; const r = CALL; super(); this.r = r } }\nconsole.log(new D().r);";
    refused_at(ctor, "g()");
}

#[test]
fn the_enclosing_receiver_as_this_arg_is_accepted() {
    for call in ["g.call(this)", "g.apply(this, [])", "f.call(this)"] {
        let src = METHOD.replace("CALL", call);
        assert!(pipeline_ok(&src), "refused:\n{src}");
    }
}

#[test]
fn a_plain_call_in_a_plain_function_is_accepted() {
    for call in ["f()", "f.call(undefined)", "f.call()"] {
        let src = format!(
            "function outer() {{ const f = function () {{ return this === undefined }}; keep(f); return {call} }}\nconsole.log(outer());"
        );
        assert!(pipeline_ok(&src), "refused:\n{src}");
    }
}

#[test]
fn a_redeclared_or_reassigned_name_is_not_tracked() {
    for src in [
        "class C { m() { { const f = function () { return this }; keep(f) } { const f = () => 1; return f() } } }\nconsole.log(new C().m());",
        "class C { m() { let f: any = function () { return this }; keep(f); f = () => 1; return f() } }\nconsole.log(new C().m());",
        "class C { m() { const f = function () { return this }; keep(f); let g: any = f; g = () => 1; return g() } }\nconsole.log(new C().m());",
    ] {
        assert!(pipeline_ok(src), "refused:\n{src}");
    }
}
