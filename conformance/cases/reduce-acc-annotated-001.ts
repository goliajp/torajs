// Annotated reducer accumulators that TypeScript accepts: the
// accumulator parameter takes the seed (or the element type when there
// is no seed) and the callback's return fits back into it.
const ys = [1, 2, 3]
const ws = ["a", "bb", "ccc"]

console.log(ys.reduce((acc: number, x) => acc + x, 0))
console.log(ys.reduce((acc: number, x: number) => acc + x))
console.log(ys.reduce((acc: string, x) => acc + x, ""))
console.log(ys.reduce((acc: string, x: number) => acc + x, ""))
console.log(ws.reduce((acc: number, w: string) => acc + w.length, 0))
console.log(ws.reduce((acc: string, w) => acc + w))
console.log(ys.reduce((acc: boolean, x) => acc && x > 0, true))
console.log(ys.reduce((acc: number, x, i) => acc + x * i, 0))

const rec = ws.reduce((acc: Record<string, number>, w) => {
  acc[w] = w.length
  return acc
}, {})
console.log(JSON.stringify(rec))

// unannotated accumulators take their type from the seed
console.log(ys.reduce((acc, x) => acc + x, 0))
console.log(ys.reduce((acc, x) => acc + x, ""))
console.log(ys.reduce((acc, x) => acc + x))

console.log(ys.reduceRight((acc: string, x) => acc + x, ""))
console.log(ys.reduceRight((acc: number, x) => acc * 10 + x, 0))

// an any accumulator takes any seed
console.log(ys.reduce((acc: any, x) => acc + x, "s"))

// rest-parameter reducers
console.log(ys.reduce((acc: string, ...xs: any[]) => acc + xs.length, ""))
console.log(ys.reduce((acc: number, ...xs: any[]) => acc + xs.length, 0))
