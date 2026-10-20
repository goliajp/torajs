// A tree class whose nodes a WeakRef observes: a node dying through the
// release fn must tell the WeakRef registry, so the release keeps its
// WeakRef test. Only live targets are read back (when a dead one is
// cleared is up to the engine).
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
let seen = 0
const refs: WeakRef<Expr>[] = []
for (let i = 0; i < 2000; i++) {
  const t = build(6)
  const w = new WeakRef(t.left!)
  const d = w.deref()
  seen += (d === undefined ? 0 : count(d)) + count(t)
  if (i % 100 === 0) refs.push(w)
}
console.log(seen, refs.length)
