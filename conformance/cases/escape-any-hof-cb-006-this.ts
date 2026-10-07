// A this-reading function expression as the callback of an escaped
// typed array's iterations: the element reaches it as it is stored,
// and the thisArg still arrives. A typed receiver keeps its typed
// call.
const d: number[] = [1, 2, 3];
const u: any = d;
u[1] = "s";
const ctx = { k: "K" };
console.log(d.map(function (this: any, v: number) { return this.k + typeof v + v; }, ctx).join(","));
d.forEach(function (this: any, v: number) { console.log(this.k, typeof v, v); }, ctx);
console.log(d.filter(function (this: any, v: number) { return typeof v === this.k; }, { k: "string" }).join(","));
console.log(d.reduce(function (this: any, acc: string, v: number) { return acc + typeof v; }, ""));
console.log(d.some(function (this: any, v: number) { return typeof v === this.k; }, { k: "string" }));
console.log(d.every(function (this: any, v: number) { return typeof v === this.k; }, { k: "number" }));
console.log(d.find(function (this: any, v: number) { return typeof v === this.k; }, { k: "string" }));
console.log(d.findIndex(function (this: any, v: number) { return typeof v === this.k; }, { k: "string" }));
console.log(d.flatMap(function (this: any, v: number) { return [this.k, typeof v]; }, ctx).join(","));
console.log(Array.from(d, function (this: any, v: number) { return this.k + typeof v; }, ctx).join(","));
const t: number[] = [4, 5];
console.log(t.map(function (this: any, v: number) { return this.k + typeof v + v; }, ctx).join(","));
t.forEach(function (this: any, v: number) { console.log(this.k, typeof v, v); }, ctx);
console.log(t.some(function (this: any, v: number) { return v > this.n; }, { n: 4 }));
