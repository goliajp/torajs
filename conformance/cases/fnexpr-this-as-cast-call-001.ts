// A `.call` / `.apply` through an `as` cast supplies the receiver
// exactly as the bare spelling does (§20.2.3.3 / §20.2.3.1), so the
// fn-expr binding it names still qualifies for the `this` promotion.
// Before the peel, `(f as any).call(o)` was an unrecognized use shape:
// the binding declined promotion and its body's `this` capture had
// nothing to bind to — "closure `__closure_0` references unknown
// identifier `__this`" — while `f.call(o)` right next to it worked.

var write = function () {
  this.p = "x2";
  return "ok";
};
var target: any = { p: "b" };
console.log((write as any).call(target), target.p);

// apply, same face.
var read = function () {
  this.p = "set";
  return "r:" + this.p;
};
var target2: any = { p: "b" };
console.log((read as any).apply(target2), target2.p);

// Arguments ride ahead of nothing — the receiver takes its own slot,
// so the declared params still line up.
var sum = function (a: number, b: number) {
  return this.base + a + b;
};
var base: any = { base: 10 };
console.log((sum as any).call(base, 1, 2));
console.log((sum as any).apply(base, [3, 4]));

// The cast face coexists with a plain direct call of the same
// binding, which keeps the no-receiver answer.
var probe = function () {
  return typeof this;
};
var empty: any = {};
console.log((probe as any).call(empty));
console.log(probe());

// An inline fn-expr under the cast promotes directly (the call is
// its only consumer).
var inline: any = { v: 7 };
console.log(
  ((function () {
    return this.v;
  }) as any).call(inline),
);

// The bare spellings must keep working alongside.
console.log(write.call(target), target.p);
console.log(sum.apply(base, [5, 6]));
