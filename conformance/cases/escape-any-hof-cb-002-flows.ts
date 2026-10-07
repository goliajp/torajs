// The callbacks of an escaped typed array see the elements as they
// are whichever way the alias escaped (an any parameter, an any field,
// a cast), and what the callback answers keeps its kind in a map
// product and a reduce accumulator.
const d: number[] = [1, 2, 3, 4];
const u: any = d;
u[1] = "5";
console.log("map", d.map(v => v + 1)[1], typeof d.map(v => v + 1)[1], d.map(v => v + 1)[1] + 1);
const m = d.map(v => v * 2);
console.log("product", m.join(","), typeof m[1]);
console.log("chain", d.map(v => v + 1).map(w => w + 1).join(","));
console.log("flatMap", d.flatMap(v => [v + 1]).join(","), typeof d.flatMap(v => [v + 1])[1]);
console.log("flatMap elems", d.flatMap(v => [typeof v]).join(","));
console.log("flatMap index", d.flatMap((v, i) => [typeof v, i]).join(","));
const r = d.reduce((a, v) => a + v, 0);
console.log("reduce", r, typeof r);
console.log("reduceRight", d.reduceRight((a, v) => a + v, 0));
console.log("reduce seedless", d.reduce((a, v) => a + v));

// handing an object element to a typed callback reads no valueOf
let hits = 0;
u[2] = {
  valueOf() {
    hits++;
    return 9;
  },
};
d.forEach(v => console.log("valueOf", typeof v));
console.log("valueOf hits", hits);

// a write through the alias during the walk is seen at its index
d.forEach((v, i) => {
  if (i === 0) u[3] = "late";
  console.log("late", i, typeof v);
});

function viaParam(x: any) {
  x[0] = "p";
}
const e: number[] = [1, 2];
viaParam(e);
console.log("param", e.map(v => typeof v).join(","), e.some(v => v === "p"));

const holder: { f: any } = { f: null };
const g: number[] = [1, 2];
holder.f = g;
holder.f[1] = true;
console.log("field", g.map(v => typeof v).join(","), g.findIndex(v => v === true));

const h: number[] = [1, 2];
(h as any)[0] = null;
console.log("cast", h.map(v => typeof v).join(","), h.filter(v => v === null).length);

const strs: string[] = ["a", "b"];
const sa: any = strs;
sa[0] = 7;
console.log("string elems", strs.map(v => typeof v).join(","), strs.some(v => typeof v === "number"));

const flags: boolean[] = [true, false];
const fa: any = flags;
fa[1] = 0;
console.log("boolean elems", flags.map(v => typeof v).join(","), flags.filter(v => v === 0).length);
