// rotation 365 store arm — an anonymous fn-expr assigned straight
// into a boxed-face store position (module doc in
// ast/arguments_object_objlit_argv.rs) joins the argv face: the
// target is any-lane-only consumption, so every call enters the
// closure cell's boxed adapter with real argc/argv. Static-name and
// computed-key spellings both admit; the readback call rides the
// any-lane member dispatch.
var args: any, callCount = 0;
var spy: any = {};
spy.cb = function () {
  args = arguments;
  callCount += 1;
  return 9;
};
console.log(spy.cb(4, 5));
console.log(callCount);
console.log(args.length);
console.log(args[0]);
console.log(args[1]);

var spy2: any = {};
var key = "dyn";
spy2[key] = function () {
  return arguments.length + (arguments[0] ?? 0);
};
console.log(spy2.dyn(7, 8, 9));
console.log(spy2[key]());
