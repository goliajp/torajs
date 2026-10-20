// A function value handed to `then` on every iteration is released
// alive once per reaction job. Three shapes of that value: a bare
// named function, one carrying an expando property, and a closure
// whose capture closes a ring with the object holding it. The output
// is what matters; the shapes pin the runtime's release paths.

function add1(v: number): number { return v + 1 }
let total = 0
for (let i = 0; i < 1000; i = i + 1) {
  total = total + (await Promise.resolve(i).then(add1))
}
console.log(total)

function twice(v: number): number { return v * 2 }
;(twice as any).hits = 0
let t2 = 0
for (let i = 0; i < 100; i = i + 1) {
  t2 = t2 + (await Promise.resolve(i).then(twice))
  ;(twice as any).hits = (twice as any).hits + 1
}
console.log(t2, (twice as any).hits)

type Node = { cb: ((v: number) => number) | null; k: number }
let t3 = 0
for (let i = 0; i < 100; i = i + 1) {
  const node: Node = { cb: null, k: i }
  const cb = (v: number): number => v + node.k
  node.cb = cb
  t3 = t3 + (await Promise.resolve(1).then(cb))
}
console.log(t3)
