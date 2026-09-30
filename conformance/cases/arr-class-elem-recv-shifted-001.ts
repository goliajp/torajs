// The token array is `shift()`ed elsewhere in the program (a
// different function, through an alias), so its deque head moves:
// element reads through `this.field[this.index]` must stay
// head-aware and still answer the logical index.

class Token {
  kind: number
  constructor(kind: number) {
    this.kind = kind
  }
}

class Stream {
  toks: Token[]
  pos: number
  constructor(toks: Token[]) {
    this.toks = toks
    this.pos = 0
  }
  peek(): number {
    return this.toks[this.pos].kind
  }
  peekSafe(): string {
    try {
      return "kind=" + this.toks[this.pos].kind
    } catch (e: any) {
      return e instanceof TypeError ? "TypeError" : "other"
    }
  }
  take(): number {
    const t = this.toks[this.pos]
    this.pos = this.pos + 1
    return t.kind
  }
}

function drain(q: Token[], n: number): number {
  let sum = 0
  for (let i = 0; i < n; i = i + 1) {
    const t = q.shift()
    if (t === undefined) break
    sum = sum + t.kind
  }
  return sum
}

const toks: Token[] = []
for (let i = 0; i < 8; i = i + 1) toks.push(new Token(i * 10))

const s = new Stream(toks)
console.log(s.peek(), s.take(), s.peek())
console.log(drain(toks, 3))
console.log(s.toks.length, s.peek(), s.take(), s.take())
console.log(drain(toks, 100))
console.log(s.peekSafe())
toks.push(new Token(99))
s.pos = 0
console.log(s.peek(), s.toks.length, s.peekSafe())
