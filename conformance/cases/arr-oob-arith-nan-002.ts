// Sibling of arr-oob-arith-nan-001, which cleaned the operand at the
// read's own exit and so only reached arithmetic whose operand *is*
// the read. The sentinel travelling any distance first — through a
// binding, a call, or an array method that misses — still arrived
// with its payload, and AArch64 handed that payload to the result.
// The engine now selects FPCR.DN at program entry, so no FP
// instruction can produce the payload at all, whatever wore it.
const zs: number[] = [1, 2, 3];

// (1) bound to a name first
const u = zs[9];
const alias: number[] = [u * 2, 5];
console.log(alias[0], alias[0] === undefined, typeof alias[0]);

// (2) carried out of a call
function id(x: number): number {
  return x;
}
const call: number[] = [id(zs[9]) * 2, 5];
console.log(call[0], call[0] === undefined, typeof call[0]);

// (3) the array methods that answer a miss with `undefined`
const empty: number[] = [];
const at: number[] = [(zs.at(9) as number) - 1, 5];
const pop: number[] = [(empty.pop() as number) - 1, 5];
const shift: number[] = [(empty.shift() as number) - 1, 5];
const find: number[] = [(zs.find((v) => v > 99) as number) - 1, 5];
console.log(at[0], pop[0], shift[0], find[0]);
console.log(typeof at[0], typeof pop[0], typeof shift[0], typeof find[0]);

// Every arithmetic form, not the enumerated few: the mode covers the
// libm kernels as well, and `%` is the one the payload used to ride
// through `fmod`.
console.log(u + 1, u - 1, u * 2, u / 2, u % 2, u ** 2);
console.log(Math.sqrt(u), Math.min(u, 1), Math.max(u, 1), Math.round(u), Math.abs(u));

// Sign writes are not arithmetic — FNEG and FABS keep any payload —
// so `-` and `+` have to spell ToNumber themselves. Unary `+` is the
// only numeric operator with nothing to emit, and used to pass the
// sentinel straight through.
const neg: number[] = [-u, 5];
const negneg: number[] = [-(-u), 5];
const plus: number[] = [+u, 5];
console.log(neg[0], negneg[0], plus[0]);
console.log(typeof neg[0], typeof negneg[0], typeof plus[0]);

// `+` on a real number is still the identity, sign of zero included.
const z = -0;
console.log(+z, 1 / +z, +0.5, +(-1.5), 1 / -(-z));

// The other half of the invariant: nothing about the mode may make a
// genuine `undefined` stop reading as one. Concatenation still spells
// it out, and a plain read still answers it.
console.log(zs[9], zs[9] === undefined, typeof zs[9], "" + zs[9]);
console.log(u, u === undefined, empty.pop(), zs.at(9));
