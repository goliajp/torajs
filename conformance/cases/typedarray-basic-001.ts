// §23.2 typed arrays — the length form, the accessors, the element
// conversions, and what each element type does to a value on its way
// in and out.
const u = new Uint8Array(4);
console.log(u.length, u.byteLength, u.byteOffset, u.BYTES_PER_ELEMENT);
console.log(u);
console.log(Object.prototype.toString.call(u));
console.log(u instanceof Uint8Array, u instanceof Int8Array);
console.log(typeof u, ArrayBuffer.isView(u));
console.log(new Uint8Array().length, new Uint8Array(0).length);

// §7.1 — the six wrapping integer kinds truncate toward zero and
// then wrap; NaN and the infinities are 0.
u[0] = 5;
u[1] = 300;
u[2] = -1;
u[3] = 3.7;
console.log(u[0], u[1], u[2], u[3]);
const w = new Uint8Array(4);
w[0] = NaN; w[1] = Infinity; w[2] = -Infinity; w[3] = -3.7;
console.log(w);
const i8 = new Int8Array(3);
i8[0] = 255; i8[1] = 128; i8[2] = -129;
console.log(i8);
const i32 = new Int32Array(2);
i32[0] = 4294967296 + 7; i32[1] = -1;
console.log(i32, new Uint32Array(1).length);

// §7.1.11 ToUint8Clamp is the odd one out — it clamps, and it rounds
// halves to EVEN rather than truncating.
const c = new Uint8ClampedArray(8);
c[0] = -5; c[1] = 300; c[2] = 1.4; c[3] = 1.6;
c[4] = 0.5; c[5] = 1.5; c[6] = 2.5; c[7] = 3.5;
console.log(c);

// Floats keep their own width.
const f32 = new Float32Array(2);
f32[0] = 0.1; f32[1] = 1 / 3;
console.log(f32[0] === 0.1, f32);
const f64 = new Float64Array(3);
f64[0] = 0.1; f64[1] = 1 / 0; f64[2] = NaN;
console.log(f64[0] === 0.1, f64);

// §10.4.5.4 — an out-of-range index reads `undefined` and does not
// continue up the prototype chain; a write to one is discarded.
console.log(u[9], u[-1], u[4]);
u[9] = 1;
console.log(u.length, u[9]);

// The two BigInt element types take BigInt and nothing else, and the
// stored 64 bits are the same for both — only the read differs.
const bi = new BigInt64Array(2);
bi[0] = -1n; bi[1] = 42n;
console.log(bi, bi[0], bi[1]);
const bu = new BigUint64Array(1);
bu[0] = -1n;
console.log(bu, bu[0]);
try { bi[0] = 1 as any; } catch (e) { console.log((e as Error).constructor.name); }
try { u[0] = 1n as any; } catch (e) { console.log((e as Error).constructor.name); }
console.log(bi[0]);

// The element write coerces BEFORE it decides the index is valid
// (§10.4.5.5 step 1), so a `valueOf` fires even for a write that
// lands nowhere.
let calls = 0;
const counted = { valueOf() { calls = calls + 1; return 7; } };
u[0] = counted as any;
console.log(u[0], calls);
u[99] = counted as any;
console.log(u.length, calls);
