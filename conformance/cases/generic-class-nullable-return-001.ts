// A generic class method returning `T | undefined` (or a promise of T)
// answers the instantiated type, not the bare type parameter.
class Stack<T> {
  private items: T[] = []
  push(x: T) {
    this.items.push(x)
  }
  pop(): T | undefined {
    return this.items.pop()
  }
  async first(): Promise<T> {
    return this.items[0]
  }
}
const ss = new Stack<string>()
ss.push('a')
ss.push('b')
const x = ss.pop()
if (x !== undefined) console.log(x.toUpperCase())
console.log(ss.pop()?.toUpperCase(), ss.pop()?.length)
const ns = new Stack<number>()
ns.push(1.5)
ns.push(2)
const y = ns.pop()
console.log(y === undefined ? -1 : y + 0.5)
ns.first().then((v) => console.log(v * 2))
