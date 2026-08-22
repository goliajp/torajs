// §25.1 resizable ArrayBuffer — `maxByteLength` is an option that is
// either present or absent, and absent is not the same as zero.
const fixed = new ArrayBuffer(4);
console.log(fixed.resizable, fixed.maxByteLength);

const r = new ArrayBuffer(4, { maxByteLength: 16 });
console.log(r.resizable, r.byteLength, r.maxByteLength);

// A maximum of zero IS present — the buffer is resizable with
// nowhere to grow, which a "0 means absent" encoding would lose.
const z = new ArrayBuffer(0, { maxByteLength: 0 });
console.log(z.resizable, z.byteLength, z.maxByteLength);

// `undefined` for the option, or a non-object bag, is absent.
console.log(new ArrayBuffer(2, { maxByteLength: undefined }).resizable);
console.log(new ArrayBuffer(2, 5 as any).resizable);
console.log(new ArrayBuffer(2, null as any).resizable);

// §25.1.3.1 — a length above the maximum is a RangeError.
try { new ArrayBuffer(8, { maxByteLength: 4 }); } catch (e) { console.log((e as Error).constructor.name); }

// §25.1.6.6 resize — grow, shrink, and the two rejections.
r.resize(12);
console.log(r.byteLength, r.maxByteLength, r);
r.resize(2);
console.log(r.byteLength, r);
r.resize(16);
console.log(r.byteLength);
try { r.resize(17); } catch (e) { console.log((e as Error).constructor.name); }
try { fixed.resize(2); } catch (e) { console.log((e as Error).constructor.name); }
try { r.resize(-1); } catch (e) { console.log((e as Error).constructor.name); }

// Step order is load-bearing: a fixed-length buffer rejects BEFORE
// coercing its argument, so the valueOf below must not run.
let coerced = 0;
const counting = { valueOf() { coerced = coerced + 1; return 2; } };
try { fixed.resize(counting as any); } catch (e) { console.log((e as Error).constructor.name, coerced); }
// On a resizable one it does run.
r.resize(counting as any);
console.log(r.byteLength, coerced);

// A slice of a resizable buffer is fixed-length (§25.1.6.7 builds
// %ArrayBuffer% with no maximum).
console.log(r.slice(0, 2).resizable);
