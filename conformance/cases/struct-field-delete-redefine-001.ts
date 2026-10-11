// A define of a declared field that `delete` removed creates the
// property again with the attributes it names (§10.1.6.3 step 2).
class C {
  x = 1
  y = 2
}
const c = new C()
delete (c as any).x
console.log(Object.getOwnPropertyDescriptor(c, "x"))
Object.defineProperty(c, "x", { value: 7, enumerable: true, writable: true, configurable: true })
console.log("x" in c, c.x, Object.getOwnPropertyDescriptor(c, "x"))
const d = new C()
delete (d as any).y
Object.defineProperty(d, "y", { value: 5, enumerable: false, writable: false, configurable: false })
console.log(d.y, Object.getOwnPropertyDescriptor(d, "y"), Object.keys(d))
try {
  ;(d as any).y = 6
} catch (e) {
  console.log("readonly", (e as Error).constructor.name)
}
console.log(d.y)
// a non-extensible object refuses to create the deleted field again
// (§10.1.6.3 step 2.a)
const e = new C()
delete (e as any).x
Object.preventExtensions(e)
try {
  Object.defineProperty(e, "x", { value: 1 })
} catch (err) {
  console.log("not extensible", (err as Error).constructor.name)
}
console.log(Object.getOwnPropertyDescriptor(e, "x"), Reflect.defineProperty(e, "x", { value: 1 }), Object.keys(e))
const f = new C()
delete (f as any).y
Object.freeze(f)
try {
  Object.defineProperty(f, "y", { value: 1 })
} catch (err) {
  console.log("frozen", (err as Error).constructor.name)
}
console.log(Object.getOwnPropertyDescriptor(f, "y"), Object.keys(f))
