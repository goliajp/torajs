// A method of the parser throws in the middle of a parse, after it has
// advanced the cursor and counted nodes in a module-level counter: the
// caller catches outside the methods and reads both, which must hold
// every change made before the throw, and the next parse starts from
// them.
// TORAJS_PIN_STATS=1 prints
//   [pin] sid <n> Parser.toks: cluster {expr term factor} entries {expr} reads 4 direct scalars {nodes pos}

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
    if (t.kind !== LP) throw this.pos * 100 + nodes
    const inner = this.expr()
    if (this.toks[this.pos].kind !== RP) throw 100000 + nodes
    this.pos = this.pos + 1
    nodes = nodes + 10
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

function run(spec: number[]): void {
  const p = new Parser(lex(spec))
  try {
    console.log("value", p.expr(), p.pos, nodes)
  } catch (e) {
    console.log("caught", e, p.pos, nodes)
  }
  nodes = nodes + 5
  console.log("after", p.pos, nodes)
}

// 2 + 3 * (4 + 5) * 6 + 7
run([NUM, 2, PLUS, 0, NUM, 3, STAR, 0, LP, 0, NUM, 4, PLUS, 0, NUM, 5, RP, 0, STAR, 0, NUM, 6, PLUS, 0, NUM, 7, END, 0])
// 1 + 2 * + 3: factor meets a PLUS after advancing past it
run([NUM, 1, PLUS, 0, NUM, 2, STAR, 0, PLUS, 0, NUM, 3, END, 0])
// 1 + (2 * 3 END: the inner expression ends without its )
run([NUM, 1, PLUS, 0, LP, 0, NUM, 2, STAR, 0, NUM, 3, END, 0])
// a throw deep inside nested parentheses
run([LP, 0, LP, 0, LP, 0, NUM, 8, STAR, 0, RP, 0, RP, 0, RP, 0, END, 0])
// a parse that completes after the throws
run([LP, 0, NUM, 4, RP, 0, END, 0])
