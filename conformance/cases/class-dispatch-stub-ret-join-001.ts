// 508-06 — the `__dispatch_<M>` stub is the third thing in a slot.
//
// It is lowered as a tag-switch over every owner's body and is what a
// chain method's Member-shape call sites enter, so it shares the slot
// as surely as any row. Its own return annotation came from the base
// owner's DECLARATION, read before `desugar_implicit_generics` gave
// the unannotated bodies theirs — so it could disagree with rows that
// all agreed with each other, and nothing compared it to them.
// Probe at the join's off position: `__dispatch_f` was `ww` while
// `__cm_A__f` was `wf`.
class A {
  f() { return 1.5 }
}
class B extends A {
  f() { return 2.5 }
}
class C extends B {
  f() { return 3.5 }
}
const xs: A[] = [new A(), new B(), new C()]
for (const x of xs) console.log(x.f())

// The same with a value/void split among the rows, so the stub joins
// with a slot that was already widening.
class P {
  g() { return 7 }
}
class Q extends P {
  g() { console.log("q") }
}
const ys: P[] = [new P(), new Q()]
for (const y of ys) console.log(y.g())
