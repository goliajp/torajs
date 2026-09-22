// §23.1.1.1 step 5 — `super(v)` in an Array subclass with ONE argument:
// a Number is the length (RangeError unless it is a uint32), anything
// else is the array's only element. No ToNumber runs on an object, so
// its valueOf is never called.
class A extends Array {
  constructor(n: any) {
    super(n);
  }
}
const log: string[] = [];
const q: any = { valueOf() { log.push("valueOf"); return 2; } };
const a = new A(q);
console.log(a.length, a[0] === q, log.length);
const b = new A({ k: 1 } as any);
console.log(b.length, JSON.stringify(b));
const c = new A("s" as any);
console.log(c.length, c[0]);
const d = new A(true as any);
console.log(d.length, d[0]);
const e = new A(3 as any);
console.log(e.length, e[0]);
try {
  new A(-1 as any);
} catch (err) {
  console.log((err as any).constructor.name);
}
console.log(a instanceof A, Array.isArray(a));
