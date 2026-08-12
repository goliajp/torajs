// §15.1.2 / §15.3 — `yield` as a parameter name outside generators
// under the sloppy goal: FormalParameters ride the fn's OWN [Yield]
// bit (a generator's params reject, its enclosing scope's bit is
// irrelevant), arrows inherit the enclosing bit.
var af = (yield) => yield + 1;
console.log(af(41));

var obj = {
  method(yield) {
    return yield;
  }
};
console.log(obj.method("arg"));

function f(yield) {
  return yield * 2;
}
console.log(f(21));
