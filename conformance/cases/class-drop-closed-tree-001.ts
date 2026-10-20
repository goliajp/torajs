// A tree class no other code can tell apart from what its own
// constructor built: no subclass, no wider literal, no instance that
// reaches `any`, a builtin or a WeakRef. Its release walks the nodes
// without the per-node tag, expando and WeakRef tests. Trees are
// built, shared, rebuilt and dropped many times over, so a node
// released twice or never shows as a crash or a wrong count.
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
class Holder {
  root: Expr | null = null
  spare: Expr | null = null
}
let seed = 7
function rnd(): number {
  seed = (seed * 48271) % 2147483647
  return seed
}
function build(depth: number): Expr {
  if (depth === 0 || rnd() % 5 === 0) return new Expr(0, rnd() % 100, null, null)
  const l = build(depth - 1)
  const r = rnd() % 3 === 0 ? null : build(depth - 1)
  return new Expr(1 + (rnd() % 3), 0, l, r)
}
function evaluate(e: Expr): number {
  if (e.kind === 0) return e.value
  const a = evaluate(e.left!)
  const b = e.right === null ? 1 : evaluate(e.right)
  if (e.kind === 1) return (a + b) | 0
  if (e.kind === 2) return (a - b) | 0
  return Math.imul(a, b) % 100003
}
function count(e: Expr | null): number {
  return e === null ? 0 : 1 + count(e.left) + count(e.right)
}
let total = 0
let nodes = 0
const h = new Holder()
for (let i = 0; i < 3000; i++) {
  const t = build(8)
  total = (total + evaluate(t)) | 0
  nodes += count(t)
  if (i % 7 === 0) h.spare = t
  if (i % 11 === 0) h.root = new Expr(1, 0, t, h.spare)
}
console.log(total, nodes, count(h.root), count(h.spare))
h.root = null
console.log(count(h.root), evaluate(h.spare!))
let chain: Expr | null = null
for (let i = 0; i < 100000; i++) chain = new Expr(1, i, chain, null)
let len = 0
for (let c: Expr | null = chain; c !== null; c = c.left) len++
console.log(len, chain!.value)
chain = null
console.log(chain === null)
