// rotation 497 — struct-layout interning must only match FINALIZED
// layouts. A zero-field class declared ahead of another class used to
// intern onto its neighbour's still-empty reserved slot, which the
// neighbour then filled: `new A()` wore B's layout (printed `v: 0`,
// Object.keys answered B's names, and the runtime drop walked B's
// child offsets over A's smaller block — the iterator-helpers SIGBUS
// behind the injection-reachability gate).
class A {}
class B { v = 7; w: any = "s" }
const a = new A();
const b = new B();
console.log(a);
console.log(b);
console.log(Object.keys(a), Object.keys(b));
const a2: any = new A();
a2.dyn = 1;
console.log(a2);

// two genuinely empty classes still intern onto one layout — and
// stay empty
class E1 {}
class E2 {}
console.log(new E1(), new E2(), Object.keys(new E2()));

// zero-field class between two fielded ones
class P { p = 1 }
class Z {}
class Q { q = "q" }
console.log(new P(), new Z(), new Q());
