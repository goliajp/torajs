// A free name that no scope in the program binds is not a capture of
// a generator expression: it is an implicit global (sloppy write) or
// an undeclared reference, answered by the hoisted body's own lanes —
// the same way the declaration form always compiled it.
var result;
var g = function* () {
  function h() {
    yield = 1;
  }
  return typeof h;
};
result = g().next();
console.log(result.value, result.done);

// control: a capture of an enclosing fn-expression's self-name is a
// real capture (bound by that function's env, declared nowhere)
var f = function outer() {
  var gg = function* () {
    yield typeof outer;
  };
  return gg().next().value;
};
console.log(f());
