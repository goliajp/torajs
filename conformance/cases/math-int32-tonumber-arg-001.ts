// ES §21.3.2.19 / §21.3.2.11 step 1 is ToUint32, not the ToNumber-to-f64
// the rest of Math takes. Both kernels read the low 32 bits of an i64,
// and the lowering reached that slot through FpToSi -- which is
// ToInteger: Infinity arrived as i64::MAX and a runtime NaN as poison.
const vo = { valueOf() { return 2; } };
const empty = {};

// §7.1.6 step 2 -- NaN, ±0 and ±Infinity are all +0.
console.log(Math.clz32(Infinity), Math.clz32(-Infinity), Math.clz32(NaN));
console.log(Math.clz32(empty), Math.clz32("bar"), Math.clz32([1, 2, 3]));
console.log(Math.clz32(null), Math.clz32(false), Math.clz32([]), Math.clz32(""));

// Values that do coerce to a number.
console.log(Math.clz32(true), Math.clz32(3.5), Math.clz32(vo), Math.clz32("15"));
console.log(Math.imul(vo, 3), Math.imul("7", vo), Math.imul(empty, 3));
console.log(Math.imul(Infinity, 3), Math.imul(true, true));

// The modulo is real: 2^32 wraps to 0, 2^32+5 to 5.
console.log(Math.clz32(4294967296), Math.clz32(4294967301));
console.log(Math.imul(4294967297, 3));

// A user valueOf runs here and may throw.
const boom = { valueOf(): number { throw new Error("boom"); } };
try { console.log(Math.clz32(boom)); } catch (e) { console.log("caught " + (e as Error).message); }
try { console.log(Math.imul(boom, 2)); } catch (e) { console.log("caught " + (e as Error).message); }

// The typed fast path is untouched.
console.log(Math.imul(3, 4), Math.clz32(1), Math.imul(-1, -1), Math.clz32(0));
