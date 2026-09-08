// A `const` binding holding a typed array, read from a named function
// body. The top-level slot promoter infers the slot type from the
// initialiser's spelling, and its `new` list knew the four collection
// constructors but none of the twelve typed-array ones — so the
// binding never got a global home and the read answered
// `ReferenceError`, while `var`, an explicit annotation, and a plain
// array literal all worked.
const f64 = new Float64Array([1.5, 2.5]);
const u32 = new Uint32Array([7, 8]);
const i8 = new Int8Array([-1, 2]);
const u8c = new Uint8ClampedArray([300, 2]);
const big = new BigInt64Array([1n, 2n]);

function readF64() { return f64[0] + f64[1] }
function readU32() { return u32[1] }
function readI8() { return i8[0] }
function readClamped() { return u8c[0] }
function readBig() { return big[1] }

console.log(readF64(), readU32(), readI8(), readClamped(), readBig());

// The buffer aliasing the Math shell relies on: two views over one
// buffer, both read from named bodies.
const src = new Float64Array([0]);
const bits = new Uint32Array(src.buffer);
function poke(v: number) { src[0] = v }
function hi() { return bits[1] }
poke(2);
console.log(hi());
