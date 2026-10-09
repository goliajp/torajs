// A call whose callee is a parameter, a local or a captured binding
// with the same name as a function declaration calls the binding, so
// the argument widths flow into the binding's function, not into the
// declaration's parameters, and the result is the binding's.

function f(a: number): number {
  return a + 1;
}

function viaParam(f: (x: number) => number, v: number): number {
  return f(v);
}
function viaParamConst(f: (x: number) => number): number {
  return f(1.5);
}
function viaLocal(v: number): number {
  const f = (x: number) => x + 0;
  return f(v);
}
function viaCapture(f: (x: number) => number, v: number): number {
  const inner = (): number => f(v);
  return inner();
}

console.log(viaParam((x) => x, 1.5));
console.log(viaParamConst((x) => x));
console.log(viaLocal(1.5));
console.log(viaCapture((x) => x, 2.25));
console.log(f(2));
