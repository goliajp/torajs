// A dispatch table of functions, a number signature beside a named
// member it does not cover, a generic key, and an interface that
// extends another beside its signature.

const ops: Record<string, (x: number) => number> = {
  double: (x) => x * 2,
  square: (x) => x * x,
}
ops.neg = (x) => -x
for (const name of ["double", "square", "neg"]) {
  const op = ops[name]
  console.log(name, op(7))
}
console.log(Object.keys(ops).join(","), "missing" in ops)

interface StringArray {
  [index: number]: string
  length: number
}
const sa: StringArray = { length: 0 }
sa[0] = "zero"
sa[1] = "one"
sa.length = 2
console.log(sa.length, sa[0], sa[1], JSON.stringify(sa))

function pick<K extends string>(r: Record<K, number>, k: K): number {
  return r[k]
}
function total<K extends string>(r: Record<K, number>): number {
  let sum = 0
  for (const k in r) sum += r[k]
  return sum
}
const scores = { ann: 3, bob: 5 }
console.log(pick(scores, "bob"), total(scores), total({ x: 1, y: 2, z: 3 }))

interface Base {
  id: number
}
interface Counts extends Base {
  [k: string]: number
}
const c: Counts = { id: 1, a: 2 }
c.b = 3
console.log(JSON.stringify(c), Object.keys(c).length)
