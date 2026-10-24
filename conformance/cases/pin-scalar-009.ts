// A method of the parser catches a throw of another one and goes on
// reading and advancing the cursor after the catch: the cursor seen
// after the catch is the one the thrower had advanced it to.

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
const BAD = 2
const END = 3

class Parser {
  toks: Token[]
  pos: number
  constructor(toks: Token[]) {
    this.toks = toks
    this.pos = 0
  }
  list(): number {
    let sum = 0
    let errors = 0
    while (this.toks[this.pos].kind !== END) {
      try {
        sum = sum + this.item()
      } catch (e) {
        errors = errors + 1
        this.pos = this.pos + 1
      }
    }
    return sum * 100 + errors
  }
  item(): number {
    const t = this.toks[this.pos]
    this.pos = this.pos + 1
    if (t.kind === BAD) throw t.value
    if (t.kind === PLUS) return this.item() + this.item()
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

const p = new Parser(lex([NUM, 1, PLUS, 0, NUM, 2, NUM, 3, BAD, 9, NUM, 4, PLUS, 0, BAD, 1, NUM, 5, NUM, 6, END, 0]))
console.log(p.list(), p.pos)
const q = new Parser(lex([BAD, 0, BAD, 0, NUM, 7, END, 0]))
console.log(q.list(), q.pos)
