// A declared array field with no initializer holds undefined until the
// constructor writes it (§10.2.1.3): reading through it first throws
// TypeError. The factory used to seed an empty array, so the read
// answered 0 and `=== undefined` answered false.
class A {
  xs: number[]
  n: number
  constructor() {
    try {
      console.log(this.xs.length)
    } catch (e) {
      console.log('caught', e instanceof TypeError)
    }
    console.log(this.xs === undefined, typeof this.xs)
    this.xs = [1, 2]
    this.n = this.xs.length
  }
}
const a = new A()
console.log(a.n, a.xs.join(','))

class B {
  xs: string[]
  ys: string[] = []
  constructor(s: string) {
    const f = () => this.xs.length
    try {
      console.log(f(), this.ys.length)
    } catch (e) {
      console.log('B caught', e instanceof TypeError)
    }
    this.xs = [s]
    console.log(f(), this.ys.length)
  }
}
console.log(new B('q').xs[0])

// a field the constructor writes first, and one with an initializer,
// read as before
class C {
  items: number[]
  seen: number[] = [7]
  constructor(n: number) {
    this.items = []
    for (let i = 0; i < n; i = i + 1) this.items.push(i * i)
  }
  sum(): number {
    let t = 0
    for (const v of this.items) t = t + v
    return t + this.seen[0]
  }
}
console.log(new C(4).sum())
