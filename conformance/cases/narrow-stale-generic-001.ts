// a generic function used as a value enters its specialization through
// no direct call: the parameter is whatever the caller of the value passed
type N = { n: number };
let q: N | null = { n: 1 };
function nullQ(): boolean { q = null; return true }
function run(label: string, t: () => void): void {
  try { t() } catch (e) { console.log(label, (e as Error).constructor.name) }
  q = { n: 1 };
}
function idn<T>(x: T, y: N): number { return y.n }
const g = idn;
run("value", () => { if (q !== null && nullQ()) console.log(g(1, q)) });
function idm<T>(x: T, y: N): number { return y.n + 1 }
const hs = [idm];
run("array", () => { if (q !== null && nullQ()) console.log(hs[0](3, q)) });
console.log(g(1, { n: 3 }), hs[0](5, { n: 6 }), idn("s", { n: 5 }), idm(2, { n: 7 }));
