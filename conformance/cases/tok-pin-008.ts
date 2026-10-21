// The same parser where `expr`, once the input is consumed, appends
// another end token: a method of the cluster grows the array, so the
// field is not pinned.
// TORAJS_PIN_STATS=1 prints
//   [pin] sid <n> Parser.toks: cluster {expr trim term factor}: none: a header store in __cm_Parser__trim

const NUM = 0
const IDENT = 1
const PLUS = 2
const MINUS = 3
const STAR = 4
const LPAREN = 5
const RPAREN = 6
const END = 7
const NEG = 8

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
  let i = 0
  while (i < src.length) {
    const c = src.charCodeAt(i)
    if (c === 32) {
      i = i + 1
    } else if (c >= 48 && c <= 57) {
      let v = 0
      while (i < src.length) {
        const d = src.charCodeAt(i)
        if (d < 48 || d > 57) break
        v = v * 10 + (d - 48)
        i = i + 1
      }
      out.push(new Token(NUM, v))
    } else if (c >= 97 && c <= 122) {
      out.push(new Token(IDENT, c - 97))
      i = i + 1
    } else {
      let kind = RPAREN
      if (c === 43) kind = PLUS
      else if (c === 45) kind = MINUS
      else if (c === 42) kind = STAR
      else if (c === 40) kind = LPAREN
      out.push(new Token(kind, 0))
      i = i + 1
    }
  }
  out.push(new Token(END, 0))
  return out
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

let nodes = 0

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
      nodes = nodes + 1
      left = new Expr(kind, 0, left, right)
    }
    if (this.pos === this.toks.length - 1) this.trim()
    return left
  }
  trim(): void {
    this.toks.push(new Token(END, 0))
  }
  term(): Expr {
    let left = this.factor()
    while (this.toks[this.pos].kind === STAR) {
      this.pos = this.pos + 1
      const right = this.factor()
      nodes = nodes + 1
      left = new Expr(STAR, 0, left, right)
    }
    return left
  }
  factor(): Expr {
    const t = this.toks[this.pos]
    this.pos = this.pos + 1
    nodes = nodes + 1
    if (t.kind === NUM) return new Expr(NUM, t.value, null, null)
    if (t.kind === IDENT) return new Expr(IDENT, t.value, null, null)
    if (t.kind === MINUS) return new Expr(NEG, 0, this.factor(), null)
    const inner = this.expr()
    this.pos = this.pos + 1
    nodes = nodes - 1
    return inner
  }
}

function evaluate(e: Expr, env: number[]): number {
  if (e.kind === NUM) return e.value
  if (e.kind === IDENT) return env[e.value]
  if (e.kind === NEG) return (0 - evaluate(e.left!, env)) | 0
  const a = evaluate(e.left!, env)
  const b = evaluate(e.right!, env)
  if (e.kind === PLUS) return (a + b) | 0
  if (e.kind === MINUS) return (a - b) | 0
  return Math.imul(a, b)
}

const lines = ['1 + 2 * 3', '-(a - 4) * b', '(c + d) * (e - 12) - -f', 'g * h * 7 + 300']
const env: number[] = [3, 5, 7, 11, 13, 17, 19, 23]
let total = 0
for (let i = 0; i < lines.length; i = i + 1) {
  const p = new Parser(tokenize(lines[i]))
  const v = evaluate(p.expr(), env)
  console.log(lines[i], '=', v, 'at', p.pos, 'left', p.toks.length)
  total = (total + v) | 0
}
console.log(total, nodes)
