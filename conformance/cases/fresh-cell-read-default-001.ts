// A ctor that reads a field before writing it sees the declared
// default, and every later read sees the last write.
class Counter {
  kind: number = 0
  total: number = 10
  label: string = "n"
  constructor(step: number) {
    this.kind = this.kind + 1
    this.total = this.total + this.kind * step
    this.label = this.label + this.kind
    this.kind = this.kind + 1
  }
}
const c = new Counter(5)
console.log(c.kind, c.total, c.label)
class Pair {
  first: number = 1
  second: number = 2
  constructor() {
    const t = this.first
    this.first = this.second
    this.second = t
  }
}
const q = new Pair()
console.log(q.first, q.second)
class Loop {
  n: number = 0
  constructor(count: number) {
    for (let i = 0; i < count; i = i + 1) this.n = this.n + i
  }
}
console.log(new Loop(4).n, new Loop(0).n)
