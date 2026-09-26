// Retains of typed heap values: shared string literals (static cells
// that must never be written), fresh strings, objects, arrays, closures,
// a null object slot, and the Map receiver `m.set` hands back.

const lit = 'static-literal'
const box = { n: 0 }
const held: { n: number }[] = []
const names: string[] = []
const m = new Map<string, string>()
const fns: (() => number)[] = []
for (let i = 0; i < 200000; i = i + 1) {
  names.push(lit)
  held.push(box)
  m.set(lit, lit).set('k' + (i % 7), lit)
  if (i % 50000 === 0) {
    const s = 'fresh-' + i
    names.push(s)
    const f = () => s.length + i
    fns.push(f)
    fns.push(f)
  }
}
box.n = 42
console.log(names.length, names[0], names[names.length - 1])
console.log(held.length, held[199999].n, held[0] === held[199999])
console.log(m.size, m.get('k3'), m.get(lit))
let total = 0
for (const f of fns) total = total + f()
console.log(fns.length, total)

let maybe: { n: number } | null = null
const alias = maybe
console.log(alias === null)
maybe = box
const alias2 = maybe
console.log(alias2 !== null ? alias2.n : -1)

function keep(xs: string[], s: string): string[] {
  const out = xs
  out.push(s)
  return out
}
let arr: string[] = []
for (let i = 0; i < 1000; i = i + 1) arr = keep(arr, lit)
console.log(arr.length, arr[999])
