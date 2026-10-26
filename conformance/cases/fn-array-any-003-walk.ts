// Every way an element of a typed function array holding an `any`
// function is read keeps the stored function's own face: a `for-of`
// head, a callback's element parameter, a binding or an argument
// initialized from the element.
const s: any = (x: any) => "s" + x;
const n: any = (x: any) => x * 0.25;
const two: any = (a: any, b: any) => a + "," + b;
const hs: ((x: number) => number)[] = [s, n, (x: number) => x];
hs[2] = two;

let acc = 0;
for (const h of hs) acc = acc + h(1);
console.log(acc);
for (const h of hs) console.log(h(4));

hs.forEach((f, i) => console.log(i, f(1)));
console.log(hs.map((f) => f(2)).join("|"));
console.log(hs.reduce((a, f) => a + f(1), 0));
console.log(hs.filter((f) => typeof f(1) === "number").length);
console.log(hs.some((f) => f(0) === "s0"));
console.log(hs.findIndex((f) => f(8) === 2));

const w: (x: number) => number = hs[1];
console.log(w(4));
const u = hs[0];
console.log(u(8));
function call(g: (x: number) => number): number {
  return g(2);
}
console.log(call(hs[1]));
