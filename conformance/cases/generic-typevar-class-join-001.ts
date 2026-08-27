// A type parameter given a subclass and its ancestor was given one
// answer, not two: every Y is an X, so `T` is X. tr used to refuse the
// second argument ("inferred as X earlier but here is Y"), which is
// what the test262 harness's `sameValue<T>(a: T, b: T)` runs into as
// soon as a method's declared return type is its own class.
class X {
  method() {
    return this
  }
}
class Y extends X {
  method() {
    return super.method()
  }
}

function sameValue<T>(a: T, b: T) {
  return a === b
}

const y = new Y()
console.log(sameValue(y.method(), y))
// the other call order binds the same T
console.log(sameValue(y, y.method()))

// three deep, and through the ancestor's own type
class Z extends Y {}
const z = new Z()
console.log(sameValue(z, z.method()))
const asX: X = z
console.log(sameValue(asX, z))
