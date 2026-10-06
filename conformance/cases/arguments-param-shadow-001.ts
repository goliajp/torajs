// Another function's parameter that happens to share a binding's
// name refers to that parameter, not to the binding. Its uses (and
// those of closures capturing it) must not count as references to the
// binding, or the binding's own body loses its arguments object.
function other(func: any): any {
  const inner = () => func(7);
  return inner() + "|" + func(1, 2);
}
function unused(func: any): any { return 0; }

var func = function (a, b) { return arguments.length + ":" + arguments[3]; };
(function () { console.log(func(0, 1, 0, 3)); })();
console.log(other(function (x) { return "p" + x; }));
console.log(unused(0));

function other2(decl: any): any { return decl(5); }
function decl(a, b) { return arguments.length + ":" + arguments[2]; }
console.log(decl(0, 1, 9), decl.length);
console.log(other2((x: any) => x * 2));

var lio = function (a, b) {
  console.log(Array.prototype.lastIndexOf.call(arguments, arguments[0]));
  console.log(Array.prototype.lastIndexOf.call(arguments, arguments[3]));
  console.log(Array.prototype.lastIndexOf.call(arguments, arguments[4]));
};
function harness(lio: any): any { return typeof lio === "function" ? lio() : 0; }
(function () { lio(0, arguments, 0, Object.prototype); })();
console.log(harness(1));
