// `a?.x` on a class instance answered "no field `x` accessible on type
// ClassRef" — about a field that is right there. The shim OptChain
// reads member types through has claimed since it was pulled out that
// it reuses the Member arm's resolution; for a class it never did,
// because the class ladder grew up next door and this one was never
// told about it.
class Point {
  x = 1
  y = 2
}
const p = new Point()
console.log(p?.x, p?.y)

const annotated: Point = new Point()
console.log(annotated?.x)

// the nullable spelling, on both sides of the short circuit
const maybe: Point | null = new Point()
console.log(maybe?.x)
const nothing: Point | null = null
console.log(nothing?.x)

// a chain of them
class Inner {
  v = 7
}
class Outer {
  inner: Inner | null = new Inner()
}
const o = new Outer()
console.log(o?.inner?.v)
const empty = new Outer()
empty.inner = null
console.log(empty?.inner?.v)

// an inherited field
class Base {
  b = 3
}
class Derived extends Base {
  d = 4
}
const derived = new Derived()
console.log(derived?.b, derived?.d)

// a field holding a class instance keeps its class, so the next `?.`
// still resolves
class Holder {
  point = new Point()
}
console.log(new Holder()?.point?.x)

// the field read is still a read: it does not evaluate twice
let evals = 0
function make(): Point {
  evals = evals + 1
  return new Point()
}
console.log(make()?.x, evals)
