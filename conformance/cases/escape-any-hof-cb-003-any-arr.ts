// An `any[]` hands its callbacks the elements as they are, whatever the
// callback's parameter annotation says, and what the callback answers
// keeps its kind in a map product, an Array.from product and a reduce
// accumulator. Callbacks taking `any` and all-number arrays keep their
// numbers.
const a: any[] = [1, "5", 3, { k: 1 }];
console.log("reduce", a.reduce((acc: number, v: number) => acc + v, 0));
const m = a.map((v: number) => v + 1);
console.log("map", m.join(","), typeof m[1], typeof m[3]);
console.log("filter", a.filter((v: number) => typeof v === "number").length);
console.log("some/every", a.some((v: number) => typeof v === "object"), a.every((v: number) => v > 0));
console.log("find", a.find((v: string) => typeof v === "string"));
console.log("findIndex", a.findIndex((v: number) => typeof v === "object"));
console.log("flatMap", a.flatMap((v: number) => [typeof v]).join(","));
console.log("from", Array.from(a, (v: number) => typeof v).join(","));
const f = Array.from(a, (v: number) => v + 1);
console.log("from product", f.join(","), typeof f[1]);
const b: any[] = [3, 1, 2];
console.log("sort", b.sort((x: number, y: number) => x - y).join(","));
const h: any[] = [1, undefined, 3];
console.log("undefined", h.map((v: number) => typeof v).join(","));

const n: any[] = [1, 2, 3, 4];
console.log("sum", n.reduce((acc: number, v: number) => acc + v, 0));
console.log("reduceRight", n.reduceRight((acc: number, v: number) => acc * 10 + v, 0));
const r = n.reduce((acc: number, v: number) => acc + v, 0);
console.log("sum kind", typeof r, r * 2);
const doubled = n.map((v: number) => v * 2);
let t = 0;
for (let i = 0; i < doubled.length; i++) t += doubled[i];
console.log("doubled", doubled.join(","), t);
const untyped = n.map(v => v * 3);
console.log("untyped", untyped.join(","), typeof untyped[0]);
