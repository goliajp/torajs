// A construction site that hands the constructor a value which may be
// `undefined` (an element popped off an empty array) keeps the field's
// reads guarded: the read after it throws the TypeError the spec asks
// for (§13.3.2.1 step 3 via ToObject), and the sibling class whose
// every construction is live is unaffected.
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
}
class Lexer {
  src: Tok[]
  constructor(src: Tok[]) {
    this.src = src
  }
  count(): number {
    return this.src.length
  }
}
const stack: Tok[][] = [[new Tok(6)]]
const first = new Parser(stack.pop()!)
console.log(first.peek(), new Lexer([new Tok(1), new Tok(2)]).count())
try {
  const second = new Parser(stack.pop()!)
  console.log(second.peek())
} catch (e) {
  console.log("popped undefined:", e instanceof TypeError)
}
