// `hasOwnProperty` / `propertyIsEnumerable` / `isPrototypeOf` used to
// dispatch ahead of everything, on the reasoning that no per-tag arm
// implements them so nothing could shadow them. Own properties can —
// and so can a class method, an array expando, a user prototype's
// override, and a program's write to %Object.prototype% itself.
// §10.1.8.1 resolves the name first and only then calls what it
// found; jumping the walk answered the native result instead, with
// nothing to catch.

const own: any = {
  hasOwnProperty: () => "own-h",
  propertyIsEnumerable: () => "own-p",
  isPrototypeOf: () => "own-i",
}
console.log(own.hasOwnProperty("x"), own.propertyIsEnumerable("x"), own.isPrototypeOf({}))

class C {
  hasOwnProperty(_k: any) {
    return "cls"
  }
}
console.log((new C() as any).hasOwnProperty("x"))

const parent: any = { hasOwnProperty: () => "proto" }
console.log((Object.create(parent) as any).hasOwnProperty("x"))

const arr: any = [1]
arr.hasOwnProperty = () => "arr"
console.log(arr.hasOwnProperty(0))

// The native answers still stand on every shape when nothing
// shadows, including the reified `.call` re-dispatch, which IS the
// body running and so keeps its place ahead of the walk.
const plain: any = { a: 1 }
console.log(plain.hasOwnProperty("a"), plain.hasOwnProperty("b"))
console.log(plain.propertyIsEnumerable("a"), plain.propertyIsEnumerable("b"))
class D {
  x = 1
}
const d: any = new D()
console.log(d.hasOwnProperty("x"), d.hasOwnProperty("y"))
console.log(([1, 2] as any).hasOwnProperty(0), ([1, 2] as any).hasOwnProperty(5))
console.log(("ab" as any).hasOwnProperty(0), (5 as any).hasOwnProperty("x"))
console.log((new Map() as any).hasOwnProperty("size"))
console.log(Object.prototype.hasOwnProperty.call(plain, "a"))
console.log(Object.prototype.propertyIsEnumerable.call(plain, "a"))
console.log(Object.prototype.isPrototypeOf.call(null, 5))
try {
  Object.prototype.hasOwnProperty.call(null, "x")
} catch (e: any) {
  console.log("nullish", e instanceof TypeError)
}

// A chain-resolved reified cell re-enters the dispatcher by mid with
// no name to probe under; that walk ends at the inherited surface,
// not at a TypeError for a method standing right there.
const chained: any = Object.create({ a: 1 })
console.log(chained.hasOwnProperty("a"), chained.propertyIsEnumerable("a"))

// A symbol key reaches ToPropertyKey step 2 unchanged.
const sk = Symbol("k")
const sym: any = { [sk]: 1 }
console.log(sym.hasOwnProperty(sk), sym.propertyIsEnumerable(sk))

// Last, because it changes the answer for every object after it: the
// program's own write to %Object.prototype% wins over the native
// surface on every receiver shape.
;(Object.prototype as any).isPrototypeOf = () => "patched-i"
console.log(({} as any).isPrototypeOf({}), ([1] as any).isPrototypeOf({}))
;(Object.prototype as any).hasOwnProperty = () => "patched-h"
console.log(({} as any).hasOwnProperty("x"), ([1] as any).hasOwnProperty(0))
console.log((new D() as any).hasOwnProperty("x"), ("ab" as any).hasOwnProperty(0))
