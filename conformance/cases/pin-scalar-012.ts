// A member walks the cursor through a local that holds the receiver
// on every path but is reassigned in the loop, while the loop test
// reads the cursor through `this`: both spellings name one field.

class Token {
  kind: number
  value: number
  constructor(kind: number, value: number) {
    this.kind = kind
    this.value = value
  }
}

const NUM = 0
const STAR = 1
const END = 2

class Parser {
  toks: Token[]
  pos: number
  constructor(toks: Token[]) {
    this.toks = toks
    this.pos = 0
  }
  product(): number {
    let left = this.factor()
    let guard = 0
    while (guard < 50 && this.toks[this.pos].kind === STAR) {
      this.skipStars()
      left = left * this.factor()
      guard = guard + 1
    }
    return left
  }
  skipStars(): void {
    let cur: Parser = this
    let guard = 0
    while (guard < 50 && this.toks[this.pos].kind === STAR) {
      cur.pos = cur.pos + 1
      if (guard > 100) cur = this
      guard = guard + 1
    }
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

function run(spec: number[]): void {
  const p = new Parser(lex(spec))
  console.log("value", p.product(), "pos", p.pos)
}

run([NUM, 2, STAR, 0, NUM, 3, STAR, 0, STAR, 0, NUM, 4, END, 0])
run([NUM, 5, END, 0])
run([NUM, 1, STAR, 0, STAR, 0, STAR, 0, NUM, 7, STAR, 0, NUM, 2, END, 0])
