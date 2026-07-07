// RFC 20260708-closure-argc-abi chunk 2 — arguments.length closure
// passed directly into an untyped HOF param (implicit generics →
// mono __clsargc slot → runtime argc prepend at the param-call arm).
function h(cb) { return cb(1, 2); }
console.log(h(function () { return arguments.length; }));
function h3(cb, x: number) { return cb(x, 8, 9); }
console.log(h3(function (a) { return arguments.length + a; }, 5));
console.log(h(function (a, b) { return a + b; }));
function hz(cb) { return cb(); }
console.log(hz(function () { return arguments.length; }));
const plain = function (q: number) { return q + 1; };
function ha(cb, v: number) { return cb(v); }
console.log(ha(plain, 41));
