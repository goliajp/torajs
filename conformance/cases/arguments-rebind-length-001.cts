// §13.15.1 — after a sloppy `arguments = v`, `arguments.length` is
// the NEW value's length: the hidden-argc read (Real tier) and the
// static fold (static face / declared arity) both kept answering the
// call's count. Every tier is exercised: a named fn with one direct
// call, one with several (the hidden-argc tier), an IIFE (static
// face), an escaped fn value (argv face), a rest-tailed fn, and a
// pre-rebind read that must still see the true count. `.expected`
// because bun's TypeScript front end rejects the assignment as an
// invalid target even in a sloppy .cts; node on the same source as
// .js is the semantic oracle behind the file.
function a1(x) { arguments = [9]; return arguments.length; }
function a2(x) { arguments = 7; return arguments.length; }
function a3(x) { var n = arguments.length; arguments = [9]; return [n, arguments.length]; }
function d1(...xs) { arguments = [1, 2]; return arguments.length; }
function i1(x) { arguments = "str"; return [arguments.length, typeof arguments]; }
console.log(a1(1, 2, 3), a2(1, 2, 3), a3(1, 2, 3), d1(1, 2, 3, 4), i1(1, 2, 3));
console.log((function (x) { arguments = [9]; return arguments.length; })(1, 2, 3));
console.log((function (x) { var n = arguments.length; arguments = [9]; return [n, arguments.length, arguments[0]]; })(1, 2, 3));
const c1 = function (x) { arguments = [9]; return arguments.length; };
const c2 = function (x) { var n = arguments.length; arguments = [9]; return [n, arguments.length]; };
const v1: any = c1;
const v2: any = c2;
console.log(v1(1, 2, 3), v2(1, 2, 3));
function k1(x) { arguments = [9]; return arguments.length; }
function k2(x) { var n = arguments.length; arguments = [9]; return [n, arguments.length, arguments[1]]; }
console.log(k1(1), k1(1, 2), k2(1), k2(1, 2));
function l1(x) { arguments = [9]; return arguments.length; }
const t: any = l1;
console.log(l1(1, 2), t(1, 2, 3));
function m1(x, y) { arguments = [9]; return [arguments.length, arguments[0], x, y]; }
function e1(x) { arguments = [5, 6]; return [...arguments]; }
function g1(x) { arguments = [5, 6, 7]; arguments.length = 1; return [arguments.length, arguments[0]]; }
function h1(x) { arguments.length = 5; arguments = [1]; return arguments.length; }
console.log(m1(1), e1(1, 2, 3), g1(1, 2, 3), h1(1, 2, 3));
