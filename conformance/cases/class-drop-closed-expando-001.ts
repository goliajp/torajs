// A tree class whose nodes get an expando property through `any`:
// the bag the runtime attaches must be released with the node, so the
// release keeps its expando test (compare class-drop-closed-tree-001).
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
let tagged = 0
for (let i = 0; i < 2000; i++) {
  const t = build(6)
  const a: any = t.left
  a.tag = [i, i + 1]
  a.name = 'n' + i
  tagged += a.tag.length + count(t)
}
console.log(tagged)
