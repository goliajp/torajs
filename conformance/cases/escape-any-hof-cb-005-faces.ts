// Callback faces that keep the direct call: a Set source for
// Array.from, and an any[] receiver whose callback takes an array
// first parameter. Their products stay typed.
const xs: number[] = [1, 2, 3];
const doubled = Array.from(new Set<number>(xs), (v: number) => v * 2);
console.log(doubled.join(","), doubled.length);
const rows: any[] = [[1, 2], [3], []];
const lens = rows.map((r: number[]) => r.length);
console.log(lens.join(","), typeof lens[1]);
const m = new Map<string, number>([["a", 1], ["b", 2]]);
console.log(Array.from(m, (e: [string, number]) => e[0] + e[1]).join(","));
