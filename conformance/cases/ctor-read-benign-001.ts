// a constructor read only for its name / length / prototype, or to
// compare or typeof it, never looks a static up on the answer
class Foo {}
class Bar extends Foo {}
function describe(e: unknown): string {
  return (e as Error).constructor.name
}
try {
  throw new TypeError("x")
} catch (e) {
  console.log(describe(e))
}
try {
  JSON.parse("{")
} catch (e) {
  console.log((e as Error).constructor.name)
}
const f: any = new Bar()
console.log(f.constructor === Bar, f.constructor === Foo, f.constructor !== Foo)
console.log(f.constructor.name, f.constructor.length)
const o: any = { a: 1 }
console.log(o.constructor.name, o.constructor.length, typeof o.constructor)
console.log(o.constructor == 5, o.constructor.prototype.hasOwnProperty("a"))
console.log(new Foo().constructor.name)
const m = new Map<string, number>()
console.log((m as any).constructor.name)
const xs = [1, 2]
console.log(xs.constructor.name)
