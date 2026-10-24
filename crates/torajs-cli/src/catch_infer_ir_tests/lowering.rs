//! The inference against the lowering's own record: a raise the scan
//! does not know, in the body or anywhere a call reaches, keeps `any`.

use super::{CLASS, boxes, lower};

#[test]
fn a_raise_only_the_lowering_knows_in_a_callee_keeps_any() {
    let src = r#"
function checks(n: number, rhs: any): boolean {
  if (n > 5) throw n
  return n instanceof rhs
}
function viaCallee(n: number): number {
  try {
    checks(n, 5)
    return 0
  } catch (e) {
    return e as number
  }
}
console.log(viaCallee(1))
"#;
    assert!(boxes(&lower(src), "viaCallee"));
}

#[test]
fn a_raise_only_the_lowering_knows_in_the_body_keeps_any() {
    let src = r#"
function inBody(n: number, rhs: any): number {
  try {
    if (n > 5) throw n
    return n instanceof rhs ? 1 : 0
  } catch (e) {
    return e as number
  }
}
console.log(inBody(1, 5))
"#;
    assert!(boxes(&lower(src), "inBody"));
}

#[test]
fn a_raise_in_a_callee_the_may_throw_set_misses_keeps_any() {
    let src = r#"
function h(x: any): number {
  return +x
}
function f(n: number): void {
  if (n > 0) throw n
}
function run(): number {
  try {
    h(1n)
    f(0)
    return 0
  } catch (e) {
    return e as number
  }
}
console.log(run())
"#;
    assert!(boxes(&lower(src), "run"));
}

#[test]
fn a_call_through_a_vtable_slot_keeps_any() {
    let src = r#"
class K {
  m(x: any): number {
    return +x
  }
}
class L extends K {
  m(x: any): number {
    return 2
  }
}
function f(n: number): void {
  if (n > 0) throw n
}
function run(k: K): number {
  try {
    k.m(1n)
    f(0)
    return 0
  } catch (e) {
    return e as number
  }
}
console.log(run(process.argv.length > 9 ? new L() : new K()))
"#;
    assert!(boxes(&lower(src), "run"));
}

#[test]
fn a_call_of_a_quiet_function_still_binds_a_number() {
    let src = r#"
function add(a: number, b: number): number {
  return a + b
}
function f(n: number): void {
  if (n > 0) throw n
}
function run(n: number): number {
  try {
    f(add(n, 1))
    return 0
  } catch (e) {
    return e as number
  }
}
console.log(run(2))
"#;
    assert!(!boxes(&lower(src), "run"));
}

/// The constructor `deep` calls is lowered after the function holding
/// the `try`, so its checks are not on record when the binding is made.
#[test]
fn a_callee_lowered_after_the_try_keeps_any() {
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
function outer(v: number): number {
  try {
    return deep(4, v)
  } catch (e) {
    return (e as Fail).code
  }
}
console.log(outer(2))
"#
    );
    assert!(boxes(&lower(&src), "outer"));
}
