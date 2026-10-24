use super::super::{SlotKey, WidthTable, analyze};
use crate::{lexer, parser};
use std::collections::HashMap;

fn table(src: &str) -> WidthTable {
    let tokens = lexer::tokenize(src).expect("lex");
    let mut ast = parser::parse(src, &tokens).expect("parse");
    crate::ast::lift_arrow_fns(&mut ast);
    analyze(
        &ast,
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
    )
}

fn ret(f: &str) -> SlotKey {
    SlotKey::Ret(f.into())
}

#[test]
fn captured_param_reaches_closure_ret() {
    let t = table(
        "function f(xs: number[], delta: number): number {\n  const add = (x: number): number => x + delta;\n  return add(xs[0]);\n}\nconsole.log(f([1], 0.5));",
    );
    assert!(t.slot_is_f64(&ret("__closure_0")));
}

#[test]
fn capture_resolves_past_same_named_toplevel() {
    // the top-level `offset` stays integral; the closure reads the
    // parameter, which a call site fed 0.5
    let t = table(
        "const offset = 3;\nfunction f(offset: number): number {\n  const add = (x: number): number => x + offset;\n  return add(1);\n}\nconsole.log(f(0.5) + offset);",
    );
    assert!(t.slot_is_f64(&ret("__closure_0")));
    assert!(!t.slot_is_f64(&SlotKey::Global("offset".into())));
}

#[test]
fn same_name_captures_keep_their_own_bindings() {
    let t = table(
        "function a(k: number): number {\n  const g = (): number => k + 1;\n  return g();\n}\nfunction b(k: number): number {\n  const h = (): number => k + 1;\n  return h();\n}\nconsole.log(a(2), b(0.5));",
    );
    assert!(!t.slot_is_f64(&ret("__closure_0")));
    assert!(t.slot_is_f64(&ret("__closure_1")));
}

#[test]
fn closure_write_reaches_the_captured_let() {
    let t = table(
        "function f(n: number): number {\n  let acc = 0;\n  const bump = (): void => { acc = acc + 0.5; };\n  bump();\n  return acc + n;\n}\nconsole.log(f(1));",
    );
    assert!(t.slot_is_f64(&SlotKey::Local("f".into(), "acc".into())));
    assert!(t.slot_is_f64(&ret("f")));
}

#[test]
fn sibling_block_closures_share_their_slots() {
    // both consts live under one `Local(f, cb)` key, so a read of
    // either may hand over either closure
    let t = table(
        "function f(xs: number[]): void {\n  {\n    const cb = (v: number): number => v + 0.5;\n    console.log(xs.map(cb));\n  }\n  {\n    const cb = (v: number): number => v + 1;\n    console.log(xs.map(cb));\n  }\n}\nf([1]);",
    );
    assert!(t.slot_is_f64(&ret("__closure_0")));
    assert!(t.slot_is_f64(&ret("__closure_1")));
}

#[test]
fn nested_closure_resolves_through_its_creator() {
    let t = table(
        "function f(d: number): number {\n  const outer = (): number => {\n    const inner = (x: number): number => x + d;\n    return inner(1);\n  };\n  return outer();\n}\nconsole.log(f(0.5));",
    );
    let lifted: Vec<SlotKey> = (0..2).map(|i| ret(&format!("__closure_{i}"))).collect();
    assert!(lifted.iter().all(|k| t.slot_is_f64(k)));
}
