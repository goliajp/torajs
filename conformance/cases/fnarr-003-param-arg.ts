// chunk 733 — fn-typed array as a fn param: an array-literal argument
// mixing named fns (forwarder-wrapped) and capturing closures reaches
// the Closure-repr element slots uniformly; elements dispatch via
// for-of load + call.
function add1(n: number): number {
  return n + 1;
}
function runAll(ops: Array<(n: number) => number>, x: number): number {
  let acc = x;
  for (const op of ops) {
    acc = op(acc);
  }
  return acc;
}
const base = 3;
console.log(runAll([add1, (n: number) => n * 2, (n: number) => n + base], 5));
