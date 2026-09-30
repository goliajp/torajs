// A class-typed element read as the receiver of a field read, on a
// `this.field[this.index]` receiver: the recursive-descent parser
// shape. The element read and the field read are one bounds test,
// one load and one nullish test; running off the end of the token
// array is still a TypeError (§13.3.2 GetValue on undefined).

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
  peek(): number {
    return this.toks[this.pos].kind
  }
  peekOr(): string {
    try {
      return "kind=" + this.toks[this.pos].kind
    } catch (e: any) {
      const m: string = e.message as string
      return (e instanceof TypeError) + ":" + (m.startsWith("undefined is not an object") ? "undefined" : m)
    }
  }
  count(): number {
    let n = 0
    try {
      while (this.toks[this.pos].kind !== END) {
        n = n + 1
        this.pos = this.pos + 1
      }
    } catch (e: any) {
      return e instanceof TypeError ? -n : -1000
    }
    return n
  }
  expr(): number {
    let left = this.term()
    while (this.toks[this.pos].kind === PLUS) {
      this.pos = this.pos + 1
      left = left + this.term()
    }
    return left
  }
  term(): number {
    let left = this.factor()
    while (this.toks[this.pos].kind === STAR) {
      this.pos = this.pos + 1
      left = left * this.factor()
    }
    return left
  }
  factor(): number {
    const t = this.toks[this.pos]
    this.pos = this.pos + 1
    if (t.kind === NUM) return t.value
    return t.value * 0
  }
}

function toks(spec: number[]): Token[] {
  const out: Token[] = []
  for (let i = 0; i < spec.length; i = i + 2) {
    out.push(new Token(spec[i], spec[i + 1]))
  }
  return out
}

// 2 + 3 * 4 + 5 = 19
const p = new Parser(toks([NUM, 2, PLUS, 0, NUM, 3, STAR, 0, NUM, 4, PLUS, 0, NUM, 5, END, 0]))
console.log(p.peek())
console.log(p.expr())
console.log(p.pos, p.peek(), p.peekOr())

// no END token: the while condition reads past the end
const q = new Parser(toks([NUM, 7, PLUS, 0, NUM, 8]))
console.log(q.count(), q.pos, q.peekOr())

// an empty token array
const r = new Parser([])
console.log(r.peekOr(), r.count())
