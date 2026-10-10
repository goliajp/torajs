// a local binding that shadows a closure-lane function, handed to a
// parameter returning a function: the parameter receives what the
// binding holds, not the lane function of the same name
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
function run(): number {
  const pick = pick2;
  return apply(pick, 1) + apply(pick, 0);
}
function relay(pick: (k: number) => (n: number) => number): number {
  return apply(pick, 1) + apply(pick, 0);
}
console.log(run(), relay(pick2), pick(1)(2), pick(0)(2));
