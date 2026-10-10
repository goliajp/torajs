// a function whose return takes the closure lane because one of its
// returns is an arrow also returns its fn-typed parameter, or a const
// binding of a top-level function: those hand back fn objects too
function dbl(n: number): number { return n * 2; }
function sq(n: number): number { return n * n; }
function pick(f: (n: number) => number, k: number): (n: number) => number {
  if (k > 0) return (n: number) => n + 100;
  return f;
}
function get(k: number): (n: number) => number {
  const a = dbl;
  if (k > 0) return (n: number) => n + 100;
  return a;
}
console.log(pick(dbl, 0)(5), pick(sq, 0)(5), pick(dbl, 1)(5), get(0)(5), get(1)(5));
console.log(pick(dbl, 0) === dbl, get(0) === dbl, pick(sq, 0) === dbl);
let t = 0;
for (let i = 0; i < 1000; i++) t += pick(i % 2 ? dbl : sq, i % 3)(i) + get(i % 2)(1);
console.log(t);
