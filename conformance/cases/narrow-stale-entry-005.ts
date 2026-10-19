// a local function named like a declared one that reads its parameter
// first: a call of the local one tests nothing for the declared one
type N = { n: number };
let q: N | null = { n: 1 };
function nullQ(): boolean { q = null; return true }
function run(label: string, t: () => void): void {
  try { t() } catch (e) { console.log(label, (e as Error).constructor.name) }
  q = { n: 1 };
}
function f(x: N): number { return x.n }
function sh(p: N): number { const f = (y: N) => 7; return f(p) }
function sh2(p: N): number { function f(y: N): number { return 8 } return f(p) }
function sh3(f: (y: N) => number, p: N): number { return f(p) }
run("local-arrow", () => { if (q !== null && nullQ()) console.log(sh(q)) });
run("local-fn", () => { if (q !== null && nullQ()) console.log(sh2(q)) });
run("param", () => { if (q !== null && nullQ()) console.log(sh3((y) => 9, q)) });
run("global", () => { if (q !== null && nullQ()) console.log(f(q)) });
console.log(f({ n: 1 }), sh({ n: 2 }), sh2({ n: 3 }), sh3((y) => y.n, { n: 4 }));
