// a method called through a receiver typed as a union of classes, or a
// type parameter, hands back the stale null as it is
type N = { n: number };
let q: N | null = { n: 1 };
function nullQ(): boolean { q = null; return true }
function run(label: string, t: () => void): void {
  try { t() } catch (e) { console.log(label, (e as Error).constructor.name) }
  q = { n: 1 };
}
class A { keep(x: N): N { return x } }
class B { keep(x: N): N { return { n: x.n + 1 } } }
function pick(i: number): A | B { return i > 0 ? new A() : new B() }
run("union", () => { if (q !== null && nullQ()) { const r: A | B = pick(1); console.log(r.keep(q).n) } });
function viaGeneric<T extends A>(t: T, x: N): number { return t.keep(x).n }
run("generic", () => { if (q !== null && nullQ()) console.log(viaGeneric(new A(), q)) });
const r2: A | B = pick(0);
console.log(r2.keep({ n: 3 }).n, pick(1).keep({ n: 4 }).n, viaGeneric(new A(), { n: 5 }));
