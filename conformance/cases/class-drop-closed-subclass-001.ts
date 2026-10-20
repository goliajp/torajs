// A tree class slot holding a subclass instance with a field of its
// own: the release keeps its tag test, so the subclass field (an array
// and another tree) is released too.
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
class Wide extends Expr {
  extra: number[]
  more: Expr | null
  constructor(v: number) {
    super(0, v, null, null)
    this.extra = [v, v, v]
    this.more = build(3)
  }
}
let total = 0
for (let i = 0; i < 2000; i++) {
  const w = new Wide(i)
  const t = new Expr(1, 0, build(5), w)
  total += count(t) + w.extra.length + count(w.more)
}
console.log(total)
