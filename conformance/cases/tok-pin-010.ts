// A parser whose token array grows between two parses: a sibling
// method the parsing methods never call pushes enough tokens to move
// the array's storage. The parsing methods may still read the array's
// length and storage once per call, since nothing grows it while they
// run; the next parse reads the new storage on entry.
// TORAJS_PIN_STATS=1 prints
//   [pin] sid <n> Parser.toks: cluster {expr term factor} entries {expr} reads 6 direct

const NUM = 0
const PLUS = 2
const MINUS = 3
const STAR = 4
const LPAREN = 5
const RPAREN = 6
const SEP = 7

class Token {
  kind: number
  value: number
  constructor(kind: number, value: number) {
    this.kind = kind
    this.value = value
  }
}

class Expr {
  kind: number
  value: number
  left: Expr | null
  right: Expr | null
  constructor(kind: number, value: number, left: Expr | null, right: Expr | null) {
    this.kind = kind
    this.value = value
    this.left = left
    this.right = right
  }
}

function evaluate(e: Expr): number {
  if (e.kind === NUM) return e.value
  const a = evaluate(e.left!)
  const b = evaluate(e.right!)
  if (e.kind === PLUS) return (a + b) | 0
  if (e.kind === MINUS) return (a - b) | 0
  return Math.imul(a, b)
}

class Parser {
  toks: Token[]
  pos: number
  constructor(toks: Token[]) {
    this.toks = toks
    this.pos = 0
  }
  expr(): Expr {
    let left = this.term()
    while (this.toks[this.pos].kind === PLUS || this.toks[this.pos].kind === MINUS) {
      const kind = this.toks[this.pos].kind
      this.pos = this.pos + 1
      const right = this.term()
      left = new Expr(kind, 0, left, right)
    }
    return left
  }
  term(): Expr {
    let left = this.factor()
    while (this.toks[this.pos].kind === STAR) {
      this.pos = this.pos + 1
      const right = this.factor()
      left = new Expr(STAR, 0, left, right)
    }
    return left
  }
  factor(): Expr {
    const t = this.toks[this.pos]
    this.pos = this.pos + 1
    if (t.kind !== LPAREN) return new Expr(NUM, t.value, null, null)
    const inner = this.expr()
    if (this.toks[this.pos].kind === RPAREN) this.pos = this.pos + 1
    return inner
  }
  feed(n: number): void {
    for (let i = 1; i <= n; i = i + 1) {
      if (i > 1) this.toks.push(new Token(i % 3 === 0 ? STAR : i % 2 === 0 ? MINUS : PLUS, 0))
      if (i % 5 === 0) {
        this.toks.push(new Token(LPAREN, 0))
        this.toks.push(new Token(NUM, i))
        this.toks.push(new Token(MINUS, 0))
        this.toks.push(new Token(NUM, 1))
        this.toks.push(new Token(RPAREN, 0))
      } else {
        this.toks.push(new Token(NUM, i))
      }
    }
    this.toks.push(new Token(SEP, 0))
  }
}

const toks: Token[] = [
  new Token(LPAREN, 0),
  new Token(NUM, 4),
  new Token(PLUS, 0),
  new Token(NUM, 5),
  new Token(RPAREN, 0),
  new Token(STAR, 0),
  new Token(NUM, 3),
  new Token(SEP, 0),
]
const p = new Parser(toks)
console.log(evaluate(p.expr()), p.pos, p.toks.length)
for (let round = 1; round <= 4; round = round + 1) {
  p.pos = p.pos + 1
  p.feed(round * 40)
  console.log(evaluate(p.expr()), p.pos, p.toks.length)
}
