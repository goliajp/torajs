// holes of an array of objects: for-of and pop / shift read undefined,
// and copies that read through [[Get]] (spread, Array.from, toReversed,
// toSorted, with) hold undefined elements that callbacks, comparators,
// printing and JSON all see as undefined
class P {
  constructor(public n: number) {}
}
function show(xs: P[]): string {
  return xs.map(x => (x === undefined ? "U" : String(x.n))).join("")
}
function kinds(xs: P[]): string {
  const out: string[] = []
  xs.forEach(x => out.push(typeof x))
  return out.join(",")
}

const a: P[] = [new P(1), new P(2), new P(3)]
delete a[1]
const seen: string[] = []
for (const x of a) seen.push(typeof x)
console.log(seen.join(","))
console.log(show([...a]), show(Array.from(a)), show(a.toReversed()), show(a.with(0, new P(9))))
console.log(kinds([...a]), kinds(a.toReversed()))
console.log(show(a.toSorted((x, y) => y.n - x.n)))
console.log(JSON.stringify([...a]), 1 in [...a], 1 in a)
console.log([...a])

const g: P[] = [new P(4)]
g.length = 3
console.log(show([...g]), kinds(Array.from(g)))

const w: P[] = [new P(5)]
w[2] = new P(6)
console.log(show([...w]), show(w.toReversed()))

const b: P[] = [new P(7), new P(8)]
delete b[1]
const last = b.pop()
console.log(last === undefined, typeof last, b.length)
const d: P[] = [new P(4), new P(5)]
delete d[0]
const first = d.shift()
console.log(first === undefined, typeof first, d.length, d[0].n)
