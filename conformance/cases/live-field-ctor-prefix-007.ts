// The factory's seed literal puts a placeholder into a field the
// constructor fills first thing, and the constructor overwrites it before
// anything reads it, so the placeholder no longer keeps the field from
// being proved live. A field read before the constructor stores it and a
// subclass seeding the base's fields keep the placeholder visible.
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
  sum(): number {
    let s = 0
    while (this.pos < this.toks.length) {
      s = s + this.toks[this.pos].kind
      this.pos = this.pos + 1
    }
    return s
  }
}
const p = new Parser([new Tok(3), new Tok(4), new Tok(5)])
console.log(p.peek(), p.sum(), p.pos)

class Early {
  xs: number[]
  constructor(xs: number[]) {
    console.log("before:", this.xs === undefined)
    this.xs = xs
  }
  first(): number {
    return this.xs[0]
  }
}
console.log(new Early([7]).first())

class SubParser extends Parser {
  extra: number
  constructor(toks: Tok[]) {
    super(toks)
    this.extra = this.toks.length
  }
}
const sp = new SubParser([new Tok(9)])
console.log(sp.peek(), sp.extra)
