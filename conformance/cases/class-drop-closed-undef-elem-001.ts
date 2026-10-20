// A tree class whose child slot is handed a read past the end of an
// array of trees: the slot holds `undefined`, so the release keeps
// testing every cell it takes for the undefined sentinel.
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
function count(e: Expr | null | undefined): number {
  return e === null || e === undefined ? 0 : 1 + count(e.left) + count(e.right)
}
function build(n: number): Expr {
  if (n === 0) return new Expr(0, 1, null, null)
  return new Expr(1, n, build(n - 1), n % 2 === 0 ? build(n - 2 < 0 ? 0 : n - 2) : null)
}
const pool: Expr[] = [build(3), build(2)]
let total = 0
let holes = 0
let k = 0
for (let i = 0; i < 2000; i++) {
  const t = new Expr(1, i, build(4), pool[k])
  k = k === 4 ? 0 : k + 1
  if (t.right === undefined) holes++
  total += count(t)
}
console.log(total, holes)
