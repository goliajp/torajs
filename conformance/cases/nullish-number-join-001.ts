// `x ?? d` where `x` is `number | undefined` and `d` a number is a
// number: the value is the field's own number when it is neither null
// nor undefined, `d` otherwise, whether the field holds a slot value,
// a value an `any` receiver wrote in its place (null, undefined, a
// number), or the answer of a getter; and `d` runs only when it is
// taken.

type Rec = { key: string; hits?: number; n: number }
const recs: Rec[] = []
for (let i = 0; i < 8; i = i + 1) {
  if (i % 3 === 0) {
    recs.push({ key: 'k' + i, n: i })
  } else {
    recs.push({ key: 'k' + i, hits: i + 0.5, n: i })
  }
}
const counts: Record<string, number> = {}
let total = 0
for (let i = 0; i < 8; i = i + 1) {
  const r = recs[i]
  counts[r.key] = (counts[r.key] ?? 0) + 1
  total = total + (r.hits ?? 1) + r.n
}
console.log(total, Object.keys(counts).length)

const a: any = recs[1]
a.hits = null
const b: any = recs[2]
b.hits = undefined
const c: any = recs[3]
c.hits = 2147483647
let gets = 0
Object.defineProperty(recs[4], 'hits', {
  get() {
    gets = gets + 1
    return -3
  },
})
Object.defineProperty(recs[5], 'hits', {
  get() {
    gets = gets + 1
    return undefined
  },
})

let calls = 0
function dflt(): number {
  calls = calls + 1
  return 0.25
}
let sum = 0
for (let i = 0; i < 8; i = i + 1) {
  const r = recs[i]
  const h = r.hits ?? dflt()
  sum = sum + h
  console.log(i, h, r.hits ?? -1, typeof (r.hits ?? 'x'))
}
console.log(sum, calls, gets)

let big = 0
for (let i = 0; i < 8; i = i + 1) {
  const r = recs[i]
  big = big + (r.hits ?? 2147483647) + 1
}
console.log(big)

class Box {
  v?: number
  constructor(v?: number) {
    this.v = v
  }
  get(): number {
    return this.v ?? 10
  }
}
const boxes = [new Box(), new Box(1.5), new Box(0), new Box(-0)]
console.log(boxes.map((x) => x.get()).join(','), Object.is(boxes[3].get(), -0))

const m = new Map<string, number>([['a', 1.5]])
console.log((m.get('a') ?? 0) + 1, (m.get('b') ?? 0) + 1)
const xs: (number | undefined)[] = [1, undefined, 3.5]
let t = 0
for (const x of xs) t = t + (x ?? 100)
console.log(t)
