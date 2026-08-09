// Rotation 345 — a this+arguments fn-expr handed to an explicit-any
// param (and to a construct-channel builtin) materializes its
// `arguments` through the boxed dual entry: the argv face must not
// evict on these argument positions (escape-store profile,
// argument-position variant), and the static fold must not claim
// the fn (its real argc arrives per call).
var thisVal: any = 0;
var args: any = 0;
var C = function () {
  thisVal = this;
  args = arguments;
};
function eats(f: any): void {
  f(10, 20);
}
eats(C);
console.log(args.length, args[0], args[1]);
console.log(thisVal === undefined ? "undef-this" : "leak");
var r = Array.from.call(C, [7]);
console.log(args.length);
console.log(r.constructor === C);
