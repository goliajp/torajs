// a function whose fn-typed return goes through the closure lane,
// handed as a value to a parameter typed as a function returning a
// function: calls through the parameter get the fn object back, and
// the other functions handed to that parameter join the lane
const objs: any = { f: (n: number) => n + 1 };
function dbl(n: number): number { return n * 2; }
function sq(n: number): number { return n * n; }
function pick(k: number): (n: number) => number {
  if (k > 0) return dbl;
  const v = objs.f;
  return v;
}
function pick2(k: number): (n: number) => number {
  if (k > 0) return sq;
  return dbl;
}
function apply(f: (k: number) => (n: number) => number, k: number): number {
  return f(k)(5);
}
function outer(g: (k: number) => (n: number) => number, k: number): number {
  return apply(g, k) + 100;
}
function keep(f: (k: number) => (n: number) => number, k: number): (n: number) => number {
  const h = f(k);
  return h;
}
console.log(apply(pick, 1), apply(pick, 0), apply(pick2, 1), apply(pick2, 0));
console.log(outer(pick, 1), outer(pick, 0), outer(pick2, 1));
console.log(pick2(1)(3), pick(0)(3), keep(pick, 0)(7), keep(pick2, 1)(7));
let t = 0;
for (let i = 0; i < 1000; i++) t += apply(i % 2 ? pick : pick2, i % 3);
console.log(t);
