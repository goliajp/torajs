// The parser with a subclass that overrides `factor`, so a call of
// `this.factor()` may enter either method through the class's method
// table, and the override counts nodes in the module-level counter too.
// The base class is then not the only layout its cells come from, so
// no cluster forms and neither the cursor field nor the counter leaves
// memory; each parser, base or subclass, must read back the values the
// program stored, with the counter shared between them.
// TORAJS_PIN_STATS=1 prints
//   [pin] sid <n> Parser.toks: none: sealed true closed false unwidened true

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
const LP = 3
const RP = 4
const END = 5

let nodes = 0

class Parser {
  toks: Token[]
  pos: number
  constructor(toks: Token[]) {
    this.toks = toks
    this.pos = 0
  }
  expr(): number {
    let left = this.term()
    while (this.toks[this.pos].kind === PLUS) {
      this.pos = this.pos + 1
      left = left + this.term()
      nodes = nodes + 1
    }
    return left
  }
  term(): number {
    let left = this.factor()
    while (this.toks[this.pos].kind === STAR) {
      this.pos = this.pos + 1
      left = left * this.factor()
      nodes = nodes + 1
    }
    return left
  }
  factor(): number {
    const t = this.toks[this.pos]
    this.pos = this.pos + 1
    nodes = nodes + 1
    if (t.kind === NUM) return t.value
    const inner = this.expr()
    this.pos = this.pos + 1
    return inner
  }
}

class Doubling extends Parser {
  factor(): number {
    const t = this.toks[this.pos]
    this.pos = this.pos + 1
    nodes = nodes + 100
    if (t.kind === NUM) return t.value * 2
    const inner = this.expr()
    this.pos = this.pos + 1
    return inner
  }
}

function lex(spec: number[]): Token[] {
  const out: Token[] = []
  for (let i = 0; i < spec.length; i = i + 2) {
    out.push(new Token(spec[i], spec[i + 1]))
  }
  return out
}

// 2 + 3 * (4 + 5) * 6 + 7
const spec = [NUM, 2, PLUS, 0, NUM, 3, STAR, 0, LP, 0, NUM, 4, PLUS, 0, NUM, 5, RP, 0, STAR, 0, NUM, 6, PLUS, 0, NUM, 7, END, 0]
const parsers: Parser[] = [new Parser(lex(spec)), new Doubling(lex(spec)), new Parser(lex(spec))]
for (let i = 0; i < parsers.length; i = i + 1) {
  const p = parsers[i]
  console.log(i, p.expr(), p.pos, nodes)
  nodes = nodes + 1
}
const d = new Doubling(lex([LP, 0, NUM, 3, STAR, 0, NUM, 4, RP, 0, END, 0]))
console.log("doubling", d.expr(), d.pos, nodes)
