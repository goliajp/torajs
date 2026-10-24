// Two module-level counters: one only the parser's methods touch, one
// that a plain function the methods call also reads and writes. That
// function would see memory while the methods held the second counter
// in a register, so the second stays in memory; the cursor field and
// the first counter could still move. Every value read after a parse
// is the one the program stored.
// TORAJS_PIN_STATS=1 prints
//   [pin] sid <n> Parser.toks: scalar nodes: none: accessed by bump
//   [pin] sid <n> Parser.toks: cluster {expr term factor} entries {expr} reads 3 direct scalars {pos steps}

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
let steps = 0

function bump(k: number): number {
  if (k > 0) return bump(k - 1) + k
  nodes = nodes + 1
  return nodes % 3
}

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
    steps = steps + 1
    if (t.kind === NUM) return t.value * 10 + bump(t.value % 4)
    const inner = this.expr()
    this.pos = this.pos + 1
    return inner + bump(inner % 3)
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
const p = new Parser(lex(spec))
console.log(p.expr(), p.pos, nodes, steps)
bump(2)
steps = steps + 4
const q = new Parser(lex([NUM, 1, STAR, 0, LP, 0, LP, 0, NUM, 9, RP, 0, RP, 0, END, 0]))
console.log(q.expr(), q.pos, nodes, steps)
