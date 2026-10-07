// A function declaration passed by name as the callback of an
// escaped typed array's iterations, or of an any[]'s: the element
// reaches it as it is stored, and the answer comes back in its own
// kind. A typed receiver keeps the direct call.
function show(v: number): string { return typeof v + ":" + v; }
function inc(v: number): number { return v + 1; }
function isStr(v: number): boolean { return typeof v === "string"; }
function cat(acc: string, v: number): string { return acc + typeof v; }
function pair(v: number): string[] { return [typeof v, "" + v]; }
const d: number[] = [1, 2, 3];
const u: any = d;
u[1] = "s";
console.log(d.map(show).join(","));
console.log(d.map(inc).join(","));
let n = 0;
function cnt(v: number) { if (typeof v === "string") n++; }
d.forEach(cnt);
console.log(n);
console.log(d.filter(isStr).join(","));
console.log(d.reduce(cat, ""));
console.log(d.some(isStr), d.every(isStr));
console.log(d.find(isStr), d.findIndex(isStr), d.findLast(isStr), d.findLastIndex(isStr));
console.log(d.flatMap(pair).join(","));
console.log(Array.from(d, show).join(","));
const e: any[] = [1, "s"];
console.log(e.map(show).join(","));
console.log(e.filter(isStr).length, e.some(isStr));
const t: number[] = [4, 5];
console.log(t.map(show).join(","), t.map(inc).join(","));
console.log(t.filter(isStr).length, t.some(isStr), t.reduce(cat, ""));
console.log(Array.from(t, show).join(","));
