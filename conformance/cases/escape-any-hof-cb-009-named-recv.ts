// A this-reading declaration at a thisArg callback position, async
// and generator declarations by name, and a this-reading function
// expression over an any[]: each sees the element as it is stored.
function g(this: any, v: number): string { return this.k + typeof v; }
async function am(v: number): Promise<string> { return typeof v; }
function* gen(v: number) { yield typeof v; }
const d: number[] = [1, 2, 3];
const u: any = d;
u[1] = "s";
console.log(d.map(g, { k: "K" }).join(","));
d.forEach(g, { k: "F" });
console.log(d.filter(g, { k: "" }).length);
console.log(d.map(gen).map((it: any) => it.next().value).join(","));
const e: any[] = [1, "s"];
console.log(e.map(function (this: any, v: number) { return this.k + typeof v; }, { k: "K" }).join(","));
const t: number[] = [4, 5];
console.log(t.map(g, { k: "T" }).join(","));
Promise.all(d.map(am)).then((r) => console.log(r.join(",")));
