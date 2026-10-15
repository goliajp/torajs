//! Which uses of an unclaimed function expression, other than a plain
//! call, `new` or `.call` / `.apply`, the desugar pipeline refuses for
//! handing its value to code that supplies a receiver of its own.
//!
//! Each refused use is checked against a control that puts `0` in the
//! same place and must pass the pipeline.

use torajs_core::{lexer, parser};

const PRELUDE: &str = "function keep(_: any) {}\nfunction take(x: any) { return x() }\n";

fn pipeline_ok(src: &str) -> bool {
    let src = format!("{src}\n{PRELUDE}");
    let tokens = lexer::tokenize(&src).expect("fixture lexes");
    let mut ast = parser::parse(&src, &tokens).expect("fixture parses");
    ast.source = src.clone();
    ast.warm_newline_cache();
    crate::ast_pipeline::run_ast_prelude(&mut ast).expect("prelude");
    crate::ast_pipeline::run_ast_desugar_pipeline(&mut ast).is_ok()
}

fn refused_at(body: &str, use_site: &str) {
    let control = body.replace("USE", "0");
    assert!(pipeline_ok(&control), "control refused:\n{control}");
    let refused = body.replace("USE", use_site);
    assert!(!pipeline_ok(&refused), "accepted:\n{refused}");
}

// `f.bind(this)` is accepted, and keeps `f` off every receiver
// promotion as soon as `USE` names `f` other than as a `.bind` receiver
const METHOD: &str = "class C { tag = 0; h: any; m() { const f: any = function () { return this }; keep(f.bind(this)); let s: any; return USE } }\nconsole.log(new C().m());";

#[test]
fn a_callee_spelled_through_another_expression_is_refused() {
    for site in [
        "f?.()",
        "(0, f)()",
        "(this.tag > 0 ? f : f)()",
        "(f || f)()",
        "(s = f)()",
    ] {
        refused_at(METHOD, site);
    }
}

// `f` called directly stays off the bind-receiver promotion
const CALLED: &str = "class C { tag = 0; m() { const f = function () { return this }; f(); return USE } }\nconsole.log(new C().m());";

#[test]
fn bind_next_to_another_use_is_refused() {
    refused_at(CALLED, "f.bind(5)()");
    refused_at(CALLED, "f.bind(undefined)");
    // every use a `.bind` receiver: `f` takes the bound receiver
    for site in ["f.bind(5)()", "f.bind(undefined)", "f.bind(this)()"] {
        let src = METHOD.replace("USE", site);
        assert!(pipeline_ok(&src), "refused:\n{src}");
    }
}

#[test]
fn a_value_handed_on_is_refused() {
    for site in [
        "f",
        "[f]",
        "({ k: f })",
        "take(f)",
        "(globalThis as any).k = f",
    ] {
        refused_at(METHOD, site);
    }
}

#[test]
fn a_write_into_an_untracked_binding_is_refused() {
    // the binding holds another value too
    let other = "class C { m() { const f: any = function () { return this }; keep(f.bind(this)); let g: any = 0; USE; return g() } }\nconsole.log(new C().m());";
    refused_at(other, "g = f");
    // declared twice
    let twice = "class C { m() { const f: any = function () { return this }; keep(f.bind(this)); var g: any = USE; var g: any = keep; return g() } }\nconsole.log(new C().m());";
    refused_at(twice, "f");
    // a parameter default, in an arrow and in a function
    let arrow = "class C { m() { const f: any = function () { return this }; keep(f.bind(this)); return ((x: any = USE) => x())() } }\nconsole.log(new C().m());";
    refused_at(arrow, "f");
    let inner = "class C { m() { const f: any = function () { return this }; keep(f.bind(this)); return (function (x: any = USE) { return x() })() } }\nconsole.log(new C().m());";
    refused_at(inner, "f");
}

#[test]
fn a_hoisted_var_in_a_nested_block_is_tracked() {
    let body = "class C { m() { if (1) { var v = function () { return this }; keep(v) } return USE } }\nconsole.log(new C().m());";
    refused_at(body, "v()");
    refused_at(body, "new (v as any)()");
}

#[test]
fn a_mixed_return_of_a_member_is_refused() {
    let body = "class Q { v = 1; m(c: boolean) { if (c) { return USE } return 5 } }\nconsole.log(new Q().m(false));";
    refused_at(body, "function () { return (this as any).v }");
}

#[test]
fn uses_that_cannot_invoke_the_value_are_accepted() {
    for site in [
        "typeof f",
        "f.name",
        "f === f",
        "!f",
        "(keep(f), 1)",
        "(this.h = f, this.h())",
        "(s = f, 1)",
    ] {
        let src = METHOD.replace("USE", site);
        assert!(pipeline_ok(&src), "refused:\n{src}");
    }
}

#[test]
fn a_store_on_the_constructed_receiver_is_accepted() {
    let src = "function It(this: any) { this.v = 4; this.get = function () { return this.v }; this[Symbol.iterator] = function () { return this }; keep(this.get) }\nconst it = new (It as any)();\nconsole.log(it.get(), it[Symbol.iterator]() === it);";
    assert!(pipeline_ok(src), "refused:\n{src}");
}
