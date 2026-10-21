class P {
  v: number
  constructor(v: number) {
    this.v = v
  }
  get(): number {
    return this.v + 1
  }
}
// printing and serializing an instance
const p = new P(16)
console.log(p)
console.log(JSON.stringify(p))
console.log(String(p.get()))
