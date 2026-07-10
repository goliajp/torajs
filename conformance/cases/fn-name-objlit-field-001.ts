// chunk 792 — ES §13.2.5.5 PropertyDefinitionEvaluation: an
// anonymous fn definition in an object-literal field position takes
// the property key as its name (fn-name registry row → fn-print).
// Previously only let/const/assign positions registered, so
// `{ cb: () => 1 }` printed [Function (anonymous)].
const obj = { cb: () => 1 };
console.log(obj.cb);
const nested = { a: { deep: (x: number) => x * 2 } };
console.log(nested.a.deep);
type Taken = { fn: () => number };
function take(o: Taken) {
  console.log(o.fn);
}
take({ fn: () => 7 });
const g = () => 42;
console.log(g);
function topf(a: number): number {
  return a;
}
console.log(topf);
