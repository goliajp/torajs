// An element of a typed array whose alias escaped into the any world
// is whatever the alias stored; a binding declared number / string
// stores it as is, through `+`, conditionals, logical operators and
// copies. Other arithmetic answers a number, and a typed array that
// never escaped keeps its number bindings.
const d: number[] = [1, 2, 3];
const u: any = d;
u[1] = "s";

let acc = 0;
for (const v of d) { acc = acc + v; }
console.log("for-of", acc, typeof acc);

let t: number = 0;
t = t + d[1];
console.log("index", typeof t, t);

let first: number = 0;
let second: number = 0;
first = first + d[1];
second = first;
console.log("relay", second, typeof second);

const pick = d.length > 0 ? d[1] : 0;
console.log("const", pick, typeof pick);
let s: string = "";
s = d[1] || "none";
console.log("str", s, typeof s);
let n2: number = 0;
n2 = n2 + (d[0] ?? 5);
console.log("nullish", n2, typeof n2);

let g: number = 10;
function addAll(xs: number[]): void {
  for (const v of xs) { g = g + v; }
}
addAll(d);
console.log("module", g, typeof g);

function local(xs: number[]): string {
  let sum: number = 0;
  for (let i = 0; i < xs.length; i++) { sum = sum + xs[i]; }
  return typeof sum + " " + sum;
}
console.log("fn", local(d));

let m: number = 1;
m = m * d[0];
m = m - d[1];
console.log("mul", m, typeof m);

const b: number[] = [1, 2, 3];
let ab = 0;
for (const v of b) { ab = ab + v; }
let ib: number = 0;
ib = ib + b[1];
console.log("plain", ab, ib, typeof ab, typeof ib);
