// A read past the end of an array of class instances, with a log line
// and a push between the read and the field read (which keeps the
// instances out of place): the TypeError comes after both.

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
  trail: number[]
  constructor(toks: Tok[]) {
    this.toks = toks
    this.pos = 0
    this.trail = []
  }
  factor(): number {
    const t = this.toks[this.pos]
    this.pos = this.pos + 1
    nodes = nodes + 1
    if (t.kind === 0) return t.value
    if (t.kind === 1) return 0 - t.value
    return 100 + t.kind
  }
  noisy(): number {
    const t = this.toks[this.pos]
    console.log('between ' + this.pos)
    this.trail.push(this.pos)
    this.pos = this.pos + 1
    return t.kind * 10 + t.value
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
p.pos = 2
for (let i = 0; i < 4; i = i + 1) {
  try {
    console.log('noisy ' + p.noisy() + ' ' + p.pos)
  } catch (e) {
    console.log('caught ' + (e instanceof TypeError) + ' ' + p.pos + ' ' + p.trail.length)
  }
}
