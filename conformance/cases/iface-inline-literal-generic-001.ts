// An object type written in place inside a generic declaration may
// name the declaration's type parameters (`{ m: () => T }`): the
// program is checked per instantiation, as any generic one is.
function first<T>(s: { m: () => T }): T {
  return s.m()
}
function idf<T>(s: { m: (x: T) => T }, x: T): T {
  return s.m(x)
}
type B<T> = { k: { m: () => T } }
function viaB<T>(b: B<T>): T {
  return b.k.m()
}
class Box<T> {
  v: T
  constructor(v: T) {
    this.v = v
  }
  apply(s: { f: (x: T) => T }): T {
    return s.f(this.v)
  }
  map<U>(s: { f: (x: T) => U }): U {
    return s.f(this.v)
  }
}
function pair<A, C>(s: { a: () => A; c: () => C }): string {
  return `${s.a()}:${s.c()}`
}
const n: { m: () => number } = { m: () => 3 }
console.log(first(n))
const d: { m: (x: string) => string } = { m: (x: string) => x + "!" }
console.log(idf(d, "a"))
const k: { m: () => number } = { m: () => 11 }
const b: B<number> = { k: k }
console.log(viaB(b))
const bx = new Box<number>(4)
const tri: { f: (x: number) => number } = { f: (x: number) => x * 3 }
console.log(bx.apply(tri))
const str: { f: (x: number) => string } = { f: (x: number) => `n${x}` }
console.log(bx.map(str))
const ab: { a: () => number; c: () => boolean } = { a: () => 1, c: () => true }
console.log(pair(ab))
