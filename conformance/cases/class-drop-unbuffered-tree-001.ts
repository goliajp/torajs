// A tree class whose nodes only ever point at nodes built before them:
// each field is written once, by the constructor, with a node that
// already existed. No cycle can pass through a node and nothing else
// holds one, so its release neither buffers a node whose count stays
// above zero nor tests a dying one for the cycle buffer's flag.
// Subtrees are shared by several parents (a release that leaves
// references), rebuilt and dropped many times over; a node released
// twice or never shows as a crash or a wrong count.
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
let seed = 11
function rnd(): number {
  seed = (seed * 48271) % 2147483647
  return seed
}
function build(depth: number, shared: Expr | null): Expr {
  if (depth === 0 || rnd() % 5 === 0) return new Expr(0, rnd() % 100, null, null)
  const l = build(depth - 1, shared)
  const r = rnd() % 4 === 0 ? shared : build(depth - 1, shared)
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
let keep: Expr | null = null
for (let i = 0; i < 3000; i++) {
  const shared = build(3, null)
  const t = build(8, shared)
  total = (total + evaluate(t)) | 0
  nodes += count(t)
  if (i % 13 === 0) keep = new Expr(1, 0, t, keep)
}
console.log(total, nodes, count(keep))
keep = null
let chain: Expr | null = null
for (let i = 0; i < 100000; i++) chain = new Expr(1, i, chain, i % 3 === 0 ? chain : null)
let len = 0
for (let c: Expr | null = chain; c !== null; c = c.left) len++
console.log(len, chain!.value, count(keep))
chain = null
console.log(chain === null)
