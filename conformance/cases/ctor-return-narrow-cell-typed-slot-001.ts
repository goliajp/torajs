// A constructor that answers another object (§10.2.2 step 13) puts a
// cell of ANOTHER layout behind a slot the checker types as the class:
// `const v: BigCtor = new BigCtor(c)` holds a Small. The typed drop of
// that slot then sizes the free by the class layout, not by the cell
// it has in hand, and the same crossing happens wherever `any` enters
// a struct-typed binding: an `as` cast, a typed call argument, a
// declared return. Each crossing here has a class of its own, so each
// has to be caught on its own, and each typed binding is the LAST
// owner of its cell, so its drop is the free. The wide cells minted
// afterwards reuse whatever the frees gave back, so a cell returned to
// the wrong size class shows up as a clobbered neighbour.
class Small {
  a: number
  constructor(a: number) {
    this.a = a
  }
}
class BigCtor {
  a: number = 0
  b: number = 0
  c: number = 0
  d: number = 0
  e: number = 0
  f: number = 0
  constructor(c: any) {
    return c
  }
}
class BigCast {
  a: number = 0
  b: number = 0
  c: number = 0
  d: number = 0
  e: number = 0
  f: number = 0
}
class BigArg {
  a: number = 0
  b: number = 0
  c: number = 0
  d: number = 0
  e: number = 0
  f: number = 0
}
class BigRet {
  a: number = 0
  b: number = 0
  c: number = 0
  d: number = 0
  e: number = 0
  f: number = 0
}
class Wide {
  a: number
  b: number
  c: number
  d: number
  e: number
  f: number
  constructor(n: number) {
    this.a = n
    this.b = n + 1
    this.c = n + 2
    this.d = n + 3
    this.e = n + 4
    this.f = n + 5
  }
}

// the narrow cells and the cells that must survive them, allocated
// alternately so each candidate's neighbour is a kept cell
const cands: Small[] = []
const kept: Small[] = []
for (let i = 0; i < 64; i++) {
  cands.push(new Small(i))
  kept.push(new Small(100 + i))
}
// hand candidate `i` out as an `any` nothing else owns
function takeOut(i: number): any {
  const c: any = cands[i]
  cands[i] = new Small(0)
  return c
}

function take(b: BigArg): BigArg {
  return b
}
function mk(i: number): BigRet {
  return takeOut(i)
}

let acc = 0
for (let i = 0; i < 64; i++) {
  {
    // narrow cell in a wide slot, through the constructor's answer: the
    // annotation names a class the cell is not an instance of, so the
    // declaration throws a TypeError where bun stores it (RFC 20260922,
    // decision 1); either way the cell is released by its own size
    const c = takeOut(i)
    acc += c.a
    try {
      const v: BigCtor = new BigCtor(c)
      acc += v.a - c.a
    } catch (e) {
      if (!(e instanceof TypeError)) throw e
    }
  }
  {
    // the same through an `as` cast out of `any`
    const w: BigCast = takeOut(i) as BigCast
    acc += w.a
  }
  {
    // a typed call argument, handed back into a typed binding, which
    // throws the same TypeError (see above); the cell is a `Small(0)`
    try {
      const t: BigArg = take(takeOut(i))
      acc += t.a
    } catch (e) {
      if (!(e instanceof TypeError)) throw e
    }
  }
  {
    // a declared return, which throws the same TypeError (see above);
    // the cell handed out here is a `Small(0)`, so `acc` is the same
    // either way
    try {
      const r: BigRet = mk(i)
      acc += r.a
    } catch (e) {
      if (!(e instanceof TypeError)) throw e
    }
  }
}
console.log('acc', acc)

// wide cells now: each takes a block the frees above handed back, and
// one of a narrower class is overrun into the kept cell next to it
const wides: Wide[] = []
for (let i = 0; i < 64; i++) wides.push(new Wide(1000 + i * 10))
let sum = 0
let okKept = 0
for (let i = 0; i < kept.length; i++) {
  sum += kept[i].a
  if (kept[i] instanceof Small) okKept++
}
console.log('kept', sum, okKept)
let wsum = 0
let okWide = 0
for (let i = 0; i < wides.length; i++) {
  wsum += wides[i].a + wides[i].e + wides[i].f
  if (wides[i] instanceof Wide) okWide++
}
console.log('wides', wsum, okWide)

// same width, a layout that is not a prefix: P and Q both fit one size
// class, so the sized free is right either way; the shape is here so
// the lane that admits it is pinned (the slot is only held and dropped:
// a typed read through Q's layout is not what a P cell answers)
class P {
  x: number
  y: number
  constructor(x: number, y: number) {
    this.x = x
    this.y = y
  }
}
class Q {
  s: number = 7
  t: number = 8
  u: number = 9
}
let qs = 0
for (let i = 0; i < 16; i++) {
  const q: Q = new P(i, i) as any
  qs += q === null ? 0 : 1
}
console.log('qs', qs)
