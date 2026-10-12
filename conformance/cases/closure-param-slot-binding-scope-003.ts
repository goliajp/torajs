// an exported binding initialized from a field or element read holds a
// closure cell like any other declaration of its block
type F = (x: number) => number;

function m(f: F): number { return f(1) }
let k = 5;
const p: { g: F } = { g: (x) => x + k };
export const g = p.g;
const fs: F[] = [(x) => x * k, (x) => x - k];
export const h = fs[1];
export const { q } = { q: fs[0] };
export function inc(x: number): number { return x + 1 }
console.log(m(g), m(h), m(q), m(inc));
k = 9;
console.log(m(g), m(h), m(q));
