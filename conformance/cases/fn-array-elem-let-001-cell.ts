// An annotated function binding initialized by an element of a function
// array holds the element's closure cell, captures included.
const k = 3;
const hs: ((x: number) => number)[] = [(x: number) => x + k, (x: number) => x * k];
const w: (x: number) => number = hs[0];
console.log(w(4));

function pick(fs: ((x: number) => number)[], i: number): number {
  const f: (x: number) => number = fs[i];
  return f(10);
}
console.log(pick(hs, 1));

let m: (x: number) => number = hs[1];
console.log(m(2));
m = hs[0];
console.log(m(2));
