// A helper the methods call enters them again through a method on
// the same receiver: a nested entry would read the two words from
// memory while the outer call holds newer values in registers, so
// neither word could move.

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

let nodes = 0

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
      left = left + this.term()
      nodes = nodes + 1
    }
    return left
  }
  term(): number {
    let left = this.factor()
    while (this.toks[this.pos].kind === STAR) {
      this.pos = this.pos + 1
      left = left * this.factor()
      nodes = nodes + 1
    }
    return left
  }
  factor(): number {
    const t = this.toks[this.pos]
    this.pos = this.pos + 1
    nodes = nodes + 1
    if (t.kind === LP && this.toks[this.pos].kind === LP) {
      this.pos = this.pos + 1
      const v = nested(0, this)
      this.pos = this.pos + 1
      return v
    }
    if (t.kind === NUM) return t.value
    const inner = this.expr()
    this.pos = this.pos + 1
    nodes = nodes - 1
    return inner
  }
}

function nested(n: number, p: Parser): number {
  return n * 0 + p.expr()
}

function lex(spec: number[]): Token[] {
  const out: Token[] = []
  for (let i = 0; i < spec.length; i = i + 2) {
    out.push(new Token(spec[i], spec[i + 1]))
  }
  return out
}

// 2 + 3 * (4 + 5) * 6 + 7
const spec = [NUM, 2, PLUS, 0, NUM, 3, STAR, 0, LP, 0, NUM, 4, PLUS, 0, NUM, 5, RP, 0, STAR, 0, NUM, 6, PLUS, 0, NUM, 7, END, 0]
const p = new Parser(lex(spec))
console.log(p.expr(), p.pos, nodes)
const q = new Parser(lex([NUM, 1, STAR, 0, LP, 0, LP, 0, NUM, 9, RP, 0, RP, 0, END, 0]))
console.log(q.expr(), q.pos, nodes)
console.log(p.pos, q.pos)
