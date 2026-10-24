// Code outside the parser's methods reads and writes the cursor between
// two parses on the same receiver: each parse starts from what memory
// holds, and memory holds what the previous parse left.

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
      left = left + this.term()
    }
    return left
  }
  term(): number {
    let left = this.factor()
    while (this.toks[this.pos].kind === STAR) {
      this.pos = this.pos + 1
      left = left * this.factor()
    }
    return left
  }
  factor(): number {
    const t = this.toks[this.pos]
    this.pos = this.pos + 1
    if (t.kind === NUM) return t.value
    const inner = this.expr()
    this.pos = this.pos + 1
    return inner
  }
}

function lex(spec: number[]): Token[] {
  const out: Token[] = []
  for (let i = 0; i < spec.length; i = i + 2) {
    out.push(new Token(spec[i], spec[i + 1]))
  }
  return out
}

// three expressions in a row, each ended by END
const p = new Parser(lex([NUM, 2, STAR, 0, NUM, 3, END, 0, LP, 0, NUM, 1, PLUS, 0, NUM, 4, RP, 0, END, 0, NUM, 9, END, 0]))
let total = 0
for (let i = 0; i < 3; i = i + 1) {
  const v = p.expr()
  console.log("parse", i, v, p.pos)
  total = total + v
  p.pos = p.pos + 1
}
console.log("total", total, p.pos)
p.pos = 0
console.log("again", p.expr(), p.pos)
p.pos = 4
console.log("middle", p.expr(), p.pos)
