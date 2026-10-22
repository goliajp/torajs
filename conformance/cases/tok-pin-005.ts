// A parsing method reaches its token array through `any` and pushes
// onto it; after the array is frozen, that push throws a TypeError.
// Both change or guard the array outside the typed path.
// TORAJS_PIN_STATS=1 prints
//   [pin] 0 pins: no field could be pinned

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
    if (t.value === 2) {
      for (let i = 0; i < 30; i = i + 1) (this as any).toks.push(new Token(NUM, 100 + i))
    }
    return new Expr(NUM, t.value, null, null)
  }
  seal(): void {
    Object.freeze(this.toks)
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
sequence(6, 0, toks)
const p = new Parser(toks)
console.log(evaluate(p.expr()), p.pos, p.toks.length)
p.seal()
try {
  p.toks.push(new Token(NUM, 1))
  console.log('pushed', p.toks.length)
} catch (e) {
  console.log(e instanceof TypeError, p.toks.length)
}
p.pos = 0
try {
  console.log(evaluate(p.expr()))
} catch (e) {
  console.log(e instanceof TypeError, p.pos, Object.isFrozen(p.toks))
}
