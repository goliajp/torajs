// A member drops the last reference to its own receiver through a
// slot of an interface type: the program's only handle on the parser
// is a field typed by an interface the parser implements, and the
// member clears it before returning, then allocates a cell of the same
// shape. The cursor's last value must not be written into the freed
// cell, which by then may be the new one.

interface Runner {
  run(): number
}

class Token {
  kind: number
  value: number
  constructor(kind: number, value: number) {
    this.kind = kind
    this.value = value
  }
}

class Parser implements Runner {
  toks: Token[]
  pos: number
  constructor(toks: Token[]) {
    this.toks = toks
    this.pos = 0
  }
  run(): number {
    const a = this.step()
    const b = this.step()
    box.last = this.pos
    const n = this.toks.length
    const mark = 900 + this.pos
    box.r = null
    box.q = new Shadow(n, mark)
    return a + b
  }
  step(): number {
    const t = this.toks[this.pos]
    this.pos = this.pos + 1
    return t.value
  }
}

// a cell of a parser's size: a cell freed by one may be reused by the other
class Shadow {
  size: number
  pos: number
  constructor(n: number, mark: number) {
    this.size = n
    this.pos = mark + n
  }
}

class Box {
  r: Runner | null = null
  q: Shadow | null = null
  last: number = -1
}

const box = new Box()

function lex(n: number): Token[] {
  const out: Token[] = []
  for (let i = 0; i < n; i = i + 1) {
    out.push(new Token(0, i * 10 + 1))
  }
  return out
}

for (let round = 0; round < 3; round = round + 1) {
  box.r = new Parser(lex(4 + round))
  const v = (box.r as Parser).run()
  const filler: Token[] = lex(8)
  console.log("round", round, "value", v, "last", box.last, "r", box.r === null, "q", box.q!.pos, filler.length)
}
