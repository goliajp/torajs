// The cursor field is read by a helper the methods call with their
// receiver: the helper reads memory, so the field has to stay in
// memory while the methods run (the counter still could move).

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
    if (t.kind === NUM) return peekAt(1, this)
    const inner = this.expr()
    this.pos = this.pos + 1
    nodes = nodes - 1
    return inner
  }
}

function peekAt(n: number, p: Parser): number {
  return p.toks[p.pos - n].value
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
