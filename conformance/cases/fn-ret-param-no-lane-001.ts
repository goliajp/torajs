// functions handed to a parameter returning a function, in a program
// where no return opens the closure lane: they keep the direct lane
function dbl(n: number): number { return n * 2; }
function sq(n: number): number { return n * n; }
function pick2(k: number): (n: number) => number {
  if (k > 0) return sq;
  return dbl;
}
function apply(f: (k: number) => (n: number) => number, k: number): number {
  return f(k)(5);
}
let t = 0;
for (let i = 0; i < 1000; i++) t += apply(pick2, i % 2);
console.log(apply(pick2, 1), apply(pick2, 0), t);
