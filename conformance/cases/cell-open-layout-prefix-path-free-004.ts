// A struct whose field is typed with a wider class, seen through a
// struct type whose same field is typed with a narrower one. The field
// is one slot under two static types: a cell stored through the wide
// view comes back out through the narrow view, and the narrow class's
// release frees it once the struct is gone. A cell of another class,
// smaller than the narrow one, written into the field through `any`
// would be read and released at the field's layout, and a free sized by
// the narrow class would hand its block to the wrong size class; the
// write refuses it with a TypeError instead, as a `let` of the class
// does, and the narrow cells allocated after each refusal are intact.
// bun stores it as it is (`seen 64 refused 0`), so this case is checked
// against its .expected.
class N {
  a: number
  b: number
  c: number
  constructor(n: number) {
    this.a = n
    this.b = n + 1
    this.c = n + 2
  }
}
class W {
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
class X {}
type S = { k: W; tag: number }
type P = { k: N }

const kept: X[] = []
const cands: X[] = []
for (let i = 0; i < 64; i++) {
  cands.push(new X())
  kept.push(new X())
}
function takeOut(i: number): any {
  const c: any = cands[i]
  cands[i] = new X()
  return c
}
function peek(p: P): N {
  return p.k
}

let seen = 0
let refused = 0
for (let i = 0; i < 64; i++) {
  let o: N | null = null
  try {
    const s: S = { k: takeOut(i), tag: i }
    o = peek(s)
  } catch (e) {
    if (e instanceof TypeError) refused++
  }
  if (o !== null) seen++
  o = null
  const fresh: N[] = []
  for (let j = 0; j < 4; j++) fresh.push(new N(1000 + j))
  let fsum = 0
  for (let j = 0; j < fresh.length; j++) fsum += fresh[j].c
  if (fsum !== 4014) console.log('fresh', i, fsum)
}
let okKept = 0
for (let i = 0; i < kept.length; i++) {
  if (kept[i] instanceof X) okKept++
}
console.log('seen', seen, 'refused', refused, 'kept', okKept)
