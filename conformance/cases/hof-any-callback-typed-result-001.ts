// A callback typed `any` sends the call down the any lane, whose map
// answers an Array<Any> block; a typed `number[]` binding or return
// type must decode it, not read the boxes as raw doubles.
function h(xs: number[], cb: any): number[] {
  return xs.map(cb)
}
console.log(h([0.5, 1.25], (x: number) => x * 2))
const cb2: any = (x: number) => x * 2
const r: number[] = [0.5, 1.25].map(cb2)
console.log(r, r[0] + r[1])
function g2(xs: number[], cb = (x: number) => x * 2): number[] {
  return xs.map(cb)
}
console.log(g2([0.5, 1.25]), g2([3], (x: number) => x / 4))
const keep: any = (x: number) => x > 1
const f: number[] = [0.5, 1.5, 2.5].filter(keep)
console.log(f, f.length)
const s: string[] = ['b', 'a'].map(cb2 === null ? cb2 : (x: any) => x + '!')
console.log(s)
