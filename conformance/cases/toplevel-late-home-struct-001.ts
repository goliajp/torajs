// top-level bindings a named function reads, whose initializer has an
// object layout no literal-shape rule names: a spread, a class instance
// field, a borrowed member / element read, a conditional
class K { v: number; constructor(v: number) { this.v = v; } get(): number { return this.v; } }
const so = { a: 1, b: "x" };
const sp = { ...so, c: 3 };
const h = { k: new K(5), n: 2, ...so };
const outer = { inner: { a: 1, s: "in" }, n: 2 };
const inn = outer.inner;
const list = [{ a: 3, s: "l0" }, { a: 4, s: "l1" }];
const first = list[0];
const pick = outer.n > 1 ? { a: 5, s: "t" } : { a: 6, s: "f" };
let cur = outer.inner;

function rdSpread() { return sp.a + sp.c + sp.b; }
function rdClass() { return h.k.get() + h.n + h.b + (h.k instanceof K); }
function rdBorrowed() { return inn.a + first.a + pick.a + inn.s + first.s + pick.s; }
function step(i: number) { cur = i % 2 === 0 ? { ...outer.inner, a: i } : outer.inner; }

console.log(rdSpread(), JSON.stringify(sp));
console.log(rdClass());
console.log(rdBorrowed());
list.length = 0;
console.log(rdBorrowed(), JSON.stringify(first));
let acc = 0;
for (let i = 0; i < 1000; i++) { step(i); acc += cur.a; }
console.log(acc, cur.s, JSON.stringify(outer));
