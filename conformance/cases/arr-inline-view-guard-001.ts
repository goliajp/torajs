// A view bound from a read past the end of an array of in-place
// instances holds undefined; the field read below raises the
// TypeError only after everything written in between has happened:
// the cursor and counter stores.

class Tok {
  kind: number
  value: number
  constructor(kind: number, value: number) {
    this.kind = kind
    this.value = value
  }
}

let nodes = 0

class Parser {
  toks: Tok[]
  pos: number
  constructor(toks: Tok[]) {
    this.toks = toks
    this.pos = 0
  }
  factor(): number {
    const t = this.toks[this.pos]
    this.pos = this.pos + 1
    nodes = nodes + 1
    if (t.kind === 0) return t.value
    if (t.kind === 1) return 0 - t.value
    return 100 + t.kind
  }
  prev(): number {
    const t = this.toks[this.pos - 1]
    this.pos = this.pos + 2
    return t.value
  }
}

const toks: Tok[] = []
for (let i = 0; i < 4; i = i + 1) toks.push(new Tok(i % 3, i + 1))
const p = new Parser(toks)
for (let i = 0; i < 6; i = i + 1) {
  try {
    console.log('factor ' + p.factor() + ' ' + p.pos + ' ' + nodes)
  } catch (e) {
    console.log('caught ' + (e instanceof TypeError) + ' ' + p.pos + ' ' + nodes)
  }
}
for (let i = 0; i < 4; i = i + 1) {
  p.pos = i * 2
  try {
    console.log('prev ' + p.prev() + ' ' + p.pos)
  } catch (e) {
    console.log('caught ' + (e instanceof TypeError) + ' ' + p.pos)
  }
}
