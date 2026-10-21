class P {
  v: number
  constructor(v: number) {
    this.v = v
  }
  get(): number {
    return this.v + 1
  }
}
// an instance stored in an any array and in a typed array boxed later
const arr: any[] = [new P(3)]
console.log(arr[0].get(), arr[0].get.call({ v: 2 }))
const ps: P[] = [new P(4)]
const view: any = ps
console.log(view[0].get.call({ v: 20 }))
