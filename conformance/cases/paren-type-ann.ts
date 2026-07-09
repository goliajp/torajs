// chunk 735 — TS ParenthesizedType: `(T)` with no `=>` after the
// close paren re-reads as a grouped type (pre-fix: parse error
// "expected `=>` in fn-type"). The dominant shape is the fn-type
// array spelling `((n: number) => number)[]`, which rides the
// chunk-733 Closure-repr element lane.
function double(n: number): number {
  return n * 2;
}
function triple(n: number): number {
  return n * 3;
}
const ops: ((n: number) => number)[] = [double, triple];
console.log(ops[0](5));
console.log(ops[1](5));
const fns: (() => string)[] = [];
for (let s = "a"; s.length <= 3; s += "x") {
  fns.push(() => s);
}
for (const f of fns) {
  console.log(f());
}
const grouped: (number)[] = [1, 2, 3];
console.log(grouped[1]);
function runAll(cbs: ((n: number) => number)[], x: number): number {
  let acc = x;
  for (const cb of cbs) {
    acc = cb(acc);
  }
  return acc;
}
console.log(runAll([double, (n: number) => n + 1], 4));
