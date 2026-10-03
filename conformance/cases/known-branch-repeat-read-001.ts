// `xs[i].k === A || xs[i].k === B` reads the field, the index and the
// element twice, each read behind a nullish guard and a bounds check.
// With nothing stored in between the second set of reads is the first
// one and its guards are already answered. A store or a call between
// the two reads must still be seen by the second, and the first read
// must still raise for a missing element.
class Tok {
  kind: number
  constructor(kind: number) {
    this.kind = kind
  }
}

class Cursor {
  toks: Tok[]
  pos: number
  bumps: number
  constructor(toks: Tok[]) {
    this.toks = toks
    this.pos = 0
    this.bumps = 0
  }
  isAdditive(): boolean {
    return this.toks[this.pos].kind === 2 || this.toks[this.pos].kind === 3
  }
  bump(): boolean {
    this.pos = this.pos + 1
    this.bumps = this.bumps + 1
    return true
  }
  // the call between the reads moves the cursor: the second read is of
  // the next element
  hereOrNext(): boolean {
    return this.toks[this.pos].kind === 2 || (this.bump() && this.toks[this.pos].kind === 3)
  }
  // the store between the reads replaces the element
  afterStore(t: Tok): number {
    const a = this.toks[this.pos].kind
    this.toks[this.pos] = t
    return a * 10 + this.toks[this.pos].kind
  }
  // the store between the reads replaces the array
  afterSwap(other: Tok[]): number {
    const a = this.toks[this.pos].kind
    this.toks = other
    return a * 10 + this.toks[this.pos].kind
  }
  countAdditive(): number {
    let n = 0
    while (this.pos < this.toks.length) {
      if (this.toks[this.pos].kind === 2 || this.toks[this.pos].kind === 3) n = n + 1
      this.pos = this.pos + 1
    }
    return n
  }
}

const kinds = [1, 2, 3, 4, 3, 2, 7]
const toks: Tok[] = []
for (let i = 0; i < kinds.length; i = i + 1) toks.push(new Tok(kinds[i]))

const c = new Cursor(toks)
const seen: boolean[] = []
for (let i = 0; i < kinds.length; i = i + 1) {
  c.pos = i
  seen.push(c.isAdditive())
}
console.log(seen.join(' '))

c.pos = 0
console.log(c.countAdditive(), c.pos)

c.pos = 1
console.log(c.hereOrNext(), c.pos, c.bumps)
c.pos = 0
console.log(c.hereOrNext(), c.pos, c.bumps)
c.pos = 3
console.log(c.hereOrNext(), c.pos, c.bumps)

c.pos = 2
console.log(c.afterStore(new Tok(9)), toks[2].kind)
console.log(c.afterSwap([new Tok(5), new Tok(6), new Tok(8)]), c.toks.length)

// past the end: the first read raises, whichever arm it is in
c.pos = 3
try {
  console.log(c.isAdditive())
} catch (e) {
  console.log((e as Error).name, e instanceof TypeError)
}
try {
  console.log(c.hereOrNext())
} catch (e) {
  console.log((e as Error).name, c.pos)
}

// in range on the first read, past the end on the second
c.pos = 2
try {
  console.log(c.hereOrNext())
} catch (e) {
  console.log((e as Error).name, c.pos, c.bumps)
}
