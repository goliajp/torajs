// An `any` entering a typed array's element slot puts a cell of
// ANOTHER layout behind every read of that element: a literal typed
// by its annotation, an indexed assignment, and an `any` array
// converted at a typed declaration. A typed binding of the element
// read back, outliving the array, then drops the cell by the element
// class's layout. Each channel has a class of its own, so each has to
// be caught on its own. The wide cells minted afterwards reuse
// whatever the frees gave back, so a cell returned to the wrong size
// class shows up as a clobbered neighbour.
class Small {
  a: number
  constructor(a: number) {
    this.a = a
  }
}
class BigLit {
  a: number = 0
  b: number = 0
  c: number = 0
  d: number = 0
  e: number = 0
  f: number = 0
}
class BigSet {
  a: number = 0
  b: number = 0
  c: number = 0
  d: number = 0
  e: number = 0
  f: number = 0
}
class BigConv {
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

function anyArr(i: number): any {
  return [takeOut(i)]
}

let acc = 0
for (let i = 0; i < 64; i++) {
  {
    // a literal typed by its annotation
    let b: BigLit | null = null
    {
      const xs: BigLit[] = [takeOut(i)]
      const t: BigLit = xs[0]
      b = t
    }
    acc += b!.a
  }
  {
    // an indexed assignment
    let b: BigSet | null = null
    {
      const xs: BigSet[] = [new BigSet()]
      xs[0] = takeOut(i)
      const t: BigSet = xs[0]
      b = t
    }
    acc += b!.a
  }
  {
    // an `any` array converted at a typed declaration
    let b: BigConv | null = null
    {
      const xs: BigConv[] = anyArr(i)
      const t: BigConv = xs[0]
      b = t
    }
    acc += b!.a
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
