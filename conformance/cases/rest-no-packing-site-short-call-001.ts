// §10.2.11 — a call may stop short of a variadic callee's fixed
// prefix: the positions it never reached bind undefined and the tail
// binds the empty array. The pass that packs a variadic call site
// only ever sees a bare-name callee, so three shapes reach the
// checker unpacked — a function VALUE, an object-literal method, and
// a `Member` call whose name several unrelated classes declare — and
// all three were refused for want of an argument the language does
// not ask for.

// a function value: the binding, not the declaration, is the callee
const g = (x, ...r) => "g:" + x + ":" + r.length
console.log(g(), g(1), g(1, 2), g(1, 2, 3))

const g2 = (x, y, ...r) => "g2:" + x + ":" + y + ":" + r.length
console.log(g2(), g2(1), g2(1, 2), g2(1, 2, 3))

// a declaration reached through a binding is still a value
function hd(x, ...r) {
  return "hd:" + x + ":" + r.length
}
const h = hd
console.log(h(), h(1), h(1, 2))

// an object-literal method
const o = {
  m(x, ...r) {
    return "o:" + x + ":" + r.length
  },
}
console.log(o.m(), o.m(1), o.m(1, 2))

// a name several unrelated classes declare: the receiver's class is
// only known at dispatch, so this call site is never packed either
class A {
  f(x, ...r) {
    return "A:" + x + ":" + r.length
  }
}
class B {
  f(p, q) {
    return "B:" + p + ":" + q
  }
}
console.log(new A().f(), new A().f(1), new A().f(1, 2))
console.log(new B().f(1, 2))

// two fixed positions on the shared name, only one supplied
class E {
  k(x, y, ...r) {
    return "E:" + x + ":" + y + ":" + r.length
  }
}
class F {
  k(p, q) {
    return "F"
  }
}
console.log(new E().k(), new E().k(1), new E().k(1, 2), new E().k(1, 2, 3))

// nothing fixed at all: the tail is empty and the call is legal
const z = (...r) => "z:" + r.length
console.log(z(), z(1), z(1, 2))

// the receiver is evaluated exactly once even when the call is short
let n = 0
function mkA() {
  n = n + 1
  return new A()
}
console.log(mkA().f(), n)
