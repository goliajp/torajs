//! The catch-parameter type inference, read off the lowered SSA: a
//! catch that binds `any` boxes the pending throw
//! (`__torajs_anyv_box_from_pair`), a typed one takes its raw bits.

use torajs_core::ssa::{InstKind, Module};
use torajs_core::{check, lexer, parser, ssa_lower};

mod lowering;

fn lower(src: &str) -> Module {
    let tokens = lexer::tokenize(src).expect("fixture lexes");
    let mut ast = parser::parse(src, &tokens).expect("fixture parses");
    ast.source = src.to_string();
    ast.warm_newline_cache();
    crate::ast_pipeline::run_ast_prelude(&mut ast).expect("prelude");
    crate::ast_pipeline::run_ast_desugar_pipeline(&mut ast).expect("desugar");
    let artifacts = check::check_with_arity(&ast).expect("fixture checks");
    ssa_lower::lower_with_arity(&ast, &artifacts)
}

/// Whether `func` boxes a value into `any` from a (tag, value) pair.
fn boxes(m: &Module, func: &str) -> bool {
    let f = (m.funcs.iter())
        .find(|f| f.name == func)
        .expect("function lowered");
    f.blocks.iter().flat_map(|b| &b.insts).any(|i| {
        matches!(&i.kind, InstKind::Call(fid, _)
            if m.funcs[fid.0 as usize].name == "__torajs_anyv_box_from_pair")
    })
}

const CLASS: &str = r#"
class Fail {
  code: number
  constructor(code: number) {
    this.code = code
  }
}
"#;

#[test]
fn a_thrown_number_read_as_number_binds_a_number() {
    let src = r#"
function trial(i: number): number {
  try {
    throw i
  } catch (e) {
    return e as number
  }
}
console.log(trial(3))
"#;
    assert!(!boxes(&lower(src), "trial"));
}

#[test]
fn a_class_thrown_through_calls_binds_the_class() {
    let src = format!(
        "{CLASS}{}",
        r#"
function deep(d: number, v: number): number {
  if (d === 0) {
    if (v % 3 === 0) throw new Fail(v)
    return v
  }
  return deep(d - 1, v + 1) + 1
}
let total = 0
for (let v = 0; v < 4; v = v + 1) {
  try {
    total = total + deep(4, v)
  } catch (e) {
    total = total + (e as Fail).code
  }
}
console.log(total)
"#
    );
    assert!(!boxes(&lower(&src), "main"));
}

#[test]
fn two_thrown_types_keep_any() {
    let src = format!(
        "{CLASS}{}",
        r#"
function mixed(n: number): number {
  try {
    if (n === 1) throw new Fail(1)
    if (n === 2) throw 2
    return 0
  } catch (e) {
    return (e as Fail).code
  }
}
console.log(mixed(1))
"#
    );
    assert!(boxes(&lower(&src), "mixed"));
}

#[test]
fn a_runtime_raise_in_the_body_keeps_any() {
    let src = r#"
function implicit(o: any): number {
  try {
    if (o === 0) throw 7
    return o.x.y
  } catch (e) {
    return e as number
  }
}
console.log(implicit(0))
"#;
    assert!(boxes(&lower(src), "implicit"));
}

#[test]
fn a_read_other_than_the_cast_keeps_any() {
    let src = format!(
        "{CLASS}{}",
        r#"
function reads(n: number): string {
  try {
    if (n > 0) throw new Fail(n)
    return ""
  } catch (e) {
    return "" + e.message + (e as Fail).code
  }
}
console.log(reads(1))
"#
    );
    assert!(boxes(&lower(&src), "reads"));
}

#[test]
fn a_cast_to_another_type_keeps_any() {
    let src = r#"
function other(i: number): string {
  try {
    throw i
  } catch (e) {
    return e as string
  }
}
console.log(other(1))
"#;
    assert!(boxes(&lower(src), "other"));
}

/// Whether every function that takes a pending throw boxes it, for a
/// catch outside a named function.
fn every_take_boxes(m: &Module) -> bool {
    let calls = |f: &torajs_core::ssa::Function, name: &str| {
        f.blocks.iter().flat_map(|b| &b.insts).any(
            |i| matches!(&i.kind, InstKind::Call(fid, _) if m.funcs[fid.0 as usize].name == name),
        )
    };
    let takers: Vec<_> = (m.funcs.iter())
        .filter(|f| calls(f, "__torajs_throw_take"))
        .collect();
    !takers.is_empty()
        && takers
            .iter()
            .all(|f| calls(f, "__torajs_anyv_box_from_pair"))
}

#[test]
fn a_builtin_constructor_in_the_body_keeps_any() {
    for ctor in [
        "new Array(n)",
        "new RegExp(s)",
        "new Iterator()",
        "new Set(xs)",
    ] {
        let src = format!(
            r#"
function built(n: number, s: string, xs: number[]): number {{
  try {{
    if (n > 9) throw n
    const v = {ctor}
    return 0
  }} catch (e) {{
    return e as number
  }}
}}
console.log(built(1, "a", [1]))
"#
        );
        assert!(boxes(&lower(&src), "built"), "{ctor}");
    }
}

#[test]
fn a_builtin_constructor_in_a_callee_keeps_any() {
    let src = r#"
function never(): void {
  new Iterator()
}
function caller(n: number): number {
  try {
    if (n > 9) throw n
    never()
    return 0
  } catch (e) {
    return e as number
  }
}
console.log(caller(1))
"#;
    assert!(boxes(&lower(src), "caller"));
}

#[test]
fn a_dynamic_new_keeps_any() {
    let src = r#"
function dynamic(n: number, c: any): number {
  try {
    if (n > 9) throw n
    const v = new c()
    return 0
  } catch (e) {
    return e as number
  }
}
console.log(dynamic(1, Object))
"#;
    assert!(boxes(&lower(src), "dynamic"));
}

#[test]
fn an_empty_collection_still_binds_a_number() {
    let src = r#"
function empty(n: number): number {
  try {
    if (n > 9) throw n
    const m = new Map<string, number>()
    return m.size
  } catch (e) {
    return e as number
  }
}
console.log(empty(1))
"#;
    assert!(!boxes(&lower(src), "empty"));
}

#[test]
fn a_top_level_await_in_the_body_keeps_any() {
    let src = r#"
function f(n: number): void {
  if (n > 0) throw n
}
const p: Promise<number> = new Promise((_res, rej) => rej("no"))
try {
  f(0)
  await p
} catch (e) {
  console.log(e as number)
}
"#;
    assert!(every_take_boxes(&lower(src)));
}

#[test]
fn a_top_level_throw_of_one_type_binds_it() {
    let src = r#"
function f(n: number): void {
  if (n > 0) throw n
}
try {
  f(1)
} catch (e) {
  console.log(e as number)
}
"#;
    assert!(!every_take_boxes(&lower(src)));
}
