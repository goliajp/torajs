// A container class whose method reads its array of in-place
// instances: the method's `any`-receiver twin exists but no `any`
// value can reach a container, so the class is held in place and the
// method reads the elements where they lie.
class Token {
  kind: number
  value: number
  constructor(kind: number, value: number) {
    this.kind = kind
    this.value = value
  }
}
class Parser {
  toks: Token[]
  pos: number
  constructor(toks: Token[]) {
    this.toks = toks
    this.pos = 0
  }
  next(): number {
    const t = this.toks[this.pos]
    this.pos = this.pos + 1
    return t.kind + t.value
  }
}
function make(n: number): Token[] {
  const ts: Token[] = []
  for (let i = 0; i < n; i = i + 1) ts.push(new Token(i, i * 2))
  return ts
}
const p = new Parser(make(4))
console.log(p.next(), p.next(), p.next(), p.pos)
