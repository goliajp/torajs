// A field default store followed by a write through a call boundary:
// the callee observes the default, the caller's write lands after it.
const seen: number[] = []
class Probe {
  v: number = 7
  w: number = 0
  constructor(n: number) {
    peek(this)
    this.v = n
    peek(this)
    this.w = this.v + 1
  }
}
function peek(o: Probe): void {
  seen.push(o.v)
}
const p = new Probe(9)
console.log(seen.join(","), p.v, p.w)
class Chain {
  head: number = 1
  constructor(n: number) {
    this.head = n
    bump(this)
    this.head = this.head + 1
  }
}
function bump(c: Chain): void {
  c.head = c.head * 10
}
console.log(new Chain(2).head)
let count = 0
class Tick {
  a: number = 0
  b: number = 0
  constructor() {
    this.a = tick()
    this.b = tick()
    this.a = this.a + this.b
  }
}
function tick(): number {
  count = count + 1
  return count
}
const t = new Tick()
console.log(t.a, t.b, count)
