// A parsing method pushes onto the token array it reads, and the push
// moves the array's storage. The array's length and storage change
// while the methods run, so every read sees the array as it is then.
// TORAJS_PIN_STATS=1 prints
//   [pin] sid <n> Parser.toks: cluster {expr term factor}: none: a runtime call in __cm_Parser__factor

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
    // each odd number asks for more input, pushed past the separator
    // and read by nobody until the next parse
    if (t.value % 2 === 1 && this.toks.length < 400) {
      for (let i = 0; i < 20; i = i + 1) this.toks.push(new Token(NUM, t.value + i))
    }
    return new Expr(NUM, t.value, null, null)
  }
}

function sequence(n: number, base: number, out: Token[]): void {
  for (let i = 1; i <= n; i = i + 1) {
    if (i > 1) out.push(new Token(i % 3 === 0 ? STAR : i % 2 === 0 ? MINUS : PLUS, 0))
    out.push(new Token(NUM, base + i))
  }
  out.push(new Token(SEP, 0))
}

const toks: Token[] = []
sequence(12, 0, toks)
const p = new Parser(toks)
console.log(evaluate(p.expr()), p.pos, p.toks.length)
let sum = 0
for (let i = p.pos; i < p.toks.length; i = i + 1) sum = (sum + p.toks[i].value) | 0
console.log(sum)
