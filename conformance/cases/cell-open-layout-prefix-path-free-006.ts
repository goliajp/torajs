// The other direction through the same pair of views: a struct whose
// field is typed with a wider class gets a cell of the narrower class
// stored through a struct type whose same field is typed narrower, and
// the struct's release frees the field as the wider class. No `any`
// takes part. The stored cell lives in a smaller size class than the
// wider class's, so a free sized by the wider class hands its block to
// the wrong class; the wider cells allocated right after it would then
// overrun the narrow cell minted beside it.
class N {
  a: number
  constructor(n: number) {
    this.a = n
  }
}
class W {
  a: number
  b: number
  c: number
  d: number
  e: number
  f: number
  g: number
  h: number
  i: number
  j: number
  constructor(n: number) {
    this.a = n
    this.b = n
    this.c = n
    this.d = n
    this.e = n
    this.f = n
    this.g = n
    this.h = n
    this.i = n
    this.j = n + 9
  }
}
type S = { k: W; tag: number }
type P = { k: N }

function put(p: P, n: number): void {
  p.k = new N(n)
}

const kept: N[] = []
let seen = 0
let bad = 0
for (let i = 0; i < 64; i++) {
  {
    const s: S = { k: new W(i), tag: i }
    put(s, i)
    kept.push(new N(100 + i))
    seen += s.k.a
  }
  const fresh: W[] = []
  for (let j = 0; j < 4; j++) fresh.push(new W(1000 + j))
  let fsum = 0
  for (let j = 0; j < fresh.length; j++) fsum += fresh[j].j
  if (fsum !== 4042) bad++
}
let ksum = 0
for (let i = 0; i < kept.length; i++) ksum += kept[i].a
console.log('seen', seen, 'bad', bad, 'kept', ksum)
