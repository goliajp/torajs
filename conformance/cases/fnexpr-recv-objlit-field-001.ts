// A `this`-using function expression only keeps its promoted receiver
// when every use of its binding is a shape the promoted ABI survives.
// Writing the binding into an OBJECT-LITERAL FIELD was not on that
// list, so `const o = { f: k }` cost `k` its receiver and the program
// failed to compile at all — while `[k]` (589-03) and a bare `k` both
// kept it.
//
// A bare-Ident field value can only be typed `__fn(` or `any`: the
// `__mth(` slot, the one repr whose call passes a receiver statically,
// is minted only for an INLINE method expression. Both of the two
// shift argv on FLAG_CLOSURE_RECV_FIRST, so every way of reading the
// field back and calling it lands on the receiver-aware lane.
let k = function (this: any) {
  this.q = 1;
  return this;
};

// construct out of the field
const o = { f: k };
const built: any = new (o.f as any)();
console.log(built.q, o.f === k);

// shorthand spells the same thing
const short = { k };
const built2: any = new (short.k as any)();
console.log(built2.q);

// a method-shaped call gives the literal itself as the receiver
let reader = function (this: any) {
  return this.n;
};
const holder = { n: 7, f: reader };
console.log((holder as any).f());

// a detached read is a plain call, whose receiver is undefined
let probe = function (this: any) {
  return this === undefined;
};
const box = { f: probe };
const detached = box.f;
console.log(detached());

// nested literals and literals inside arrays are the same shape
let deep = function (this: any) {
  this.q = 5;
};
const nested = { a: { f: deep } };
console.log((new (nested.a.f as any)() as any).q);
const inArray = [{ f: deep }];
console.log((new (inArray[0].f as any)() as any).q);
