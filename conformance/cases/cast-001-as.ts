// V3-07 — `expr as T` TS type assertion. Three shapes:
//   1. Identity widening on a primitive (`3 as number`).
//   2. Cast of a class instance into a heterogeneous container
//      (the `arr.push(x as any)` cycle / Array<Any> pattern).
//   3. Narrow downcast that the typechecker accepts under the
//      bidirectional spec rule.
//
// Cast is identity at runtime — only the typechecker's view of
// the expression's static type changes. Lowered as a passthrough
// to the inner expression; any required Any-box happens at the
// surrounding assignment / push site, not in the cast itself.

let a = 3 as number
console.log(a)

class Cell {
  v: number;
  constructor(v: number) { this.v = v; }
}
let c = new Cell(7)

let xs: any[] = [c as any, 1, 'two']
console.log(xs.length)

let n = (a + 1) as number
console.log(n)
