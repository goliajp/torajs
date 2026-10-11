// Sloppy code reads the value of a refused [[Set]] on the typed lane
// (§13.15.2 step 5 answers the right-hand side): returned, passed,
// chained into another field, bound, read through a member, and as the
// last operand of a comma in a `for` update or a statement. Each read
// value outlives the refusal, the field keeps its own value, and a
// setter still receives the value.
class Cell { s: string = "a"; t: string = "b"; }
function take(x: string): number { return x.length; }
function ret(c: Cell, s: string): string { return (c.s = s + "?"); }
function arg(c: Cell, s: string): number { return take(c.t = s + "!"); }
function chain(c: Cell, d: Cell, s: string): string { d.s = (c.s = s + "~"); return d.s; }
function bind(c: Cell, s: string): string { const r = (c.t = s + "#"); return r + r.length; }
function member(c: Cell, s: string): number { return (c.t = s + "%").length; }
function step(c: Cell, s: string): number { let m = 0; for (let k = 0; k < 2; k++, c.t = s + k) { m++; } return m; }
function stmt(c: Cell, s: string): number { let m = 0; (m++, c.s = s + "&"); return m; }
function run(label: string, c: Cell): void {
  const d = new Cell();
  console.log(label, ret(c, "r"), arg(c, "ab"), chain(c, d, "x"), bind(c, "q"), member(c, "mm"), step(c, "z"), stmt(c, "w"), c.s, c.t);
}
run("plain", new Cell());
const frozen = new Cell();
Object.freeze(frozen);
run("frozen", frozen);
const getter = new Cell();
Object.defineProperty(getter, "s", { get() { return "G"; } });
Object.defineProperty(getter, "t", { get() { return "H"; } });
run("getter", getter);
const seen: string[] = [];
const setter = new Cell();
Object.defineProperty(setter, "s", { get() { return "S"; }, set(v: string) { seen.push(v); } });
run("setter", setter);
console.log(seen.join(","));
