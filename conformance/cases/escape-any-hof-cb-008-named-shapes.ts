// Named callbacks over escaped arrays: struct and defaulted
// parameters, recursion, a product read back after a push, a
// comparator, a nested declaration, a shell that already exists.
interface P { x: number }
function px(p: P): string { return typeof p + ":" + p.x; }
function dflt(v: number, k: number = 10): string { return typeof v + (v + k); }
function cmp(a: number, b: number): number { return ("" + a) < ("" + b) ? -1 : 1; }
function self(v: number): number { return typeof v === "number" ? v : self(1) * 100; }
function show(v: number): string { return typeof v + ":" + v; }
const g: any = show;
console.log(g("x"));
const ps: P[] = [{ x: 1 }, { x: 2 }];
const pu: any = ps;
pu[1] = "str";
console.log(ps.map(px).join(","));
const d: number[] = [1, 2, 3];
const u: any = d;
u[1] = "s";
console.log(d.map(dflt).join(","));
console.log(d.map(self).join(","));
console.log(d.map(show).join(","), show(4));
const m = d.map(self);
m.push(7);
console.log(m.join(","), m.length);
const srt = d.slice();
console.log(d.sort(cmp).join(","));
function outer(): string {
  function inner(v: number): string { return typeof v; }
  return d.map(inner).join(",");
}
console.log(outer());
function sum(a: number, b: number): number { return a + b; }
console.log(d.reduce(sum), srt.length);
