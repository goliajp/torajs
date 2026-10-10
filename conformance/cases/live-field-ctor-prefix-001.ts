// A non-nullable pointer-shaped field the constructor fills in its
// leading writes, and that every other write fills from a live value,
// is never `undefined`: reads through `this`, through a binding, in a
// subclass, after a later reassignment, and from a value that came
// through a function all of whose returns are live.
class Tok {
  kind: number
  constructor(kind: number) {
    this.kind = kind
  }
}
class Parser {
  toks: Tok[]
  pos: number
  constructor(toks: Tok[]) {
    this.toks = toks
    this.pos = 0
  }
  peek(): number {
    return this.toks[this.pos].kind
  }
  advance(): number {
    const k = this.toks[this.pos].kind
    this.pos = this.pos + 1
    return k
  }
  reset(toks: Tok[]): void {
    this.toks = toks
    this.pos = 0
  }
}
class Counting extends Parser {
  seen: number
  constructor(toks: Tok[]) {
    super(toks)
    this.seen = 0
  }
  take(): number {
    this.seen = this.seen + 1
    return this.toks[this.pos].kind + this.advance()
  }
}
function lex(src: string): Tok[] {
  const out: Tok[] = []
  for (let i = 0; i < src.length; i = i + 1) {
    out.push(new Tok(src.charCodeAt(i) - 48))
  }
  return out
}
function pick(flag: boolean, a: Tok[], b: Tok[]): Tok[] {
  if (flag) return a
  return b
}
const p = new Parser(lex("314"))
console.log(p.peek(), p.advance(), p.advance(), p.toks.length)
p.reset(pick(p.pos > 1, lex("9"), lex("27")))
console.log(p.peek(), p.toks.length)
const c = new Counting(lex("55"))
console.log(c.take(), c.seen, c.toks.length)
const held: Tok[] = lex("8")
const q = new Parser(held)
held.push(new Tok(1))
console.log(q.toks.length, q.advance(), q.advance())
