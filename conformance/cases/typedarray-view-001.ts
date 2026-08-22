// §23.2.5.1.5 — a view over an existing buffer, and what happens to
// it when that buffer moves underneath.
const b = new ArrayBuffer(16);
const whole = new Uint8Array(b);
console.log(whole.length, whole.byteOffset, whole.byteLength);
const part = new Int32Array(b, 4, 2);
console.log(part.length, part.byteOffset, part.byteLength);
console.log(part.buffer === b, whole.buffer === b);

// Two views over the same bytes see each other's writes.
part[0] = -1;
console.log(whole[4], whole[5], whole[6], whole[7], whole[3], whole[8]);
whole[8] = 0xFF;
console.log(part[1]);

// The offset must divide by the element size, the length must fit,
// and a whole-buffer view needs the byte length to divide evenly.
try { new Int32Array(b, 1); } catch (e) { console.log((e as Error).constructor.name); }
try { new Int32Array(b, 0, 99); } catch (e) { console.log((e as Error).constructor.name); }
try { new Uint16Array(new ArrayBuffer(3)); } catch (e) { console.log((e as Error).constructor.name); }
console.log(new Int32Array(b, 16).length);

// §10.4.5 — a view over a RESIZABLE buffer with no explicit length
// tracks it, and there is nowhere for that length to be stored.
const rab = new ArrayBuffer(4, { maxByteLength: 16 });
const track = new Uint8Array(rab);
console.log(track.length);
rab.resize(12);
console.log(track.length, track.byteLength);
rab.resize(0);
console.log(track.length);
rab.resize(8);
console.log(track.length);

// An explicit length does NOT track — and it goes out of bounds when
// the buffer shrinks under it, which reads as length 0 (and offset
// 0) rather than as an error.
const fixed = new Uint8Array(rab, 2, 4);
console.log(fixed.length, fixed.byteOffset);
rab.resize(16);
console.log(fixed.length, fixed.byteOffset);
rab.resize(3);
console.log(fixed.length, fixed.byteOffset, fixed[0]);
rab.resize(8);
console.log(fixed.length, fixed.byteOffset);

// Every element type over the same eight bytes.
const eight = new ArrayBuffer(8);
new Uint8Array(eight)[0] = 1;
console.log(
  new Int8Array(eight).length,
  new Uint8ClampedArray(eight).length,
  new Int16Array(eight).length,
  new Uint16Array(eight).length,
  new Int32Array(eight).length,
  new Uint32Array(eight).length,
  new Float32Array(eight).length,
  new Float64Array(eight).length,
  new BigInt64Array(eight).length,
  new BigUint64Array(eight).length,
);
console.log(new Uint16Array(eight)[0], new Float64Array(eight)[0]);
