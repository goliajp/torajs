// An Ident-target assignment used as the return value already carries
// its own stake (`mint_consumer_stake` takes the +1). The return's
// consume walk marked the target moved on top of it, so the slot's own
// share never released (one string per `return (a = s + "!")`), and it
// reached the rhs as well: `return (a = xs[0])` over a LOCAL array marked
// `xs` moved and stranded the whole array per call. The answer leaves
// owned; every binding keeps its scope drop.
function fresh(s: string): string { let a = "z"; return (a = s + "!"); }
function elem(i: number): any { const xs: any[] = [{ s: "first" + i }, { s: "second" + i }]; let a: any = null; return (a = xs[0]); }
function field(p: { inner: { v: string } }): { v: string } { let a = { v: "" }; return (a = p.inner); }
function str(ss: string[]): string { let a = ""; return (a = ss[0]); }
function chain(i: number): string { let a = ""; let b = ""; return (a = (b = "c" + i)); }
function pick(i: number, c: boolean): any { const o: any = { s: "o" + i }; let a: any = c ? o : null; return (a = a ?? { s: "alt" + i }); }
function run(i: number): void {
  { const r = fresh("q" + i); console.log(r); }
  { const r = elem(i); } const t = { s: "later" + i }; console.log(elem(i).s, t.s);
  const p = { inner: { v: "in" + i } }; { const r = field(p); } const t2 = { v: "ot" + i }; console.log(p.inner.v, t2.v);
  const ss = ["str" + i + "x"]; { const r = str(ss); } const t3 = "abcde" + i; console.log(ss[0], t3);
  console.log(chain(i), pick(i, true).s, pick(i, false).s);
}
for (let i = 1; i <= 3; i++) run(i);
