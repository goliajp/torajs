// The same two shapes with no prologue: an ordinary sloppy Script, so
// the arguments object aliases its parameters and an undefined
// receiver is substituted with the global object.
function mapped(a, b) {
  a = 99;
  return arguments[0] + "," + arguments[1];
}
console.log(mapped(1, 2));

function receiver() {
  return this === undefined ? "undefined" : typeof this;
}
console.log(receiver());
