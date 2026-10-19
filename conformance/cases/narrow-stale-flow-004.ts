// a narrowed value a call set to null, handed to a function the call
// site names in a way that does not say which: a method spelled like a
// builtin lookup on the program's own object, a parameter spelled like
// a declared function, a closure binding the checker retargets
type Node = { n: number };
let q: Node | null = { n: 1 };
function f(): boolean { q = null; return true }
function run(label: string, t: () => void): void {
  try { t() } catch (e) { console.log(label, (e as Error).constructor.name) }
  q = { n: 1 };
}

const reg = { get: (x: Node): number => x.n, has: (x: Node): boolean => x.n > 0 };
run("get", () => { if (q !== null && f()) console.log(reg.get(q)) });
run("has", () => { if (q !== null && f()) console.log(reg.has(q)) });
run("get ok", () => { if (q !== null) console.log(reg.get(q)) });

function h(x: Node): number { return 0 }
function k(h: (x: Node) => number, v: Node): number { return h(v) }
run("param named like a function", () => {
  if (q !== null && f()) console.log(k((y: Node): number => y.n, q))
});
run("declared function", () => { if (q !== null && f()) console.log(h(q)) });

function noop(): void {}
function call(noop: () => void): void { noop() }
run("store through a parameter", () => {
  if (q !== null) { call(() => { q = null }); console.log(q.n) }
});
run("declared noop", () => { if (q !== null) { noop(); console.log(q.n) } });

const w = (a: string, x: Node): number => x.n;
const z: any = "s";
run("widened closure", () => { if (q !== null && f()) console.log(w(z, q)) });
run("widened closure ok", () => { if (q !== null) console.log(w(z, q)) });
console.log("end");
