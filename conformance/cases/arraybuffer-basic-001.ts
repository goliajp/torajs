// §25.1 ArrayBuffer — construction, the four accessors, slice,
// isView, and the surfaces a buffer shows to the rest of the
// language (inspect / badge / instanceof / own keys).
const b = new ArrayBuffer(8);
console.log(b.byteLength, b.maxByteLength, b.resizable, b.detached);
console.log(b);
console.log(Object.prototype.toString.call(b));
console.log(b instanceof ArrayBuffer);
console.log(Object.keys(b).length, Object.getOwnPropertyNames(b).length);
console.log(typeof b, typeof ArrayBuffer);

// §25.1.4.1 step 2 — ToIndex(length): NaN and undefined are 0,
// a fractional length truncates, and a negative one is a RangeError.
console.log(new ArrayBuffer(0).byteLength);
console.log(new ArrayBuffer(3.9).byteLength);
console.log(new ArrayBuffer(NaN).byteLength);
try { new ArrayBuffer(-1); } catch (e) { console.log((e as Error).constructor.name); }
try { new ArrayBuffer(Infinity); } catch (e) { console.log((e as Error).constructor.name); }

// The length argument coerces through valueOf like any other.
let seen = 0;
const lengthLike = { valueOf() { seen = seen + 1; return 5; } };
console.log(new ArrayBuffer(lengthLike as any).byteLength, seen);

// §25.1.6.7 slice — relative bounds, clamping, and a fresh
// fixed-length buffer every time.
const src = new ArrayBuffer(8);
console.log(src.slice().byteLength);
console.log(src.slice(2).byteLength);
console.log(src.slice(-3).byteLength);
console.log(src.slice(1, -1).byteLength);
console.log(src.slice(6, 2).byteLength);
console.log(src.slice(-99, 99).byteLength);
console.log(src.slice(0, 4).resizable);
console.log(src.slice(0, 4) === src);

// §25.1.5.1 isView — a question about the argument, and nothing in
// this slab is a view yet.
console.log(ArrayBuffer.isView(b), ArrayBuffer.isView(1), ArrayBuffer.isView(null));
console.log(ArrayBuffer.isView({}), ArrayBuffer.isView([]));

// Allocation churn — the byte store is the cell's own and a
// resizable one gives back its MAXIMUM, not its current length, so a
// drop that reads the wrong figure corrupts the allocator.
let total = 0;
for (let i = 0; i < 200; i++) {
  const t = new ArrayBuffer(i % 64, { maxByteLength: 128 });
  t.resize(i % 128);
  total = total + t.byteLength + t.slice(0, 4).byteLength;
}
console.log(total);
