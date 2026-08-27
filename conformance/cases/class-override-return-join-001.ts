// 508-06 — one vtable slot, one return type.
//
// A base that answers a value and an override that does not are not
// in conflict about the language: falling out of a body is `return
// undefined` (§10.2.1.4 step 11). They are in conflict about the
// machine — one answers in a register, the other in none — and a
// call through the slot is emitted with a single body's signature.
// The slot's type is their join.
class A {
  f() { return 1 }
}
class B extends A {
  f() { console.log("b") }
}
const xs: A[] = [new A(), new B()]
for (const x of xs) console.log(x.f())

// The same rule where the rows disagree about which register, not
// whether: string in a word, number in a float.
class C {
  g() { return "s" }
}
class D extends C {
  g() { return 42 }
}
const ys: C[] = [new C(), new D()]
for (const y of ys) console.log(y.g())

// Rows that agree keep their narrow slot — nothing to join.
class E {
  m() { return 1 }
}
class F extends E {
  m() { return 2 }
}
const zs: E[] = [new E(), new F()]
for (const z of zs) console.log(z.m())

// An unrelated declarer shares the slot INDEX but no call site sees
// both rows, so it keeps its own return type.
class Alone {
  m() { return "alone" }
}
console.log(new Alone().m())
