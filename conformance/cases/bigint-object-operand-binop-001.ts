// §13.12 / §13.6-13.10: an object operand next to a BigInt reaches ToPrimitive / valueOf and
// steps in the BigInt domain — a BigInt wrapper object, a [Symbol.toPrimitive] literal, a
// valueOf literal, and an any slot holding a BigInt.
var a: any = 5n;
console.log(a & 3n, 3n | a, a ^ 6n, a << 1n, a >> 1n, a * 2n, a + 1n, a - 1n, a ** 2n, a % 3n);
var w = Object(5n);
console.log(w & 3n, 3n | w, w * 2n, w + 1n, w << 1n);
var o = { [Symbol.toPrimitive]: function () { return 5n; }, valueOf: function () { throw new Error("no"); } };
console.log(o & 3n, 3n ^ o, o * 2n, o ** 2n);
var v = { valueOf: function () { return 7n; } };
console.log(v & 3n, v - 1n, v >> 1n, 1n + v);
try { console.log(o & 3); } catch (e) { console.log((e as any).constructor.name); }
