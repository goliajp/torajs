// rotation 365 — boxed-only object-literal method argv: a field
// closure (here `valueOf`) with an arguments touch and ZERO visible
// call sites — no member read anywhere — joins the argv face (module
// doc in ast/arguments_object_objlit_argv.rs). Every call arrives
// through a builtin protocol (ToPrimitive / ToNumber) on the dynobj
// any-lane, entering the closure cell's boxed adapter with the REAL
// argc (zero) and receiver. Covers both head shapes: `valueOf` reads
// `this` (method shape, [__this, __torajs_argv]) while `toString`-free
// helpers would ride the value shape — the second object below has a
// this-free `valueOf` covering that split.
var args: any, thisValue: any, callCount = 0;
var arg = {
  valueOf: function () {
    args = arguments;
    thisValue = this;
    callCount += 1;
    return 7;
  },
};
console.log(Number(arg));
console.log(callCount);
console.log(args.length);
console.log(thisValue === arg);
var date = new Date(2016, 6);
console.log(date.setDate(arg) === new Date(2016, 6, 7).getTime());
console.log(callCount);

var args2: any, count2 = 0;
var plain = {
  valueOf: function () {
    args2 = arguments;
    count2 += 1;
    return 41;
  },
};
console.log(1 + (plain as any));
console.log(count2);
console.log(args2.length);
