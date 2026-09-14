// §10.1.8.1 — an integer-key read on a Symbol wrapper or a BigInt receiver walks
// to its prototype's expando like the Number / Boolean wrappers do; an own expando
// on the wrapper still answers first.
(Symbol.prototype as any)[0] = "s0";
(Symbol.prototype as any).length = 1;
(BigInt.prototype as any)[0] = 9n;
(BigInt.prototype as any).length = 1;
(Number.prototype as any)[0] = "n0";
const s: any = Object(Symbol("x"));
const b: any = Object(1n);
const n: any = Object(2);
console.log(s[0], s.length, b[0], b.length, n[0]);
s[0] = "own";
console.log(s[0], Object(Symbol("y"))[0]);
console.log((1n as any)[0], (3 as any)[0]);
console.log("done");
