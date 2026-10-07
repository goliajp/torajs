// The callbacks of an escaped typed array see its elements as they are
// when they are bound to a name, when the array is the product of
// another iteration, a copy or a concatenation, and through Array.from.
// A function declaration passed by name over such an array still fills
// a product every alias reads back.
const d: number[] = [1, 2, 3, 4];
const u: any = d;
u[0] = { k: 1 };
u[1] = "5";
console.log("filter.map", d.filter((v: number) => true).map((v: number) => typeof v).join(","));
console.log("slice.map", d.slice().map((v: number) => typeof v).join(","));
console.log("slice.filter", d.slice(1).filter((v: number) => typeof v === "string").length);
console.log("map.map", d.map((v: number) => v).map((v: number) => typeof v).join(","));
console.log("concat.map", d.concat([9]).map((v: number) => typeof v).join(","));
const p = d.slice().map((v: number) => v + 1);
console.log("slice product", p.join(","), typeof p[1]);
console.log("filter.some", d.filter((v: number) => true).some((v: number) => typeof v === "string"));
console.log("slice.reduce", d.slice(1).reduce((s: number, v: number) => s + v, 0));
const kind = (v: number) => typeof v;
console.log("bound", d.map(kind).join(","));
const plus = (v: number) => v + 1;
console.log("bound product", d.map(plus).join(","));
console.log("from", Array.from(d, (v: number) => typeof v).join(","));
const fp = Array.from(d, (v: number) => v + 1);
console.log("from product", fp.join(","), typeof fp[1]);

function inc(v: number) {
  return v + 1;
}
function total(xs: number[]): number {
  let s = 0;
  for (let i = 0; i < xs.length; i++) s += xs[i];
  return s;
}
const e: number[] = [1, 2, 3, 4];
const eu: any = e;
eu[2] = 10;
const m: number[] = e.map(inc);
console.log("named", m.length, m[2], m.join(","));
m.push(7);
console.log("named total", total(m), m.indexOf(11), m.includes(5));
let s = 0;
for (const x of m) s += x;
console.log("named for-of", s);
