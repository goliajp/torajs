// a call whose callee is a choice among functions (`c ? f : g`, `f || g`,
// `f ?? g`) calls the chosen one
function twice(n: number): number {
  return n * 2;
}
function inc(n: number): number {
  return n + 1;
}
const c = Math.random() < 2;
console.log((c ? twice : inc)(5), (!c ? twice : inc)(5));
const add = (a: number, b: number): number => a + b;
const sub = (a: number, b: number): number => a - b;
console.log((c ? add : sub)(7, 3), (c ? sub : add)(7, 3));
let k = 0;
for (let i = 0; i < 4; i++) k += (i % 2 === 0 ? add : sub)(k, i);
console.log(k);
console.log((c ? (x: number): number => x * 10 : (x: number): number => x)(4));
let maybe: ((n: number) => number) | undefined;
const fallback = (n: number): number => n - 100;
console.log((maybe ?? fallback)(1));
maybe = (n: number): number => n + 100;
console.log((maybe ?? fallback)(1));
const picked = [twice, inc];
console.log((picked[0] || inc)(3));
