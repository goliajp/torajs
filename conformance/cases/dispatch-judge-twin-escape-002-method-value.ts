class P {
  v: number
  constructor(v: number) {
    this.v = v
  }
  get(): number {
    return this.v + 1
  }
}
// a typed method value re-bound onto a foreign receiver
const p = new P(2)
const g = p.get
console.log(g.call({ v: 6 }))
const h: any = p.get
console.log(h.call({ v: 9 }))
