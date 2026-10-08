// an interface inherits the members of the interfaces it extends
interface A { a: number }
interface B extends A { b: string }
const x: B = { a: 1, b: "s" }
console.log(x.a, x.b)
function f(v: A) { return v.a }
console.log(f(x))
interface Box<T> { v: T }
interface Labeled<T> extends Box<T>, A { label: string }
const l: Labeled<string> = { v: "val", a: 2, label: "L" }
console.log(l.v.toUpperCase(), l.a + 1, l.label)
interface C extends B { a: number; c: boolean }
const c: C = { a: 3, b: "t", c: true }
console.log(c.a, c.b, c.c, f(c))
class Impl { a = 9; b = "impl" }
const asB: B = new Impl()
console.log(asB.a, asB.b)
