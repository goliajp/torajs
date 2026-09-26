// indexOf / lastIndexOf / includes on an array of objects with a needle
// typed any, undefined or null: identity for a live object, undefined
// matches a hole only where the method reads holes (includes), and
// null never matches a hole
class P {
  constructor(public n: number) {}
}
const p1 = new P(1)
const p2 = new P(2)
const a: P[] = [p1, p2, new P(3)]
const any2: any = p2
const other: any = new P(2)
console.log(a.indexOf(any2), a.includes(any2), a.indexOf(other), a.includes(other), a.lastIndexOf(any2))
console.log(a.includes(undefined as any), a.indexOf(undefined as any), a.includes(null as any))
delete a[1]
console.log(a.includes(undefined as any), a.indexOf(undefined as any), a.includes(null as any), a.indexOf(null as any))
const c = [...a]
console.log(c.includes(undefined as any), c.indexOf(undefined as any), c.lastIndexOf(undefined as any), c.includes(null as any))
function has(xs: P[], v: any): string {
  return xs.indexOf(v) + ":" + xs.includes(v)
}
console.log(has(c, undefined), has(c, p1), has(c, null), has(a, undefined))
