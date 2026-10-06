// an optional callback param accepts every value the caller passes

const xs = [1, 2, 3].map((v: number, i?: number) => v * 10 + (i ?? -1));
console.log(xs.join(","));
const ys = [5, 6].filter((v: number, i?: number) => i !== undefined && i > 0);
console.log(ys.join(","));
type P = { n: number };
const ps: P[] = [{ n: 1 }, { n: 2 }];
ps.forEach((p: P, i?: number) => console.log(p.n, i));
function each(f: (s: string, k: number) => void) { f("a", 0); f("b", 1); }
each((s: string, k?: number) => console.log(s, k));
const names = ["x", "y"].map((s: string, i?: number) => s + (i ?? "?"));
console.log(names.join(" "));
function eachN(f: (s: string, k: number) => void) { f("a", 7); f("b", 1.5); f("c", -3); }
eachN((s: string, k?: number) => console.log(s, k, typeof k, k === undefined));
function each2(f: (k: number, t: string) => string) { return f(42, "zz"); }
console.log(each2((k: number, t?: string) => t === undefined ? "u" : t + k));
const zs = [2.5, 8].map((v: number, i?: number) => (i ?? 0) * 100 + v);
console.log(zs.join(","));
