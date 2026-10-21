class P {
  v: number
  constructor(v: number) {
    this.v = v
  }
  get(): number {
    return this.v + 1
  }
}
// an instance captured by a closure that reaches the any world
const p = new P(12)
const c: any = () => p
console.log(c().get.call({ v: 13 }))
const m = new Map<string, P>()
m.set('k', new P(14))
const am: any = m
console.log(am.get('k').get.call({ v: 15 }))
