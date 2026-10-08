// a receiver typed by the all-any instance of a generic base class
// dispatches to the override its instance's class declares
class Base<T> {
  v: T
  constructor(v: T) { this.v = v }
  get(): T { return this.v }
}
class I extends Base<number> { get(): number { return this.v + 1 } }
class S extends Base<string> { get(): string { return this.v + "!" } }
function show(z: Base<any>): any { return z.get() }
const b: Base<any> = new I(1)
const c: Base<any> = new S("a")
console.log(b.get(), c.get(), show(new I(3)), show(new S("x")))
const xs: Base<any>[] = [new I(1), new S("a")]
for (const x of xs) console.log(x.get())
const a: Base<number> = new I(5)
console.log(a.get())
