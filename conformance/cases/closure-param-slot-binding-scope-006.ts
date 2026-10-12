// a for-of binding over an array of fn values holds a closure cell,
// the same as a binding initialized from an element read
type F = (x: number) => number;

function m(f: F): number { return f(1) }
function twice(x: number): number { return x * 2 }
let k = 3;

function each(fs: F[]) { let s = 0; for (const g of fs) s += m(g); return s }
function eachArray(fs: Array<F>) { let s = 0; for (const g of fs) { const t = () => m(g); s += t() } return s }
function local() {
  const fs: F[] = [(x) => x + k, twice];
  let s = 0;
  for (const g of fs) s += m(g);
  return s;
}
function sum(xs: number[]) { let s = 0; for (const g of xs) s += g; return s }

console.log(each([(x) => x + k, (x) => x * 10]), each([twice]));
console.log(eachArray([(x) => x - k, twice]), local(), sum([1, 2, 3]));
console.log(m(twice));
