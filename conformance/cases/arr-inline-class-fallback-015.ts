// An array of a class that could hold its instances in place, in a
// program where the class of its container is reached through `any`,
// so the container's methods run on an `any` receiver too: the class
// keeps its cells, and every answer is the one a cell gives.
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
console.log(p.next(), p.next())
const q: any = new Parser(make(3))
console.log(q.next(), q.next(), q.pos)
