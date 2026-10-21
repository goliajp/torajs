// A parsing method throws on a token it does not expect; the caller
// catches, and parses the next line with a new parser over a new token
// array. Each parse reads its own array's length and storage once per
// method call, and a throw out of the methods leaves nothing behind.
// TORAJS_PIN_STATS=1 prints
//   [pin] sid <n> Parser.toks: none: no verdict

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
    if (t.kind === NUM) return new Expr(NUM, t.value, null, null)
    if (t.kind === LPAREN) {
      const inner = this.expr()
      if (this.toks[this.pos].kind !== RPAREN) throw new Error('expected ) at ' + this.pos)
      this.pos = this.pos + 1
      return inner
    }
    throw new Error('unexpected token ' + t.kind + ' at ' + (this.pos - 1))
  }
}

function tokenize(src: string): Token[] {
  const out: Token[] = []
  for (let i = 0; i < src.length; i = i + 1) {
    const c = src.charCodeAt(i)
    if (c >= 48 && c <= 57) out.push(new Token(NUM, c - 48))
    else if (c === 43) out.push(new Token(PLUS, 0))
    else if (c === 45) out.push(new Token(MINUS, 0))
    else if (c === 42) out.push(new Token(STAR, 0))
    else if (c === 40) out.push(new Token(LPAREN, 0))
    else if (c === 41) out.push(new Token(RPAREN, 0))
  }
  out.push(new Token(SEP, 0))
  return out
}

const lines = ['1+2*3', '(4+5)*(6-', '7*(8-9', '2*(3+4)-5', '+1', '9-8-7*(1+1)']
let ok = 0
for (let i = 0; i < lines.length; i = i + 1) {
  const p = new Parser(tokenize(lines[i]))
  try {
    const v = evaluate(p.expr())
    console.log(lines[i], '=', v, 'at', p.pos, 'of', p.toks.length)
    ok = ok + 1
  } catch (e) {
    console.log(lines[i], 'fails:', (e as Error).message)
  }
}
console.log('parsed', ok)
