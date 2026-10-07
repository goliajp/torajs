// A this-reading callback with a number return, handed a string
// element, answers a string: the map product keeps it as it came back
function g(this: any, v: number): number {
  return this.k + v;
}

const d = [1, 2, 3];
const u: any = d;
u[1] = "s";
const r = d.map(g, { k: 10 });
console.log(r.join(","), typeof r[1]);

const r2 = d.map(
  function (this: any, v: number): number {
    return this.k + v;
  },
  { k: 20 },
);
console.log(r2.join(","), typeof r2[1]);

const a: any[] = [1, "t"];
const r3 = a.map(g, { k: 30 });
console.log(r3.join(","), typeof r3[1]);

const t = [1, 2];
const r4 = t.map(g, { k: 40 });
console.log(r4.join(","), typeof r4[1]);
