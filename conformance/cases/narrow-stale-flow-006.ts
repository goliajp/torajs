// a narrowed array a call set to null, spread after it was passed
// along: an array spread or a destructuring of null throws
type Node = { n: number };
let q: Node | null = { n: 1 };
let a: number[] | null = [1, 2];
function f(): boolean { q = null; a = null; return true }
function run(label: string, t: () => void): void {
  try { t() } catch (e) { console.log(label, (e as Error).constructor.name) }
  q = { n: 1 };
  a = [1, 2];
}

function cp(xs: number[]): number[] { return [0, ...xs] }
run("array spread", () => { if (a !== null && f()) console.log([...a].length) });
run("array spread param", () => { if (a !== null && f()) console.log(cp(a).length) });
run("array spread ok", () => { if (a !== null) console.log(cp(a).length) });
function sum(...xs: number[]): number { let s = 0; for (const x of xs) s += x; return s }
function fwd(xs: number[]): number { return sum(...xs) }
run("call spread param", () => { if (a !== null && f()) console.log(fwd(a)) });
function head(xs: number[]): number { const [h] = xs; return h }
run("destructure param", () => { if (a !== null && f()) console.log(head(a)) });
run("destructure ok", () => { if (a !== null) console.log(head(a)) });
console.log("end");
