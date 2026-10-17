//! A closure capturing a null-narrowed binding, checked through the
//! whole front end (async bodies and nested function declarations are
//! desugared before the checker sees them).
//!
//! A closure reads the binding when it is called, so it is typed at
//! the declared `T | null` unless its body needs the narrow; then the
//! narrow is kept only when no null store can run while the closure
//! exists, and a later one is refused.

use torajs_core::ast::Ast;
use torajs_core::{check, lexer, parser};

const PRELUDE: &str = "type Node = { v: number };\n\
     function mk(): Node { return { v: 7 } }\n\
     function g(n: Node): number { return n.v }\n";

fn desugared(src: &str) -> Ast {
    let tokens = lexer::tokenize(src).expect("fixture lexes");
    let mut ast = parser::parse(src, &tokens).expect("fixture parses");
    ast.source = src.to_string();
    ast.warm_newline_cache();
    crate::ast_pipeline::run_ast_prelude(&mut ast).expect("prelude");
    crate::ast_pipeline::run_ast_desugar_pipeline(&mut ast).expect("desugar");
    ast
}

fn checked(body: &str) -> Result<(), String> {
    let src = format!("{PRELUDE}{body}");
    check::check_with_arity(&desugared(&src)).map(|_| ())
}

fn accepted(body: &str) {
    let r = checked(body);
    assert!(r.is_ok(), "{r:?}\n{body}");
}

fn refused(body: &str) {
    let e = checked(body).expect_err(body);
    assert!(e.to_lowercase().contains("null"), "{e}\n{body}");
}

#[test]
fn a_body_that_checks_at_the_declared_type_leaves_later_stores_alone() {
    accepted(
        "function h(a: Node | null): number { let q: Node | null = a; if (q === null) q = mk();\n\
         const f = (): number => q !== null ? g(q) : 0; q = null; return f() }\n\
         console.log(h(null), h({ v: 3 }));\n",
    );
}

#[test]
fn a_null_store_in_the_body_that_needs_the_narrow_is_refused() {
    refused(
        "function h(a: Node): number { let q: Node | null = a;\n\
         const f = (): number => { const r = g(q); q = null; return r }; f(); return f() }\n\
         console.log(h(mk()));\n",
    );
}

#[test]
fn a_null_store_in_the_loop_around_the_closure_is_refused() {
    refused(
        "function h(xs: (Node | null)[]): number { let q: Node | null = null;\n\
         const fs: (() => number)[] = [];\n\
         for (const x of xs) { q = x; if (q === null) continue; fs.push((): number => g(q)) }\n\
         return fs[0]() }\n\
         console.log(h([{ v: 1 }, null]));\n",
    );
}

#[test]
fn a_member_narrow_does_not_reach_a_closure_called_after_a_null_store() {
    refused(
        "type O = { f: Node | null }; const o: O = { f: mk() };\n\
         let c = (): number => 0; if (o.f !== null) { c = (): number => g(o.f) }\n\
         o.f = null; console.log(c());\n",
    );
}

#[test]
fn a_non_null_store_keeps_the_narrow() {
    accepted(
        "function h(a: Node | null): number { let q: Node | null = a; if (q === null) q = mk();\n\
         const f = (): number => g(q); q = mk(); return f() }\n\
         console.log(h(null));\n",
    );
    accepted(
        "function h(a: Node | null): number { let q: Node | null = a; if (q === null) q = mk();\n\
         const f = (): number => g(q); return f() }\n\
         console.log(h(null));\n",
    );
}

#[test]
fn only_the_capture_that_needs_the_narrow_keeps_it() {
    accepted(
        "function h(a: Node | null, b: Node | null): number {\n\
         let q: Node | null = a; let r: Node | null = b;\n\
         if (q === null) q = mk(); if (r === null) r = mk();\n\
         const f = (): number => g(q) + (r === null ? 1 : 0); r = null; return f() }\n\
         console.log(h(null, null));\n",
    );
}

#[test]
fn a_null_store_after_an_await_is_refused() {
    refused(
        "async function h(a: Node | null): Promise<number> { let q: Node | null = a;\n\
         if (q === null) q = mk(); const f = (): number => g(q);\n\
         await Promise.resolve(0); q = null; return f() }\n\
         h(null).then((v) => console.log(v), () => console.log(\"err\"));\n",
    );
}

#[test]
fn a_nested_function_declaration_reads_the_narrow() {
    accepted(
        "function h(a: Node | null): number { let q: Node | null = a; if (q === null) q = mk();\n\
         return k(); function k(): number { return g(q) } }\n\
         console.log(h(null));\n",
    );
}
