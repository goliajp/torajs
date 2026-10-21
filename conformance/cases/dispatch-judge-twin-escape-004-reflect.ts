class P {
  v: number
  constructor(v: number) {
    this.v = v
  }
  get(): number {
    return this.v + 1
  }
}
// reflection on an instance: keys, prototype, constructor
class R {
  n: number
  constructor() {
    this.n = 5
  }
  twice(): number {
    return this.n * 2
  }
}
const r = new R()
console.log(Object.keys(r).join(','), r.twice())
const proto: any = Object.getPrototypeOf(new P(0))
console.log(proto.get.call({ v: 30 }))
const ctor: any = new P(0).constructor
console.log(ctor.name, ctor.prototype.get.call({ v: 1 }))
