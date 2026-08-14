// 401-03 — a this-reading fn-expr BINDING under `cb as any` in an
// array HOF callback slot promotes its receiver channel: the as-any
// shell routes the call through the any lane, whose kernels seed
// argv[0] off the closure's header flag, so a thisArg binds `this`.
const cb = function (n: any): any {
  return n + (this as any).k;
};
console.log([1, 2].map(cb as any, { k: 10 } as any));

// The any-receiver spelling of the same shape.
const xs: any = [1, 2];
console.log(xs.map(cb as any, { k: 100 } as any));

// No thisArg: plain-call `this` semantics (undefined).
const probe = function (n: any): any {
  return this === undefined ? n * 3 : 0;
};
console.log([1, 2].map(probe as any));

// A predicate-family slot with a thisArg.
const pick = function (n: any): any {
  return n > (this as any).min;
};
console.log([1, 2, 3].filter(pick as any, { min: 1 } as any));
