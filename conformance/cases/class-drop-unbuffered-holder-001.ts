// A second class holds nodes of a tree class, and itself forms rings:
// the tree's nodes are no longer held only by tree nodes, so their
// release keeps the cycle buffer and the flag test.
class Expr {
  value: number
  left: Expr | null
  constructor(value: number, left: Expr | null) {
    this.value = value
    this.left = left
  }
}
class Box {
  e: Expr | null
  other: Box | null = null
  constructor(e: Expr | null) {
    this.e = e
  }
}
let sum = 0
let keep: Box | null = null
for (let i = 0; i < 20000; i++) {
  const e = new Expr(i, new Expr(i + 1, null))
  const a = new Box(e)
  const b = new Box(e.left)
  a.other = b
  b.other = a
  sum = (sum + a.other.other!.e!.value + b.e!.value) | 0
  if (i % 997 === 0) keep = a
}
console.log(sum, keep!.e!.value, keep!.other!.e!.value)
keep = null
console.log(keep === null)
