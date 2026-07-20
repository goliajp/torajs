// RFC 20260721-builtin-method-reflection 刀 1 (G1) — Promise/BigInt/
// Symbol prototype own methods read as VALUES off the static proto
// singleton and off receivers.
const t: any = Promise.prototype.then;
console.log(typeof t, t.name, t.length);
const c: any = Promise.prototype.catch;
console.log(typeof c, c.name, c.length);
const f: any = Promise.prototype.finally;
console.log(typeof f, f.name, f.length);
const bs: any = BigInt.prototype.toString;
console.log(typeof bs, bs.name, bs.length);
const bl: any = BigInt.prototype.toLocaleString;
console.log(typeof bl, bl.name, bl.length);
const ss: any = Symbol.prototype.toString;
console.log(typeof ss, ss.name, ss.length);
// descriptor face (test262 length.js / name.js shape)
const d1 = Object.getOwnPropertyDescriptor(Promise.prototype.then, "name");
console.log(d1 === undefined ? "no-desc" : JSON.stringify(d1));
const d2 = Object.getOwnPropertyDescriptor(Promise.prototype.then, "length");
console.log(d2 === undefined ? "no-desc" : JSON.stringify(d2));
const d3 = Object.getOwnPropertyDescriptor(BigInt.prototype.toString, "name");
console.log(d3 === undefined ? "no-desc" : JSON.stringify(d3));
// receiver-side value reads answer the same interned cells
const p = Promise.resolve(1);
const pa: any = p;
console.log(pa.then === Promise.prototype.then);
const bn: any = 10n;
console.log(bn.toString === BigInt.prototype.toString);
// identity is stable
console.log(Promise.prototype.then === Promise.prototype.then);
// dispatch still works after reify
p.then((v) => {
  console.log("then-ran", v);
});
