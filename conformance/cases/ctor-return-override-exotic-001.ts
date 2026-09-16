// §10.2.2 step 13 on classes whose constructor CAN return a value:
// the object the factory hands back must stay alive and keep its
// class identity — including exotic builtin subclasses.
let flag = false;
class A2 extends Array<number> {
  constructor() {
    super();
    if (flag) return {} as any;
  }
}
class P4 extends Promise<unknown> {
  constructor(ex: any) {
    super(ex);
    if (flag) return {} as any;
  }
}
class M4 extends Map<string, number> {
  constructor() {
    super();
    if (flag) return {} as any;
  }
}
class B0 {
  s = "x";
  n = 1;
  constructor() {
    if (flag) return {} as any;
  }
}
class Prim {
  t = "y";
  constructor() {
    return 5 as any;
  }
}
class Base {
  constructor(o: any) {
    return o;
  }
}
class Der extends Base {
  k = 3;
  constructor(o: any) {
    super(o);
  }
}
const a = new A2();
a.push(1, 2);
console.log("A2", a instanceof A2, Object.getPrototypeOf(a) === A2.prototype, a.length);
const p = new P4((r: any) => r(7));
console.log("P4", (p as any) instanceof P4);
const m = new M4();
m.set("k", 1);
console.log("M4", m instanceof M4, m.get("k"));
let acc = 0;
for (let i = 0; i < 2000; i++) {
  const b = new B0();
  const q = new Prim();
  const d = new Der({ i });
  acc += b.n + (q as any).t.length + (d as any).k + (d as any).i;
}
console.log("loop", acc);
const d = new Der({ z: 1 });
console.log("adopted", (d as any).z, (d as any).k, d instanceof Der);
flag = true;
console.log("override taken", new A2() instanceof A2, new B0() instanceof B0);
p.then((v) => console.log("P4 resolved", v));
