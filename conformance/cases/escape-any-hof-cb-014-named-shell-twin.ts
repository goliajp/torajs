// A function declaration passed by name over an escaped array whose
// relay shell already took its twin pair through an any binding
function inc(v: number): number { return v + 1; }
function tag(v: number): string { return typeof v + ":" + v; }
const g: any = inc;
const h: any = tag;
console.log(g("x"), h(true));
const d: number[] = [1, 2, 3];
const u: any = d;
u[1] = "s";
console.log(d.map(inc).join(","));
const r = d.map(inc);
console.log(typeof r[1], r.length);
console.log(d.map(tag).join(","));
console.log(Array.from(d, inc).join(","));
const t: number[] = [4, 5];
console.log(t.map(inc).join(","), t.map(tag).join(","));
