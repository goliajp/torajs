// T-11 (v0.4.0) — `arguments[<dynamic-i>]` materialization. The
// existing literal-index rewrite (`arguments[2]` → `Ident(p2)`)
// stays as a zero-cost fast-path. Only when ssa_lower's pre-pass
// detects a non-literal index does it prepend a synthesized
// `let __torajs_arguments: any[] = [p0, p1, ...]` to the fn body
// and rewrite the dynamic accesses to read from it. The Array<Any>
// substrate from T-10 carries the mixed primitive + heap types.

function show(a: i64, b: string, c: boolean): void {
  for (let i: i64 = 0; i < 3; i = i + 1) {
    console.log(arguments[i])
  }
}
show(42, 'hi', true)

// Mixed types: ensures the synth Array<Any> codegen emits the
// matching tag dispatch per param type (i64 / string / bool).
function describe(name: string, n: i64, ok: boolean): void {
  for (let i: i64 = 0; i < 3; i = i + 1) {
    console.log(arguments[i])
  }
}
describe('alpha', 7, false)

// Two homogeneous-i64 params — verifies `is_assignable_to(Array<Any>,
// Array<I64>)` widening lets the synth init typecheck cleanly.
function homo(a: i64, b: i64, c: i64): void {
  for (let i: i64 = 0; i < 3; i = i + 1) {
    console.log(arguments[i])
  }
}
homo(100, 200, 300)
