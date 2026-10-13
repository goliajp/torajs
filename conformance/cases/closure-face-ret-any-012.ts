// A receiver the checker types `any` whose classes the return walk
// cannot name from an annotation on the receiver itself: a union under
// an alias, the answer of a function declared to return a union, a
// binding initialized with one, `?.` on a union. Its members are the
// ones each class declares, here `any`, and a function declared
// `number` hands back what they hold as is.
class A { v: any = "a"; m(): any { return "am"; } }
class B { v: any = "b"; m(): any { return "bm"; } }
type AB = A | B;
function f1(o: AB): number { return o.m(); }
function f2(o: AB): number { return o.v; }
function pick(n: number): A | B { return n > 0 ? new A() : new B(); }
function f3(): number { return pick(1).m(); }
function f4(): number { const o = pick(-1); return o.v; }
function h1(o: A | B): number { return o?.m(); }
function h2(o: A | B | undefined): number { return o?.v; }
function f5(os: (A | B)[]): number { return os[0].m(); }
console.log(f1(new A()), f2(new B()), f3(), f4(), h1(new A()), h2(new B()), f5([new B()]));
