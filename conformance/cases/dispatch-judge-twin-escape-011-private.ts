class P {
  v: number
  constructor(v: number) {
    this.v = v
  }
  get(): number {
    return this.v + 1
  }
}
// instances that never leave the typed lanes keep working with the
// twins gone, beside another class whose instances do escape
class Counter {
  n: number
  constructor() {
    this.n = 0
  }
  bump(): number {
    this.n = this.n + 1
    return this.n
  }
}
const c = new Counter()
c.bump()
const p = new P(c.bump())
console.log(p.get(), c.bump())
class Leaky {
  s: string
  constructor() {
    this.s = 'x'
  }
  say(): string {
    return this.s + '!'
  }
}
const l: any = new Leaky()
console.log(l.say(), l.say.call({ s: 'y' }))
