// apply/bind forms the fn-proto desugar does NOT swallow
// (rotation 291): a dynamic argArray (`f.apply(t, arr)`) and
// surplus bind partials keep the member call, so the fn-name
// receiver wraps (desugar's swallows predicate is the single
// source of truth) and the any-method apply/bind kernels take
// over. Swallowed forms keep their direct-call fast path.

function add(a: number, b: number) {
  return a + b;
}

const dyn: any = [3, 4];
console.log(add.apply(null, dyn));

const viaExpr: any = [10, 20];
console.log(add.apply(undefined, viaExpr));

function one(a: number) {
  return a;
}
// surplus partials: extra args evaluate, bound fn ignores them
const b1: any = one.bind(null, 1, 2, 3);
console.log(b1());

// swallowed forms stay direct
console.log(add.apply(null, [7, 8]));
console.log(one.call(null, 9));
