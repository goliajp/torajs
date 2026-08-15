// L3b ⑤ — a named fn reached through an `as any` cast goes out via
// the `__forward_` relay; the relay must hand the head-less callee
// its OWN runtime argc, not its declared arity, so `arguments.length`
// answers the true call-site count (§10.2.1.4).
function f(a: number): number {
  return arguments.length;
}
console.log((f as any)(1, 2, 3));
console.log(f(7));
const g: any = f;
console.log(g(4, 5));

function h(a?: number): number {
  return arguments.length;
}
console.log((h as any)(...[4, 5]));
console.log((h as any)());
console.log((h as any)(1, 2, 3, 4, 5, 6));
