// A method override whose parameter is typed with a narrower class
// than the base's, called through the base, and a closure called
// through a function type, both handed a wider-class value whose slot
// holds a cell of unknown layout; the narrower class's slot is where
// it is released. The leaked cell is smaller than the narrow class
// and lives in a smaller size class, so a free sized by the narrow
// class hands its block to the wrong class; the narrow cells
// allocated right after it would then overrun the empty cells minted
// beside it.
class N {
  a: number
  b: number
  c: number
  d: number
  constructor(n: number) {
    this.a = n
    this.b = n + 1
    this.c = n + 2
    this.d = n + 3
  }
}
class W {
  a: number
  b: number
  c: number
  d: number
  e: number
  constructor(n: number) {
    this.a = n
    this.b = n + 1
    this.c = n + 2
    this.d = n + 3
    this.e = n + 4
  }
}
class X {}
class Holder {
  n: N | null = null
}
class Base {
  take(w: W, h: Holder): void {
    h.n = null
  }
}
class Derived extends Base {
  take(n: N, h: Holder): void {
    h.n = n
  }
}

const kept: X[] = []
const cands: X[] = []
for (let i = 0; i < 128; i++) {
  cands.push(new X())
  kept.push(new X())
}
function takeOut(i: number): any {
  const c: any = cands[i]
  cands[i] = new X()
  return c
}
function apply(f: (w: W) => void, w: W): void {
  f(w)
}
function refill(): number {
  const fresh: N[] = []
  for (let j = 0; j < 4; j++) fresh.push(new N(1000 + j))
  let fsum = 0
  for (let j = 0; j < fresh.length; j++) fsum += fresh[j].d
  return fsum
}

const h = new Holder()
let bad = 0
for (let i = 0; i < 64; i++) {
  const w: W = takeOut(i)
  apply((n: W) => {
    const m: N = n
    h.n = m
  }, w)
  h.n = null
  if (refill() !== 4018) bad++
}
const bs: Base[] = [new Base(), new Derived()]
for (let i = 64; i < 128; i++) {
  const w: W = takeOut(i)
  bs[i % 2].take(w, h)
  h.n = null
  if (refill() !== 4018) bad++
}
let okKept = 0
for (let i = 0; i < kept.length; i++) {
  if (kept[i] instanceof X) okKept++
}
console.log('bad', bad, 'kept', okKept)
