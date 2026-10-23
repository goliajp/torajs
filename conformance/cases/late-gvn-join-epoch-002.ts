// The element read twice in `a || b` names one address both times, so
// the second read is the first one's value only while nothing between
// them can store: a call that moves the cursor, or one that rewrites
// the element, makes the second read load again.

class Tok {
  kind: number
  value: number
  constructor(kind: number, value: number) {
    this.kind = kind
    this.value = value
  }
}

class Cursor {
  toks: Tok[]
  pos: number
  constructor(toks: Tok[]) {
    this.toks = toks
    this.pos = 0
  }
  step(): number {
    this.pos = this.pos + 1
    return 9
  }
  twice(): boolean {
    return this.toks[this.pos].kind === 2 || this.toks[this.pos].kind === 3
  }
  movedBetween(): boolean {
    return this.toks[this.pos].kind === this.step() || this.toks[this.pos].kind === 3
  }
}

function rewrite(ts: Tok[], i: number): number {
  ts[i] = new Tok(3, 0)
  return 9
}

const toks: Tok[] = []
for (let i = 0; i < 6; i = i + 1) toks.push(new Tok(i % 4, i))
const c = new Cursor(toks)
for (let i = 0; i < 5; i = i + 1) {
  c.pos = i
  console.log('twice', i, c.twice())
}
for (let i = 0; i < 5; i = i + 1) {
  c.pos = i
  console.log('moved', i, c.movedBetween(), c.pos)
}
let hits = 0
for (let i = 0; i < 6; i = i + 1) {
  if (toks[i].kind === rewrite(toks, i) || toks[i].kind === 3) hits = hits + 1
}
console.log('rewritten', hits, toks[5].kind)
