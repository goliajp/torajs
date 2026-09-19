// instanceof / in / for-in follow a re-parented struct's user chain.
class C { m() { return 1; } get g() { return 1; } }
class D { dm() { return 2; } }
const c: any = new C();
Object.setPrototypeOf(c, D.prototype);
console.log(c instanceof D, c instanceof C, "dm" in c, "m" in c, "g" in c);
Object.setPrototypeOf(c, C.prototype);
console.log(c instanceof D, c instanceof C, "dm" in c, "m" in c, "g" in c);

const o = { own: 1 };
const p = { inh: 2 };
Object.setPrototypeOf(o, p);
const ks: string[] = [];
for (const k in o) ks.push(k);
console.log(ks.join(","), "inh" in o, "toString" in o);
Object.setPrototypeOf(o, null);
console.log("toString" in o, "own" in o, "inh" in o);

const q = { a: 1 };
const r = { b: 2 };
const s = { c: 3 };
Object.setPrototypeOf(r, s);
Object.setPrototypeOf(q, r);
const ks2: string[] = [];
for (const k in q) ks2.push(k);
console.log(ks2.join(","), "c" in q, (q as any).c);
