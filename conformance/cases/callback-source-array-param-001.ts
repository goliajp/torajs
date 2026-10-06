// an array callback may declare its source-array parameter with the
// element type, optional or not, on a typed or an `any[]` receiver

interface P { v: number }
const ps: P[] = [{ v: 1 }, { v: 2 }];
ps.forEach((p: P, i?: number, arr?: P[]) => console.log(p.v, i, arr === undefined ? -1 : arr.length));
ps.forEach((p: P, i: number, arr: P[]) => console.log(p.v + arr[1 - i].v));
console.log(ps.map((p: P, i: number, arr: P[]) => arr[i] === p).join(","));
console.log(ps.filter((p: P, i?: number, arr?: P[]) => arr !== undefined && arr.length === 2).length);
const ns: number[] = [3, 4];
ns.forEach((n: number, i: number, arr: number[]) => console.log(n * arr.length));
const ss = ["a", "b"];
console.log(ss.map((s: string, i: number, arr: string[]) => s + arr.length).join(""));
const an: any[] = [{ v: 5 }, { v: 6 }];
an.forEach((p: P, i: number, arr: P[]) => console.log(p.v, arr.length, arr[i].v));
an.forEach((p: any, i: number, arr: P[]) => console.log(arr[i].v + p.v));
console.log(an.map((p: any, i: number, arr?: P[]) => arr === undefined ? -1 : arr[0].v).join(","));
console.log(ps.some((p: P, i: number, arr: P[]) => arr[0] === p));
console.log(ps.reduce((acc: number, p: P, i: number, arr: P[]) => acc + p.v * arr.length, 0));
