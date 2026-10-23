// A field read again below a join keeps the first read's value only
// when no arm into the join can have written the field: one arm that
// stores to it, an arm that calls a method storing to it, and an arm
// that stores through another reference to the same object each make
// the second read see the new value.

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
  other: Cursor | null
  constructor(toks: Tok[]) {
    this.toks = toks
    this.pos = 0
    this.other = null
  }
  armStores(flag: boolean): number {
    const p0 = this.pos
    if (flag) this.pos = this.pos + 2
    return this.pos * 10 + p0
  }
  bump(n: number): number {
    if (n > 0) return this.bump(n - 1)
    this.pos = this.pos + 5
    return 7
  }
  pastEndCalls(): number {
    const p0 = this.pos
    let k = 0
    if (p0 >= this.toks.length) k = this.bump(2)
    else k = this.toks[p0].kind
    return this.pos * 100 + k
  }
  pastEndAliases(): number {
    const p0 = this.pos
    let k = 0
    if (p0 >= this.toks.length) {
      const o = this.other
      if (o !== null) o.pos = o.pos + 3
    } else {
      k = this.toks[p0].value
    }
    return this.pos * 100 + k
  }
  viewThenRead(): number {
    const t = this.toks[this.pos % this.toks.length]
    const q = this.pos
    this.pos = this.pos + 1
    return q * 10 + t.kind
  }
}

const toks: Tok[] = []
for (let i = 0; i < 4; i = i + 1) toks.push(new Tok(i + 1, i * 3))
const c = new Cursor(toks)
c.other = c
let sum = 0
for (let i = 0; i < 6; i = i + 1) {
  sum = sum + c.armStores(i % 2 === 0)
  console.log('stores', i, c.pos, sum)
  c.pos = i % 5
}
for (let i = 0; i < 6; i = i + 1) {
  c.pos = i
  console.log('call', i, c.pastEndCalls(), c.pos)
}
for (let i = 0; i < 6; i = i + 1) {
  c.pos = i
  console.log('alias', i, c.pastEndAliases(), c.pos)
}
c.pos = 0
for (let i = 0; i < 6; i = i + 1) console.log('view', i, c.viewThenRead(), c.pos)
