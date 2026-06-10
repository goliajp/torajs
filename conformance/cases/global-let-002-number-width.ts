// K.3b — `: number` annotations parse to the I64 slot default, so a
// genuinely-fractional initializer must widen the slot to F64.
// Before the fix, `let x: number = 1.5` stored f64 bits into an i64
// slot and every read printed a garbage integer (silent wrong).

let x: number = 1.5
function f(): number {
  return x * 2
}
console.log(f())

let y = 2.5
function g(): void {
  console.log(y + 0.25)
}
g()
