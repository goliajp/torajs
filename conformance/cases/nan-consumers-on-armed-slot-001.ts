// The undefined sentinel rides a quiet NaN bit pattern, so every
// consumer that asks a NaN-flavoured question has to ask first
// whether the slot is armed. undefined and NaN answer the same on
// isFinite / isInteger / isSafeInteger, and differently on these
// three:
//
//   Number.isNaN   §21.1.2.4 does not coerce — a non-Number is false
//   x !== x        undefined is equal only to itself (§7.2.16)
//   Object.is      SameValue(undefined, NaN) is false
const zs: number[] = [1, 2, 3];
const u = zs[9];
const real = 0 / 0;

console.log(Number.isNaN(u), Number.isNaN(real), Number.isNaN(zs[0]));
console.log(u !== u, real !== real, zs[0] !== zs[0]);
console.log(u === u, real === real, u === zs[0]);
console.log(Object.is(u, NaN), Object.is(real, NaN), Object.is(u, u));
console.log(Number.isFinite(u), Number.isInteger(u), Number.isSafeInteger(u));

function show(p: number): void {
  console.log("param", Number.isNaN(p), p !== p, Object.is(p, NaN));
}
show(zs[9]);
show(2);
