// A typed catch binding of `void`, `undefined`, `null` or a function
// type takes the thrown value as it is.
//
// `catch (e: T)` with a T other than `any` / `unknown` is no
// TypeScript (TS1196); tr takes the annotation as the slot's type. A
// `void` parameter binds as `any`, an `undefined` or `null` one admits
// that value (what it rejects is in -004), and a function-typed one
// takes any function object, whatever its declared parameters.

try {
  throw undefined
} catch (e: void) {
  console.log("void", e === undefined)
}
try {
  throw undefined
} catch (e: undefined) {
  console.log("undefined", e === undefined)
}
try {
  throw null
} catch (e: null) {
  console.log("null", e === null)
}
try {
  throw (a: number, b?: number) => a * (b ?? 3)
} catch (e: (a: number) => number) {
  console.log("fn", e(5))
}
let calls = 0
try {
  throw () => {
    calls = calls + 1
    return calls
  }
} catch (e: () => number) {
  e()
  console.log("fn twice", e())
}
