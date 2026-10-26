// An `any` entering a struct-typed field, or a struct-typed binding a
// closure shares (a capture box), puts a cell of ANOTHER layout where
// a typed drop sizes the free by the declared class. The box is
// filled from the `any` at its declaration; in one shape only the
// closures ever read or write it, in the other the declaring function
// replaces it. Each channel has a class of its own, and the typed
// slot is the LAST owner of its cell, so its drop is the free. The
// wide cells minted afterwards reuse whatever the frees gave back, so
// a cell returned to the wrong size class shows up as a clobbered
// neighbour.
//
// A field or binding annotated with a class holds an `any` value as
// it is, so the mismatched cell is held as a box and released by its
// own layout, never by the declared class's. Each channel counts the
// writes it refused (none), and the wide and kept cells below must
// still come out intact.
class Small {
  a: number
  constructor(a: number) {
    this.a = a
  }
}
class BigField {
  a: number = 0
  b: number = 0
  c: number = 0
  d: number = 0
  e: number = 0
  f: number = 0
}
class BigBoxClosure {
  a: number = 0
  b: number = 0
  c: number = 0
  d: number = 0
  e: number = 0
  f: number = 0
}
class BigBoxOuter {
  a: number = 0
  b: number = 0
  c: number = 0
  d: number = 0
  e: number = 0
  f: number = 0
}
class Holder {
  b: BigField | null = null
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

// the declaring function fills the box from an `any` and never
// touches it again; the closures read it and replace it
function viaBoxClosure(i: number): number {
  let b: BigBoxClosure = takeOut(i)
  const get = () => b.a
  const reset = () => {
    b = new BigBoxClosure()
  }
  const r = get()
  reset()
  return r + get()
}
// the declaring function fills the box from an `any` and replaces it
function viaBoxOuter(i: number): number {
  let b: BigBoxOuter = takeOut(i)
  const get = () => b.a
  const r = get()
  b = new BigBoxOuter()
  return r + get()
}

let acc = 0
let rejField = 0
let rejBoxClosure = 0
let rejBoxOuter = 0
for (let i = 0; i < 64; i++) {
  {
    // a field, read back into a typed binding that outlives the holder
    let b: BigField | null = null
    {
      const h = new Holder()
      try {
        h.b = takeOut(i)
      } catch (e) {
        if (e instanceof TypeError) rejField++
      }
      if (h.b !== null) {
        const t: BigField = h.b!
        b = t
      }
    }
    if (b !== null) acc += b!.a
  }
  try {
    acc += viaBoxClosure(i)
  } catch (e) {
    if (e instanceof TypeError) rejBoxClosure++
  }
  try {
    acc += viaBoxOuter(i)
  } catch (e) {
    if (e instanceof TypeError) rejBoxOuter++
  }
}
console.log('rejected', rejField, rejBoxClosure, rejBoxOuter)
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
