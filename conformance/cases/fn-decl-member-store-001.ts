// A function declaration stored through a member write is a function
// object in every receiver shape. Through `globalThis`, an `as any`
// receiver, a nested member or a call result the store was a
// compile-time reject; into a struct field typed `any` the later call
// threw "value is not a function"; replacing a function-typed struct
// or class field with another declaration crashed the program.
function Mercury(x: number): number { return x + 1; }
function Venus(x: number): number { return x + 2; }

globalThis.Mercury2 = Mercury;
console.log(typeof (globalThis as any).Mercury2, (globalThis as any).Mercury2(1));

const m = new Map<string, number>();
(m as any).f = Mercury;
console.log((m as any).f(2));

const w: any = { in: {} };
w.in.f = Venus;
console.log(w.in.f(2));

function mk(): any { return {}; }
mk().f = Mercury;
const r = mk();
r.f = Venus;
console.log(r.f(2));

type T = { k: any };
const t: T = { k: 1 };
t.k = Mercury;
console.log(t.k(3));

const o = { f: Mercury };
o.f = Venus;
console.log(o.f(3));

class C { k: any = 1; }
const c = new C();
c.k = Venus;
console.log(c.k(3));

class D { f: (x: number) => number = Mercury; }
const d = new D();
d.f = Venus;
console.log(d.f(4), d.f === Venus);
