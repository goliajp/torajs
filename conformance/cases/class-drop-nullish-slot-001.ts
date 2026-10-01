// The nullable child slots of a tree node hold null, an optional
// field left unset, or a live node; the release must skip the first
// two and walk the third. The nodes churn through typed locals,
// typed fields and a typed function return so every site goes
// through the one release fn.
class Expr {
  kind: number
  value: number
  left: Expr | null
  right: Expr | null
  note?: Expr
  constructor(kind: number, value: number, left: Expr | null, right: Expr | null) {
    this.kind = kind
    this.value = value
    this.left = left
    this.right = right
  }
}
function leaf(v: number): Expr {
  return new Expr(0, v, null, null)
}
function pair(a: Expr | null, b: Expr | null): Expr {
  return new Expr(1, 0, a, b)
}
function evalExpr(e: Expr | null): number {
  if (e === null) return 0
  if (e.kind === 0) return e.value
  return evalExpr(e.left) + evalExpr(e.right)
}
let acc = 0
for (let i = 0; i < 5000; i++) {
  const l = i % 2 === 0 ? leaf(i) : null
  const r = i % 3 === 0 ? leaf(i * 2) : null
  const p = pair(l, r)
  if (i % 5 === 0) p.note = leaf(1)
  acc += evalExpr(p) + (p.note === undefined ? 0 : p.note.value)
}
console.log(acc)
let slot: Expr | null = leaf(7)
slot = null
console.log(slot === null)
const half = pair(leaf(3), null)
console.log(evalExpr(half), half.right === null, half.note === undefined)
