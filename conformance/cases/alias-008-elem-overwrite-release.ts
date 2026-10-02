// A binding that reads an element, then outlives whatever releases
// that element — an overwrite, a pop, a truncation, a rebinding of the
// container's only owner — must still name the old value.
class N {
  a: number
  b: number
  constructor(n: number) {
    this.a = n
    this.b = n + 1
  }
}
class Box {
  k: N
  constructor(n: number) {
    this.k = new N(n)
  }
}
class Tok {
  v: number
  constructor(v: number) {
    this.v = v
  }
}
class Empty {
  z: number
  constructor() {
    this.z = 0
  }
}
class Other {
  w: number
  q: number
  constructor(w: number) {
    this.w = w
    this.q = w
  }
}
class C {
  toks: Tok[]
  constructor(toks: Tok[]) {
    this.toks = toks
  }
}

function swapAt(xs: N[], i: number, n: number): N {
  const e = xs[i]
  xs[i] = new N(n)
  return e
}

function popKeep(xs: N[]): N {
  const e = xs[xs.length - 1]
  xs.pop()
  return e
}

function truncKeep(xs: N[]): N {
  const e = xs[0]
  xs.length = 0
  return e
}

function rootSwap(n: number): N {
  let b = new Box(n)
  const o = b.k
  b = new Box(n + 1000)
  return o
}

const kept: N[] = []
const xs: N[] = [new N(0), new N(1)]
for (let i = 1; i <= 1000; i++) kept.push(swapAt(xs, i & 1, i))
for (let i = 1; i <= 1000; i++) {
  const ys: N[] = [new N(i), new N(i * 10)]
  kept.push(popKeep(ys))
  kept.push(truncKeep(ys))
  kept.push(rootSwap(i))
}

// an element of an `any` array, swapped out and then handed to a
// constructor through an array literal
const spare: any[] = []
for (let i = 0; i < 64; i++) spare.push(new Other(i))
let held: any = null
let seen = 0
for (let r = 0; r < 200; r++) {
  for (let i = 0; i < 64; i++) {
    const e = spare[i]
    spare[i] = r % 2 === 0 ? new Empty() : new Other(r + i)
    held = new C([e])
    seen += held.toks.length
    held = null
  }
}

// a for-of variable whose element the body releases before reading it
const ws: N[] = []
for (let i = 0; i < 100; i++) ws.push(new N(i))
let ft = 0
for (const w of ws) {
  ws[w.a & 127] = new N(-1)
  ft += w.a + w.b
}
const vs: N[] = [new N(5), new N(6)]
for (const v of vs) {
  vs[0] = new N(9)
  vs[1] = new N(9)
  ft += v.b
}
console.log(ft)

const junk: N[] = []
for (let i = 0; i < 4000; i++) junk.push(new N(100000 + i))
let total = 0
for (const x of kept) total += x.a + x.b
console.log(total, kept.length, xs[0].a, xs[1].a, seen, held, junk.length)
