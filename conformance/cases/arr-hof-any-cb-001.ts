// 398-10 — an Array-receiver HOF whose callback types `any` routes to
// the runtime any-method lane instead of dying at the typed per-arg
// gate; a named this-reading callback under `as any` rides the
// recv-first forwarder so a thisArg binds `this` (§23.1.3.{18,20}).
function cb(n: any): any {
  return n + (this as any).k;
}
console.log([1, 2].map(cb as any, { k: 10 } as any));

// Bare `any` binding in the callback slot — same route, no receiver.
const dbl: any = (n: any): any => n * 2;
console.log([1, 2].map(dbl));
console.log([1, 2, 3].filter(((n: any): any => n > 1) as any));

// reduce takes no thisArg — args[1] is the init.
const add: any = (a: any, b: any): any => a + b;
console.log([1, 2, 3].reduce(add, 10));

// No thisArg present: the callback keeps plain-call `this` semantics.
function probe(n: any): any {
  return this === undefined ? n * 10 : 0;
}
console.log([1, 2].map(probe as any));

// The any-receiver spelling of the same callback shape.
const xs: any = [1, 2];
console.log(xs.map(cb as any, { k: 100 } as any));
