// `a?.m` on a class instance — the chain read as a VALUE.
//
// The shim OptChain resolves member types through only ever knew about
// the struct a class instance IS. What a class keeps on its prototype —
// its methods — is not in that struct, and neither is a name nobody
// declared. The ladder next door has always answered both with `Any`,
// which is why `a.m` and `a.nosuch` work; only the chain spelling
// refused them, and then the lowering stopped at a struct that was
// never going to hold the answer.

class A {
  x = 1
  m(q) {
    return "m:" + q
  }
}

const a: A | null = new A()

// a declared field still answers its own type
console.log(a?.x)

// a method read as a value
console.log(typeof a?.m)
const f = a?.m
console.log(typeof f)

// a name nobody declared is undefined, exactly as the plain read says
console.log(a?.nosuch)
const plain = new A()
console.log(plain.nosuch, typeof plain.m)

// a nullish base still short-circuits the whole thing
const nil: A | null = null
console.log(nil?.x, nil?.m, nil?.nosuch)

// inherited members come from the chain the class actually has
class B extends A {
  y = 2
  n() {
    return "n"
  }
}
const b: B | null = new B()
console.log(b?.x, b?.y, typeof b?.m, typeof b?.n)

// a field holding an instance, then a chain off it
class Holder {
  inner: A | null = new A()
}
const h: Holder | null = new Holder()
console.log(h?.inner?.x, typeof h?.inner?.m)

// the base is evaluated exactly once for a value read too
let made = 0
function mk(): A {
  made = made + 1
  return new A()
}
console.log(mk()?.x, made)

// a getter reached through the chain
class G {
  get val() {
    return 9
  }
}
const g: G | null = new G()
console.log(g?.val)
