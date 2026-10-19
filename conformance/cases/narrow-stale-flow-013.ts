// a narrowing rules out only the forms its test names: a strict test
// against undefined leaves a stale null to raise, and the caller checks
// for that raise right after the call
type N = { n: number };
let q: N | null = { n: 1 };
function nullQ(): boolean { q = null; return true }
function run(label: string, t: () => void): void {
  try { t() } catch (e) { console.log(label, (e as Error).constructor.name) }
  q = { n: 1 };
}
function other(): number { return 1 }
function loose(p: N): number { if (p != null) { other(); return p.n } return -1 }
function truthy(p: N): number { if (!p) return -2; other(); return p.n }
function strictU(p: N): number { if (p !== undefined) { return p.n } return -3 }
function strictN(p: N): number { if (p !== null) { return p.n } return -4 }
function outer(f: (p: N) => number, p: N): string {
  try { return "ok " + f(p) } catch (e) { return "caught " + (e as Error).constructor.name }
}
run("loose-null", () => { if (q !== null && nullQ()) console.log(outer(loose, q), loose(q)) });
run("truthy-null", () => { if (q !== null && nullQ()) console.log(outer(truthy, q), truthy(q)) });
run("strictU-null", () => { if (q !== null && nullQ()) console.log(strictU(q)) });
run("strictU-null-outer", () => { if (q !== null && nullQ()) console.log(outer(strictU, q)) });
run("strictN-null", () => { if (q !== null && nullQ()) console.log(strictN(q)) });
// an element read past the end holds the undefined cell
const none: N[] = [];
run("loose-cell", () => console.log(outer(loose, none[0]), loose(none[0])));
run("truthy-cell", () => console.log(outer(truthy, none[0]), truthy(none[0])));
run("strictU-cell", () => console.log(strictU(none[0])));
run("strictN-cell", () => console.log(strictN(none[0])));
run("strictN-cell-outer", () => console.log(outer(strictN, none[0])));
const k: N = { n: 5 };
console.log(loose(k), truthy(k), strictU(k), strictN(k));
