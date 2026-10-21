class P {
  v: number
  constructor(v: number) {
    this.v = v
  }
  get(): number {
    return this.v + 1
  }
}
// a subclass instance escapes: the parent's faces are reachable too
class Q extends P {
  w: number
  constructor() {
    super(10)
    this.w = 2
  }
  both(): number {
    return this.v + this.w
  }
}
const q: any = new Q()
console.log(q.both(), q.get())
const parent: any = Object.getPrototypeOf(Object.getPrototypeOf(q))
console.log(parent.get.call({ v: 50 }))
console.log(q.both.call({ v: 1, w: 1 }))
