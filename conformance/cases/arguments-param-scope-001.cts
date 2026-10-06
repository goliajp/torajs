// With parameter expressions, a body-level `function arguments` or
// `let arguments` / `const arguments` binds only in the body; the
// parameter list still reads the arguments object (§10.2.11).
var args;
var f = function (x = args = arguments) {
  function arguments() {}
  var inner = () => typeof arguments + ':' + arguments.name;
  function own() {
    return arguments.length;
  }
  return inner() + ':' + own(1, 2, 3);
};
console.log(f(undefined, 9));
console.log(typeof args, args.length, args[1]);

var args2;
var g = function (x = args2 = arguments) {
  let arguments = 41;
  return arguments + 1;
};
console.log(g(), typeof args2, args2.length);

var args3;
function h(x = args3 = arguments) {
  const arguments = 'c';
  return arguments + x.length;
}
console.log(h(), typeof args3, args3.length);

// without a parameter expression that reads it, the body binding
// shadows the whole function as before
var k = function (x = 1) {
  function arguments() {}
  return typeof arguments + ':' + arguments.name + ':' + x;
};
console.log(k());
