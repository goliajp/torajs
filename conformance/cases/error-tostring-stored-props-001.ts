// §20.5.3.4 Error.prototype.toString: `? ToString(? Get(O, "name"))`
// and the same for "message". A value a Str slot cannot hold — an
// object, a number, `undefined` — used to be ignored.
const a: any = new Error("m");
a.name = { toString() { return "NN"; } };
console.log(String(a), a.toString());
const b: any = new Error("m");
b.message = { toString() { return "MM"; } };
console.log(String(b));
const c: any = new RangeError("m");
c.name = 42;
console.log(String(c));
const d: any = new TypeError("m");
d.name = undefined;
console.log(String(d));
class E extends Error {}
const f: any = new E("m");
f.name = [1, 2];
console.log(String(f), `${f}`);
const g = new Error("plain");
const h = new SyntaxError("");
console.log(String(g), String(h), String(new Error("x", { cause: 1 })));
