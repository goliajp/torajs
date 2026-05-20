// P8.4 — expression-body arrow with Call-shaped return: inferred
// return type. Beyond the class-super-arrow fixture (which exercises
// the same code path via `__cm_<C>__<m>` synthesized FnDecls), this
// case covers the broader receive surface: regular user FnDecls.
//
// fn_sigs is built at desugar_implicit_generics entry from non-
// `__closure_*`, non-generic top-level FnDecls with an explicit
// return_type. Any bare-Ident-callee Call in an arrow body resolves
// its return type through fn_sigs, so the lifted closure FnDecl gets
// the right return ann and ssa_lower lowers it on the typed tier
// (avoiding Any-boxed fallback).

function num_id(n: number): number { return n }
function s_tag(s: string): string { return "[" + s + "]" }
function b_neg(b: boolean): boolean { return !b }

function main(): void {
  // 1) Number-returning fn — arrow infers number.
  const fn = () => num_id(42)
  console.log(fn())

  // 2) String-returning fn — arrow infers string.
  const fs = () => s_tag("hi")
  console.log(fs())

  // 3) Boolean-returning fn — arrow infers boolean.
  const fb = () => b_neg(true)
  console.log(fb())

  // 4) Block-body arrow with explicit `return` of a Call — same
  //    inference path through collect_return_anns_stmt.
  const fblk = () => { return num_id(7) }
  console.log(fblk())

  // 5) Arrow with parameter, parameter forwarded to a user fn.
  //    Verifies fn_sigs lookup works when the Call has args sourced
  //    from the arrow's own param scope, not just literals.
  const fparam = (x: number) => num_id(x * 2)
  console.log(fparam(5))
}

main()
