// An `any` stored into a struct-typed binding puts a cell of another
// layout where a typed drop sizes the free by the declared class, so
// that class frees by the cell. Its base, whose layout is a prefix of
// it, is reached by such a cell only where a value of the subclass
// lands in a base-typed slot; here one does, and that slot is the last
// owner, so its drop is the free. The leaked cell is smaller than the
// base and lives in a smaller size class: a free sized by the base
// hands its block to the base's class, and the base-sized cells
// allocated right after it would overrun the empty cells minted beside
// it.
//
// A class-annotated binding holds an `any` value as it is, so the
// mismatched cell is held as a box and released by its own layout,
// never by the base's. Each write the binding refused is counted
// (none), and the refill and kept cells must stay clean.
class Base {
  a: number
  b: number
  c: number
  constructor(a: number) {
    this.a = a
    this.b = a + 1
    this.c = a + 2
  }
}
class Mid extends Base {
  d: number
  constructor(a: number) {
    super(a)
    this.d = a + 3
  }
}
class X {}
class Holder {
  x: Base | null = null
}

const cands: X[] = []
const kept: X[] = []
for (let i = 0; i < 64; i++) {
  cands.push(new X())
  kept.push(new X())
}
function takeOut(i: number): any {
  const c: any = cands[i]
  cands[i] = new X()
  return c
}
function refill(): number {
  const fresh: Base[] = []
  for (let j = 0; j < 4; j++) fresh.push(new Base(1000 + j))
  let sum = 0
  for (let j = 0; j < fresh.length; j++) sum += fresh[j].c
  return sum
}

let seen = 0
let bad = 0
let rejected = 0
for (let i = 0; i < 64; i++) {
  const h = new Holder()
  try {
    const m: Mid = takeOut(i)
    h.x = m
  } catch (e) {
    if (e instanceof TypeError) rejected++
  }
  if (h.x !== null) seen++
  h.x = null
  if (refill() !== 4014) bad++
}
console.log('seen', seen, 'rejected', rejected, 'bad', bad)

// a Mid that only ever holds its own cells, beside the open one
let own = 0
for (let i = 0; i < 64; i++) {
  const m = new Mid(i)
  own += m.d
}
console.log('own', own)

let okKept = 0
for (let i = 0; i < kept.length; i++) {
  if (kept[i] instanceof X) okKept++
}
console.log('kept', okKept)
