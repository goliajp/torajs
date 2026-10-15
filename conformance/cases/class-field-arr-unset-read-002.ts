// More reads of a declared array field before anything writes it: from
// a method, an index store, a for-of, a subclass that writes it after
// super(), and JSON.stringify, which leaves the undefined field out.
class A {
  xs: number[]
  add(v: number): string {
    try {
      this.xs.push(v)
      return 'added'
    } catch (e) {
      return 'add ' + (e instanceof TypeError)
    }
  }
  put(): string {
    try {
      this.xs[0] = 1
      return 'put'
    } catch (e) {
      return 'put ' + (e instanceof TypeError)
    }
  }
  walk(): string {
    try {
      let t = 0
      for (const v of this.xs) t = t + v
      return 'walked ' + t
    } catch (e) {
      return 'walk ' + (e instanceof TypeError)
    }
  }
}
const a = new A()
console.log(a.add(3), a.put(), a.walk())
console.log(JSON.stringify(a), Object.keys(a).join(','), a.xs === undefined)
a.xs = [5, 6]
console.log(a.add(7), a.walk(), JSON.stringify(a))

class Base {
  tag: string = 'b'
  constructor() {
    console.log('base sees', typeof (this as any).ys)
  }
}
class Sub extends Base {
  ys: string[]
  constructor() {
    super()
    let n = -1
    try {
      n = this.ys.length
    } catch (e) {
      console.log('sub caught', e instanceof TypeError)
    }
    console.log(n, JSON.stringify(this))
    this.ys = ['p']
    console.log(this.ys.length, JSON.stringify(this))
  }
}
new Sub()
