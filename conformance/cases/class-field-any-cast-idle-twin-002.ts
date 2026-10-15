// No method value escapes, so the any twins never run and the casts in
// the constructor they would call copy nothing: the fields stay exact.
class Expr2 {
  kind: number
  value: number
  left: Expr2 | null
  constructor(kind: number, value: number, left: Expr2 | null) {
    this.kind = kind
    this.value = value
    this.left = left
  }
}

class Reader {
  vals: number[]
  pos: number
  constructor(vals: number[]) {
    this.vals = vals
    this.pos = 0
  }
  one(): Expr2 {
    const v = this.vals[this.pos]
    this.pos = this.pos + 1
    if (v < 0) return new Expr2(2, 0, this.one())
    return new Expr2(1, v, null)
  }
}

function evalE(e: Expr2): number {
  if (e.kind === 1) return e.value
  return 0 - evalE(e.left!)
}

const r = new Reader([-1, -1, 7, 4, -1, 9])
let total = 0
while (r.pos < r.vals.length) total = total + evalE(r.one())
console.log(total, r.pos)
const big = new Expr2(1, 2 ** 40, null)
console.log(big.value, big.kind === 1)
