// RFC 20260815-fn-value-rest-spread 刀 1-3 — a named rest fn as a
// VALUE: the signature registers the per-argument Rest sentinel
// (direct calls fold back to apply_rest_args' packed-array shape),
// the value binding wraps to a closure cell (the variadic lane
// dispatches through the boxed dual entry), and a TYPED rest's
// adapter converts the collected Arr<Any> through the
// assign-boundary kernel. Spread-forwarding between rest fns rides
// the same machinery.
function tail(...args: number[]): number {
  return args.length;
}
console.log(tail(1, 2, 3), tail());
const g = tail;
console.log(g(1, 2, 3), g());
function two(a: string, ...rest: number[]): string {
  return a + rest.length;
}
console.log(two("x", 1, 2), two("y"));
const h = two;
console.log(h("z", 5, 6, 7));
function fwd(...args: number[]): number {
  return g(...args);
}
console.log(fwd(1, 2, 3), fwd());
class MySet extends Set<number> {
  addAll(...xs: number[]): MySet {
    for (const x of xs) this.add(x);
    return this;
  }
}
const m = new MySet();
m.addAll(1, 2, 3);
console.log(m.size, m.has(2));
const spread = [7, 8];
m.addAll(...spread);
console.log(m.size);
