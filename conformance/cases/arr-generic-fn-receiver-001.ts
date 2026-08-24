// A function value as a generic array-like receiver — §7.1.19
// stringified expando writes/reads on the closure cell (typed
// spelling; the any lane reached them since rotation 408), and the
// scan family reads §20.2.4.1 length (= parameter count, expando
// shadow included).
var fun = function (a, b) { return a + b; };
fun[0] = 11;
fun[1] = 9;
console.log(fun[0], fun[1], fun.length);
function gt10(val, idx, obj) { return val > 10; }
console.log(Array.prototype.every.call(fun, gt10));
console.log(Array.prototype.some.call(fun, gt10));
console.log(Array.prototype.indexOf.call(fun, 9));
console.log(Array.prototype.join.call(fun, "-"));
// arity 0 fn: vacuous scan whatever indices are installed
var z = function () { return 1; };
z[0] = 5;
console.log(Array.prototype.some.call(z, function () { return true; }));
