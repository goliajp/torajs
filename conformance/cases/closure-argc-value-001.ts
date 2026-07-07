// RFC 20260708-closure-argc-abi chunk 1 — closure-VALUE arguments.length
// real argc: direct binding + alias, zero/beyond/below declared arity,
// beyond-arity args still evaluate (side effects), plain closures
// unchanged.
const f = function () { return arguments.length; };
console.log(f());
console.log(f(1));
console.log(f(1, 2, 3));
const g = f;
console.log(g(7, 8));
const h = function (a, b) { return arguments.length + a; };
console.log(h(10, 2));
console.log(h(10, 2, 99));
console.log(h(10));
let side = 0;
const bump = function () { side = side + 1; return side; };
const k = function () { return arguments.length; };
console.log(k(bump(), bump()));
console.log(side);
const plain = function (x: number) { return x * 2; };
console.log(plain(21));
