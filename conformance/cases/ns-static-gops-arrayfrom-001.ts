// RFC 20260721-builtin-method-reflection 刀 3 — ns-static batch 6:
// Object.getOwnPropertySymbols + Array.from reified as VALUES.
// gOPS is also callable detached (the W-N-c empty-list truth);
// Array.from is reflection-surface only (call face recorded).
const gops: any = Object.getOwnPropertySymbols;
console.log(typeof gops);
console.log(gops.name);
console.log(gops.length);
const from: any = Array.from;
console.log(typeof from);
console.log(from.name);
console.log(from.length);
const o: any = { a: 1, b: 2 };
const syms: any = gops(o);
console.log(syms.length);
console.log(Object.getOwnPropertySymbols(o).length);
try {
  gops(null);
  console.log("no throw");
} catch (e) {
  console.log("caught");
}
