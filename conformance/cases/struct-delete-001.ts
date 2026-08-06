// §10.1.10 OrdinaryDelete over a class-instance receiver: the `+24`
// expando dict deletes like any other, and a non-configurable own
// property — expando or declared field — refuses with the strict
// TypeError rather than answering false in silence.

class C {
  x: number = 1;
}

// An expando is an ordinary own property of the instance.
const a: any = new C();
a.y = 5;
a.z = 6;
console.log(a.y, Object.keys(a).join(","));
console.log(delete a.y);
console.log(a.y, Object.keys(a).join(","));
console.log("y" in a, a.hasOwnProperty("z"));

// Deleting a key that was never there is a spec success, with or
// without a dict on the cell.
console.log(delete a.never, delete (new C() as any).never);

// A defineProperty'd expando can be non-configurable; module code is
// strict, so the refusal throws.
const b: any = new C();
Object.defineProperty(b, "w", { value: 9, configurable: false });
try {
  delete b.w;
  console.log("no throw");
} catch (e) {
  console.log("threw", (e as Error).constructor.name, b.w);
}

// Same sentence for a DECLARED field demoted to non-configurable.
const c: any = new C();
Object.defineProperty(c, "x", { configurable: false });
try {
  delete c.x;
  console.log("no throw");
} catch (e) {
  console.log("threw", (e as Error).constructor.name, c.x);
}

// §7.3.14 SetIntegrityLevel moves the same default: a sealed or
// frozen instance has no configurable field left.
const d: any = new C();
Object.seal(d);
try {
  delete d.x;
  console.log("no throw");
} catch (e) {
  console.log("threw", d.x);
}

const e: any = new C();
Object.freeze(e);
try {
  delete e.x;
  console.log("no throw");
} catch (err) {
  console.log("threw", e.x);
}

// A symbol-keyed own property lives in the same dict.
const s = Symbol("s");
const f: any = new C();
f[s] = 3;
console.log(f[s], delete f[s], f[s]);
