// ES §10.1.7 — class getter return-type inferred from body when the
// declaration omits the `: T` annotation. tora previously defaulted
// `get` with no annotation to Void and reported "return type mismatch"
// at typecheck. Body shapes covered: `return this.<field>` (field
// type-ann is the inferred ann) and bare literal returns.
class P {
  x: number = 42
  s: string = 'hi'
  b: boolean = true
  get xVal() { return this.x }
  get sVal() { return this.s }
  get bVal() { return this.b }
  get litN() { return 7 }
  get litS() { return 'lit' }
  get litB() { return false }
}
const p = new P()
console.log(p.xVal, typeof p.xVal)
console.log(p.sVal, typeof p.sVal)
console.log(p.bVal, typeof p.bVal)
console.log(p.litN, typeof p.litN)
console.log(p.litS, typeof p.litS)
console.log(p.litB, typeof p.litB)
