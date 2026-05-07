// T-15.g.6 (v0.5.0) — `let s: T = await heap_promise` pattern locks
// in the type-erasure caveat. The wired-but-not-active side channel
// (check_with_types + lower_with_types + LowerCtx.expr_types) is
// substrate that T-15.g.6.c will switch on once IntToPtr / narrow-
// bitcast InstKinds land — then `console.log(await heap_p)` direct
// form works without the let intermediate.
//
// Today this fixture verifies the let-typed pattern across primitive
// + heap T to lock the regression coverage in.

let p1 = Promise.resolve('hello')
let s: string = await p1
console.log(s)                                          // hello

let p2 = Promise.resolve(42)
let n: number = await p2
console.log(n)                                          // 42

let p3 = Promise.resolve(true)
let b: boolean = await p3
console.log(b)                                          // true

let xs = [10, 20, 30]
let arr_p = Promise.resolve(xs)
let r: number[] = await arr_p
console.log(r.length)                                   // 3
console.log(r[1])                                       // 20

async function getName(): string {
  return 'torajs'
}
let g: string = await getName()
console.log(g)                                          // torajs
