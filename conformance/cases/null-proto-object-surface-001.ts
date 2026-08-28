// A receiver whose [[Prototype]] chain never reaches
// %Object.prototype% inherits none of its surface. The READ channel
// has answered that correctly since the dynobj proto pair learned the
// null-proto flag; the CALL channel never asked, so
// `typeof o.toString` was undefined while `o.toString()` answered
// "[object Object]" — the two faces of one name disagreeing.

const o: any = Object.create(null)
console.log(typeof o.toString, typeof o.hasOwnProperty, o.x)

for (const call of [
  () => o.toString(),
  () => o.valueOf(),
  () => o.toLocaleString(),
  () => o.hasOwnProperty("x"),
  () => o.propertyIsEnumerable("x"),
  () => o.isPrototypeOf({}),
]) {
  try {
    call()
    console.log("no throw")
  } catch (e: any) {
    console.log(e instanceof TypeError)
  }
}

// An own entry is still the receiver's own, and still shadows
// nothing because there is nothing above it to shadow.
o.f = function () {
  return 1
}
o.toString = () => "own"
console.log(o.f(), String(o), o.toString())

// The chain is walked, not just the receiver: a child of a
// null-proto object is off the chain too.
const child: any = Object.create(Object.create(null))
try {
  child.hasOwnProperty("x")
  console.log("no throw")
} catch (e: any) {
  console.log("child", e instanceof TypeError)
}

// `setPrototypeOf(o, null)` reaches the same state as
// `Object.create(null)`.
const cut: any = { a: 1 }
Object.setPrototypeOf(cut, null)
try {
  cut.hasOwnProperty("a")
  console.log("no throw")
} catch (e: any) {
  console.log("cut", e instanceof TypeError)
}

// Ordinary objects are unchanged — the chain reaches the root
// through an implicit parent, a user one, and a builtin family
// prototype alike.
console.log(({ a: 1 } as any).hasOwnProperty("a"), ({} as any).toString())
console.log((Object.create({ b: 1 }) as any).hasOwnProperty("b"))
console.log((Object.create([1]) as any).hasOwnProperty("0"))
class C {
  x = 1
}
console.log((Object.create(C.prototype) as any).hasOwnProperty("x"))
console.log(Object.prototype.toString(), Object.prototype.hasOwnProperty("toString"))

// `Object.prototype.<m>.call(o, ...)` is not a lookup on `o` — it
// names the function it wants — so a null prototype does not put it
// out of reach. The AST rewrite of `X.prototype.m.call(recv, ...)`
// into `recv.m(...)` had made it one.
console.log(Object.prototype.hasOwnProperty.call(o, "f"))
console.log(Object.prototype.propertyIsEnumerable.call(o, "f"))
console.log(Object.prototype.valueOf.call(o) === o)
console.log(Object.prototype.toString.call(o))
console.log(Object.prototype.isPrototypeOf.call(o, child))

// The same rewrite let the receiver's OWN member shadow the very
// function being called.
const shadowing: any = {
  a: 1,
  hasOwnProperty: () => "own",
  propertyIsEnumerable: () => "own",
  valueOf: () => "own",
}
console.log(Object.prototype.hasOwnProperty.call(shadowing, "a"))
console.log(Object.prototype.propertyIsEnumerable.call(shadowing, "a"))
console.log(Object.prototype.valueOf.call(shadowing) === shadowing)

// The built-in null-prototype objects the spec mints: a match
// result's `groups` (§22.2.7.2) and Array.prototype's unscopables
// list (§23.1.3.35).
const m: any = "abc".match(/(?<g>b)/)
console.log(Object.getPrototypeOf(m.groups), Object.prototype.hasOwnProperty.call(m.groups, "g"))
const un: any = (Array.prototype as any)[Symbol.unscopables]
console.log(Object.getPrototypeOf(un), Object.prototype.hasOwnProperty.call(un, "flat"))
