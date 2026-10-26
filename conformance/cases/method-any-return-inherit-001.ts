// Overrides share one return slot: when one of them returns an any
// value, a call through the base type, through an array of the base
// type and through an abstract method all hand that value back as it
// is, while the overrides that return numbers keep answering them.
const raw: any = JSON.parse('{"x": 1.5, "y": "s"}');

class A {
  c: any;
  constructor(v: any) {
    this.c = v;
  }
  m(): number {
    return 7;
  }
}
class B extends A {
  m(): number {
    return this.c.x;
  }
}
class D extends A {
  m(): number {
    return this.c.y;
  }
}
class E extends B {}
const xs: A[] = [new A(raw), new B(raw), new D(raw), new E(raw)];
for (const a of xs) console.log(a.m());
function call(a: A): number {
  return a.m();
}
console.log(call(new B(raw)), call(new D(raw)), call(new A(raw)));
const sum: number = new B(raw).m() + new A(raw).m();
console.log(sum);

abstract class S {
  abstract m(): number;
  twice(): string {
    return String(this.m()) + "|" + String(this.m());
  }
}
class T extends S {
  constructor(private v: any) {
    super();
  }
  m(): number {
    return this.v.y;
  }
}
class U extends S {
  m(): number {
    return 2;
  }
}
console.log(new T(raw).twice(), new U().twice());
