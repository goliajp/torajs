// A method of the parser throws in the middle of a parse, after it has
// advanced the cursor: the caller catches outside the methods and reads
// the cursor, which must hold every advance made before the throw.

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
    if (t.kind !== LP) throw this.pos - 1
    const inner = this.expr()
    if (this.toks[this.pos].kind !== RP) throw 1000 + this.pos
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

function run(spec: number[]): void {
  const p = new Parser(lex(spec))
  try {
    console.log("value", p.expr(), p.pos)
  } catch (e) {
    console.log("caught", e, p.pos)
  }
  console.log("after", p.pos)
}

// 2 + 3 * (4 + 5) * 6 + 7
run([NUM, 2, PLUS, 0, NUM, 3, STAR, 0, LP, 0, NUM, 4, PLUS, 0, NUM, 5, RP, 0, STAR, 0, NUM, 6, PLUS, 0, NUM, 7, END, 0])
// 1 + 2 * + 3: factor meets a PLUS after advancing past it
run([NUM, 1, PLUS, 0, NUM, 2, STAR, 0, PLUS, 0, NUM, 3, END, 0])
// 1 + (2 * 3 END: the inner expression ends without its )
run([NUM, 1, PLUS, 0, LP, 0, NUM, 2, STAR, 0, NUM, 3, END, 0])
// a throw deep inside nested parentheses
run([LP, 0, LP, 0, LP, 0, NUM, 8, STAR, 0, RP, 0, RP, 0, RP, 0, END, 0])
