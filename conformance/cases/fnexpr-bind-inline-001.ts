// §22.2.3.2 — an INLINE fn-expr as the `.bind` receiver:
// `(function () { …this… }).bind(obj)`. The binding-profile promote
// (fnexpr_bind_this step 2) never saw it — there is no binding —
// so the body's `this` stayed an unresolved `__this` capture and
// compilation refused. The expression node has exactly one parent,
// so the promotion is alias-free by construction; the lifted mint
// rides the runtime bind kernel with FLAG_CLOSURE_RECV_FIRST.
const b1 = (function (x: any) {
  return this.k + x;
}).bind({ k: 100 });
console.log(b1(1));

// a leading partial after the thisArg
const b2 = (function (x: any, y: any) {
  return this.p * x + y;
}).bind({ p: 3 }, 5);
console.log(b2(2));

// receiver identity survives the bound hop
const obj = { tag: "T" };
const b3 = (function () {
  return this === obj;
}).bind(obj);
console.log(b3());

// the bound value is a first-class fn: typeof + re-call
console.log(typeof b1, b1(9));
