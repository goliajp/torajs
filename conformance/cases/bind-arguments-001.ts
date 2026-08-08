// RFC 20260808-bind-arguments-unified 刀 2 — an arguments-touching
// fn-expr behind `.bind` leaves the static synth lane (whose wrapper
// forwards a FIXED declared-arity list) for the runtime bind kernel,
// whose bound_entry concatenates partials with the live call's argv —
// real argc end to end. The collector's kill walk stops counting
// fn-local param shadows (the t262 harness declares a `func` param)
// and the boxed adapter shifts the arguments window past the
// receiver slot on a recv-first face.
var obj = { prop: "abc" };

// this + arguments together — the t262 bind-family shape
var func = function () {
  return (
    this === obj &&
    arguments.length === 2 &&
    arguments[0] === 1 &&
    arguments[1] === 2
  );
};
var nf1 = Function.prototype.bind.call(func, obj, 1, 2);
console.log(nf1());

// partial + call-site args concatenate; receiver not counted
var f2 = function () {
  console.log(this === obj);
  console.log(arguments.length);
  console.log(arguments[0]);
  console.log(arguments[2]);
};
f2.bind(obj, 5)(6, 7);

// arguments-only (no this), capturing
var tag = "x";
var f3 = function () {
  return tag + arguments.length;
};
console.log(f3.bind(null, 7, 8)(9));

// harness-shape param shadow must not kill the binding's admit
function helper(a: any, func: any): any {
  if (typeof func !== "function") {
    return null;
  }
  return func;
}
console.log(helper(0, null));
console.log(func.bind(obj, 1, 2)());
