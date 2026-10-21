class P {
  v: number
  constructor(v: number) {
    this.v = v
  }
  get(): number {
    return this.v + 1
  }
}
// the class itself as a value
const K: any = P
console.log(K.prototype.get.call({ v: 99 }))
const made = new K(7)
console.log(made.get())
