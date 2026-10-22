// A program that can put no `then` or `constructor` on any object:
// a promise that carries other own properties still chains through
// the builtin `then`.
function add1(v: number): number {
  return v + 1
}

let p = Promise.resolve(0)
;(p as any).tag = "a"
for (let i = 0; i < 5; i = i + 1) {
  p = p.then(add1)
  ;(p as any).tag = "b"
}
p.then((v: number) => console.log("chain", v, (p as any).tag))
const q = Promise.resolve(10)
;(q as any).extra = 1
q.then(add1).then((v: number) => console.log("second", v))
