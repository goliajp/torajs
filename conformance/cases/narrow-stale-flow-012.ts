// a member read straight off a conditional, a comma, an assignment or a
// logical operator whose operand holds the stale null
type N = { n: number };
let q: N | null = { n: 1 };
function nullQ(): boolean { q = null; return true }
let s: string | null = "ab";
function nullS(): boolean { s = null; return true }
function run(label: string, t: () => void): void {
  try { t() } catch (e) { console.log(label, (e as Error).constructor.name) }
  q = { n: 1 };
  s = "ab";
}
let c = true;
const o: N = { n: 9 };
function tern(p: N): number { return (c ? p : o).n }
function comma(p: N): number { return (0, p).n }
function asg(p: N): number { let t: N = o; return (t = p).n }
function and(p: N): number { return (p && o).n }
function or(p: N): number { return (p || o).n }
function nul(p: N): number { return (p ?? o).n }
function slen(x: string): number { return (c ? x : "zz").length }
run("ternary", () => { if (q !== null && nullQ()) console.log(tern(q)) });
run("comma", () => { if (q !== null && nullQ()) console.log(comma(q)) });
run("assign", () => { if (q !== null && nullQ()) console.log(asg(q)) });
run("and", () => { if (q !== null && nullQ()) console.log(and(q)) });
run("or", () => { if (q !== null && nullQ()) console.log(or(q)) });
run("nullish", () => { if (q !== null && nullQ()) console.log(nul(q)) });
run("str-ternary", () => { if (s !== null && nullS()) console.log(slen(s)) });
const k: N = { n: 2 };
console.log(tern(k), comma(k), asg(k), and(k), or(k), nul(k), slen("abc"));
