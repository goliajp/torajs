// Deep recursion through the parser's methods, with many values live
// across each recursive call: a cluster that keeps its cursor in a
// register has one register fewer for them, and every one must come
// back intact at every depth.

class Token {
  kind: number
  value: number
  constructor(kind: number, value: number) {
    this.kind = kind
    this.value = value
  }
}

const NUM = 0
const PLUS = 1
const STAR = 2
const LP = 3
const RP = 4
const END = 5

class Parser {
  toks: Token[]
  pos: number
  constructor(toks: Token[]) {
    this.toks = toks
    this.pos = 0
  }
  expr(): number {
    let left = this.term()
    while (this.toks[this.pos].kind === PLUS) {
      this.pos = this.pos + 1
      left = (left + this.term()) % 1000003
    }
    return left
  }
  term(): number {
    let left = this.factor()
    while (this.toks[this.pos].kind === STAR) {
      this.pos = this.pos + 1
      left = (left * this.factor()) % 1000003
    }
    return left
  }
  factor(): number {
    const at = this.pos
    const t = this.toks[at]
    this.pos = this.pos + 1
    if (t.kind === NUM) return t.value
    const a = at * 3 + 1
    const b = at * 5 + 2
    const c = at * 7 + 3
    const d = at * 11 + 4
    const e = at * 13 + 5
    const f = at * 17 + 6
    const g = at * 19 + 7
    const h = at * 23 + 8
    const inner = this.expr()
    this.pos = this.pos + 1
    const mix = (a ^ b) + (c ^ d) + (e ^ f) + (g ^ h) - (a + b + c + d + e + f + g + h)
    return (inner + mix + this.pos) % 1000003
  }
}

function nested(depth: number): Token[] {
  const out: Token[] = []
  for (let i = 0; i < depth; i = i + 1) {
    out.push(new Token(LP, 0))
    out.push(new Token(NUM, i))
    out.push(new Token(i % 2 === 0 ? PLUS : STAR, 0))
  }
  out.push(new Token(NUM, 7))
  for (let i = 0; i < depth; i = i + 1) out.push(new Token(RP, 0))
  out.push(new Token(END, 0))
  return out
}

for (const depth of [1, 10, 200, 2000]) {
  const p = new Parser(nested(depth))
  const v = p.expr()
  console.log(depth, v, p.pos)
}
