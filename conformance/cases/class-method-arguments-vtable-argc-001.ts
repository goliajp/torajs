// An overridden method that reads `arguments.length` answers the count
// of its call (§10.4.4.6) on every lane that reaches it: the vtable
// slot, a direct call, `super.m`, the any lane and a rebound method
// value. Rows that never read `arguments` share the slot unchanged.

class A {
  m(x?: number) {
    return arguments.length;
  }
  k(x: number, y: number = 2) {
    return "A" + arguments.length + ":" + y;
  }
  plain(x?: number) {
    return "A" + x;
  }
}
class B extends A {
  m(x?: number) {
    return 10 + arguments.length;
  }
  k(x: number, y: number = 2) {
    return "B" + arguments.length + ":" + y + "/" + super.k(x);
  }
  s() {
    return super.m() + "," + super.m(1);
  }
  plain(x?: number) {
    return "B" + arguments.length;
  }
}
class C extends B {
  m(x?: number) {
    return 100 + arguments.length + super.m(x);
  }
}

const a: A = new B();
console.log(a.m(), a.m(1));
const b = new B();
console.log(b.m(), b.m(1), b.s());
const c: A = new C();
console.log(c.m(), c.m(1));
console.log(a.k(1), a.k(1, 5), b.k(7));
const r: any = a;
console.log(r.m(), r.m(1, 2, 3), r.k(1, 2, 3));
console.log(a.plain(), a.plain(1), new A().plain(2));
const f = a.m;
console.log(f.call(a, 1, 2), (f as any).call(a), (c as any).m(1, 2, 3));
const all: A[] = [new A(), new B(), new C()];
for (const e of all) {
  console.log(e.m(), e.m(4));
}

// an unrelated class declaring the same name shares the slot's index
class P {
  h(a?: number) {
    return "P" + arguments.length;
  }
}
class Q extends P {
  h(a?: number) {
    return "Q" + arguments.length;
  }
}
class Z {
  h(a?: number) {
    return "Z" + arguments.length;
  }
}
const ps: P[] = [new P(), new Q()];
for (const p of ps) {
  console.log(p.h(), p.h(5));
}
console.log(new Z().h(), new Z().h(1), (new Z() as any).h(1, 2));
