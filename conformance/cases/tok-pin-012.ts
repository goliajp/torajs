// The parser of tok-pin-001 with ten counters in term live across its
// calls of factor, so the method needs most of the callee-saved
// registers while the array's length and data pointer hold two of them.
// TORAJS_PIN_STATS=1 prints
//   [pin] sid <n> Parser.toks: cluster {expr term factor} entries {expr} reads 6 direct

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

let mix = 0

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
    return left
  }
  term(): Expr {
    let a: number = this.pos | 0
    let b: number = (a * 3 + 1) | 0
    let c: number = (b ^ 5) | 0
    let d: number = (c + a) | 0
    let e: number = (d * 7) | 0
    let f: number = (e - b) | 0
    let g: number = (f + c) | 0
    let h: number = (g ^ d) | 0
    let k: number = (h + e) | 0
    let m: number = (k * 2 + f) | 0
    let left = this.factor()
    while (this.toks[this.pos].kind === STAR) {
      this.pos = this.pos + 1
      const right = this.factor()
      nodes = nodes + 1
      a = (a + this.toks.length) | 0
      b = (b + a) | 0
      c = (c ^ b) | 0
      d = (d + c) | 0
      e = (e + d) | 0
      f = (f ^ e) | 0
      g = (g + f) | 0
      h = (h + g) | 0
      k = (k ^ h) | 0
      m = (m + k) | 0
      left = new Expr(STAR, 0, left, right)
    }
    mix = (mix + a + b + c + d + e + f + g + h + k + m) | 0
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

const lines = ['1 + 2 * 3 * 4 * 5', '-(a - 4) * b * c * d', '(c + d) * (e - 12) * -f * g', 'g * h * 7 * 300 * a * b * c']
const env: number[] = [3, 5, 7, 11, 13, 17, 19, 23]
let total = 0
for (let i = 0; i < lines.length; i = i + 1) {
  const p = new Parser(tokenize(lines[i]))
  const v = evaluate(p.expr(), env)
  console.log(lines[i], '=', v, 'at', p.pos)
  total = (total + v) | 0
}
console.log(total, nodes, mix)
