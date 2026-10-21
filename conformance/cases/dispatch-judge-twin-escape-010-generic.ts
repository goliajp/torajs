// a generic class's rows
class Box<T> {
  t: T
  constructor(t: T) {
    this.t = t
  }
  get(): T {
    return this.t
  }
}
const b = new Box<number>(3)
console.log(b.get())
const ab: any = new Box<string>('s')
console.log(ab.get(), ab.get.call({ t: 'foreign' }))
