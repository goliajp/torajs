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
    // `f.bind(this)` keeps `f` unclaimed; a plain call through an
    // alias of a claimed `f` is seeded and needs no refusal
    let ctor = "class B { constructor() {} }\nclass D extends B { r: any; constructor() { const f = function () { return this }; const g = f; const r = CALL; super(); keep(f.bind(this)); this.r = r } }\nconsole.log(new D().r);";
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

// a reassigned name is not followed to its uses, so writing an
// unclaimed function expression into it refuses rather than leaving a
// call through it unjudged
#[test]
fn a_write_into_a_reassigned_name_is_refused() {
    for src in [
        "class C { m() { const f = function () { return this }; keep(f.bind(this)); let g: any = f; g = () => 1; return g() } }\nconsole.log(new C().m());",
        // ran `f.call(5)` on the instance
        "class C { m() { let f: any = function () { return this }; const r = f.call(5); f = () => 1; return [r, f()] } }\nconsole.log(new C().m());",
    ] {
        assert!(!pipeline_ok(src), "accepted:\n{src}");
    }
    for src in [
        // `const` in sibling blocks: two bindings, each holding its
        // initializer alone, and not followed
        "class C { m() { { const f = function () { return this }; keep(f) } { const f = () => 1; return f() } } }\nconsole.log(new C().m());",
        // every use receiver-safe: the function expression is promoted
        // to a receiver of its own, and no write of it is left to judge
        "class C { m() { let f: any = function () { return this }; keep(f); f = () => 1; return f() } }\nconsole.log(new C().m());",
        "class C { m() { const f = function () { return this }; keep(f); let g: any = f; g = () => 1; return g() } }\nconsole.log(new C().m());",
    ] {
        assert!(pipeline_ok(src), "refused:\n{src}");
    }
}

// one of two `const g` in sibling blocks holds the function
// expression: that block's uses are judged (they ran `f` with the
// method's receiver)
#[test]
fn a_call_through_a_sibling_block_binding_is_refused() {
    let body = "class C { m() { const f = function () { return this }; { const g = f; keep(CALL) } { const g = 1; keep(g) } return 0 } }\nconsole.log(new C().m());";
    for call in ["g()", "g.call(undefined)", "(() => g())()"] {
        refused_at(body, call);
    }
    // an object-literal method: a control without the call is refused
    // too, so only the refusal is pinned (this ran `f` on `o`)
    let objlit = "const o = { m() { const f = function () { return this }; { const g = f; keep(g.call(undefined)) } { const g = 2; keep(g) } return 0 } };\nconsole.log(o.m());";
    assert!(!pipeline_ok(objlit), "accepted:\n{objlit}");
}

#[test]
fn a_call_through_the_other_sibling_block_binding_is_accepted() {
    let src = "class C { m() { const f = function () { return this }; keep(f.bind(this)); { const g = f; keep(g.call(this)) } { const g = function () { return 5 }; keep(g()) } return 0 } }\nconsole.log(new C().m());";
    assert!(pipeline_ok(src), "refused:\n{src}");
}

// a nested redeclaration hides the outer `g` only inside its own block:
// a call after that block still reaches the binding holding `f`
#[test]
fn a_call_past_a_nested_redeclaration_is_refused() {
    let body = "class C { m() { const f = function () { return this }; { const g = f; { const g = 1; keep(g) } keep(CALL) } { const g = 2; keep(g) } return 0 } }\nconsole.log(new C().m());";
    refused_at(body, "g()");
}

// an arrow minted in a sibling block captures that block's `g`
#[test]
fn an_arrow_is_judged_in_the_block_it_is_minted_in() {
    let other = "class C { m() { const f = function () { return this }; { const g = f; keep(g.call(this)) } { const g = function () { return 5 }; keep([1].map(() => g())) } return 0 } }\nconsole.log(new C().m());";
    assert!(pipeline_ok(other), "refused:\n{other}");
    let held = "class C { m() { const f = function () { return this }; { const g = f; keep([1].map(() => CALL)) } { const g = 2; keep(g) } return 0 } }\nconsole.log(new C().m());";
    refused_at(held, "g()");
}
