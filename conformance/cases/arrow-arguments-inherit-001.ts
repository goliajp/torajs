function outer(a: number, b: number, c: number) {
  const f = () => arguments.length;
  return f();
}
console.log(outer(1, 2, 3));

function pick(i: number, j: number) {
  const g = () => (arguments as any)[1];
  return g();
}
console.log(pick(42, 99));

function nestedArrow(x: number, y: number) {
  const h = () => () => arguments.length;
  return h()();
}
console.log(nestedArrow(7, 8));

function ownScope(z: number) {
  const inner = function (w: number) {
    const k = () => arguments.length;
    return k();
  };
  return inner(z);
}
console.log(ownScope(5));

function spreadThrough(p: number, q: number) {
  const s = () => [...(arguments as any)];
  return s();
}
console.log(JSON.stringify(spreadThrough(10, 20)));

function mixedUse(m: number, n: number) {
  const direct = arguments.length;
  const viaArrow = (() => arguments.length)();
  return direct === viaArrow;
}
console.log(mixedUse(1, 2));
