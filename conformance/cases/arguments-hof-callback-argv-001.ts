// Rotation 363 — argv-face inline callbacks on the array HOF inline
// loops: an anonymous fn-expr at the map/filter/forEach callback
// slot whose body reads arguments VALUES joins the argv face (the
// collector's HOF-anon arm), and the loop routes the call through
// the boxed variadic dispatch so the adapter feeds real argc/argv.
// Pre-knife every one of these bodies was a loud checker reject.
console.log([10, 20].map(function () { return arguments[0]; })[1]);
console.log([10, 20].map(function () { return arguments[1]; })[1]);
console.log([10, 20].map(function () { return arguments[5]; })[0]);
console.log([3, 0, 5].filter(function () { return arguments[0] > 2; }).join(","));
[7, 8].forEach(function () { console.log(arguments[1], arguments.length); });
console.log([4].map(function () { return arguments.length + arguments[0]; })[0]);
console.log([9].map(function () { return [...arguments].length; })[0]);
function wrap() {
  return [1, 2].map(function () { return arguments[0] * 10; });
}
console.log(wrap().join(","));
