// No method of these classes ever leaves a call position, so no
// foreign receiver can enter their any-typed twins: an index keyed by
// a field (`this.toks[this.pos]`) reads at a pinned head. The reads
// still answer the right elements, out-of-range reads still answer
// undefined, and an element read off a short array still raises.
class Tok {
  kind: number
  constructor(kind: number) {
    this.kind = kind
  }
}

class Reader {
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
      s = s + this.toks[this.pos].kind * (this.pos + 1)
      this.pos = this.pos + 1
    }
    return s
  }
  has(): boolean {
    return this.toks[this.pos] !== undefined
  }
}

const toks: Tok[] = []
for (let i = 0; i < 6; i = i + 1) toks.push(new Tok(i * 3 + 1))
const r = new Reader(toks)
console.log(r.peek())
console.log(r.sum())
console.log(r.has())
try {
  console.log(r.peek())
} catch (e) {
  console.log((e as Error).name)
}
const r2 = new Reader(toks.slice(2))
console.log(r2.peek(), r2.sum())
