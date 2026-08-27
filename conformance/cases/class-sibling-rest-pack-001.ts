// A rest parameter is ONE array, and every ordinary call site builds
// it at AST level: `apply_rest_args` rewrites `f(a, b, c)` into
// `f(a, [b, c])`. That pass only ever sees an `Ident` callee, and a
// `Member`-shape call survives desugar whenever several unrelated
// classes declare the name — the receiver's class is not known until
// sibling-class dispatch resolves it at SSA level. So that one lane
// handed a rest-declaring body its trailing arguments one per
// register and the parameter read a scalar as an array pointer:
// every shape below used to be `exit 139`.
class A {
  f(x, ...r) {
    return "A" + x + r.length
  }
}
class D {
  f(p, q) {
    return "D" + p * q
  }
}
console.log(new A().f(1, 2, 3), new A().f(1), new D().f(4, 2))

// both siblings variadic, and one of them reached polymorphically
class P {
  g(x, ...r) {
    return "P" + r.length
  }
}
class Q {
  g(x, ...s) {
    return "Q" + s.length
  }
}
console.log(new P().g(1, 2, 3), new Q().g(4, 5))

// a hierarchy whose slot takes a tail, with an unrelated class
// declaring the same name — the vtable branch of the same lane
class E {
  m(x, ...r) {
    return "E" + r.length
  }
}
class F extends E {
  m(x, ...r) {
    return "F" + r.length
  }
}
class Unrelated {
  m(p, q) {
    return "U"
  }
}
const es: E[] = [new E(), new F()]
for (const e of es) console.log(e.m(1, 2, 3))

// a TYPED tail converts through the assign-boundary kernel rather
// than being read as the wrong thing
class T {
  h(x: number, ...r: number[]) {
    return x + r.length
  }
}
class U {
  h(p: number, q: number) {
    return p * q
  }
}
console.log(new T().h(1, 2, 3), new U().h(4, 2))

// a spread inside the tail is still the tail
const more: any[] = [7, 8]
console.log(new A().f(1, ...more))

// a name owned by a variadic row is not padded by an unrelated
// class's default: what the name-keyed table would supply there is
// not the omitted argument the language owes but an extra one, and
// it lands in the tail
class V {
  k(x, ...r) {
    return "V" + r.length
  }
}
class W {
  k(p, q = 3) {
    return "W" + p * q
  }
}
console.log(new V().k(1), new W().k(4))

// the any lane keeps answering the same thing it always did
const a: any = new A()
console.log(a.f(1, 2, 3))
