// a conditional over named functions returned through the closure
// lane: by a function the lane opens on itself, and by a nested and a
// top-level function that join it by being handed to the same
// parameter as a lane function
const objs: any = { f: (n: number) => n + 1 };
function dbl(n: number): number { return n * 2; }
function sq(n: number): number { return n * n; }
function pick(k: number): (n: number) => number {
  if (k > 1) return k > 2 ? dbl : sq;
  const v = objs.f;
  return v;
}
function apply(f: (k: number) => (n: number) => number, k: number): number {
  return f(k)(5);
}
function local(k: number): (n: number) => number {
  return k > 0 ? (k > 1 ? sq : dbl) : sq;
}
function nest(): number {
  function inner(k: number): (n: number) => number { return k > 0 ? dbl : sq; }
  return apply(inner, 1) + apply(inner, 0);
}
console.log(pick(3)(5), pick(2)(5), pick(0)(5));
console.log(apply(pick, 3), apply(pick, 0), apply(local, 2), apply(local, 1), nest());
