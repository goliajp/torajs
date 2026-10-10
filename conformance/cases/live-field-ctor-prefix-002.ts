// The roads a live field is proved against stay open here, so every
// read still tests for the cell: a write through `any` puts `undefined`
// and `null` back into a field the constructor had filled, and the
// next read throws the TypeError the spec asks for (§13.3.2.1 step 3
// via ToObject).
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
function name(e: unknown): string {
  return e instanceof TypeError ? "TypeError" : "other"
}
const good = new Parser([new Tok(3)])
console.log(good.peek())
try {
  ;(good as any).toks = undefined
  console.log(good.peek())
} catch (e) {
  console.log("undefined write:", name(e))
}
const alias: any = new Parser([new Tok(4)])
try {
  alias.toks = null
  console.log((alias as Parser).toks.length)
} catch (e) {
  console.log("null write:", name(e))
}
