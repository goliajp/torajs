// A tree class slot holding an object literal of the class's shape:
// the literal is no instance the class's constructor made, so the
// release keeps its tag test and hands it to the runtime's walk.
class Expr {
  kind: number
  value: number
  left: Expr | null
  right: Expr | null
  constructor(kind: number, value: number, left: Expr | null, right: Expr | null) {
    this.kind = kind
    this.value = value
    this.left = left
    this.right = right
  }
}
function count(e: Expr | null): number {
  return e === null ? 0 : 1 + count(e.left) + count(e.right)
}
function build(n: number): Expr {
  if (n === 0) return new Expr(0, 1, null, null)
  return new Expr(1, n, build(n - 1), n % 2 === 0 ? build(n - 2 < 0 ? 0 : n - 2) : null)
}
let total = 0
for (let i = 0; i < 2000; i++) {
  const lit: Expr = { kind: 0, value: i, left: null, right: build(2) }
  const t = new Expr(1, 0, build(5), lit)
  total += count(t) + t.right!.value
}
console.log(total)
