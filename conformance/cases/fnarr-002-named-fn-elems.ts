// chunk 733 — bare named-fn references stored into fn-typed array
// slots wrap through __forward_<name> closures (array-literal init /
// push / index-assign store-sites), and an Index-read callee
// (`ops[i](x)`) dispatches through the generalized indirect lane.
function double(n: number): number {
  return n * 2;
}
function triple(n: number): number {
  return n * 3;
}
function dec(n: number): number {
  return n - 1;
}
const ops: Array<(n: number) => number> = [double, triple];
console.log(ops[0](5));
console.log(ops[1](5));
ops.push((n: number) => n * 10);
console.log(ops[2](5));
ops[0] = dec;
console.log(ops[0](5));
