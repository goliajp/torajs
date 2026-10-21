class P {
  v: number
  constructor(v: number) {
    this.v = v
  }
  get(): number {
    return this.v + 1
  }
}
// an instance in an any slot hands out its method faces
const p = new P(1)
const a: any = p
console.log(a.get())
const f = a.get
console.log(f.call({ v: 41 }))
console.log(p.get())
