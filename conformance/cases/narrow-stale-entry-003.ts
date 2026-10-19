// a parameter with a default runs its initialiser before the body reads
// another parameter: the initialiser's effect comes before the TypeError
type N = { n: number };
let q: N | null = { n: 1 };
function nullQ(): boolean { q = null; return true }
function run(label: string, t: () => void): void {
  try { t() } catch (e) { console.log(label, (e as Error).constructor.name) }
  q = { n: 1 };
}
function g(): number { console.log("default ran"); return 1 }
function f(x: N, y: number = g()): number { return x.n + y }
function h(x: N, y: N = { n: g() }): number { return x.n + y.n }
function k(x: N, y?: number): number { return x.n + (y ?? 0) }
run("default", () => { if (q !== null && nullQ()) console.log(f(q)) });
run("default-obj", () => { if (q !== null && nullQ()) console.log(h(q)) });
run("optional", () => { if (q !== null && nullQ()) console.log(k(q)) });
run("given", () => { if (q !== null && nullQ()) console.log(f(q, 2)) });
console.log(f({ n: 3 }), f({ n: 3 }, 4), h({ n: 5 }), h({ n: 5 }, { n: 6 }), k({ n: 7 }), k({ n: 7 }, 8));
