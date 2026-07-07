// mono closure-shape ann fix — an untyped HOF param (implicit
// generics __T) instantiated by a CLOSURE arg gets a __cls( slot,
// not __fn( (env ptr jumped as fn ptr = SIGBUS). bare-fn args keep
// the __fn( path; both shapes through one generic fn split monos.
function h(cb) { return cb(1, 2); }
console.log(h(function (a, b) { return a + b; }));
function h0(cb) { return cb(); }
console.log(h0(function () { return 7; }));
const c = function () { return 9; };
console.log(h0(c));
function add(a: number, b: number) { return a + b; }
console.log(h(add));
function apply1(f, v: number) { return f(v); }
function twice(x: number) { return x * 2; }
console.log(apply1(twice, 21));
const dbl = function (x: number) { return x * 3; };
console.log(apply1(dbl, 10));
console.log(h0(function () { return 7; }) + h0(c));
