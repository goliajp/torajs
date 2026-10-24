// Four words the parser's methods could keep in registers, the cursor
// field and three module-level counters, but only two registers: the two
// most accessed get them and the other two stay in memory, written by the
// same methods in between. All four must read back as the program
// stored them, after each parse and after a nested one.
// TORAJS_PIN_STATS=1 prints
//   [pin] sid <n> Parser.toks: scalar depth: none: 2 more accessed
//   [pin] sid <n> Parser.toks: scalar deepest: none: 2 more accessed
//   [pin] sid <n> Parser.toks: cluster {expr term factor} entries {expr} reads 3 direct scalars {pos nodes}

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
let depth = 0
let deepest = 0

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
    depth = depth + 1
    if (depth > deepest) deepest = depth
    const inner = this.expr()
    this.pos = this.pos + 1
    depth = depth - 1
    return inner + depth * 1000
  }
}

function lex(spec: number[]): Token[] {
  const out: Token[] = []
  for (let i = 0; i < spec.length; i = i + 2) {
    out.push(new Token(spec[i], spec[i + 1]))
  }
  return out
}

// 2 + 3 * (4 + (5 * (6 + 1))) * 6 + 7
const spec = [
  NUM, 2, PLUS, 0, NUM, 3, STAR, 0, LP, 0, NUM, 4, PLUS, 0, LP, 0, NUM, 5, STAR, 0,
  LP, 0, NUM, 6, PLUS, 0, NUM, 1, RP, 0, RP, 0, RP, 0, STAR, 0, NUM, 6, PLUS, 0, NUM, 7, END, 0,
]
const p = new Parser(lex(spec))
console.log("first", p.expr(), p.pos, nodes, depth, deepest)
depth = 5
deepest = 0
const q = new Parser(lex([LP, 0, LP, 0, NUM, 9, STAR, 0, NUM, 2, RP, 0, RP, 0, END, 0]))
console.log("second", q.expr(), q.pos, nodes, depth, deepest)
nodes = 0
p.pos = 0
console.log("again", p.expr(), p.pos, nodes, depth, deepest)
