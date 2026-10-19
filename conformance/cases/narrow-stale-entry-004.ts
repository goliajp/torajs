// calls that do not enter a declared function: one through a local
// binding of the same name, one through the top-level `this`; neither
// argument is tested for the declared function
type N = { n: number };
let q: N | null = { n: 1 };
function nullQ(): boolean { q = null; return true }
function run(label: string, t: () => void): void {
  try { t() } catch (e) { console.log(label, (e as Error).constructor.name) }
  q = { n: 1 };
}
function f(x: N): number { return x.n }
function sh(): number { const f = (y: N | null) => (y === null ? 7 : 8); return f(null) }
function sh2(f: (y: N | null) => number): number { return f(null) }
console.log(f({ n: 1 }), sh(), sh2((y) => (y === null ? 9 : 10)));
function tf(x: N): number { return x.n }
const self: any = this;
console.log(typeof self);
run("this", () => { if (q !== null && nullQ()) console.log(self.tf(q)) });
console.log(tf({ n: 2 }));
