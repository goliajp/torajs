// The return consume walk marks only what can leave with the value. A
// ternary's condition is tested and stays; an index key finds the element
// and never aliases it. Both used to take the moved mark and strand the
// local; the reads after each call prove neither is released twice.
function cond(i: number): string { const n = "n" + i; return n ? n : "anon"; }
function condOther(i: number): string { const n = "n" + i; const m = "m" + i; return n.length > 3 ? n : m; }
const TABLE: any = { k0: { v: 10 }, k1: { v: 20 }, k2: { v: 30 } };
function keyed(o: any, i: number): any { const k = "k" + (i % 3); return o[k]; }
function elem(i: number): string { const xs: string[] = ["a" + i, "b" + i]; const j = i % 2; return xs[j]; }
function optKeyed(o: any, i: number): any { const k = "k" + (i % 4); return o?.[k]; }
let acc = 0;
let text = "";
for (let i = 0; i < 3000; i++) {
  const a = cond(i), b = condOther(i), e = elem(i);
  const c: any = keyed(TABLE, i), d: any = optKeyed(TABLE, i);
  acc += a.length + b.length + e.length + c.v + (d === undefined ? 1 : d.v);
  if (i < 3) text += a + b + e + c.v + (d === undefined ? "u" : d.v) + " ";
}
console.log(acc, text, TABLE.k1.v);
