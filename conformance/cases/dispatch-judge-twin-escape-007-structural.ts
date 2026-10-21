class P {
  v: number
  constructor(v: number) {
    this.v = v
  }
  get(): number {
    return this.v + 1
  }
}
// an instance seen through a structural type, then boxed
type HasV = { v: number }
function rd(h: HasV): any {
  return h
}
const viaType: any = rd(new P(8))
console.log(viaType.get(), viaType.get.call({ v: 0 }))
