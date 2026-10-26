// A function an object literal of an interface whose method returns an
// any value holds in that member keeps its own return, and is also
// called through a binding annotated with its function type. The
// literal's member is called through the interface, so its typed answer
// reaches a caller reading the member as any: fractions, strings and
// objects come back as they are.
const raw: any = JSON.parse('{"x":1.5,"s":"ab","o":{"k":3}}');
interface J {
  m(): number;
  n: number;
}
class S implements J {
  v: any;
  n = 1;
  constructor(v: any) {
    this.v = v;
  }
  m(): number {
    return this.v.x;
  }
}
class T implements J {
  v: any;
  n = 2;
  constructor(v: any) {
    this.v = v;
  }
  m(): number {
    return this.v.s;
  }
}
class U implements J {
  v: any;
  n = 3;
  constructor(v: any) {
    this.v = v;
  }
  m(): number {
    return this.v.o;
  }
}
function call(o: J) {
  return o.m();
}
function g(): number {
  return 2.5;
}
const o: J = { m: g, n: 1 };
const k: () => number = g;
console.log(call(o), call(new S(raw)), call(new T(raw)), JSON.stringify(call(new U(raw))), k(), g() + 1);
