// A parser that computes the value as it parses: no class has a field
// that holds a cell of its own class, so no release fn tests anything.
// The token array is still asked about, and it holds still while the
// three methods run.
// TORAJS_PIN_STATS=1 prints
//   [pin] sid <n> Parser.toks: cluster {expr term factor} entries {expr} reads 3 direct

const NUM = 0
const PLUS = 2
const STAR = 4
const END = 7
class Token {
  kind: number
  value: number
  constructor(kind: number, value: number) {
    this.kind = kind
    this.value = value
  }
}
function tokenize(src: string): Token[] {
  const out: Token[] = []
  for (let i = 0; i < src.length; i = i + 1) {
    const c = src.charCodeAt(i)
    if (c >= 48 && c <= 57) out.push(new Token(NUM, c - 48))
    else if (c === 43) out.push(new Token(PLUS, 0))
    else if (c === 42) out.push(new Token(STAR, 0))
  }
  out.push(new Token(END, 0))
  return out
}
class Parser {
  toks: Token[]
  pos: number
  constructor(toks: Token[]) {
    this.toks = toks
    this.pos = 0
  }
  expr(): number {
    let v = this.term()
    while (this.toks[this.pos].kind === PLUS) {
      this.pos = this.pos + 1
      v = (v + this.term()) | 0
    }
    return v
  }
  term(): number {
    let v = this.factor()
    while (this.toks[this.pos].kind === STAR) {
      this.pos = this.pos + 1
      v = Math.imul(v, this.factor())
    }
    return v
  }
  factor(): number {
    const t = this.toks[this.pos]
    this.pos = this.pos + 1
    return t.value
  }
}
const lines = ['1+2*3', '4*5+6*7+8', '9']
let total = 0
for (let i = 0; i < lines.length; i = i + 1) {
  const p = new Parser(tokenize(lines[i]))
  const v = p.expr()
  console.log(lines[i], '=', v, 'at', p.pos)
  total = (total + v) | 0
}
console.log(total)
