// A method keeps the cursor it read in a local and uses it after a call
// to another method that advances the cursor: the local still holds
// the value read before the call.

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
const END = 3

class Parser {
  toks: Token[]
  pos: number
  constructor(toks: Token[]) {
    this.toks = toks
    this.pos = 0
  }
  expr(): number {
    const start = this.pos
    let left = this.term()
    while (this.toks[this.pos].kind === PLUS) {
      this.pos = this.pos + 1
      left = left + this.term()
    }
    return left * 1000 + (this.pos - start) * 10 + start
  }
  term(): number {
    const start = this.pos
    let left = this.factor()
    while (this.toks[this.pos].kind === STAR) {
      this.pos = this.pos + 1
      left = left * this.factor()
    }
    return left + start
  }
  factor(): number {
    const t = this.toks[this.pos]
    this.pos = this.pos + 1
    return t.value
  }
}

function lex(spec: number[]): Token[] {
  const out: Token[] = []
  for (let i = 0; i < spec.length; i = i + 2) {
    out.push(new Token(spec[i], spec[i + 1]))
  }
  return out
}

const p = new Parser(lex([NUM, 2, PLUS, 0, NUM, 3, STAR, 0, NUM, 4, PLUS, 0, NUM, 5, END, 0]))
console.log(p.expr(), p.pos)
const q = new Parser(lex([NUM, 6, STAR, 0, NUM, 7, END, 0]))
console.log(q.expr(), q.pos)
