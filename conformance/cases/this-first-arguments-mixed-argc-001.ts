// A this-first body (a top-level function that reads `this`, a
// static method's receiver twin) reading `arguments` at direct calls
// of different sizes: each call sees its own count and values, not
// the declared arity and not a fold of some other site.
function t(a: number) { return [this === undefined, arguments.length, a, arguments[2]]; }
console.log(t.call({}, 1, 2), t.call({}, 7), t(1, 2, 3));
console.log(t.apply({}, [4, 5, 6, 7]));
function take(f: any) { return f(4, 5, 6); }
console.log(take(t));
var h = t;
console.log(h.call({}, 1, 2, 3), h.call({}, 0));
class C { static both(a: number) { return [this === C, arguments.length, a]; } }
console.log(C.both.call({}, 1, 2), C.both.call({}, 7), C.both(1, 2, 3), C.both(5));
class D { static g(a: number) { return [this === D, arguments.length, a, arguments[1]]; } }
console.log(D.g.call(D, 1, 2, 3), D.g.call({}, 0), D.g.apply({}, [1, 2]));
var xs = [8, 9];
console.log(D.g.call(D, ...xs), t.call({}, ...xs), D.g.call({}, 0, ...xs));
