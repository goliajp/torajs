// A call through a fn-typed parameter of a generic function answers what
// the value handed to it returns, in every specialization, also when the
// value returns something other than the parameter's declared return.
function mkS(k: number): (y?: number) => any { return (y?: number) => "s" + k }
function mkN(k: number): (y?: number) => any { return (y?: number) => (y ?? 0) + k }

function show<T>(x: T, f: (y?: number) => number): string {
  const v = f();
  const w = f(2);
  return typeof v + ":" + v + " " + typeof w + ":" + w;
}
function both<T>(x: T, f: (y?: number) => number): string {
  return [f(), f(1)].join(",") + " " + typeof x;
}

console.log(show(1, mkS(2)));
console.log(show("a", mkN(3)));
console.log(show(true, (y?: number) => 4));
console.log(both([1], mkS(5)), both(null, mkN(6)));
